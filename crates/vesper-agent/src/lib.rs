#![forbid(unsafe_code)]
//! `vesper-agent` — multi-turn tool-executing agent loop (ADR 0010, Tier C).
//!
//! Composes `vesper-runtime`'s single-turn provider dispatch into a ReAct
//! agent loop bounded by `max_tool_iterations`. Owns the tool registry, the
//! permission gate, and the loop mechanics. The runtime stays pure and
//! single-turn; this crate holds the multi-turn, tool-executing layer above
//! it.
//!
//! ## Layout
//!
//! - [`executor`] — `ToolExecutor` trait, `ToolContext`, `ToolResult`.
//! - [`tools`] — the nine parity-critical confined executors.
//! - [`registry`] — `ToolRegistry` mapping tool names → executors.
//! - [`permission`] — pure `(mode × permission × class)` gate.
//! - [`agent_loop`] — the `AgentLoop` ReAct driver.
//!
//! ## DOX
//!
//! See `crates/vesper-agent/AGENTS.md` for purpose, ownership, contracts, and
//! verification.

pub mod agent_loop;
pub mod confinement;
pub mod executor;
pub mod permission;
pub mod project_context;
pub mod references;
pub mod registry;
pub mod tools;

pub use agent_loop::{
    AgentLoop, AgentLoopConfig, AgentLoopError, AgentProgressEvent, AgentProgressPort,
    AgentTurnOutcome, DEFAULT_MAX_TOOL_ITERATIONS, MAX_CONTEXT_MESSAGES,
};
pub use executor::{
    HostedTool, ToolContext, ToolError, ToolExecutor, ToolFuture, ToolResult, ToolService,
    schema_definition,
};
pub use permission::{
    ApprovalBroker, DenyPermissionPort, PermissionDecision, PermissionPort, PermissionRequest,
    check_tool_permission,
};
pub use project_context::{MAX_PROJECT_CONTEXT_BYTES, project_instructions};
pub use references::{
    MAX_FOLDER_FILES, MAX_REFERENCE_BYTES, MAX_REFERENCE_FILE_BYTES, MAX_REFERENCES,
    ReferenceError, expand_references,
};
pub use registry::ToolRegistry;
