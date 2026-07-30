use serde::{Deserialize, Serialize};

/// Trust classification for provider endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointTrust {
    /// Provider-owned endpoint pinned by an adapter.
    Official,
    /// User-configured remote endpoint.
    ConfiguredRemote,
    /// Loopback or explicitly local endpoint.
    Local,
    /// Endpoint origin is not trusted.
    Untrusted,
}
