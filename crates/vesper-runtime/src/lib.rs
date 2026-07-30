#![forbid(unsafe_code)]
//! Minimal provider-neutral runtime for ephemeral ACP sessions.

mod cancellation;
mod error;
mod persistence;
mod registry;
mod session;
mod supervisor;

pub use cancellation::RuntimeCancellation;
pub use error::RuntimeError;
pub use persistence::{RuntimeSessionReads, RuntimeSessionWrites};
pub use registry::ProviderRegistry;
pub use session::{SessionSnapshot, SessionTurnResult};
pub use supervisor::{RuntimeDefaults, RuntimeEventReceiver, RuntimeResponse, RuntimeSupervisor};
pub use vesper_sessions::{
    AvailableCommandDescriptor, ReplayError, ReplayFuture, ReplayMessage, ReplayMetadata,
    ReplayPlan, ReplayPlanEntry, ReplayPlanPriority, ReplayPlanStatus, ReplaySink, ReplayUpdate,
    SessionConfigurationStatus, SessionSource, WriteOutcome,
};
