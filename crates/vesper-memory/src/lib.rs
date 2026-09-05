#![forbid(unsafe_code)]
//! Provider-neutral persistent memory graph, learned skills, user profile,
//! and bounded epistemic ledger for Agent Vesper (ADR 0011 — Stage 12).
//!
//! This crate backs the Tier C Phase 8 un-stubbed TUI commands: `/memory`,
//! `/goal`, `/subgoal`, `/skills`, `/profile`, `/awareness`,
//! `/metacognition`, `/deliberation`, `/repository`, `/meta-learning`,
//! `/observability`, `/curator`, `/journey`. It mirrors the Python
//! oracle's `glm_acp/memory.py` + `glm_acp/awareness.py` data models,
//! adapted to Rust's type system and Vesper's secret-safe confinement
//! rules.
//!
//! ## Storage layout
//!
//! All artefacts live under one configurable root directory (the
//! composition boundary chooses the path; this crate never creates the
//! root itself):
//!
//! - `memory.jsonl` — append-only [`MemoryEntry`] log (the source of truth
//!   for `/memory`, `/goal`, `/subgoal`).
//! - `skills/<slug>.md` — one markdown file per learned skill
//!   (`/skills`).
//! - `user.md` — cross-project user profile (`/profile`).
//! - `awareness.json` — persisted epistemic ledger (`/awareness`).
//!
//! All writes are atomic (write-to-temp + `fsync` + rename), confined to
//! the absolute root, and bounded by configured byte limits — the same
//! discipline as the Stage 6 session writer.
//!
//! ## Architecture
//!
//! Depends only on `vesper-domain` and `vesper-security`. No provider,
//! runtime, ACP, sessions, agent, testkit, SQLite, HTTP, or TUI
//! dependency.

pub mod awareness;
pub mod error;
pub mod io;
pub mod profile;
pub mod skill_orchestrator;
pub mod skills;
pub mod store;
pub mod types;

pub use awareness::{AwarenessLedger, MAX_RECORDS as MAX_AWARENESS_RECORDS};
pub use error::MemoryError;
pub use profile::{MAX_PROFILE_BYTES, MAX_PROFILE_LINE_CHARS, PROFILE_FILENAME, UserProfile};
pub use skill_orchestrator::{
    AUTO_ACTIVATION_SCORE, LoadedSkill, MAX_SELECTED_SKILLS, MAX_SKILL_CONTEXT_CHARS,
    MAX_TOTAL_SKILL_CONTEXT_CHARS, SkillCandidate, SkillExecutionMode, SkillInvocationPolicy,
    SkillMetadata, SkillOutcomeTracker, SkillRisk, SkillRoutingQuery, SkillRoutingReport,
    parse_metadata,
};
pub use skills::{
    MAX_BUNDLE_BYTES, MAX_BUNDLE_SKILLS, MAX_SKILL_BYTES, MAX_SKILL_FILES, SkillBundle, SkillStore,
    SkillSummary,
};
pub use store::MemoryStore;
pub use types::{
    Confidence, EpistemicRecord, EvidenceEvent, EvidenceSource, MAX_ENTRIES, MAX_EVIDENCE,
    MAX_ID_CHARS, MAX_SCOPES, MAX_SUMMARY_CHARS, MemoryEntry, MemoryKind, RecordStatus, SkillSlug,
};
