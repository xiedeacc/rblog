//! Minimal in-memory session store.
//!
//! For v1 we keep sessions in process — the admin SPA expects a handful of
//! human users and reboots flush them out. Replacing this with a persistent
//! store (sqlite-backed via `tower-sessions`, or a future `Session`
//! Extension kind) is intentionally a single-trait swap.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rblog_auth::SessionToken;

#[derive(Clone, Debug)]
pub struct SessionRecord {
    pub token: SessionToken,
    pub user: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct SessionStore {
    inner: Arc<RwLock<HashMap<String, SessionRecord>>>,
}

impl SessionStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a brand new session for `user`. The returned [`SessionRecord`]
    /// contains the opaque token the HTTP layer drops into a cookie.
    pub fn create(&self, user: impl Into<String>, ttl: Duration) -> SessionRecord {
        let token = SessionToken::new();
        let expires_at = Utc::now()
            + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::days(14));
        let record = SessionRecord {
            token: token.clone(),
            user: user.into(),
            expires_at,
        };
        self.inner
            .write()
            .insert(token.as_str().to_owned(), record.clone());
        record
    }

    /// Look up an active, non-expired session.
    pub fn lookup(&self, token: &str) -> Option<SessionRecord> {
        let now = Utc::now();
        let mut guard = self.inner.write();
        let entry = guard.get(token).cloned()?;
        if entry.expires_at < now {
            guard.remove(token);
            None
        } else {
            Some(entry)
        }
    }

    pub fn revoke(&self, token: &str) {
        self.inner.write().remove(token);
    }

    /// Drop everything older than `now`. Cheap enough to call on every
    /// authenticated request, but for now we only invoke it from a periodic
    /// background task in the binary's main loop.
    pub fn gc(&self) {
        let now = Utc::now();
        self.inner.write().retain(|_, v| v.expires_at >= now);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[test]
    fn create_and_lookup_round_trip() {
        let store = SessionStore::new();
        let r = store.create("alice", StdDuration::from_secs(60));
        let got = store.lookup(r.token.as_str()).unwrap();
        assert_eq!(got.user, "alice");
    }

    #[test]
    fn expired_sessions_are_dropped_on_lookup() {
        let store = SessionStore::new();
        let r = store.create("alice", StdDuration::from_millis(1));
        std::thread::sleep(StdDuration::from_millis(5));
        assert!(store.lookup(r.token.as_str()).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn revoke_removes_session() {
        let store = SessionStore::new();
        let r = store.create("admin", StdDuration::from_secs(60));
        store.revoke(r.token.as_str());
        assert!(store.lookup(r.token.as_str()).is_none());
    }
}
