//! VRO-14 PR-1: the Perception Engine core — a strictly pure-logic
//! HTML-to-Markdown transformation pipeline (Parse → Strip → Prune →
//! Convert) with zero I/O of any kind.
//!
//! This crate owns no transport, no clock, no filesystem access, and no
//! network. Every function is a mapping from HTML bytes (already fetched
//! by a caller through the sandbox boundary) to bounded Markdown text.
//! The crate is deliberately provider-neutral, host-neutral, and
//! workspace-confined to `crates/vesper-web` (PRD
//! `docs/web-oracle-extraction-prd.md`, Feature 1).
//!
//! Upstream design references (naming rule, PRD §0): the map/scrape
//! lifecycle of **web oracle alpha** (strip semantics of its
//! `removeUnwantedElements` intent), the heuristic content-pruning of
//! **web oracle beta** (its exact default metric weights), and the
//! byte-density behavior validated by the PR-0 portability spike
//! (VERDICT.md under the disposable experiment directory).

#![forbid(unsafe_code)]

pub mod arena;
pub mod bm25;
pub mod convert;
pub mod crawl;
pub mod density;
pub mod dom;
pub mod links;
pub mod meta;
pub mod pipeline;
pub mod rank;
pub mod strip;

pub use arena::{ArenaNode, Dom, NodeId};
pub use convert::{ConvertOptions, blocks_to_markdown, document_to_markdown};
pub use density::{TextDensityFilter, ThresholdType};
pub use dom::{Document, Element, Node, parse};
pub use pipeline::{PipelineOutput, run_pipeline};
