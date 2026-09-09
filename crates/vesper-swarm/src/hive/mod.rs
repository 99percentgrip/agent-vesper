//! Hive orchestration semantics (VRO-15 PR-5).
//!
//! This module owns the pure decision layer between the bus (PR-4), the
//! pool (PR-3), and the topology (PR-2): which worker should take a task
//! ([`assignment`]), what bus tier a task's urgency maps to
//! ([`assignment`]), and how a dispatched turn is bounded in time and
//! cancellable end to end ([`timeout`]). Everything here is decision
//! logic; nothing performs I/O or mutates shared state.

pub mod assignment;
pub mod timeout;

pub mod orchestrator;
