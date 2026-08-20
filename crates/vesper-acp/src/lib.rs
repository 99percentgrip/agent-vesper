#![forbid(unsafe_code)]
//! ACP protocol-v1 adapter for the minimal Agent Vesper runtime.

mod adapter;
mod compat;
pub mod controls;
mod engine;
mod mapping;
mod vro_events;

pub use adapter::{AcpAdapter, AcpAdapterConfig};
pub use compat::{ACP_SDK_VERSION, ACP_WIRE_PROTOCOL, prompt_response_value};
pub use controls::{
    AcpControlCategory, AcpControlOption, AcpSessionControl, AppliedSelection,
    SessionControlSurface,
};
pub use engine::{
    AcpEngineEvent, AcpEventSink, AcpPermissionDecision, AcpPermissionRequest,
    AcpPermissionRequester, AcpPromptEngine, AcpPromptFuture, AcpPromptRequest, AcpPromptResult,
};
pub use mapping::truthful_initialize_response;
pub use vro_events::{
    RecordingVroEventSink, VroEvent, VroEventSink, VroEventSinkError, sample_happy_path_sequence,
    translate_vro_event_to_acp,
};
