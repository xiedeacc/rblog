//! Tiny in-memory IP rate limiter.
//!
//! Implements a fixed-window counter keyed by client IP: the window
//! starts at the first request, and the count resets after `period`
//! elapses since that anchor. Good enough for "no more than 5 comments
//! per minute per IP" without dragging in tower-governor.
//!
//! The store is `Send + Sync` and lives behind an `Arc<RwLock<_>>`. A
//! coarse periodic GC removes long-idle entries; in v1 the GC runs
//! opportunistically inside every lookup.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// Verdict returned by [`RateLimiter::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateVerdict {
    Allow { remaining: u32 },
    Reject { retry_after: Duration },
}

#[derive(Debug, Clone)]
struct Window {
    anchor: Instant,
    count: u32,
}

/// Per-IP fixed-window limiter. Cheap to clone (Arc inside).
#[derive(Debug, Clone)]
pub struct RateLimiter {
    inner: Arc<RwLock<HashMap<IpAddr, Window>>>,
    limit: u32,
    period: Duration,
}

impl RateLimiter {
    #[must_use]
    pub fn new(limit: u32, period: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            limit,
            period,
        }
    }

    /// Default for comment submissions: 5 / minute / IP.
    #[must_use]
    pub fn comments_default() -> Self {
        Self::new(5, Duration::from_secs(60))
    }

    pub fn check(&self, ip: IpAddr) -> RateVerdict {
        let now = Instant::now();
        let mut guard = self.inner.write();
        let verdict = {
            let entry = guard.entry(ip).or_insert(Window {
                anchor: now,
                count: 0,
            });
            if now.duration_since(entry.anchor) >= self.period {
                entry.anchor = now;
                entry.count = 0;
            }
            if entry.count < self.limit {
                entry.count += 1;
                RateVerdict::Allow {
                    remaining: self.limit - entry.count,
                }
            } else {
                let retry_after = self.period.saturating_sub(now.duration_since(entry.anchor));
                RateVerdict::Reject { retry_after }
            }
        };
        // Opportunistic GC: drop any entry whose window is older than 10×
        // the configured period. Avoids unbounded growth from long-lived
        // processes that see many unique IPs.
        if guard.len() > 512 {
            let stale = self.period * 10;
            guard.retain(|_, w| now.duration_since(w.anchor) < stale);
        }
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn allows_until_limit_then_rejects() {
        let rl = RateLimiter::new(3, Duration::from_secs(30));
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert!(matches!(rl.check(ip), RateVerdict::Allow { .. }));
        assert!(matches!(rl.check(ip), RateVerdict::Allow { .. }));
        assert!(matches!(rl.check(ip), RateVerdict::Allow { remaining: 0 }));
        assert!(matches!(rl.check(ip), RateVerdict::Reject { .. }));
    }

    #[test]
    fn distinct_ips_have_distinct_buckets() {
        let rl = RateLimiter::new(1, Duration::from_secs(30));
        let a = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let b = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        assert!(matches!(rl.check(a), RateVerdict::Allow { .. }));
        assert!(matches!(rl.check(a), RateVerdict::Reject { .. }));
        assert!(matches!(rl.check(b), RateVerdict::Allow { .. }));
    }
}
