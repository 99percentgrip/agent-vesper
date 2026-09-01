#![forbid(unsafe_code)]
//! Platform-aware, legacy-safe configuration contracts.

mod app;
mod atomic;
mod paths;
mod profile;
mod sandbox_config;

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
