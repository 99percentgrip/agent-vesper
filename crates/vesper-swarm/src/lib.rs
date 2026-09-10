#![forbid(unsafe_code)]
//! Pure, provider-neutral swarm-coordination foundations for Agent Vesper.
//!
//! VRO-15 extracts the coordination paradigms of *the swarm oracle* — a
//! trusted upstream orchestration repository — into a native Rust layer.
//! Coordination performs no network/filesystem I/O or process spawning and
//! names no provider. Execution, inference and sandboxing use composition ports.
//! Pool/turn deadlines and bus TTL currently use monotonic clocks; clock injection
//! remains tracked acceptance work rather than an asserted purity property.
//!
//! Modules own topology, pooling, priority messaging, assignment, the ephemeral
//! ledger, lease coordination and hive orchestration. The harness adapter is an
//! optional default-off `swarm` dependency; host activation remains gated.

pub mod topology;

pub mod manager;

pub mod worker;

pub mod pool;

pub mod error;

pub mod bus;

pub mod hive;

pub mod ledger;

pub mod sandbox;
