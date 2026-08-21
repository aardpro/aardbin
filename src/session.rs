//! Stateless HMAC-signed session cookie (PRD §7.2.1).
//!
//! Cookie value: `v1.{expiry_unix}.{hmac_hex}`
//! where hmac = HMAC-SHA256(SESSION_KEY, "v1.{expiry_unix}")
//! and   SESSION_KEY = SHA-256("aardbin-session-v1:" + ACCESS_KEY).
//!
//! Properties: survives restarts, invalidates on ACCESS_KEY change, unforgeable.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "aardbin_session";

#[derive(Clone)]
pub struct SessionManager {
    key: [u8; 32],
    ttl: Duration,
    cookie_secure: bool,
}

impl SessionManager {
    pub fn new(access_key: &str, ttl: Duration, cookie_secure: bool) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"aardbin-session-v1:");
        hasher.update(access_key.as_bytes());
        let key: [u8; 32] = hasher.finalize().into();
        SessionManager {
            key,
            ttl,
            cookie_secure,
        }
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn mac_for(&self, expiry: u64) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac accepts any key size");
        mac.update(format!("v1.{expiry}").as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Cookie value to hand to a freshly-authenticated client.
    pub fn issue(&self) -> String {
        let expiry = Self::now() + self.ttl.as_secs();
        format!("v1.{expiry}.{}", self.mac_for(expiry))
    }

    pub fn validate(&self, value: &str) -> bool {
        let mut parts = value.splitn(3, '.');
        let (Some("v1"), Some(expiry_s), Some(mac_hex)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Ok(expiry) = expiry_s.parse::<u64>() else {
            return false;
        };
        if expiry <= Self::now() {
            return false;
        }
        let expected = self.mac_for(expiry);
        expected.as_bytes().ct_eq(mac_hex.as_bytes()).into()
    }

    /// `Set-Cookie` header that establishes the session.
    pub fn issue_set_cookie(&self) -> String {
        let mut s = format!(
            "{COOKIE_NAME}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
            self.issue(),
            self.ttl.as_secs()
        );
        if self.cookie_secure {
            s.push_str("; Secure");
        }
        s
    }

    /// `Set-Cookie` header that immediately expires the session (logout).
    pub fn clear_set_cookie(&self) -> String {
        let mut s = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
        if self.cookie_secure {
            s.push_str("; Secure");
        }
        s
    }
}

/// Extract and validate the session cookie from a Cookie header value.
pub fn extract_valid_session(cookie_header: Option<&str>, mgr: &SessionManager) -> bool {
    let Some(header) = cookie_header else {
        return false;
    };
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix(&format!("{COOKIE_NAME}=")) {
            return mgr.validate(value);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> SessionManager {
        SessionManager::new("test-access-key-0123456789", Duration::from_secs(3600), true)
    }

    #[test]
    fn issue_and_validate() {
        let m = mgr();
        let v = m.issue();
        assert!(m.validate(&v));
    }

    #[test]
    fn tampered_mac_rejected() {
        let m = mgr();
        let v = m.issue();
        let mut bad = v.clone();
        let n = bad.len();
        let last = bad.chars().last().unwrap();
        let repl = if last == 'a' { 'b' } else { 'a' };
        bad.replace_range(n - 1.., &repl.to_string());
        assert!(!m.validate(&bad));
    }

    #[test]
    fn tampered_expiry_rejected() {
        let m = mgr();
        let v = m.issue();
        // extend expiry without re-signing → mac no longer matches
        let parts: Vec<&str> = v.splitn(3, '.').collect();
        let forged = format!(
            "v1.{}.{}",
            parts[1].parse::<u64>().unwrap() + 10,
            parts[2]
        );
        assert!(!m.validate(&forged));
    }

    #[test]
    fn wrong_access_key_rejected() {
        let m1 = mgr();
        let m2 = SessionManager::new("different-key-0123456789", Duration::from_secs(3600), true);
        assert!(!m2.validate(&m1.issue()));
    }

    #[test]
    fn expired_rejected() {
        let m = SessionManager::new("k-0123456789abcdef", Duration::from_secs(0), true);
        // ttl=0 → expiry == now, validate requires expiry > now
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(!m.validate(&m.issue()));
    }

    #[test]
    fn garbage_rejected() {
        let m = mgr();
        assert!(!m.validate(""));
        assert!(!m.validate("v1"));
        assert!(!m.validate("v1.abc.def"));
        assert!(!m.validate("v2.9999999999.aa"));
    }

    #[test]
    fn cookie_extraction() {
        let m = mgr();
        let v = m.issue();
        assert!(extract_valid_session(
            Some(&format!("other=x; {COOKIE_NAME}={v}; more=y")),
            &m
        ));
        assert!(!extract_valid_session(Some("other=x"), &m));
        assert!(!extract_valid_session(None, &m));
    }

    #[test]
    fn set_cookie_flags() {
        let secure = mgr().issue_set_cookie();
        assert!(secure.contains("HttpOnly"));
        assert!(secure.contains("SameSite=Lax"));
        assert!(secure.contains("Secure"));
        assert!(secure.contains("Max-Age=3600"));

        let insecure =
            SessionManager::new("k-0123456789abcdef", Duration::from_secs(60), false)
                .issue_set_cookie();
        assert!(!insecure.contains("Secure"));

        let cleared = mgr().clear_set_cookie();
        assert!(cleared.contains("Max-Age=0"));
    }
}
