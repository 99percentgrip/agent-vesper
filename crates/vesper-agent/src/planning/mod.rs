//! Planning layer above the agent loop (ADR 0010 + ADR 0017).
//!
//! Currently owns [`vesper_lens`] — the native human-in-the-loop HTML
//! review oracle. Future planner extensions (UI-aware step routing, etc.)
//! will live alongside it.

pub mod vesper_lens;

pub use vesper_lens::{
    Action, DEFAULT_REVIEW_TIMEOUT, DomAnnotation, LensError, LensFeedback, VesperLens,
    inject_review_overlay, serve_and_collect_feedback,
};
