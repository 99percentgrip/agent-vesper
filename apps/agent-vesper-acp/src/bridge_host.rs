//! VB-PRD-001 Phase 2: the `/bridge` command surface for the ACP host.
//!
//! Host-level, read-only command answers. Session control flows through
//! the model tool surface (`bridge_connect`/`bridge_execute`/…) so every
//! operation passes the same Bridge gate — `/bridge` never dispatches an
//! application action itself. The answer text itself lives in
//! `vesper-harness::bridge_command`, shared with the TUI host so the
//! command surface cannot drift between hosts (BR-21).

pub use vesper_harness::bridge_command::command;
