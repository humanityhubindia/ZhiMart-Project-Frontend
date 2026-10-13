//! Opaque refresh tokens: 256 random bits, base64url (no padding, 43 chars).
//! The server stores only SHA-256(token).

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const TOKEN_BYTES: usize = 32;
/// Encoded length of [`TOKEN_BYTES`] in unpadded base64url.
pub const TOKEN_LEN: usize = 43;

pub struct RefreshToken {
    /// Goes to the client (cookie) only.
    pub token: SecretString,
    /// Persisted.
    pub hash: [u8; 32],
}

pub fn generate() -> RefreshToken {
    let mut raw = Zeroizing::new([0u8; TOKEN_BYTES]);
    OsRng.fill_bytes(raw.as_mut());
    let token = SecretString::from(URL_SAFE_NO_PAD.encode(raw.as_ref()));
    let hash = hash_token(token.expose_secret());
    RefreshToken { token, hash }
}

pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Cheap syntactic check so garbage cookie values never reach the database.
pub fn is_well_formed(token: &str) -> bool {
    token.len() == TOKEN_LEN
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_well_formed_and_unique() {
        let a = generate();
        let b = generate();
        assert_eq!(a.token.expose_secret().len(), TOKEN_LEN);
        assert!(is_well_formed(a.token.expose_secret()));
        assert_ne!(a.token.expose_secret(), b.token.expose_secret());
        assert_eq!(
            URL_SAFE_NO_PAD
                .decode(a.token.expose_secret())
                .expect("b64")
                .len(),
            TOKEN_BYTES
        );
    }

    #[test]
    fn hash_matches_sha256() {
        let t = generate();
        assert_eq!(t.hash, hash_token(t.token.expose_secret()));
        assert_eq!(t.hash.len(), 32);
        // Known vector: SHA-256("abc").
        assert_eq!(hash_token("abc")[..4], [0xba, 0x78, 0x16, 0xbf],);
    }

    #[test]
    fn rejects_malformed() {
        assert!(!is_well_formed(""));
        assert!(!is_well_formed(&"a".repeat(42)));
        assert!(!is_well_formed(&"a".repeat(44)));
        assert!(!is_well_formed(&format!("{}=", "a".repeat(42))));
        assert!(!is_well_formed(&format!("{}+", "a".repeat(42))));
        assert!(!is_well_formed(&format!("{}/", "a".repeat(42))));
        assert!(!is_well_formed(&format!("{}é", "a".repeat(41))));
        assert!(is_well_formed(&format!("{}-_", "A".repeat(41))));
    }
}
