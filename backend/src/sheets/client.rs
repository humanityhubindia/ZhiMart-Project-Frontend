//! Minimal Google Sheets v4 REST client: just what the order sync needs.
//!
//! * Bearer auth from [`TokenSource`]; a 401 drops the cached token and the
//!   request is retried once with a freshly refreshed one.
//! * Connect timeout 5 s, total 20 s per request; redirects never followed.
//! * No blind retries here: the caller (the sync worker) retries the whole
//!   batch with backoff. Every write is safe to repeat: rows are found by
//!   order number before each write, so a lost append response turns into an
//!   in-place update next time, not a duplicate.
//! * Values are always written with `valueInputOption=RAW` (customer text is
//!   never parsed as a formula); appends use `insertDataOption=INSERT_ROWS`.
//! * Logs/errors carry the operation, HTTP status and Google's error status /
//!   message only — never tokens or cell values (customer PII).

use std::time::Duration;

use reqwest::{Method, Url, redirect};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    SheetsError,
    row::{self, COLUMN_COUNT},
    token::{TokenSource, read_body, transport_summary},
};
use crate::config::GoogleSheetsConfig;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(20);
/// Column A of a sheet with ~100k orders is a few MB; anything bigger is garbage.
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOGGED_MESSAGE: usize = 200;

/// What [`SheetsClient::upsert_rows`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UpsertOutcome {
    /// Rows rewritten in place (order number found in column A).
    pub updated: usize,
    /// Rows appended (new order, or its row was deleted from the sheet).
    pub appended: usize,
    pub tab_created: bool,
    pub header_written: bool,
}

/// What [`SheetsClient::ensure_ready`] found (for `google-sheets-check`).
#[derive(Debug, Clone)]
pub struct ReadyReport {
    pub spreadsheet_title: String,
    pub tab: String,
    pub tab_created: bool,
    pub header_written: bool,
    /// Non-empty order numbers currently in column A (header excluded).
    pub order_rows: usize,
}

#[derive(Debug, Deserialize)]
struct SpreadsheetMeta {
    #[serde(default)]
    properties: SpreadsheetProps,
    #[serde(default)]
    sheets: Vec<SheetEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct SpreadsheetProps {
    #[serde(default)]
    title: String,
}

#[derive(Debug, Deserialize)]
struct SheetEntry {
    properties: SheetProps,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SheetProps {
    #[serde(default)]
    sheet_id: i64,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchGetResponse {
    #[serde(default)]
    value_ranges: Vec<ValueRange>,
}

#[derive(Debug, Default, Deserialize)]
struct ValueRange {
    /// Absent when the range is empty.
    #[serde(default)]
    values: Vec<Vec<Value>>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleErrorEnvelope {
    #[serde(default)]
    error: GoogleError,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleError {
    #[serde(default)]
    message: String,
    #[serde(default)]
    status: String,
}

/// Google Sheets client bound to one spreadsheet + tab. Cheap to share via `Arc`.
pub struct SheetsClient {
    http: reqwest::Client,
    tokens: TokenSource,
    api_base: String,
    spreadsheet_id: String,
    tab: String,
}

impl std::fmt::Debug for SheetsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetsClient")
            .field("api_base", &self.api_base)
            .field("spreadsheet_id", &self.spreadsheet_id)
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

/// `'My Tab'` → `'My Tab'` quoted for A1 notation (`'` doubled).
pub fn quote_tab(tab: &str) -> String {
    format!("'{}'", tab.replace('\'', "''"))
}

impl SheetsClient {
    pub fn new(cfg: &GoogleSheetsConfig) -> anyhow::Result<Self> {
        let https_only =
            cfg.api_base.starts_with("https://") && cfg.oauth.token_url.starts_with("https://");
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .redirect(redirect::Policy::none())
            .https_only(https_only)
            .user_agent(concat!("zhimart-backend/", env!("CARGO_PKG_VERSION")))
            .build()?;
        // Fail at boot, not on the first sync, if the base URL is unusable.
        Url::parse(&cfg.api_base)
            .map_err(|e| anyhow::anyhow!("GOOGLE_SHEETS_API_BASE is not a valid URL: {e}"))?;
        Ok(Self {
            tokens: TokenSource::new(http.clone(), cfg.oauth.clone(), cfg.refresh_token.clone()),
            http,
            api_base: cfg.api_base.trim_end_matches('/').to_owned(),
            spreadsheet_id: cfg.spreadsheet_id.clone(),
            tab: cfg.tab.clone(),
        })
    }

    pub fn tab(&self) -> &str {
        &self.tab
    }

    pub fn spreadsheet_id(&self) -> &str {
        &self.spreadsheet_id
    }

    /// `'Orders'!<range>`.
    fn a1(&self, range: &str) -> String {
        format!("{}!{range}", quote_tab(&self.tab))
    }

    /// `{base}/v4/spreadsheets/{id}` + `tail` as further path segments
    /// (each percent-encoded), e.g. `["values", "'Orders'!A1:append"]`.
    fn url(&self, tail: &[&str], query: &[(&str, &str)]) -> Result<Url, SheetsError> {
        let mut url = Url::parse(&self.api_base)
            .map_err(|_| SheetsError::Transient("invalid GOOGLE_SHEETS_API_BASE".into()))?;
        {
            let mut segs = url
                .path_segments_mut()
                .map_err(|_| SheetsError::Transient("invalid GOOGLE_SHEETS_API_BASE".into()))?;
            segs.pop_if_empty().extend(["v4", "spreadsheets"]);
            match tail.split_first() {
                // `{id}:batchUpdate` style methods hang off the id segment.
                Some((first, rest)) if first.starts_with(':') => {
                    segs.push(&format!("{}{first}", self.spreadsheet_id));
                    segs.extend(rest);
                }
                _ => {
                    segs.push(&self.spreadsheet_id);
                    segs.extend(tail);
                }
            }
        }
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    /// Send one request; on 401 refresh the token and retry once.
    async fn call(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
        op: &'static str,
    ) -> Result<Value, SheetsError> {
        let mut refreshed = false;
        loop {
            let token = self.tokens.access_token().await?;
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(token.expose_secret());
            if let Some(b) = body {
                req = req.json(b);
            }
            let res = req
                .send()
                .await
                .map_err(|e| SheetsError::Transient(transport_summary(op, &e)))?;
            let status = res.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && !refreshed {
                tracing::debug!(op, "google sheets: 401, refreshing the access token");
                self.tokens.invalidate().await;
                refreshed = true;
                continue;
            }
            let bytes = read_body(res, MAX_RESPONSE_BYTES).await?;
            if status.is_success() {
                if bytes.is_empty() {
                    return Ok(Value::Null);
                }
                return serde_json::from_slice(&bytes).map_err(|_| {
                    SheetsError::Transient(format!("{op}: unreadable response from Google"))
                });
            }
            return Err(classify_api_error(status.as_u16(), &bytes, op));
        }
    }

    async fn meta(&self) -> Result<SpreadsheetMeta, SheetsError> {
        let url = self.url(
            &[],
            &[(
                "fields",
                "properties.title,sheets.properties(sheetId,title)",
            )],
        )?;
        let v = self.call(Method::GET, url, None, "get spreadsheet").await?;
        serde_json::from_value(v).map_err(|_| {
            SheetsError::Transient("get spreadsheet: unexpected response shape".into())
        })
    }

    fn find_tab(&self, meta: &SpreadsheetMeta) -> Option<i64> {
        meta.sheets
            .iter()
            .find(|s| s.properties.title == self.tab)
            .map(|s| s.properties.sheet_id)
    }

    /// Make sure the tab exists. Returns its sheet id and whether it was created.
    async fn ensure_tab(&self) -> Result<(i64, bool), SheetsError> {
        let meta = self.meta().await?;
        if let Some(id) = self.find_tab(&meta) {
            return Ok((id, false));
        }
        let body = json!({
            "requests": [{
                "addSheet": {
                    "properties": {
                        "title": self.tab,
                        "gridProperties": { "frozenRowCount": 1 }
                    }
                }
            }]
        });
        let url = self.url(&[":batchUpdate"], &[])?;
        match self.call(Method::POST, url, Some(&body), "add tab").await {
            Ok(v) => {
                let id = v["replies"][0]["addSheet"]["properties"]["sheetId"]
                    .as_i64()
                    .unwrap_or_default();
                tracing::info!(tab = %self.tab, "google sheets: created tab");
                Ok((id, true))
            }
            // Another replica may have created it a moment ago.
            Err(e @ SheetsError::Rejected { .. }) => match self.find_tab(&self.meta().await?) {
                Some(id) => Ok((id, false)),
                None => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    /// Row 1 (header) and column A, in one request.
    async fn read_header_and_keys(
        &self,
    ) -> Result<(Vec<Vec<Value>>, Vec<Vec<Value>>), SheetsError> {
        let header = self.a1(&format!("A1:{}1", row::last_column()));
        let keys = self.a1("A:A");
        let url = self.url(
            &["values:batchGet"],
            &[
                ("ranges", header.as_str()),
                ("ranges", keys.as_str()),
                ("majorDimension", "ROWS"),
            ],
        )?;
        let v = self.call(Method::GET, url, None, "read column A").await?;
        let mut parsed: BatchGetResponse = serde_json::from_value(v)
            .map_err(|_| SheetsError::Transient("read column A: unexpected response".into()))?;
        let keys = parsed.value_ranges.pop().unwrap_or_default().values;
        let header = parsed.value_ranges.pop().unwrap_or_default().values;
        Ok((header, keys))
    }

    /// Read header + column A; if the tab does not exist yet (Google answers
    /// 400 "Unable to parse range"), create it and read again.
    async fn read_index(
        &self,
    ) -> Result<(Vec<Vec<Value>>, Vec<Vec<Value>>, Option<i64>), SheetsError> {
        match self.read_header_and_keys().await {
            Ok((h, k)) => Ok((h, k, None)),
            Err(e @ SheetsError::Rejected { status: 400, .. }) => {
                let (sheet_id, created) = self.ensure_tab().await?;
                if !created {
                    return Err(e);
                }
                let (h, k) = self.read_header_and_keys().await?;
                Ok((h, k, Some(sheet_id)))
            }
            Err(e) => Err(e),
        }
    }

    /// Write the header row (RAW) and, best effort, make it bold + frozen.
    async fn write_header(&self, sheet_id: Option<i64>) -> Result<(), SheetsError> {
        let range = self.a1(&format!("A1:{}1", row::last_column()));
        let url = self.url(&["values", &range], &[("valueInputOption", "RAW")])?;
        let body =
            json!({ "range": range, "majorDimension": "ROWS", "values": [row::header_row()] });
        self.call(Method::PUT, url, Some(&body), "write header")
            .await?;
        tracing::info!(tab = %self.tab, "google sheets: wrote header row");

        let sheet_id = match sheet_id {
            Some(id) => Some(id),
            None => self.meta().await.ok().and_then(|m| self.find_tab(&m)),
        };
        if let Some(sheet_id) = sheet_id
            && let Err(e) = self.format_header(sheet_id).await
        {
            tracing::debug!(error = %e, "google sheets: formatting the header failed (ignored)");
        }
        Ok(())
    }

    async fn format_header(&self, sheet_id: i64) -> Result<(), SheetsError> {
        let body = json!({
            "requests": [
                {
                    "repeatCell": {
                        "range": {
                            "sheetId": sheet_id,
                            "startRowIndex": 0,
                            "endRowIndex": 1,
                            "startColumnIndex": 0,
                            "endColumnIndex": COLUMN_COUNT
                        },
                        "cell": { "userEnteredFormat": { "textFormat": { "bold": true } } },
                        "fields": "userEnteredFormat.textFormat.bold"
                    }
                },
                {
                    "updateSheetProperties": {
                        "properties": { "sheetId": sheet_id, "gridProperties": { "frozenRowCount": 1 } },
                        "fields": "gridProperties.frozenRowCount"
                    }
                }
            ]
        });
        let url = self.url(&[":batchUpdate"], &[])?;
        self.call(Method::POST, url, Some(&body), "format header")
            .await
            .map(|_| ())
    }

    /// Write `rows` (each from [`row::order_row`], column A = order number):
    /// rows whose order number is already in column A are rewritten in place
    /// (one `values:batchUpdate`), the rest are appended (one `values:append`).
    /// Creates the tab and the header row when missing.
    pub async fn upsert_rows(&self, rows: &[Vec<Value>]) -> Result<UpsertOutcome, SheetsError> {
        let mut outcome = UpsertOutcome::default();
        if rows.is_empty() {
            return Ok(outcome);
        }
        let (header, keys, created_sheet) = self.read_index().await?;
        outcome.tab_created = created_sheet.is_some();
        if row::row_is_empty(&header) {
            self.write_header(created_sheet).await?;
            outcome.header_written = true;
        }
        let index = row::index_column_a(&keys);

        let last = row::last_column();
        let mut updates = Vec::new();
        let mut appends = Vec::new();
        for r in rows {
            let key = r
                .first()
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            match index.get(key) {
                Some(n) => updates.push(json!({
                    "range": self.a1(&format!("A{n}:{last}{n}")),
                    "majorDimension": "ROWS",
                    "values": [r],
                })),
                None => appends.push(r),
            }
        }

        if !updates.is_empty() {
            let url = self.url(&["values:batchUpdate"], &[])?;
            let body = json!({ "valueInputOption": "RAW", "data": updates });
            self.call(Method::POST, url, Some(&body), "update rows")
                .await?;
            outcome.updated = updates.len();
        }
        if !appends.is_empty() {
            let range = self.a1("A1");
            let url = self.url(
                &["values", &format!("{range}:append")],
                &[
                    ("valueInputOption", "RAW"),
                    ("insertDataOption", "INSERT_ROWS"),
                ],
            )?;
            let body = json!({ "majorDimension": "ROWS", "values": appends });
            self.call(Method::POST, url, Some(&body), "append rows")
                .await?;
            outcome.appended = appends.len();
        }
        Ok(outcome)
    }

    /// Token refresh + spreadsheet title + tab/header creation, for
    /// `google-sheets-check`.
    pub async fn ensure_ready(&self) -> Result<ReadyReport, SheetsError> {
        let meta = self.meta().await?;
        let (sheet_id, tab_created) = match self.find_tab(&meta) {
            Some(id) => (id, false),
            None => self.ensure_tab().await?,
        };
        let (header, keys) = self.read_header_and_keys().await?;
        let header_written = row::row_is_empty(&header);
        if header_written {
            self.write_header(Some(sheet_id)).await?;
        }
        Ok(ReadyReport {
            spreadsheet_title: meta.properties.title,
            tab: self.tab.clone(),
            tab_created,
            header_written,
            order_rows: row::index_column_a(&keys).len(),
        })
    }
}

/// Map a non-2xx Sheets API response to an error class.
pub fn classify_api_error(status: u16, body: &[u8], op: &str) -> SheetsError {
    let err = serde_json::from_slice::<GoogleErrorEnvelope>(body)
        .unwrap_or_default()
        .error;
    let message: String = format!("{op}: HTTP {status} {} {}", err.status, err.message)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_LOGGED_MESSAGE)
        .collect();
    match status {
        429 | 500..=599 => SheetsError::Transient(message),
        401 => SheetsError::AuthConfig(message),
        403 | 404 => SheetsError::Access { status, message },
        _ => SheetsError::Rejected { status, message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_quoting() {
        assert_eq!(quote_tab("Orders"), "'Orders'");
        assert_eq!(quote_tab("Ravi's Orders"), "'Ravi''s Orders'");
    }

    #[test]
    fn api_errors_classified() {
        let body = br#"{"error":{"code":403,"message":"The caller does not have permission","status":"PERMISSION_DENIED"}}"#;
        match classify_api_error(403, body, "read column A") {
            SheetsError::Access { status, message } => {
                assert_eq!(status, 403);
                assert!(message.contains("PERMISSION_DENIED"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            classify_api_error(404, b"", "x"),
            SheetsError::Access { status: 404, .. }
        ));
        assert!(matches!(
            classify_api_error(429, b"", "x"),
            SheetsError::Transient(_)
        ));
        assert!(matches!(
            classify_api_error(503, b"<html>", "x"),
            SheetsError::Transient(_)
        ));
        assert!(matches!(
            classify_api_error(400, b"{}", "x"),
            SheetsError::Rejected { status: 400, .. }
        ));
    }
}
