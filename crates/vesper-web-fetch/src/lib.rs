//! VRO-14 PR-3: the sandboxed fetch route.
//!
//! [`WebSandboxPort`] implements [`vesper_web::transport::FetchTransport`]
//! by running the `vesper-web-fetch` helper binary **inside a sandboxed
//! backend** that demands `IsolationRequirement::Network` with an explicit
//! `allow_network = true` grant. The harness process itself never opens a
//! socket: every byte of web egress happens inside the provisioned
//! sandbox, bounded by the backend's CPU/memory/timeout limits.
//!
//! Layering (PRD Feature 4):
//!
//! 1. **Egress gate (pure, pre-flight).** [`EgressPolicy`] classifies the
//!    URL *before* any sandbox is provisioned: scheme allowlist, host
//!    allowlist, private/loopback/link-local IP denial (v4, v6, and
//!    v4-mapped v6), and the robots verdict supplied by the caller (the
//!    harness may cache robots per origin). A denied URL never reaches a
//!    backend — no process is spawned for it.
//! 2. **Capability gate (fail-closed).** The backend must honestly report
//!    `network: Available`; anything else yields the typed
//!    [`FetchError::SandboxUnavailable`] refusal — never a silent
//!    unsandboxed run.
//! 3. **Execution.** The helper binary runs with argv = `[helper, url]`,
//!    writes the body to stdout (64 KiB cap enforced by the helper), and
//!    a `VWMETA:` JSON line to stderr carrying status, content type,
//!    charset, final URL, redirect count, and truncation flags. The port
//!    parses that line into [`FetchResponse`]; stdout beyond the cap is
//!    reported as `truncated`.
//!
//! This crate is the only production unit (besides the supervisor) that
//! knows the helper binary exists; hosts wire it at the composition
//! boundary exactly like the sandbox route.

#![forbid(unsafe_code)]

pub mod browser;
pub mod parse;
pub mod route;

pub use route::{WebSandboxPort, WebSandboxRouteConfig};
pub use vesper_web::transport::{EgressPolicy, FetchError, FetchRequest, FetchResponse};

/// Default wall-clock bound for one sandboxed fetch (seconds).
pub const DEFAULT_FETCH_TIMEOUT_SECONDS: u64 = 45;

/// Default body cap carried from the helper's own 64 KiB streaming cap.
pub const DEFAULT_BODY_CAP_BYTES: usize = 64 * 1024;
