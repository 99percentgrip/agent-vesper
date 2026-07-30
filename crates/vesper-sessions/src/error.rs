use std::io;

use thiserror::Error;

use crate::{SessionFileNameError, UnsupportedSessionOperation};

/// Safe failures from read-only session stores.
#[derive(Debug, Error)]
pub enum SessionStoreError {
    #[error("session operation {0:?} is unsupported by this read-only repository")]
    UnsupportedOperation(UnsupportedSessionOperation),
    #[error("session store root must be absolute")]
    RootNotAbsolute,
    #[error("configured bounds must be non-zero")]
    InvalidBounds,
    #[error("session directory contains more than {maximum} entries")]
    EntryLimitExceeded { maximum: usize },
    #[error("session filename exceeds {maximum} bytes")]
    FilenameLimitExceeded { maximum: usize },
    #[error("session record exceeds {maximum} bytes")]
    RecordLimitExceeded { maximum: u64 },
    #[error("session record is not contained by its configured root")]
    PathEscapesRoot,
    #[error("session record is not a regular file or directory")]
    NotRegularFile,
    #[error(transparent)]
    InvalidFileName(#[from] SessionFileNameError),
    #[error("session filesystem operation failed: {0}")]
    Io(#[source] io::Error),
    #[error("session blocking task failed")]
    BlockingTaskFailed,
    #[error("session blocking gate is closed")]
    BlockingGateClosed,
    #[error("composite session sources do not match memory, Agent Vesper, legacy order")]
    InvalidSourceOrder,
    #[error("session record could not be serialized for atomic write")]
    SerializationFailed,
}

impl From<io::Error> for SessionStoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
