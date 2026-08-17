//! Planning layer above the agent loop (ADR 0010 + ADR 0017).
//!
//! Currently owns [`vesper_lens`] — the native human-in-the-loop HTML review
//! and structured planning-interview oracle.

pub mod vesper_lens;

pub use vesper_lens::{
    Action, DEFAULT_REVIEW_TIMEOUT, DomAnnotation, LensAnswer, LensError, LensFeedback,
    LensQuestion, VesperLens, inject_review_overlay, render_interview_artifact,
    serve_and_collect_feedback,
};
