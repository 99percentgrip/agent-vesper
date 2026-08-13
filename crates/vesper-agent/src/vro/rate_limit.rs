//! Provider rate-limit accounting (VRO-10, PRD §10.4).
//!
//! PRD §10.4 mandates the Budget Manager "account for provider rate limits"
//! and the VRO-10 directive specifies: a global or session-level rate-limit
//! tracker that catches HTTP 429 errors and halts the orchestrator with a
//! specific [`RateLimitExceeded`](vesper_domain::OutcomeStatus::RateLimitExceeded)
//! budget outcome instead of a generic network crash.
//!
//! ## Design
//!
//! [`RateLimitTracker`] is a small, thread-safe (`Arc<Mutex>`-free — interior
//! mutability via atomic counters) accounting struct:
//!
//! - The composition boundary (TUI / ACP host) constructs one tracker per
//!   session and shares it (via `Arc`) between the provider adapter and the
//!   orchestrator.
//! - The provider adapter calls [`RateLimitTracker::record_429`] whenever it
//!   observes an HTTP 429 response (with the optional `Retry-After` hint).
//! - The orchestrator's Generate-Verify-Repair loop consults
//!   [`RateLimitTracker::status`] before every Generate. When blocked, it
//!   returns [`OutcomeStatus::RateLimitExceeded`] with a clear risk note
//!   naming the remaining backoff window.
//! - [`RateLimitTracker::clear`] resets the tracker after the backoff window
//!   elapses, restoring normal operation.
//!
//! ## Zero-breakage
//!
//! The default tracker is [`RateLimitTracker::untracked`], which never blocks
//! — every call returns [`RateLimitStatus::Available`]. The orchestrator's
//! GVR loop consults the tracker unconditionally; with the default, behavior
//! is byte-identical to VRO-9.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Live rate-limit status the orchestrator consults before each Generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitStatus {
    /// No rate limit is in effect; the orchestrator may proceed.
    Available,
    /// A 429 was observed; the orchestrator must halt with
    /// [`RateLimitExceeded`](vesper_domain::OutcomeStatus::RateLimitExceeded).
    /// `unblock_at` is the wall-clock instant after which the tracker
    /// auto-clears (carried as `Option<Duration>` since `Instant` epoch, so
    /// it is portable across threads and processes).
    Blocked { retry_after_ms: Option<u64> },
}

/// Thread-safe provider rate-limit tracker (VRO-10, PRD §10.4).
///
/// Wraps a small amount of atomic state so a provider adapter (writing on
/// one task) and the orchestrator loop (reading on another) can share one
/// tracker via `Arc<RateLimitTracker>`. The state is:
///
/// - `blocked` — atomic bool set when a 429 is observed.
/// - `unblock_at_monotonic_ms` — atomic u64 carrying the monotonic-clock
///   millisecond instant after which `blocked` auto-clears. `0` means
///   "no deadline" (cleared only by an explicit [`Self::clear`] call).
/// - `observed_429_count` — atomic u64 diagnostic counter for telemetry.
///
/// The tracker uses the monotonic clock (via [`Instant::now`]) so it is
/// robust against wall-clock adjustments. All atomic operations use
/// `Ordering::SeqCst` because the read (orchestrator) and write (provider)
/// happen on different tasks and we need a strict happens-before edge.
#[derive(Debug)]
pub struct RateLimitTracker {
    blocked: AtomicBool,
    unblock_at_monotonic_ms: AtomicU64,
    observed_429_count: AtomicU64,
    /// Cached `Instant` of construction so monotonic deltas are computable
    /// without exposing the Instant across threads.
    epoch: Instant,
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::untracked()
    }
}

impl RateLimitTracker {
    /// Constructs a fresh, never-blocking tracker (the zero-breakage
    /// default). Every call to [`Self::status`] returns `Available`.
    #[must_use]
    pub fn untracked() -> Self {
        Self {
            blocked: AtomicBool::new(false),
            unblock_at_monotonic_ms: AtomicU64::new(0),
            observed_429_count: AtomicU64::new(0),
            epoch: Instant::now(),
        }
    }

    /// Wraps this tracker in an `Arc` for sharing between the provider
    /// adapter and the orchestrator.
    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::untracked())
    }

    /// Records an HTTP 429 observation. `retry_after_ms` is the optional
    /// `Retry-After` header (in milliseconds) the provider returned; when
    /// `None`, the tracker blocks indefinitely until [`Self::clear`] is
    /// called.
    ///
    /// Idempotent: calling this twice in quick succession refreshes the
    /// deadline to the latest observation.
    pub fn record_429(&self, retry_after_ms: Option<u64>) {
        self.observed_429_count.fetch_add(1, Ordering::SeqCst);
        self.blocked.store(true, Ordering::SeqCst);
        if let Some(ms) = retry_after_ms {
            let unblock_at = self.epoch.elapsed().as_millis() as u64 + ms;
            self.unblock_at_monotonic_ms
                .store(unblock_at, Ordering::SeqCst);
        } else {
            // No Retry-After hint — block until explicitly cleared.
            self.unblock_at_monotonic_ms.store(0, Ordering::SeqCst);
        }
    }

    /// Clears the rate-limit block. Called by the composition boundary
    /// after the backoff window has elapsed (or by an explicit user
    /// gesture to retry).
    pub fn clear(&self) {
        self.blocked.store(false, Ordering::SeqCst);
        self.unblock_at_monotonic_ms.store(0, Ordering::SeqCst);
    }

    /// Returns the live rate-limit status, auto-clearing the block if the
    /// deadline has elapsed.
    #[must_use]
    pub fn status(&self) -> RateLimitStatus {
        if !self.blocked.load(Ordering::SeqCst) {
            return RateLimitStatus::Available;
        }
        let deadline_ms = self.unblock_at_monotonic_ms.load(Ordering::SeqCst);
        if deadline_ms > 0 {
            let now_ms = self.epoch.elapsed().as_millis() as u64;
            if now_ms >= deadline_ms {
                // Deadline elapsed — auto-clear and report available.
                self.clear();
                return RateLimitStatus::Available;
            }
            let remaining = deadline_ms - now_ms;
            return RateLimitStatus::Blocked {
                retry_after_ms: Some(remaining),
            };
        }
        // Blocked with no deadline — block indefinitely.
        RateLimitStatus::Blocked {
            retry_after_ms: None,
        }
    }

    /// Number of 429 observations recorded since construction (telemetry).
    #[must_use]
    pub fn observed_429_count(&self) -> u64 {
        self.observed_429_count.load(Ordering::SeqCst)
    }

    /// Returns `true` if the tracker is currently blocking new requests.
    /// Equivalent to `self.status().is_blocked()` but cheaper (no
    /// auto-clear side effect).
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.blocked.load(Ordering::SeqCst)
    }
}

impl RateLimitStatus {
    /// `true` when this status blocks new requests.
    #[must_use]
    pub fn is_blocked(self) -> bool {
        matches!(self, Self::Blocked { .. })
    }

    /// The remaining backoff window in milliseconds, or `None` when not
    /// blocked / blocked indefinitely.
    #[must_use]
    pub fn retry_after_ms(self) -> Option<u64> {
        match self {
            Self::Available => None,
            Self::Blocked { retry_after_ms } => retry_after_ms,
        }
    }
}

/// Convenience: returns a `Duration` for a blocked status, useful for
/// telemetry and ACP event payloads.
#[must_use]
pub fn backoff_duration(status: RateLimitStatus) -> Option<Duration> {
    status.retry_after_ms().map(Duration::from_millis)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn untracked_tracker_never_blocks() {
        let t = RateLimitTracker::untracked();
        assert_eq!(t.status(), RateLimitStatus::Available);
        assert_eq!(t.observed_429_count(), 0);
        assert!(!t.is_blocked());
    }

    #[test]
    fn record_429_blocks_until_retry_after_elapses() {
        let t = RateLimitTracker::untracked();
        t.record_429(Some(50)); // 50 ms backoff
        assert!(t.is_blocked());
        let status = t.status();
        assert!(status.is_blocked());
        let remaining = status.retry_after_ms().expect("must have remaining");
        assert!(
            remaining <= 50,
            "remaining must not exceed the configured retry_after"
        );
        // Sleep past the deadline and re-check — auto-clear must fire.
        thread::sleep(Duration::from_millis(80));
        assert_eq!(t.status(), RateLimitStatus::Available);
        assert!(!t.is_blocked());
    }

    #[test]
    fn record_429_without_retry_after_blocks_indefinitely() {
        let t = RateLimitTracker::untracked();
        t.record_429(None);
        assert!(t.is_blocked());
        assert_eq!(
            t.status(),
            RateLimitStatus::Blocked {
                retry_after_ms: None
            }
        );
        // Even after a sleep, still blocked (no deadline).
        thread::sleep(Duration::from_millis(20));
        assert!(t.is_blocked());
    }

    #[test]
    fn clear_resets_the_block() {
        let t = RateLimitTracker::untracked();
        t.record_429(None);
        assert!(t.is_blocked());
        t.clear();
        assert!(!t.is_blocked());
        assert_eq!(t.status(), RateLimitStatus::Available);
    }

    #[test]
    fn observed_429_count_accumulates_across_calls() {
        let t = RateLimitTracker::untracked();
        t.record_429(Some(10));
        t.record_429(Some(10));
        t.record_429(Some(10));
        assert_eq!(t.observed_429_count(), 3);
    }

    #[test]
    fn shared_returns_an_arc_for_cross_task_sharing() {
        let t = RateLimitTracker::shared();
        let cloned = Arc::clone(&t);
        cloned.record_429(Some(100));
        assert!(t.is_blocked());
        assert_eq!(t.observed_429_count(), 1);
    }

    #[test]
    fn backoff_duration_helper_converts_status() {
        assert_eq!(backoff_duration(RateLimitStatus::Available), None);
        assert_eq!(
            backoff_duration(RateLimitStatus::Blocked {
                retry_after_ms: Some(250)
            }),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            backoff_duration(RateLimitStatus::Blocked {
                retry_after_ms: None
            }),
            None
        );
    }

    #[test]
    fn record_429_refreshes_deadline_to_latest_observation() {
        let t = RateLimitTracker::untracked();
        t.record_429(Some(100));
        // Wait a bit, then a second 429 refreshes the deadline further.
        thread::sleep(Duration::from_millis(20));
        t.record_429(Some(100));
        let status = t.status();
        assert!(status.is_blocked());
        // The remaining window must be ≤ 100 (refreshed), not 80 (residual).
        let remaining = status.retry_after_ms().expect("must have remaining");
        assert!(
            remaining > 70,
            "refreshed deadline must extend past residual; got {remaining}"
        );
    }
}
