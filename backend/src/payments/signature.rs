//! Razorpay signature verification and identifier validation.
//!
//! * Checkout handler signature: hex `HMAC_SHA256(order_id + "|" + payment_id, key_secret)`.
//! * Webhook signature (`X-Razorpay-Signature`): hex `HMAC_SHA256(raw_body, webhook_secret)`.
//!
//! Signatures are decoded strictly (exactly 64 hex digits) and compared in
//! constant time. A valid checkout signature only proves the browser relayed
//! values Razorpay produced; it is never by itself a reason to mark an order
//! paid (the payment service always confirms with the Razorpay API).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Length of a hex-encoded SHA-256 MAC.
pub const SIGNATURE_HEX_LEN: usize = 64;

/// `true` for `<prefix><6..=40 ASCII alphanumerics>`, e.g. `order_N3xYz12ABCdef`.
fn is_prefixed_id(s: &str, prefix: &str) -> bool {
    s.strip_prefix(prefix).is_some_and(|rest| {
        (6..=40).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// Razorpay order id: `^order_[A-Za-z0-9]{6,40}$`.
pub fn is_order_id(s: &str) -> bool {
    is_prefixed_id(s, "order_")
}

/// Razorpay payment id: `^pay_[A-Za-z0-9]{6,40}$`.
pub fn is_payment_id(s: &str) -> bool {
    is_prefixed_id(s, "pay_")
}

/// Razorpay refund id: `^rfnd_[A-Za-z0-9]{6,40}$`.
pub fn is_refund_id(s: &str) -> bool {
    is_prefixed_id(s, "rfnd_")
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

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Strictly decode a 64-digit hex signature into 32 bytes. Anything else
/// (wrong length, whitespace, non-hex) is `None`.
pub fn decode_signature(s: &str) -> Option<[u8; 32]> {
    let bytes = s.as_bytes();
    if bytes.len() != SIGNATURE_HEX_LEN {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        out[i] = (hex_val(pair[0])? << 4) | hex_val(pair[1])?;
    }
    Some(out)
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of any length"));
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

fn ct_matches(expected: &[u8; 32], presented_hex: &str) -> bool {
    match decode_signature(presented_hex) {
        Some(presented) => presented.ct_eq(expected).into(),
        None => false,
    }
}

/// Hex signature Razorpay Checkout returns for `(order_id, payment_id)`.
pub fn checkout_signature(key_secret: &[u8], order_id: &str, payment_id: &str) -> String {
    hex_encode(&hmac_sha256(
        key_secret,
        &[order_id.as_bytes(), b"|", payment_id.as_bytes()],
    ))
}

/// Verify the Checkout `razorpay_signature`. The ids must already be
/// well-formed; malformed ids are rejected here as well.
pub fn verify_checkout(
    key_secret: &[u8],
    order_id: &str,
    payment_id: &str,
    signature_hex: &str,
) -> bool {
    if !is_order_id(order_id) || !is_payment_id(payment_id) {
        return false;
    }
    let expected = hmac_sha256(
        key_secret,
        &[order_id.as_bytes(), b"|", payment_id.as_bytes()],
    );
    ct_matches(&expected, signature_hex)
}

/// Hex signature of a webhook body.
pub fn webhook_signature(webhook_secret: &[u8], body: &[u8]) -> String {
    hex_encode(&hmac_sha256(webhook_secret, &[body]))
}

/// Verify `X-Razorpay-Signature` over the exact raw request body.
pub fn verify_webhook(webhook_secret: &[u8], body: &[u8], signature_hex: &str) -> bool {
    let expected = hmac_sha256(webhook_secret, &[body]);
    ct_matches(&expected, signature_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"test_key_secret_0123456789";
    const OID: &str = "order_DBJOWzybf0sJbb";
    const PID: &str = "pay_DESlfW9H8K9uqM";

    /// Independent computation with the hmac crate directly.
    fn reference_hex(key: &[u8], msg: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(key).unwrap_or_else(|_| unreachable!());
        mac.update(msg);
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
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
    fn checkout_signature_roundtrip_and_tampering() {
        let good = reference_hex(KEY, format!("{OID}|{PID}").as_bytes());
        assert_eq!(checkout_signature(KEY, OID, PID), good);
        assert!(verify_checkout(KEY, OID, PID, &good));
        assert!(
            verify_checkout(KEY, OID, PID, &good.to_ascii_uppercase()),
            "hex is case-insensitive"
        );

        // Wrong key, swapped/changed ids.
        assert!(!verify_checkout(
            b"another_secret_0123456789",
            OID,
            PID,
            &good
        ));
        assert!(!verify_checkout(KEY, "order_DBJOWzybf0sJbc", PID, &good));
        assert!(!verify_checkout(KEY, OID, "pay_DESlfW9H8K9uqN", &good));
        // A signature over the concatenation without the separator must not verify.
        let no_sep = reference_hex(KEY, format!("{OID}{PID}").as_bytes());
        assert!(!verify_checkout(KEY, OID, PID, &no_sep));

        // Flip one hex digit.
        let mut flipped = good.clone().into_bytes();
        flipped[10] = if flipped[10] == b'0' { b'1' } else { b'0' };
        assert!(!verify_checkout(
            KEY,
            OID,
            PID,
            std::str::from_utf8(&flipped).unwrap_or_default()
        ));

        // Strict decoding.
        assert!(!verify_checkout(KEY, OID, PID, ""));
        assert!(!verify_checkout(KEY, OID, PID, &good[..63]));
        assert!(!verify_checkout(KEY, OID, PID, &format!("{good}0")));
        assert!(!verify_checkout(KEY, OID, PID, &format!(" {}", &good[1..])));
        assert!(!verify_checkout(KEY, OID, PID, &"g".repeat(64)));

        // Malformed ids never verify, even with a matching MAC.
        let bad_ids = reference_hex(KEY, b"order_x|pay_y");
        assert!(!verify_checkout(KEY, "order_x", "pay_y", &bad_ids));
    }

    #[test]
    fn webhook_signature_roundtrip_and_tampering() {
        let secret = b"test_webhook_secret_0123456789";
        let body = br#"{"event":"payment.captured","payload":{}}"#;
        let good = reference_hex(secret, body);
        assert_eq!(webhook_signature(secret, body), good);
        assert!(verify_webhook(secret, body, &good));
        assert!(!verify_webhook(
            secret,
            br#"{"event":"payment.captured","payload":{} }"#,
            &good
        ));
        assert!(!verify_webhook(b"test_key_secret_0123456789", body, &good));
        assert!(!verify_webhook(secret, body, ""));
        assert!(!verify_webhook(secret, body, "zz"));
    }

    #[test]
    fn id_formats() {
        assert!(is_order_id(OID));
        assert!(is_payment_id(PID));
        assert!(is_refund_id("rfnd_FP8QHiV938haTz"));
        assert!(!is_order_id("order_"));
        assert!(!is_order_id("order_abc12"));
        assert!(!is_order_id(&format!("order_{}", "a".repeat(41))));
        assert!(is_order_id(&format!("order_{}", "a".repeat(40))));
        assert!(!is_order_id("order_abc-123"));
        assert!(!is_order_id("order_abc123\n"));
        assert!(!is_order_id(PID));
        assert!(!is_payment_id(OID));
        assert!(!is_payment_id("pay_ünicode1"));
        assert!(!is_refund_id("rfnd_ab"));
    }

    #[test]
    fn hex_roundtrip() {
        let bytes: Vec<u8> = (0u8..32).collect();
        let hex = hex_encode(&bytes);
        assert_eq!(decode_signature(&hex).map(|a| a.to_vec()), Some(bytes));
    }
}
