//! Opaque session token type.
//!
//! Tokens are 32 random bytes encoded as URL-safe base64 (no padding). The
//! HTTP layer uses these as the session cookie value and the lookup key for
//! whatever store (in-memory, sqlite, redis) it picks.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand_core::{OsRng, RngCore};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("invalid token format")]
    InvalidFormat,
}

/// A 32-byte opaque session token rendered as URL-safe base64 (43 chars).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generate a fresh token using the OS RNG.
    #[must_use]
    pub fn new() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Parse an existing token string. Returns `Err` if it doesn't decode to
    /// exactly 32 bytes.
    pub fn parse(s: &str) -> Result<Self, SessionError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|_| SessionError::InvalidFormat)?;
        if bytes.len() != 32 {
            return Err(SessionError::InvalidFormat);
        }
        Ok(Self(s.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionToken {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<str> for SessionToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn new_returns_43_char_base64() {
        let t = SessionToken::new();
        assert_eq!(t.as_str().len(), 43); // 32 bytes -> 43 url-safe-no-pad chars
        assert!(t
            .as_str()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn parse_round_trips() {
        let t = SessionToken::new();
        let back = SessionToken::parse(t.as_str()).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn parse_rejects_wrong_length() {
        let err = SessionToken::parse("YWJjZA").expect_err("too short");
        assert!(matches!(err, SessionError::InvalidFormat));
    }

    #[test]
    fn parse_rejects_invalid_base64() {
        let err = SessionToken::parse("***not valid***").expect_err("bad chars");
        assert!(matches!(err, SessionError::InvalidFormat));
    }

    #[test]
    fn tokens_are_unique() {
        let a = SessionToken::new();
        let b = SessionToken::new();
        assert_ne!(a, b);
    }
}
