//! Cashfree webhook signature verification and gateway id validation.
//!
//! * Webhook signature (`x-webhook-signature`): standard base64 of
//!   `HMAC_SHA256(key = CASHFREE_SECRET_KEY, msg = x-webhook-timestamp ‖ raw_body)`
//!   (plain concatenation, no separator; the timestamp is the header value as
//!   sent, epoch milliseconds).
//!
//! The signature is decoded strictly (exactly 44 characters of padded standard
//! base64 encoding exactly 32 bytes; no whitespace, no URL-safe alphabet) and
//! compared in constant time over the EXACT raw bytes received — never a
//! re-serialised body. A valid signature only makes a delivery worth looking
//! at: the payment service re-reads the authoritative state from the Cashfree
//! API before changing anything, so a replayed (old but genuine) delivery can
//! never move money or state on its own.

use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Length of a padded standard-base64 SHA-256 MAC.
pub const SIGNATURE_B64_LEN: usize = 44;
/// `x-webhook-timestamp`: 1-20 ASCII digits.
const MAX_TIMESTAMP_DIGITS: usize = 20;

/// `[A-Za-z0-9_-]{min..=max}`.
fn is_token(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Any id we put in a Cashfree URL path or store in a gateway id column:
/// `^[A-Za-z0-9_-]{1,64}$` (the DB checks of migration 000013).
pub fn is_gateway_id(s: &str) -> bool {
    is_token(s, 1, 64)
}

/// Cashfree order id (ours, see `cashfree::gateway_order_id`):
/// `^[A-Za-z0-9_-]{3,64}$` (Cashfree itself caps it at 45).
pub fn is_gateway_order_id(s: &str) -> bool {
    is_token(s, 3, 64)
}

/// Cashfree `cf_payment_id` / `cf_refund_id`, stored as a string:
/// `^[A-Za-z0-9_-]{1,64}$` (digits in practice).
pub fn is_gateway_payment_id(s: &str) -> bool {
    is_gateway_id(s)
}

/// See [`is_gateway_payment_id`].
pub fn is_gateway_refund_id(s: &str) -> bool {
    is_gateway_id(s)
}

/// Lowercase hex encoding.
pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// Strictly decode a base64 signature into 32 bytes. Anything else (wrong
/// length, whitespace, missing padding, URL-safe alphabet, non-canonical
/// trailing bits) is `None`.
pub fn decode_signature(s: &str) -> Option<[u8; 32]> {
    if s.len() != SIGNATURE_B64_LEN {
        return None;
    }
    let bytes = STANDARD.decode(s).ok()?;
    bytes.try_into().ok()
}

fn is_timestamp(ts: &str) -> bool {
    (1..=MAX_TIMESTAMP_DIGITS).contains(&ts.len()) && ts.bytes().all(|b| b.is_ascii_digit())
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of any length"));
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// Base64 signature Cashfree sends for `(timestamp, body)`.
pub fn webhook_signature(secret_key: &[u8], timestamp: &str, body: &[u8]) -> String {
    STANDARD.encode(hmac_sha256(secret_key, &[timestamp.as_bytes(), body]))
}

/// Verify `x-webhook-signature` for `x-webhook-timestamp` over the exact raw
/// request body. An empty secret never verifies anything.
pub fn verify_webhook(
    secret_key: &[u8],
    timestamp: &str,
    body: &[u8],
    signature_b64: &str,
) -> bool {
    if secret_key.is_empty() || !is_timestamp(timestamp) {
        return false;
    }
    let Some(presented) = decode_signature(signature_b64) else {
        return false;
    };
    let expected = hmac_sha256(secret_key, &[timestamp.as_bytes(), body]);
    presented.ct_eq(&expected).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"cfsk_ma_test_0123456789abcdef";
    const TS: &str = "1760950000123";

    /// Independent computation with the hmac crate directly.
    fn reference_b64(key: &[u8], msg: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(key).unwrap_or_else(|_| unreachable!());
        mac.update(msg);
        STANDARD.encode(mac.finalize().into_bytes())
    }

    #[test]
    fn rfc4231_vector() {
        // RFC 4231 test case 2.
        let mac = hmac_sha256(b"Jefe", &[b"what do ya want for nothing?"]);
        assert_eq!(
            hex_encode(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn webhook_signature_roundtrip_and_tampering() {
        let body =
            br#"{"data":{"order":{"order_id":"ZM-1_abc"}},"type":"PAYMENT_SUCCESS_WEBHOOK"}"#;
        let mut msg = TS.as_bytes().to_vec();
        msg.extend_from_slice(body);
        let good = reference_b64(KEY, &msg);
        assert_eq!(good.len(), SIGNATURE_B64_LEN);
        assert_eq!(webhook_signature(KEY, TS, body), good);
        assert!(verify_webhook(KEY, TS, body, &good));

        // Body, timestamp or key changed.
        let mut spaced = body.to_vec();
        spaced.push(b' ');
        assert!(!verify_webhook(KEY, TS, &spaced, &good));
        assert!(!verify_webhook(KEY, "1760950000124", body, &good));
        assert!(!verify_webhook(
            b"cfsk_ma_test_another_secret",
            TS,
            body,
            &good
        ));
        assert!(!verify_webhook(b"", TS, body, &reference_b64(b"", &msg)));
        // A signature over the body alone (no timestamp) must not verify.
        assert!(!verify_webhook(KEY, TS, body, &reference_b64(KEY, body)));
        // With a separator ("ts.body") must not verify either.
        let mut dotted = TS.as_bytes().to_vec();
        dotted.push(b'.');
        dotted.extend_from_slice(body);
        assert!(!verify_webhook(KEY, TS, body, &reference_b64(KEY, &dotted)));

        // Strict decoding.
        assert!(!verify_webhook(KEY, TS, body, ""));
        assert!(!verify_webhook(KEY, TS, body, &good[..43]));
        assert!(!verify_webhook(KEY, TS, body, &format!("{good}=")));
        assert!(!verify_webhook(KEY, TS, body, &format!(" {}", &good[1..])));
        assert!(!verify_webhook(KEY, TS, body, &"A".repeat(44)));
        // Hex (Razorpay style) is not accepted.
        assert!(!verify_webhook(KEY, TS, body, &hex_encode(&[0u8; 32])));

        // Malformed timestamps.
        for ts in ["", "17609x0000123", " 1760950000123", "-1", &"9".repeat(21)] {
            assert!(
                !verify_webhook(KEY, ts, body, &webhook_signature(KEY, ts, body)),
                "{ts:?}"
            );
        }
    }

    #[test]
    fn decode_is_strict() {
        let sig = STANDARD.encode([7u8; 32]);
        assert_eq!(decode_signature(&sig), Some([7u8; 32]));
        assert_eq!(decode_signature(&STANDARD.encode([7u8; 31])), None);
        assert_eq!(decode_signature(&sig.replace('=', "A")), None);
        // URL-safe alphabet is not standard base64.
        let std_sig = STANDARD.encode([0xfbu8; 32]);
        assert!(std_sig.contains('+') || std_sig.contains('/'));
        assert_eq!(decode_signature(&std_sig), Some([0xfbu8; 32]));
        let url_safe = base64::engine::general_purpose::URL_SAFE.encode([0xfbu8; 32]);
        assert_eq!(decode_signature(&url_safe), None);
        // Non-canonical final symbol (trailing bits set) is rejected.
        // `[7; 32]` encodes to "...Bwc=": the last symbol carries 2 unused bits.
        let mut bad = sig.into_bytes();
        bad[42] = b'x';
        let bad = String::from_utf8(bad).unwrap_or_default();
        assert_eq!(decode_signature(&bad), None);
    }

    #[test]
    fn id_formats() {
        assert!(is_gateway_order_id("ZM-261020-1001_8f3a2b1c9d0e"));
        assert!(
            is_gateway_order_id("order_DBJOWzybf0sJbb"),
            "legacy Razorpay ids still valid"
        );
        assert!(!is_gateway_order_id("ab"));
        assert!(!is_gateway_order_id(&"a".repeat(65)));
        assert!(is_gateway_order_id(&"a".repeat(64)));
        assert!(!is_gateway_order_id("ZM-1/../../x"));
        assert!(!is_gateway_order_id("ZM-1 1"));
        assert!(!is_gateway_order_id("ZM-1\n"));
        assert!(is_gateway_payment_id("5114910253538"));
        assert!(is_gateway_refund_id("11325632"));
        assert!(!is_gateway_payment_id(""));
        assert!(!is_gateway_payment_id("ünicode"));
    }

    #[test]
    fn hex_encoding() {
        assert_eq!(hex_encode(&[0x00, 0xab, 0xff]), "00abff");
    }
}
