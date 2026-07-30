#![forbid(unsafe_code)]

//! Read-only session persistence contracts and bounded store discovery.
//!
//! This crate deliberately exposes no mutation API. Compatibility conversion
//! and replay planning are pure, bounded operations over already decoded data.

mod composite;
mod contracts;
mod conversion;
mod decoder;
mod error;
mod filename;
mod filesystem;
mod layout;
mod metadata;
mod replay;
mod vesper_format;
mod writer;

pub use composite::{CompositeSessionRepository, EmptySessionRepository};
pub use contracts::{
    BoxSessionFuture, MetadataOrigin, SessionCapability, SessionListFilter, SessionLister,
    SessionMetadata, SessionReadIntent, SessionReader, SessionRecord, SessionRepository,
    SessionRepositoryCapabilities, SessionSource, UnsupportedSessionOperation,
};
pub use conversion::{
    CompatibilityAvailability, ConfigurationIssue, LegacyCompatibilityData, LegacyRuntimeConverter,
    PersistedProviderConfiguration, PersistedSessionState, SessionCompatibilityData,
    SessionConfigurationStatus, SessionConversionError,
};
pub use decoder::{
    BoundViolation, CorruptLegacyRecord, DecodedLegacySession, LegacyDecodeBounds,
    LegacyLoadOutcome, LegacySessionDecoder,
};
pub use error::SessionStoreError;
pub use filename::{MAX_SESSION_ID_BYTES, SessionFileName, SessionFileNameError};
pub use filesystem::{DiscoveryBounds, FilesystemSessionStore};
pub use layout::{AgentVesperSessionLayout, LegacySessionLayout};
pub use metadata::sort_session_metadata;
pub use replay::{
    AvailableCommandDescriptor, ReplayError, ReplayFuture, ReplayMessage, ReplayMetadata,
    ReplayPlan, ReplayPlanEntry, ReplayPlanPriority, ReplayPlanStatus, ReplaySink, ReplayUpdate,
};
pub use vesper_format::{
    VesperDecodeBounds, VesperLoadOutcome, VesperSessionDecoder, VesperSessionV1,
};
pub use writer::{SessionWriter, VesperSessionWriter, WriteBounds, WriteOutcome};
