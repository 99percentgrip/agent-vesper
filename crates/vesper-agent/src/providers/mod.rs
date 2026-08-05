//! Provider adapters for the VRO orchestrator.
//!
//! Adapters here implement the `vesper-agent`-owned generation seam
//! ([`crate::vro::CandidateGenerator`]) for specific providers. They live in
//! `vesper-agent` (not a dedicated `vesper-provider-*` crate) because the
//! generation seam is a `vesper-agent` trait, and a provider implementing it
//! would otherwise invert the crate dependency direction. Generic
//! provider-neutral capability contracts (e.g.
//! [`ModelCapabilities`](vesper_domain::ModelCapabilities)) stay in
//! `vesper-domain`.
//!
//! ## Child modules
//!
//! - [`lmstudio`] — LM Studio local/LAN OpenAI-compatible model server
//!   (VRO-3.1): network config, model discovery + capability probe, and the
//!   `CandidateGenerator` adapter.

pub mod lmstudio;
