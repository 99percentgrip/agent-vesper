//! The swarm's shared memory ledger (VRO-15).
//!
//! The pure vector index ([`hnsw`]) supports the coherent hybrid ledger,
//! scoped retrieval and bounded transactional knowledge transfer, per `docs/swarm-oracle-extraction-prd.md` §2.
//!
//! Scope discipline: the ledger is **ephemeral and swarm-scoped**. It
//! never touches `vesper-memory` (durable project memory) or
//! `vesper-cognition` (the workspace's semantic engine, which owns the
//! public cosine helper and embedding ports) — see the audit note in
//! [`hnsw`].

pub mod hnsw;

pub mod store;

mod snapshot_writer;

mod filter;

mod retention;
