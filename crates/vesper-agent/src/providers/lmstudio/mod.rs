//! LM Studio provider adapter for the VRO orchestrator (VRO-3.1, PRD §13).
//!
//! LM Studio is a local/LAN OpenAI-compatible model server. This module ships
//! the network configuration, model discovery + capability probe, and the
//! [`CandidateGenerator`](crate::vro::CandidateGenerator) adapter that lets the
//! VRO Generate-Verify-Repair loop drive an LM Studio model.
//!
//! ## Why this lives in `vesper-agent`, not `vesper-provider-lmstudio`
//!
//! [`CandidateGenerator`](crate::vro::CandidateGenerator) is a `vesper-agent`
//! trait (the orchestrator's generation seam). A provider crate implementing it
//! would invert the dependency direction (provider → agent), which the crate
//! boundary rules forbid. Per the directive ("`crates/vesper-provider-lmstudio`
//! (or `vesper-agent/src/providers/`)"), the adapter therefore lives here. The
//! generic capability contract [`ModelCapabilities`](vesper_domain::ModelCapabilities)
//! lives in `vesper-domain` so any future real provider crate can reuse it.
//!
//! ## Transport seam
//!
//! HTTP request **construction** is pure and unit-testable (custom LAN URL,
//! bearer header, correction-bearing body). Request **transport** is an
//! [`LmStudioTransport`] trait port — the real HTTP-backed client is
//! supplied at the composition boundary; tests inject a capturing fake. No live
//! LM Studio integration runs in VRO-3.1 (per the execution constraints).

pub mod client;
pub mod config;
pub mod discovery;
pub mod generator;
pub mod react;

pub use client::{
    ChatMessage, HttpMethod, LmStudioError, LmStudioHttpRequest, LmStudioHttpResponse,
    LmStudioTransport, auth_headers, build_chat_request, build_models_request, join_url,
    parse_chat_response,
};
pub use config::{LmStudioApiKey, LmStudioConfig};
pub use discovery::{
    CapabilityRegistry, ModelInfo, ServerHealth, build_health_request, discover_model,
    discover_models, parse_models_response, probe_capabilities, probe_health,
};
pub use generator::LmStudioCandidateGenerator;
pub use react::{
    LmStudioReactAgent, MALFORMED_TOOL_NAME, REACT_SYSTEM_PROMPT, parse_react_decision,
};
