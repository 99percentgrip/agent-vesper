#![forbid(unsafe_code)]
//! Platform-aware, legacy-safe configuration contracts.

mod app;
mod atomic;
mod paths;
mod profile;
mod sandbox_config;
mod web_config;
pub use web_config::{
    DEFAULT_OUTPUT_BUDGET_BYTES, MAX_OUTPUT_BUDGET_BYTES, WebConfigError, WebScopeConfig,
    is_digest_pinned_image, parse_web_table, read_web_scope,
};

pub use app::{
    ApplicationConfig, ConfigSource, ProviderConfigEnvelope, ProviderConfigError, ResolvedValue,
};
pub use atomic::{AtomicWriteError, AtomicWriter};
pub use paths::{
    LegacyLocation, LegacyLocationKind, PathEnvironment, Platform, VesperPaths, VesperPathsError,
};
pub use profile::{ProfileName, ProfileNameError};
pub use sandbox_config::{
    SandboxConfigError, SandboxScopeConfig, parse_sandbox_table, read_sandbox_scope,
};
