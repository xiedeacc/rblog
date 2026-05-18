//! Object key derivation.

use std::fmt::Write as _;

use chrono::{Datelike, Utc};
use sha2::{Digest, Sha256};

/// Sanitize a filename + derive a storage key of the form
/// `<group?>/<YYYY>/<MM>/<sha8>-<safe-name>`. Special characters in `name`
/// are replaced with `_`.
pub fn object_key(group: Option<&str>, name: &str, bytes: &[u8]) -> String {
    let now = Utc::now();
    let safe = sanitize(name);
    let sha = sha_prefix(bytes);
    let date = format!("{:04}/{:02}", now.year(), now.month());
    match group {
        Some(g) if !g.is_empty() => {
            let group = sanitize(g);
            format!("{group}/{date}/{sha}-{safe}")
        }
        _ => format!("{date}/{sha}-{safe}"),
    }
}

/// Hash + hex-encode + truncate to 8 chars for an object-key prefix.
pub fn sha_prefix(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(8);
    for b in digest.iter().take(4) {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_pattern_for_naked_filename() {
        let key = object_key(None, "Photo.JPG", b"hello");
        assert!(key.ends_with("-Photo.JPG"));
        assert_eq!(key.matches('/').count(), 2);
    }

    #[test]
    fn key_pattern_with_group() {
        let key = object_key(Some("photos"), "name with spaces.png", b"x");
        assert!(key.starts_with("photos/"));
        assert!(key.contains("name_with_spaces.png"));
    }

    #[test]
    fn sha_prefix_is_8_hex_chars() {
        assert_eq!(sha_prefix(b"hello").len(), 8);
    }
}
