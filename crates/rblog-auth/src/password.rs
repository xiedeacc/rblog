//! Halo-compatible password hashing.
//!
//! New Halo installs use Spring Security's delegating password encoder with
//! Argon2id as the default. Older installs, including some imported dumps, may
//! carry `{bcrypt}` hashes. rblog produces Argon2id PHC strings for new
//! passwords and verifies both formats so Halo User records migrate unchanged.
//!
//! Argon2id hashes are standard PHC-format strings like
//! `$argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>`. The `argon2` crate uses
//! the same PHC format.

use argon2::{Algorithm, Argon2, Params, Version};
use password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher as _, PasswordVerifier, SaltString,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("password hashing failed: {0}")]
    Hash(String),
    #[error("password verification failed: {0}")]
    Verify(String),
    #[error("invalid encoded hash")]
    InvalidEncoded,
}

/// Wraps an [`Argon2`] hasher pinned to argon2id v1.3 with parameters that
/// match Spring Security's default for compatibility with existing Halo
/// installs (`m=65536, t=3, p=4`).
#[derive(Clone)]
pub struct PasswordHasher {
    argon2: Argon2<'static>,
}

impl PasswordHasher {
    /// Construct with the same parameters Spring Security ships by default:
    /// `Argon2id`, v1.3, `m_cost=65536 KiB`, `t_cost=3 iterations`,
    /// `p_cost=4 lanes`. Output length 32 bytes.
    pub fn new() -> Self {
        let params =
            Params::new(65_536, 3, 4, Some(32)).expect("static argon2 parameters must be valid");
        Self {
            argon2: Argon2::new(Algorithm::Argon2id, Version::V0x13, params),
        }
    }

    /// Hash `plain` and return the PHC-encoded string.
    pub fn hash(&self, plain: &str) -> Result<String, PasswordError> {
        let salt = SaltString::generate(&mut OsRng);
        self.argon2
            .hash_password(plain.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| PasswordError::Hash(e.to_string()))
    }

    /// Verify `plain` against a PHC-encoded `encoded` hash.
    ///
    /// Returns `Ok(true)` on a match, `Ok(false)` on a clean mismatch, and
    /// [`PasswordError::InvalidEncoded`] if the encoded string cannot be
    /// parsed (e.g. came from a different algorithm).
    pub fn verify(&self, plain: &str, encoded: &str) -> Result<bool, PasswordError> {
        let encoded = strip_spring_id(encoded, "argon2@SpringSecurity_v5_8")
            .or_else(|| strip_spring_id(encoded, "argon2"))
            .unwrap_or(encoded);
        if let Some(hash) = strip_spring_id(encoded, "bcrypt") {
            return bcrypt::verify(plain, hash).map_err(|e| PasswordError::Verify(e.to_string()));
        }

        let parsed = PasswordHash::new(encoded).map_err(|_| PasswordError::InvalidEncoded)?;
        match self.argon2.verify_password(plain.as_bytes(), &parsed) {
            Ok(()) => Ok(true),
            Err(password_hash::Error::Password) => Ok(false),
            Err(e) => Err(PasswordError::Verify(e.to_string())),
        }
    }

    /// Returns true when a stored hash should be upgraded after a successful
    /// login. Imported bcrypt hashes verify for migration, but new persisted
    /// credentials should be Argon2id PHC strings with the current parameters.
    #[must_use]
    pub fn needs_rehash(&self, encoded: &str) -> bool {
        let encoded = strip_spring_id(encoded, "argon2@SpringSecurity_v5_8")
            .or_else(|| strip_spring_id(encoded, "argon2"))
            .unwrap_or(encoded);
        let Ok(parsed) = PasswordHash::new(encoded) else {
            return true;
        };
        if parsed.algorithm.as_str() != "argon2id" {
            return true;
        }
        let params = &parsed.params;
        params.get("m").and_then(|v| v.decimal().ok()) != Some(65_536)
            || params.get("t").and_then(|v| v.decimal().ok()) != Some(3)
            || params.get("p").and_then(|v| v.decimal().ok()) != Some(4)
    }
}

fn strip_spring_id<'a>(encoded: &'a str, id: &str) -> Option<&'a str> {
    encoded
        .strip_prefix('{')?
        .strip_prefix(id)?
        .strip_prefix('}')
}

impl Default for PasswordHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_format_matches_spring_security() {
        let h = PasswordHasher::new();
        let s = h.hash("hunter2").unwrap();
        assert!(s.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"), "got: {s}");
    }

    #[test]
    fn verify_round_trip() {
        let h = PasswordHasher::new();
        let s = h.hash("correct horse battery staple").unwrap();
        assert!(h.verify("correct horse battery staple", &s).unwrap());
        assert!(!h.verify("wrong", &s).unwrap());
    }

    #[test]
    fn invalid_encoded_returns_error() {
        let h = PasswordHasher::new();
        let err = h.verify("x", "not-a-hash").expect_err("must fail");
        assert!(matches!(err, PasswordError::InvalidEncoded));
    }

    #[test]
    fn verifies_halo_bcrypt_hash() {
        let h = PasswordHasher::new();
        let (password, hash) = halo_bcrypt_fixture();
        assert!(h.verify(&password, &hash).unwrap());
        assert!(!h.verify("wrong", &hash).unwrap());
    }

    #[test]
    fn bcrypt_needs_rehash_after_successful_login() {
        let h = PasswordHasher::new();
        let (_password, hash) = halo_bcrypt_fixture();
        assert!(h.needs_rehash(&hash));
        let argon = h.hash("same").unwrap();
        assert!(!h.needs_rehash(&argon));
    }

    fn halo_bcrypt_fixture() -> (String, String) {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace root")
            .join("rblog.local.toml");
        if let Ok(config) = std::fs::read_to_string(local) {
            let password = read_local_test_value(&config, "halo_bcrypt_password");
            let hash = read_local_test_value(&config, "halo_bcrypt_hash");
            if let (Some(password), Some(hash)) = (password, hash) {
                return (password, hash);
            }
        }
        (
            "halo-test-password".to_owned(),
            "{bcrypt}$2b$10$L8DdL/vXsA/6lel3T/Q.xOp.qxyBcVtFOeEhFctSyEJuWG.dyXU7W".to_owned(),
        )
    }

    fn read_local_test_value(config: &str, key: &str) -> Option<String> {
        config.lines().find_map(|line| {
            let line = line.trim();
            let (found, value) = line.split_once('=')?;
            if found.trim() != key {
                return None;
            }
            Some(value.trim().trim_matches('"').to_owned())
        })
    }

    #[test]
    fn hash_uses_unique_salt_each_time() {
        let h = PasswordHasher::new();
        let a = h.hash("same").unwrap();
        let b = h.hash("same").unwrap();
        assert_ne!(a, b, "salts must differ between hashes");
        assert!(h.verify("same", &a).unwrap());
        assert!(h.verify("same", &b).unwrap());
    }
}
