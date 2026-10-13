//! Capability token for guest orders. Customers have no account, so the token
//! returned at checkout is what authorizes paying for and viewing *that* order
//! (order numbers are sequential and therefore guessable).
//!
//! `token = base64url(HMAC-SHA256(K, order_id))`, where `K` is derived from
//! `OTP_PEPPER` with a fixed domain-separation label. Nothing is stored: an
//! idempotent checkout replay can re-derive the same token, and rotating the
//! pepper invalidates all outstanding order links.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use secrecy::ExposeSecret;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::config::Config;

type HmacSha256 = Hmac<Sha256>;

const DOMAIN: &[u8] = b"zhimart/order-access-token/v1";
/// Encoded length of a 32-byte MAC in unpadded base64url.
pub const TOKEN_LEN: usize = 43;

fn mac(config: &Config, order_id: Uuid) -> [u8; 32] {
    // Derive a purpose-specific key so this MAC can never collide with OTP hashes.
    let mut kdf = HmacSha256::new_from_slice(config.otp_pepper.expose_secret().as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of any length"));
    kdf.update(DOMAIN);
    let key = kdf.finalize().into_bytes();

    let mut m = HmacSha256::new_from_slice(&key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts keys of any length"));
    m.update(order_id.as_bytes());
    m.finalize().into_bytes().into()
}

/// Token to hand to the customer for `order_id`.
pub fn issue(config: &Config, order_id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(mac(config, order_id))
}

/// Constant-time check of a presented token. Malformed input is simply `false`.
pub fn verify(config: &Config, order_id: Uuid, presented: &str) -> bool {
    if presented.len() != TOKEN_LEN {
        return false;
    }
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(presented) else {
        return false;
    };
    bytes.ct_eq(&mac(config, order_id)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pepper: &str) -> Config {
        let mut c = crate::config::test_support::config();
        c.otp_pepper = pepper.to_owned().into();
        c
    }

    #[test]
    fn roundtrip_and_rejections() {
        let c = cfg("pepper-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let id = Uuid::now_v7();
        let t = issue(&c, id);
        assert_eq!(t.len(), TOKEN_LEN);
        assert!(verify(&c, id, &t));
        assert!(
            !verify(&c, Uuid::now_v7(), &t),
            "token is bound to one order"
        );
        assert!(
            !verify(&cfg("pepper-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"), id, &t),
            "bound to the pepper"
        );
        assert!(!verify(&c, id, ""));
        assert!(!verify(&c, id, &t[..42]));
        assert!(!verify(&c, id, &"!".repeat(TOKEN_LEN)));
        let mut tampered = t.clone().into_bytes();
        tampered[0] = if tampered[0] == b'A' { b'B' } else { b'A' };
        assert!(!verify(
            &c,
            id,
            std::str::from_utf8(&tampered).unwrap_or_default()
        ));
    }
}
