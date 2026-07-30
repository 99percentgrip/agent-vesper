//! `ProviderFactory` implementation for the synthetic adapter.

use std::sync::Arc;

use vesper_domain::ProviderId;
use vesper_provider::{
    CancellationSignal, ProviderConfiguration, ProviderError, ProviderFactory, ProviderFuture,
};

use crate::{SyntheticConfig, SyntheticSession, provider_id};

/// Default deterministic reply when no configuration overrides it.
pub const DEFAULT_REPLY: &str = "synthetic-ok";

/// Production-grade synthetic provider factory.
///
/// Implements the same [`ProviderFactory`] contract as the GLM adapter without
/// any network, authentication, or GLM dependency. It is the multi-provider
/// proof-of-concept: the runtime can register and dispatch to it exactly as it
/// does for GLM.
#[derive(Clone)]
pub struct SyntheticFactory {
    provider_id: ProviderId,
    default_reply: String,
}

impl Default for SyntheticFactory {
    fn default() -> Self {
        Self::new(DEFAULT_REPLY)
    }
}

impl std::fmt::Debug for SyntheticFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyntheticFactory")
            .field("provider_id", &self.provider_id)
            .field("default_reply", &self.default_reply)
            .finish()
    }
}

impl SyntheticFactory {
    /// Creates a factory whose sessions stream `default_reply` unless the
    /// session configuration overrides it.
    #[must_use]
    pub fn new(default_reply: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id(),
            default_reply: default_reply.into(),
        }
    }

    /// Stable provider descriptor.
    #[must_use]
    pub fn descriptor() -> vesper_provider::ProviderDescriptor {
        crate::config::descriptor()
    }

    /// Default versioned configuration envelope.
    #[must_use]
    pub fn default_configuration() -> ProviderConfiguration {
        crate::config::default_configuration()
    }
}

impl ProviderFactory for SyntheticFactory {
    type Session = SyntheticSession;

    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    fn create_session<'a>(
        &'a self,
        configuration: &'a ProviderConfiguration,
        _cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<Self::Session, ProviderError>> {
        let config = SyntheticConfig::from_configuration(configuration);
        let reply = config.reply.unwrap_or_else(|| self.default_reply.clone());
        let provider_id = self.provider_id.clone();
        Box::pin(async move { Ok(SyntheticSession::new(provider_id, reply)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_configuration;

    struct NeverCancelled;
    impl CancellationSignal for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn factory_creates_session_under_the_synthetic_identity() {
        let factory = SyntheticFactory::default();
        assert_eq!(factory.provider_id(), &provider_id());
        let session = factory
            .create_session(
                &default_configuration(),
                Arc::new(NeverCancelled) as Arc<dyn CancellationSignal>,
            )
            .await
            .expect("session creation never fails for the synthetic adapter");
        assert_eq!(session.provider_id(), &provider_id());
    }
}
