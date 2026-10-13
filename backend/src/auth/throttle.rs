//! Keys for login throttling and privacy-preserving audit fingerprints.
//!
//! Failed attempts are throttled per `(normalized e-mail, client IP bucket)`.
//! The key is an HMAC under a sub-key derived from `OTP_PEPPER`, so the table
//! neither stores e-mail addresses nor lets anyone holding a DB dump test
//! guesses against it, and rows exist alike for registered and unknown e-mails.

use std::net::IpAddr;

use anyhow::anyhow;
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

use crate::auth::otp::mask_email;

const THROTTLE_LABEL: &[u8] = b"zhimart/login-throttle/v1";
const AUDIT_LABEL: &[u8] = b"zhimart/audit-email/v1";

fn hmac(key: &[u8], parts: &[&[u8]]) -> anyhow::Result<[u8; 32]> {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|e| anyhow!("hmac key: {e}"))?;
    for p in parts {
        mac.update(p);
    }
    Ok(mac.finalize().into_bytes().into())
}

/// Sub-key for a purpose, so one pepper never keys two different constructions.
fn subkey(pepper: &SecretString, label: &[u8]) -> anyhow::Result<[u8; 32]> {
    hmac(pepper.expose_secret().as_bytes(), &[label])
}

/// The client's IP bucket: the full IPv4 address (IPv4-mapped IPv6 included),
/// or the /64 for IPv6 (one subscriber typically controls a whole /64).
pub fn ip_bucket(ip: IpAddr) -> String {
    if ip.is_unspecified() {
        return "none".to_owned();
    }
    match ip {
        IpAddr::V4(v4) => format!("4:{v4}"),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => format!("4:{v4}"),
            None => {
                let s = v6.segments();
                format!("6:{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

/// Throttle key for `(normalized_email, ip bucket)`.
pub fn throttle_key(
    pepper: &SecretString,
    normalized_email: &str,
    ip: IpAddr,
) -> anyhow::Result<[u8; 32]> {
    let key = subkey(pepper, THROTTLE_LABEL)?;
    hmac(
        &key,
        &[
            normalized_email.as_bytes(),
            &[0u8],
            ip_bucket(ip).as_bytes(),
        ],
    )
}

/// Audit-safe description of an attempted (possibly unregistered) e-mail: the
/// masked form plus a keyed fingerprint, so repeated attempts can be correlated
/// without storing the raw address.
pub fn email_audit_fields(pepper: &SecretString, normalized_email: &str) -> serde_json::Value {
    let fingerprint = subkey(pepper, AUDIT_LABEL)
        .and_then(|k| hmac(&k, &[normalized_email.as_bytes()]))
        .map(|h| {
            h[..16]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        })
        .unwrap_or_default();
    serde_json::json!({ "emailMasked": mask_email(normalized_email), "emailFingerprint": fingerprint })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn pepper() -> SecretString {
        SecretString::from("test-pepper-0123456789abcdefghijklmnop")
    }

    #[test]
    fn buckets() {
        assert_eq!(
            ip_bucket(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
            "4:203.0.113.7"
        );
        assert_eq!(
            ip_bucket("::ffff:203.0.113.7".parse().expect("ip")),
            "4:203.0.113.7"
        );
        assert_eq!(
            ip_bucket("2001:db8:1:2:aaaa::1".parse().expect("ip")),
            "6:2001:db8:1:2::/64"
        );
        assert_eq!(
            ip_bucket("2001:db8:1:2:aaaa::1".parse().expect("ip")),
            ip_bucket("2001:db8:1:2:ffff:ffff:ffff:ffff".parse().expect("ip"))
        );
        assert_eq!(ip_bucket(IpAddr::V6(Ipv6Addr::UNSPECIFIED)), "none");
    }

    #[test]
    fn keys_depend_on_email_bucket_and_pepper() {
        let ip: IpAddr = "203.0.113.7".parse().expect("ip");
        let other_ip: IpAddr = "203.0.113.8".parse().expect("ip");
        let k = throttle_key(&pepper(), "a@b.c", ip).expect("key");
        assert_eq!(k, throttle_key(&pepper(), "a@b.c", ip).expect("key"));
        assert_ne!(k, throttle_key(&pepper(), "a@b.d", ip).expect("key"));
        assert_ne!(k, throttle_key(&pepper(), "a@b.c", other_ip).expect("key"));
        let other = SecretString::from("another-pepper-0123456789abcdefghijkl");
        assert_ne!(k, throttle_key(&other, "a@b.c", ip).expect("key"));
    }

    #[test]
    fn audit_fields_hide_the_address() {
        let v = email_audit_fields(&pepper(), "someone@example.com");
        assert_eq!(v["emailMasked"], "s***e@example.com");
        let fp = v["emailFingerprint"].as_str().expect("fp");
        assert_eq!(fp.len(), 32);
        assert!(!v.to_string().contains("someone"));
    }
}
