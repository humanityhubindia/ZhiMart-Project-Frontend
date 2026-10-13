//! Email one-time codes. Only `HMAC-SHA256(OTP_PEPPER, challenge_id || code)` is
//! stored, so a database leak alone does not reveal live codes, and a hash
//! cannot be replayed against a different challenge.

use anyhow::anyhow;
use hmac::{Hmac, Mac};
use rand::{Rng, rngs::OsRng};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub const CODE_LEN: usize = 6;
const CODE_SPACE: u32 = 1_000_000;

/// A uniformly random 6-digit code (leading zeros preserved).
pub fn generate_code() -> SecretString {
    let n: u32 = OsRng.gen_range(0..CODE_SPACE);
    SecretString::from(format!("{n:06}"))
}

/// True iff `code` is exactly six ASCII digits. Checked before any other work.
pub fn is_valid_code_format(code: &str) -> bool {
    code.len() == CODE_LEN && code.bytes().all(|b| b.is_ascii_digit())
}

/// `HMAC-SHA256(key = pepper, msg = challenge_id bytes || code)`.
pub fn hash_code(
    pepper: &SecretString,
    challenge_id: Uuid,
    code: &str,
) -> anyhow::Result<[u8; 32]> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pepper.expose_secret().as_bytes())
        .map_err(|e| anyhow!("otp hmac key: {e}"))?;
    mac.update(challenge_id.as_bytes());
    mac.update(code.as_bytes());
    Ok(mac.finalize().into_bytes().into())
}

/// Constant-time comparison of a submitted code against the stored hash.
pub fn verify_code(
    pepper: &SecretString,
    challenge_id: Uuid,
    code: &str,
    stored_hash: &[u8],
) -> anyhow::Result<bool> {
    let computed = hash_code(pepper, challenge_id, code)?;
    Ok(bool::from(computed.as_slice().ct_eq(stored_hash)))
}

/// `admin@zhimart.com` -> `a***n@zhimart.com`. Local parts of one or two
/// characters only reveal the first character.
pub fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.rsplit_once('@') else {
        return "***".to_owned();
    };
    let mut chars = local.chars();
    let first = chars.next();
    let count = local.chars().count();
    match (first, count) {
        (None, _) => format!("***@{domain}"),
        (Some(f), 1 | 2) => format!("{f}***@{domain}"),
        (Some(f), _) => {
            let last = local.chars().next_back().unwrap_or(f);
            format!("{f}***{last}@{domain}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pepper() -> SecretString {
        SecretString::from("test-pepper-0123456789abcdefghijklmnop")
    }

    #[test]
    fn code_format() {
        for _ in 0..2_000 {
            let c = generate_code();
            assert!(
                is_valid_code_format(c.expose_secret()),
                "{}",
                c.expose_secret()
            );
        }
    }

    #[test]
    fn code_distribution_is_roughly_uniform() {
        // Leading digit buckets should each get ~10% of 20k samples.
        let mut buckets = [0u32; 10];
        let mut saw_leading_zero = false;
        for _ in 0..20_000 {
            let c = generate_code();
            let first = c.expose_secret().as_bytes()[0] - b'0';
            saw_leading_zero |= first == 0;
            buckets[first as usize] += 1;
        }
        assert!(saw_leading_zero, "zero-padding must be preserved");
        for (d, n) in buckets.iter().enumerate() {
            assert!((1_600..=2_400).contains(n), "digit {d} appeared {n} times");
        }
    }

    #[test]
    fn format_validation() {
        assert!(is_valid_code_format("000000"));
        assert!(is_valid_code_format("123456"));
        assert!(!is_valid_code_format("12345"));
        assert!(!is_valid_code_format("1234567"));
        assert!(!is_valid_code_format("12a456"));
        assert!(!is_valid_code_format(" 23456"));
        assert!(!is_valid_code_format("١٢٣٤٥٦")); // non-ASCII digits
        assert!(!is_valid_code_format(""));
    }

    #[test]
    fn hmac_verify() {
        let id = Uuid::now_v7();
        let h = hash_code(&pepper(), id, "123456").expect("hash");
        assert!(verify_code(&pepper(), id, "123456", &h).expect("verify"));
        assert!(!verify_code(&pepper(), id, "123457", &h).expect("verify"));
        // Bound to the challenge id.
        assert!(!verify_code(&pepper(), Uuid::now_v7(), "123456", &h).expect("verify"));
        // Bound to the pepper.
        let other = SecretString::from("another-pepper-0123456789abcdefghijkl");
        assert!(!verify_code(&other, id, "123456", &h).expect("verify"));
        // Length mismatch never matches (and does not panic).
        assert!(!verify_code(&pepper(), id, "123456", &h[..31]).expect("verify"));
        assert!(!verify_code(&pepper(), id, "123456", &[]).expect("verify"));
    }

    #[test]
    fn mask() {
        assert_eq!(mask_email("admin@zhimart.com"), "a***n@zhimart.com");
        assert_eq!(mask_email("ab@zhimart.com"), "a***@zhimart.com");
        assert_eq!(mask_email("a@zhimart.com"), "a***@zhimart.com");
        assert_eq!(mask_email("abc@x.io"), "a***c@x.io");
        assert_eq!(mask_email("@x.io"), "***@x.io");
        assert_eq!(mask_email("no-at-sign"), "***");
        assert_eq!(mask_email("élodie@ex.fr"), "é***e@ex.fr");
    }
}
