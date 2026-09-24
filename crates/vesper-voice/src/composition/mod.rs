//! PR-1 core compositions (still pure: no I/O, no clock, no runtime).
//!
//! - [`blocking`] wraps synchronous adapter internals on a caller-supplied
//!   executor; cancellation is checked at entry, at yield points, and is
//!   surfaced as `VoiceError::Cancelled`. Stopping the awaiter is not
//!   proof the blocking work stopped — that limit is documented at each
//!   adapter and bounded by the adapter's own deadline.
//! - [`failover`] composes interchangeable STT adapters under the frozen
//!   policy (only `Unavailable` advances; silence is an answer) with
//!   per-attempt egress rechecks and bounded attempts.
//! - [`partials`] is the buffered-repass partial gate: bounded buffer,
//!   coalesced obsolete work, finals-first priority, stale rejection.

pub mod blocking;
pub mod failover;
pub mod partials;
