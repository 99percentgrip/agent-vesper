#![forbid(unsafe_code)]
//! Platform-aware, legacy-safe configuration contracts.

mod app;
mod atomic;
mod paths;
mod profile;

pub use app::{
    ApplicationConfig, ConfigSource, ProviderConfigEnvelope, ProviderConfigError, ResolvedValue,
};
pub use atomic::{AtomicWriteError, AtomicWriter};
pub use paths::{
    LegacyLocation, LegacyLocationKind, PathEnvironment, Platform, VesperPaths, VesperPathsError,
};
pub use profile::{ProfileName, ProfileNameError};
