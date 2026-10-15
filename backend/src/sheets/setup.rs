//! Operator commands for the Google Sheets sync:
//!
//! * `google-auth`: one-time OAuth consent (loopback redirect + PKCE) that
//!   prints the long-lived refresh token for `GOOGLE_OAUTH_REFRESH_TOKEN`.
//!   Needs only `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET`.
//! * `google-sheets-check`: refreshes a token with the configured settings,
//!   creates the tab + header if missing and prints the spreadsheet title.
//!
//! The refresh token is printed to stdout because that is the point of the
//! command; it is never logged.

use std::{
    io::BufRead,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{Rng, distributions::Alphanumeric};
use reqwest::Url;
use secrecy::ExposeSecret;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::client::SheetsClient;
use crate::config::{self, GoogleOAuthClientConfig, GoogleSheetsConfig};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const SHEETS_SCOPE: &str = "https://www.googleapis.com/auth/spreadsheets";
pub const DEFAULT_REDIRECT_PORT: u16 = 53682;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Query parameters Google appends to the redirect URI.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

impl CallbackParams {
    fn is_callback(&self) -> bool {
        self.code.is_some() || self.error.is_some()
    }
}

#[derive(Deserialize)]
struct CodeExchange {
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: String,
}

#[derive(Deserialize, Default)]
struct OAuthError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

/// `GOOGLE_OAUTH_REDIRECT_PORT`, default 53682.
pub fn redirect_port() -> anyhow::Result<u16> {
    match std::env::var("GOOGLE_OAUTH_REDIRECT_PORT") {
        Ok(v) if !v.trim().is_empty() => {
            let port: u16 = v
                .trim()
                .parse()
                .map_err(|_| anyhow!("GOOGLE_OAUTH_REDIRECT_PORT must be a port number"))?;
            anyhow::ensure!(
                port >= 1024,
                "GOOGLE_OAUTH_REDIRECT_PORT must be 1024-65535"
            );
            Ok(port)
        }
        _ => Ok(DEFAULT_REDIRECT_PORT),
    }
}

fn random_token(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// PKCE S256 challenge: `BASE64URL(SHA256(verifier))` without padding.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The Google consent screen URL (offline access, forced consent so a refresh
/// token is always issued, PKCE S256).
pub fn consent_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> anyhow::Result<Url> {
    Url::parse_with_params(
        AUTH_URL,
        [
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", SHEETS_SCOPE),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .context("building the consent URL")
}

/// Parse `code` / `state` / `error` out of a request target (`/?code=…`) or
/// a full pasted URL (`http://127.0.0.1:53682/?code=…`).
pub fn parse_callback(target_or_url: &str) -> Option<CallbackParams> {
    let s = target_or_url.trim();
    let url = if s.starts_with("http://") || s.starts_with("https://") {
        Url::parse(s).ok()?
    } else if s.starts_with('/') {
        Url::parse(&format!("http://127.0.0.1{s}")).ok()?
    } else {
        return None;
    };
    if url.path() != "/" {
        return None;
    }
    let mut p = CallbackParams::default();
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => p.code = Some(v.into_owned()),
            "state" => p.state = Some(v.into_owned()),
            "error" => p.error = Some(v.into_owned()),
            _ => {}
        }
    }
    Some(p)
}

/// Validate the redirect: no `error`, the `state` we sent, a `code`.
pub fn check_callback(p: &CallbackParams, expected_state: &str) -> anyhow::Result<String> {
    if let Some(err) = &p.error {
        let hint = if err == "access_denied" {
            " (consent was cancelled, or the Google account is not allowed: while the consent \
             screen is in \"Testing\", add the account under Test users, or publish the app)"
        } else {
            ""
        };
        bail!("Google returned error={err}{hint}");
    }
    let state_ok = p
        .state
        .as_deref()
        .is_some_and(|s| bool::from(s.as_bytes().ct_eq(expected_state.as_bytes())));
    if !state_ok {
        bail!(
            "the redirect's state parameter does not match this run (stale tab or forged request); run google-auth again"
        );
    }
    match p.code.as_deref().map(str::trim) {
        Some(code) if !code.is_empty() => Ok(code.to_owned()),
        _ => bail!("the redirect carried no authorization code"),
    }
}

/// Serve one HTTP request on the loopback listener. `Some` when it was the
/// OAuth redirect (with `code` or `error`); other requests (favicon, …) get
/// a 404 and `None`. The page never echoes request data.
async fn handle_connection(mut stream: TcpStream) -> Option<CallbackParams> {
    let mut buf = Vec::with_capacity(1024);
    let read = tokio::time::timeout(REQUEST_READ_TIMEOUT, async {
        let mut chunk = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < MAX_REQUEST_BYTES {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
    })
    .await;
    if read.is_err() {
        return None;
    }
    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.lines().next().unwrap_or_default().split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let params = (method == "GET")
        .then(|| parse_callback(target))
        .flatten()
        .filter(CallbackParams::is_callback);

    let (status, body) = match &params {
        Some(p) if p.error.is_none() => (
            "200 OK",
            "<h2>Zhi-Mart: Google authorization received.</h2>\
             <p>You can close this tab and return to the terminal.</p>",
        ),
        Some(_) => (
            "200 OK",
            "<h2>Zhi-Mart: Google authorization was not granted.</h2>\
             <p>See the terminal for details.</p>",
        ),
        None => ("404 Not Found", "Not found"),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    params
}

/// Wait for the redirect on the listener, or for the operator to paste the
/// redirected URL into the terminal (when the browser runs on another
/// machine and cannot reach this loopback port).
async fn wait_for_callback(listener: &TcpListener) -> anyhow::Result<CallbackParams> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);
    // A detached thread: blocking stdin reads cannot be cancelled, and this
    // must not hold up runtime shutdown.
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.blocking_send(line).is_err() {
                break;
            }
        }
    });
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("accepting the redirect connection")?;
                if let Some(p) = handle_connection(stream).await {
                    return Ok(p);
                }
            }
            Some(line) = rx.recv() => {
                if line.trim().is_empty() {
                    continue;
                }
                match parse_callback(&line).filter(CallbackParams::is_callback) {
                    Some(p) => return Ok(p),
                    None => eprintln!(
                        "That is not the redirect address (it starts with http://127.0.0.1:…/?state=…&code=…). Paste the whole address bar, or wait for the browser."
                    ),
                }
            }
        }
    }
}

fn try_open_browser(url: &str) -> bool {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", url])
    } else {
        ("xdg-open", vec![url])
    };
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

async fn exchange_code(
    oauth: &GoogleOAuthClientConfig,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> anyhow::Result<CodeExchange> {
    let http = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let res = http
        .post(&oauth.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", oauth.client_id.as_str()),
            ("client_secret", oauth.client_secret.expose_secret()),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .context("could not reach Google's token endpoint")?;
    let status = res.status();
    let body = res.bytes().await.context("reading the token response")?;
    if !status.is_success() {
        let e: OAuthError = serde_json::from_slice(&body).unwrap_or_default();
        let hint = match e.error.as_str() {
            "invalid_client" => {
                " — GOOGLE_OAUTH_CLIENT_SECRET does not match GOOGLE_OAUTH_CLIENT_ID (was the secret rotated?)"
            }
            "redirect_uri_mismatch" => {
                " — add http://127.0.0.1:<port>/ to the OAuth client's Authorized redirect URIs, or use a Desktop app client"
            }
            "invalid_grant" => " — the code expired or was already used; run google-auth again",
            _ => "",
        };
        bail!(
            "token exchange failed: HTTP {} {} {}{hint}",
            status.as_u16(),
            e.error,
            e.error_description
        );
    }
    serde_json::from_slice(&body).context("unreadable token response from Google")
}

/// `open_browser`: try to open the consent URL in the default browser.
pub async fn google_auth(open_browser: bool) -> anyhow::Result<()> {
    let oauth = GoogleOAuthClientConfig::from_env()?;
    let port = redirect_port()?;
    let redirect_uri = format!("http://127.0.0.1:{port}/");
    let listener = TcpListener::bind(("127.0.0.1", port)).await.with_context(|| {
        format!(
            "cannot listen on 127.0.0.1:{port} (in use?). Set GOOGLE_OAUTH_REDIRECT_PORT to another \
             port, and register http://127.0.0.1:<that port>/ on a Web application client"
        )
    })?;

    let state = random_token(32);
    let verifier = random_token(64);
    let url = consent_url(
        &oauth.client_id,
        &redirect_uri,
        &state,
        &pkce_challenge(&verifier),
    )?;

    println!("Google Sheets authorization for Zhi-Mart");
    println!("========================================");
    println!("Before you continue:");
    println!("  * OAuth client type (Cloud Console → APIs & Services → Credentials):");
    println!("      - \"Desktop app\": works as is.");
    println!("      - \"Web application\": add {redirect_uri} to its Authorized redirect URIs");
    println!("        (otherwise Google shows Error 400: redirect_uri_mismatch).");
    println!("  * OAuth consent screen: publish it to \"In production\". While it is in");
    println!("    \"Testing\", Google expires the refresh token after 7 days.");
    println!("  * Sign in with the Google account that can EDIT the spreadsheet.");
    println!("  * \"Google hasn't verified this app\" is expected for your own app: click");
    println!("    Advanced → Go to <app name> (unsafe), then allow access to Sheets.");
    println!();
    println!("Open this URL in a browser:");
    println!();
    println!("  {url}");
    println!();
    if open_browser && try_open_browser(url.as_str()) {
        println!("(Tried to open it in your default browser.)");
    }
    println!("Waiting for Google to redirect to {redirect_uri} (10 minutes max).");
    println!("If the browser is on another machine, its tab will fail to load: copy the full");
    println!("address from that tab's address bar, paste it here and press Enter.");

    let params = tokio::time::timeout(CALLBACK_TIMEOUT, wait_for_callback(&listener))
        .await
        .map_err(|_| anyhow!("timed out after 10 minutes waiting for the Google redirect"))??;
    drop(listener);
    let code = check_callback(&params, &state)?;

    let tokens = exchange_code(&oauth, &code, &redirect_uri, &verifier).await?;
    if !tokens.scope.split_whitespace().any(|s| s == SHEETS_SCOPE) {
        eprintln!(
            "warning: the Sheets permission was not granted (granted scopes: \"{}\"). Run google-auth again and tick \"See, edit, create and delete all your Google Sheets spreadsheets\".",
            tokens.scope
        );
    }
    let Some(refresh_token) = tokens.refresh_token.filter(|t| !t.trim().is_empty()) else {
        bail!(
            "Google returned no refresh token. Remove the app's access at \
             https://myaccount.google.com/permissions and run google-auth again"
        );
    };

    println!();
    println!(
        "Success. Put this line in backend/.env (it is a password: never commit or share it):"
    );
    println!();
    println!("GOOGLE_OAUTH_REFRESH_TOKEN={refresh_token}");
    println!();
    println!("Then set GOOGLE_SHEETS_SPREADSHEET_ID and GOOGLE_SHEETS_ENABLED=true, check the");
    println!("setup with `zhimart-backend google-sheets-check`, and restart the API.");
    Ok(())
}

pub async fn sheets_check() -> anyhow::Result<()> {
    let app_env = config::app_env_from_env()?;
    let cfg = GoogleSheetsConfig::load(app_env)?;
    let enabled = std::env::var("GOOGLE_SHEETS_ENABLED")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let client = SheetsClient::new(&cfg)?;
    let report = match client.ensure_ready().await {
        Ok(r) => r,
        Err(e) => bail!("{e}\nhint: {}", e.hint()),
    };
    println!(
        "OK: connected to spreadsheet \"{}\" (ID {}).",
        report.spreadsheet_title,
        client.spreadsheet_id()
    );
    println!(
        "Tab \"{}\": {}{}; {} order row(s) present.",
        report.tab,
        if report.tab_created {
            "created now"
        } else {
            "exists"
        },
        if report.header_written {
            ", header row written"
        } else {
            ", header row present"
        },
        report.order_rows
    );
    if !enabled {
        println!(
            "Note: GOOGLE_SHEETS_ENABLED is not true, so the server does not sync yet. Set it and restart."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_rfc7636_example() {
        // RFC 7636 appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn consent_url_has_required_params() {
        let url = consent_url(
            "123-abc.apps.googleusercontent.com",
            "http://127.0.0.1:53682/",
            "st4te",
            "ch4llenge",
        )
        .expect("url");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], SHEETS_SCOPE);
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:53682/");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["state"], "st4te");
        assert_eq!(q["response_type"], "code");
    }

    #[test]
    fn callback_parsing_and_checks() {
        let p = parse_callback("/?state=abc&code=4%2F0Ab-xyz&scope=x").expect("parsed");
        assert_eq!(p.code.as_deref(), Some("4/0Ab-xyz"));
        assert_eq!(check_callback(&p, "abc").expect("ok"), "4/0Ab-xyz");
        assert!(check_callback(&p, "other").is_err(), "state mismatch");

        let pasted = parse_callback("http://127.0.0.1:53682/?state=abc&code=c1").expect("url");
        assert_eq!(pasted.code.as_deref(), Some("c1"));

        let denied = parse_callback("/?error=access_denied&state=abc").expect("parsed");
        let err = check_callback(&denied, "abc").expect_err("denied");
        assert!(format!("{err}").contains("access_denied"));

        assert!(parse_callback("/favicon.ico").is_none());
        assert!(!parse_callback("/").expect("root").is_callback());
        assert!(parse_callback("not a url").is_none());
        let no_code = parse_callback("/?state=abc").expect("parsed");
        assert!(check_callback(&no_code, "abc").is_err());
    }
}
