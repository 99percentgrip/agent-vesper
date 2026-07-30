#![forbid(unsafe_code)]
//! Provider-neutral values shared across Agent Vesper ports and adapters.

pub mod bounded;
pub mod capability;
pub mod command;
pub mod compatibility;
pub mod content;
pub mod error;
pub mod event;
pub mod finish;
pub mod goal;
pub mod ids;
pub mod message;
pub mod metadata;
pub mod permission;
pub mod plan;
pub mod session;
pub mod tool;
pub mod usage;
pub mod version;

pub use bounded::{BoundedString, BoundedStringError, ContentText, SafeMessage};
pub use capability::{CapabilityFallback, CapabilityId, CapabilityRequest, FeatureRequirement};
pub use command::{
    CommandInitiator, HarnessCommand, HarnessCommandPayload, PromptSubmission,
    RuntimeAuthenticationMethod, RuntimeCapability, RuntimeInitialization, SessionListFilter,
    WorkspaceRoot,
};
pub use compatibility::{LegacyGlmSettings, LegacySessionError, LegacySessionV1};
pub use content::{
    AudioDescriptor, ContentPart, EmbeddedContextReference, ImageDescriptor, InlineDataDescriptor,
    MediaSource, OpaqueContent, OpaqueProviderData, OpaqueProviderDataError, ReasoningBlock,
    ReasoningKind, ReasoningRetention,
};
pub use error::{
    ErrorCategory, ErrorInfo, RedactedDiagnostics, Retryability, SafeErrorCause, SafeProviderCode,
};
pub use event::{
    EventLog, EventSequence, EventSequenceError, HarnessEvent, HarnessEventPayload, SessionSummary,
};
pub use finish::FinishOutcome;
pub use goal::{Goal, GoalStatus};
pub use ids::{
    CheckpointRef, CommandId, CorrelationId, EndpointId, EventId, GoalId, IdError, MessageId,
    ModelId, PlanId, ProviderId, ProviderRequestId, ProviderResponseId, QualifiedModelId, Revision,
    SessionId, ToolCallId, ToolId, ToolResultId, TurnId, WorkerId,
};
pub use message::{ConversationMessage, MessageRole, SystemInstruction};
pub use metadata::{ExtensionError, ExtensionMap, ExtensionNamespace};
pub use permission::{
    PermissionOutcome, PermissionRequestId, SessionOperatingMode, SessionPermissionMode,
};
pub use plan::{Plan, PlanStatus, PlanStep};
pub use session::{ReasoningRetentionMode, SessionLineage, SessionSnapshotHeader};
pub use tool::{
    CompletedToolArguments, DiffSummary, FragmentedToolCallIdentity, HarnessToolName,
    ProviderToolName, StructuredLocation, ToolCall, ToolChoiceIntent, ToolDefinition,
    ToolExecutionClass, ToolNameError, ToolResult, ToolResultStatus,
};
pub use usage::{
    EstimatedCost, NormalizedUsage, UsageArithmeticError, UsageMeasurement, UsageMode,
    UsageProvenance, UsageTotalConsistency,
};
pub use version::{
    CommandSchemaVersion, CompatibilityRecordVersion, ContractFamily, DomainSchemaVersion,
    EventSchemaVersion, SchemaVersion, VersionCompatibilityError, VersionedExtensionEnvelope,
};
