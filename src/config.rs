//! Environment-driven configuration (SPEC §36).

use std::env;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub access_key: String,
    pub crypto_key: [u8; 32],
    pub max_attachment_bytes: u64,
    pub max_request_bytes: u64,
    pub max_content_bytes: usize,
    pub page_size: i64,
    pub session_ttl: Duration,
    pub cookie_secure: bool,
    pub listen_addr: String,
    pub data_dir: PathBuf,
    pub templates_dir: PathBuf,
    pub static_dir: PathBuf,
}

#[derive(Debug)]
pub enum ConfigError {
    Missing(&'static str),
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Missing(k) => write!(f, "missing required environment variable: {k}"),
            ConfigError::Invalid(m) => write!(f, "invalid configuration: {m}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let access_key = env::var("ACCESS_KEY").map_err(|_| ConfigError::Missing("ACCESS_KEY"))?;
        if access_key.len() < 16 {
            return Err(ConfigError::Invalid(
                "ACCESS_KEY must be at least 16 characters".into(),
            ));
        }

        let crypto_key_hex =
            env::var("CRYPTO_KEY").map_err(|_| ConfigError::Missing("CRYPTO_KEY"))?;
        let crypto_key = parse_crypto_key(&crypto_key_hex)?;

        Ok(Config {
            access_key,
            crypto_key,
            max_attachment_bytes: parse_u64_env("MAX_ATTACHMENT_BYTES", 2 * 1024 * 1024)?,
            max_request_bytes: parse_u64_env("MAX_REQUEST_BYTES", 8 * 1024 * 1024)?,
            max_content_bytes: parse_u64_env("MAX_CONTENT_BYTES", 1024 * 1024)? as usize,
            page_size: parse_u64_env("PAGE_SIZE", 20)? as i64,
            session_ttl: parse_duration_env("SESSION_TTL", Duration::from_secs(7 * 24 * 3600))?,
            cookie_secure: parse_bool_env("COOKIE_SECURE", true)?,
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            data_dir: PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "./data".into())),
            templates_dir: PathBuf::from(
                env::var("TEMPLATES_DIR").unwrap_or_else(|_| "./templates".into()),
            ),
            static_dir: PathBuf::from(env::var("STATIC_DIR").unwrap_or_else(|_| "./static".into())),
        })
    }
}

/// CRYPTO_KEY: 64-character hex → 32 bytes (SPEC §8.1).
fn parse_crypto_key(s: &str) -> Result<[u8; 32], ConfigError> {
    let s = s.trim();
    if s.len() != 64 {
        return Err(ConfigError::Invalid(
            "CRYPTO_KEY must be 64 hex characters (32 bytes)".into(),
        ));
    }
    let bytes =
        hex::decode(s).map_err(|_| ConfigError::Invalid("CRYPTO_KEY is not valid hex".into()))?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn parse_u64_env(name: &str, default: u64) -> Result<u64, ConfigError> {
    match env::var(name) {
        Ok(v) => v
            .trim()
            .parse::<u64>()
            .map_err(|_| ConfigError::Invalid(format!("{name} must be a positive integer"))),
        Err(_) => Ok(default),
    }
}

fn parse_bool_env(name: &str, default: bool) -> Result<bool, ConfigError> {
    match env::var(name) {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Ok(true),
            "false" | "0" | "no" => Ok(false),
            _ => Err(ConfigError::Invalid(format!("{name} must be true/false"))),
        },
        Err(_) => Ok(default),
    }
}

/// Parses "7d" / "12h" / "30m" / plain seconds.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let multiplier = match unit {
        "d" | "D" => 86400,
        "h" | "H" => 3600,
        "m" | "M" => 60,
        "s" | "S" => 1,
        _ => {
            // no unit suffix → treat whole string as seconds
            return s.parse::<u64>().ok().map(Duration::from_secs);
        }
    };
    num.parse::<u64>()
        .ok()
        .map(|n| Duration::from_secs(n * multiplier))
}

fn parse_duration_env(name: &str, default: Duration) -> Result<Duration, ConfigError> {
    match env::var(name) {
        Ok(v) => parse_duration(&v)
            .ok_or_else(|| ConfigError::Invalid(format!("{name} must be like 7d/12h/30m/seconds"))),
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("7d"), Some(Duration::from_secs(7 * 86400)));
        assert_eq!(parse_duration("12h"), Some(Duration::from_secs(12 * 3600)));
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("45s"), Some(Duration::from_secs(45)));
        assert_eq!(parse_duration("90"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("bogus"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn crypto_key_validation() {
        assert!(parse_crypto_key(&"a".repeat(64)).is_ok());
        assert!(parse_crypto_key(&"g".repeat(64)).is_err());
        assert!(parse_crypto_key(&"a".repeat(63)).is_err());
        assert!(parse_crypto_key(&"a".repeat(65)).is_err());
    }
}
