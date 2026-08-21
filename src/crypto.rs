//! AES-256-GCM encryption of record title + content (SPEC §8).
//!
//! Blob layout: 12-byte random nonce || ciphertext || 16-byte GCM tag.
//! Plaintext is JSON: {"title": "...", "content": "..."}.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Plaintext<'a> {
    title: &'a str,
    content: &'a str,
}

#[derive(Debug)]
pub struct DecryptError;

impl std::fmt::Display for DecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "decryption failed")
    }
}

impl std::error::Error for DecryptError {}

#[derive(Clone)]
pub struct Crypto {
    cipher: Aes256Gcm,
}

impl Crypto {
    pub fn new(key: &[u8; 32]) -> Self {
        Crypto {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)),
        }
    }

    /// Encrypt title+content together. Output: nonce || ciphertext+tag.
    pub fn encrypt(&self, title: &str, content: &str) -> Vec<u8> {
        let plain = Plaintext { title, content };
        let json = serde_json::to_vec(&plain).expect("serialization cannot fail");

        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &json,
                    aad: b"aardbin-record-v1",
                },
            )
            .expect("encryption cannot fail");

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        out
    }

    /// Decrypt a blob produced by `encrypt`. Errors on tamper / wrong key.
    pub fn decrypt(&self, blob: &[u8]) -> Result<(String, String), DecryptError> {
        if blob.len() < 12 + 16 {
            return Err(DecryptError);
        }
        let (nonce_bytes, ciphertext) = blob.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let json = self
            .cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: b"aardbin-record-v1",
                },
            )
            .map_err(|_| DecryptError)?;

        #[derive(Deserialize)]
        struct Owned {
            title: String,
            content: String,
        }
        let parsed: Owned = serde_json::from_slice(&json).map_err(|_| DecryptError)?;
        Ok((parsed.title, parsed.content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn roundtrip() {
        let c = Crypto::new(&key());
        let blob = c.encrypt("标题 with emoji 🔒", "line1\nline2");
        let (t, b) = c.decrypt(&blob).unwrap();
        assert_eq!(t, "标题 with emoji 🔒");
        assert_eq!(b, "line1\nline2");
    }

    #[test]
    fn empty_fields_roundtrip() {
        let c = Crypto::new(&key());
        let blob = c.encrypt("", "");
        let (t, b) = c.decrypt(&blob).unwrap();
        assert_eq!(t, "");
        assert_eq!(b, "");
    }

    #[test]
    fn nonces_differ() {
        let c = Crypto::new(&key());
        let a = c.encrypt("same", "same");
        let b = c.encrypt("same", "same");
        assert_ne!(a, b, "each save must use a fresh nonce");
        assert_eq!(a[..12].len(), 12);
    }

    #[test]
    fn tamper_detected() {
        let c = Crypto::new(&key());
        let mut blob = c.encrypt("t", "c");
        let n = blob.len();
        blob[n - 1] ^= 1;
        assert!(c.decrypt(&blob).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let c1 = Crypto::new(&key());
        let c2 = Crypto::new(&[8u8; 32]);
        let blob = c1.encrypt("t", "c");
        assert!(c2.decrypt(&blob).is_err());
    }

    #[test]
    fn truncated_fails() {
        let c = Crypto::new(&key());
        let blob = c.encrypt("t", "c");
        assert!(c.decrypt(&blob[..10]).is_err());
    }
}
