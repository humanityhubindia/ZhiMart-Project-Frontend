//! Argon2id password hashing (OWASP parameters: m = 19 MiB, t = 2, p = 1).
//!
//! Hashing is CPU- and memory-heavy by design, so every hash/verify runs on the
//! blocking pool and — for request paths — only while holding a permit from
//! [`AppState::hash_permits`], which bounds how many cores a login flood can pin.

use std::{sync::LazyLock, time::Duration};

use anyhow::anyhow;
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{
        self, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroizing;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

/// Argon2 memory cost in KiB (19 MiB).
const M_COST_KIB: u32 = 19_456;
const T_COST: u32 = 2;
const P_COST: u32 = 1;

/// Upper bound on password length (bytes) accepted anywhere. Keeps the hashing
/// input bounded; the login endpoint enforces the same limit.
pub const MAX_PASSWORD_BYTES: usize = 256;
/// Minimum password length (characters) for admin accounts.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// How long a request waits for a hashing permit before giving up with 503.
const PERMIT_TIMEOUT: Duration = Duration::from_secs(5);

fn argon2() -> anyhow::Result<Argon2<'static>> {
    let params =
        Params::new(M_COST_KIB, T_COST, P_COST, None).map_err(|e| anyhow!("argon2 params: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn hash_blocking(password: &[u8]) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2()?
        .hash_password(password, &salt)
        .map_err(|e| anyhow!("argon2 hash: {e}"))?;
    Ok(hash.to_string())
}

/// Verify `password` against a PHC string. The hash's own embedded parameters
/// are used, so hashes created with older parameters keep working.
fn verify_blocking(hash: &str, password: &[u8]) -> anyhow::Result<bool> {
    let parsed =
        PasswordHash::new(hash).map_err(|e| anyhow!("stored password hash is malformed: {e}"))?;
    match argon2()?.verify_password(password, &parsed) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(anyhow!("argon2 verify: {e}")),
    }
}

/// A hash of a random, never-disclosed password, used to make logins for
/// unknown e-mail addresses cost exactly as much as real ones.
static DUMMY_HASH: LazyLock<Option<String>> = LazyLock::new(|| {
    let mut random = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(random.as_mut());
    match hash_blocking(random.as_ref()) {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::error!(error = %e, "failed to build dummy password hash");
            None
        }
    }
});

/// Argon2id hash in PHC string format. Runs on the blocking pool.
/// (Used by the CLI; no request-level permit is needed there.)
pub async fn hash_password(password: SecretString) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || hash_blocking(password.expose_secret().as_bytes()))
        .await
        .map_err(|e| anyhow!("password hashing task failed: {e}"))?
}

async fn run_with_permit<F>(state: &AppState, f: F) -> AppResult<bool>
where
    F: FnOnce() -> anyhow::Result<bool> + Send + 'static,
{
    let permit = match tokio::time::timeout(
        PERMIT_TIMEOUT,
        state.hash_permits.clone().acquire_owned(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        Ok(Err(_closed)) => return Err(AppError::ServiceUnavailable),
        Err(_elapsed) => {
            tracing::warn!("timed out waiting for a password hashing permit");
            return Err(AppError::ServiceUnavailable);
        }
    };
    // The permit moves into the blocking task so it is held for the full
    // duration of the hash, even if the request future is dropped meanwhile.
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f()
    })
    .await
    .map_err(|e| AppError::Internal(anyhow!("password verification task failed: {e}")))?
    .map_err(AppError::Internal)
}

/// Constant-cost verification of `password` against the stored PHC `hash`.
pub async fn verify_password(
    state: &AppState,
    hash: &str,
    password: SecretString,
) -> AppResult<bool> {
    let hash = hash.to_owned();
    run_with_permit(state, move || {
        verify_blocking(&hash, password.expose_secret().as_bytes())
    })
    .await
}

/// Burn the same amount of work as a real verification, for unknown or
/// inactive accounts (anti-enumeration). Always "fails".
pub async fn dummy_verify(state: &AppState, password: SecretString) -> AppResult<()> {
    run_with_permit(state, move || {
        let pw = password.expose_secret().as_bytes();
        match DUMMY_HASH.as_deref() {
            Some(hash) => verify_blocking(hash, pw),
            // Equivalent cost: hashing with the same parameters.
            None => hash_blocking(pw).map(|_| false),
        }
    })
    .await
    .map(|_| ())
}

/// Minimum password policy for admin accounts (used by the CLI).
pub fn check_password_policy(password: &str) -> Result<(), &'static str> {
    const POLICY: &str = "password must be 12-256 characters (max 256 bytes), not only whitespace, \
                          and contain at least 3 of: lowercase letter, uppercase letter, digit, symbol";

    if password.len() > MAX_PASSWORD_BYTES || password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(POLICY);
    }
    if password.chars().all(char::is_whitespace) {
        return Err(POLICY);
    }
    if password.chars().any(char::is_control) {
        return Err("password must not contain control characters");
    }

    let (mut lower, mut upper, mut digit, mut symbol) = (false, false, false, false);
    for c in password.chars() {
        if c.is_lowercase() {
            lower = true;
        } else if c.is_uppercase() {
            upper = true;
        } else if c.is_ascii_digit() {
            digit = true;
        } else if !c.is_whitespace() {
            symbol = true;
        }
    }
    let classes = [lower, upper, digit, symbol]
        .into_iter()
        .filter(|&b| b)
        .count();
    if classes < 3 {
        return Err(POLICY);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_accepts_strong_passwords() {
        assert!(check_password_policy("Correct-Horse-Battery").is_ok());
        assert!(check_password_policy("abcdefgh1234!").is_ok());
        assert!(check_password_policy("ABCDEFGH1234abc").is_ok());
    }

    #[test]
    fn policy_rejects_weak_passwords() {
        assert!(check_password_policy("Short1!").is_err());
        assert!(check_password_policy("alllowercaseletters").is_err());
        assert!(check_password_policy("lowercase123456").is_err()); // 2 classes
        assert!(check_password_policy("                    ").is_err());
        assert!(check_password_policy(&"Aa1!".repeat(65)).is_err()); // 260 bytes
        assert!(check_password_policy("Abcdefgh123\u{0007}x").is_err());
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_blocking(b"Correct-Horse-Battery").expect("hash");
        assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_blocking(&hash, b"Correct-Horse-Battery").expect("verify"));
        assert!(!verify_blocking(&hash, b"wrong-password").expect("verify"));
        assert!(verify_blocking("not-a-phc-string", b"x").is_err());
    }

    #[test]
    fn salts_are_random() {
        let a = hash_blocking(b"same").expect("hash");
        let b = hash_blocking(b"same").expect("hash");
        assert_ne!(a, b);
    }

    #[test]
    fn dummy_hash_is_available() {
        assert!(DUMMY_HASH.is_some());
    }
}
