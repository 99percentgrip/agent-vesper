use std::path::{Path, PathBuf};

use thiserror::Error;

/// Contract for future same-directory durable replacement.
pub trait AtomicWriter {
    /// Writes bytes atomically or leaves the original untouched.
    fn write_private(&self, target: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError>;
}

/// Atomic-write failure without raw file contents.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AtomicWriteError {
    /// Target is outside the writer's authorized root.
    #[error("target is outside the authorized configuration root")]
    UnauthorizedPath,
    /// Platform operation failed. Detail is safe and bounded by the implementor.
    #[error("atomic write failed for {target}")]
    Platform {
        /// Safe target path.
        target: PathBuf,
    },
}
