//! The swarm's shared memory ledger (VRO-15).
//!
//! PR-6 owns the pure vector index ([`hnsw`]). PR-7 will layer the
//! hybrid structured+vector ledger, scopes, and bounded knowledge
//! transfer on top, per `docs/swarm-oracle-extraction-prd.md` §2.
//!
//! Scope discipline: the ledger is **ephemeral and swarm-scoped**. It
//! never touches `vesper-memory` (durable project memory) or
//! `vesper-cognition` (the workspace's semantic engine, which owns the
//! public cosine helper and embedding ports) — see the audit note in
//! [`hnsw`].

pub mod hnsw;

pub mod store;
