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
//! - [`tools`] — the nine parity-critical stub executors.
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
pub mod registry;
pub mod tools;

pub use agent_loop::{
    AgentLoop, AgentLoopConfig, AgentLoopError, AgentTurnOutcome, DEFAULT_MAX_TOOL_ITERATIONS,
};
pub use executor::{ToolContext, ToolError, ToolExecutor, ToolFuture, ToolResult};
pub use permission::{PermissionDecision, check_tool_permission};
pub use registry::ToolRegistry;
