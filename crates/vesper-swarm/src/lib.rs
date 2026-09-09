#![forbid(unsafe_code)]
//! Pure, provider-neutral swarm-coordination foundations for Agent Vesper.
//!
//! VRO-15 extracts the coordination paradigms of *the swarm oracle* — a
//! trusted upstream orchestration repository — into a native Rust layer.
//! This crate is **pure logic by construction**: no network, no filesystem,
//! no clock, no process spawning, and no provider names. Execution,
//! inference, and sandboxing are trait ports fulfilled at the composition
//! boundary in later PRs; nothing in here can even name a provider.
//!
//! PR-1 owns only the topology data model ([`topology`]). Later PRs add
//! worker pooling, the priority message bus, task assignment, the shared
//! memory ledger, and sandbox lease coordination per
//! `docs/swarm-oracle-extraction-prd.md`.
//!
//! Integration is default-off: no production crate or application depends on
//! `vesper-swarm` until the composition PR wires a default-off `swarm`
//! feature at the host boundary.

pub mod topology;

pub mod manager;

pub mod worker;

pub mod pool;

pub mod error;

pub mod bus;

pub mod hive;

pub mod ledger;

pub mod sandbox;
