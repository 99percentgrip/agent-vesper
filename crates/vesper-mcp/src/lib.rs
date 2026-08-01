#![forbid(unsafe_code)]
//! MCP stdio client and Ed25519-signed plugin loader for Agent Vesper
//! (ADR 0013 — Stage 15).
//!
//! This crate backs the Tier C Phase 10 un-stubbed TUI commands `/mcp`
//! and `/plugins`. It mirrors the Python oracle's `glm_acp/mcp.py` +
//! `glm_acp/plugins.py` data models, adapted to Rust's type system and
//! the lead architect's stronger security mandate: **the unsigned-plugin
//! loading code path is structurally erased from `--release` builds via
//! `#[cfg(debug_assertions)]`** — not merely gated by a runtime env var
//! like the oracle's `REQUIRE_SIGNED_ENV`.
//!
//! ## The No-Leak Guarantee
//!
//! [`PluginLoader::load`] ALWAYS requires a valid Ed25519 signature from
//! a trusted publisher. The [`PluginLoader::load_unsigned_debug`] method
//! exists ONLY under `#[cfg(debug_assertions)]`; in a `--release` build
//! the method does not exist at all, and any caller that attempts to
//! invoke it is a compile error. A release binary therefore CANNOT load
//! an unsigned plugin — there is no code path by which it could.
//!
//! ## Storage layout
//!
//! All artefacts live under one configurable root directory:
//!
//! - `mcp.jsonl` — append-only [`McpServerConfig`] registry.
//! - `plugins.jsonl` — append-only [`PluginRecord`] log of loaded plugins.
//! - `publishers.jsonl` — append-only [`TrustedPublisher`] registry.
//!
//! All writes are atomic (write-to-temp + `fsync` + rename), confined to
//! the absolute root, and bounded by configured byte limits — the same
//! discipline as the Stage 6/12/14 writers.
//!
//! ## Architecture
//!
//! Depends only on `vesper-domain` and `vesper-security`. No provider,
//! runtime, ACP, sessions, agent, testkit, SQLite, HTTP, or TUI
//! dependency. The MCP stdio client spawns bounded subprocesses (scoped
//! `Child` — RAII reaps the process); HTTP MCP servers are not yet
//! supported (the oracle's HTTP path requires live provider credentials,
//! which foundation verification forbids).

pub mod error;
pub mod mcp;
pub mod plugins;

pub use error::McpError;
pub use mcp::{McpClient, McpRegistry, McpServerConfig, McpToolDescriptor, McpTransport};
pub use plugins::{
    MAX_PLUGIN_BYTES, MAX_PLUGIN_FILES, PluginLoader, PluginManifest, PluginRecord,
    PluginSignature, TrustedPublisher, TrustedPublishers,
};
