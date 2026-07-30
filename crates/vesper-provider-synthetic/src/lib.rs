#![forbid(unsafe_code)]
//! Deterministic, in-process reference provider adapter.
//!
//! `vesper-provider-synthetic` exists to prove the provider contract in
//! `vesper-provider` is genuinely provider-neutral: a second, independent
//! adapter implements [`ProviderFactory`] and [`ProviderSession`] end to end
//! without any GLM, network, or secret dependency. It is the multi-provider
//! proof-of-concept alongside the production GLM adapter.
//!
//! The provider emits a deterministic, configured reply for each turn with
//! ordered stream events, normalized usage, and a single terminal completion,
//! honouring the same stream invariants every concrete adapter must satisfy.

mod config;
mod factory;
mod session;

pub use config::{SyntheticCatalog, SyntheticConfig};
pub use factory::SyntheticFactory;
pub use session::SyntheticSession;

use vesper_domain::ProviderId;

/// Stable provider identity for the synthetic adapter (`vesper-synthetic`).
#[must_use]
pub fn provider_id() -> ProviderId {
    ProviderId::new("vesper-synthetic").expect("static provider ID")
}
