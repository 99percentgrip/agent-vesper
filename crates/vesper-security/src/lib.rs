#![forbid(unsafe_code)]
//! Security primitives that describe authority without granting it.

mod endpoint;
mod environment;
mod output;
mod path;
mod sandbox;
mod scope;
mod secret;
mod untrusted;
mod url_redaction;

pub use endpoint::EndpointTrust;
pub use environment::{EnvironmentScrubber, ScrubbedEnvironment};
pub use output::BoundedOutput;
pub use path::{PathCapability, RelativePath, RootIdentity};
pub use sandbox::{CapabilityStatus, IsolationRequirement, SandboxCapabilities, SecurityStrength};
pub use scope::{SecretScope, SecretScopeError, is_multiplex_active, set_multiplex_active};
pub use secret::{
    SecretExposure, SecretReference, SecretReferenceError, SecretSource, SecretValue,
};
pub use untrusted::UntrustedContext;
pub use url_redaction::{RedactedUrl, RedactedUrlError};
