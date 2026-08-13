//! Vesper Reasoning Orchestrator (VRO) domain contracts (Phase VRO-1).
//!
//! This module establishes the **core data structures** defined in the VRO PRD
//! (`docs/agent-vesper-reasoning-orchestrator-prd.md`):
//!
//! - [`ReasoningMode`] — user-facing modes (PRD §8.1).
//! - [`ReasoningStrategy`] — policy-engine strategy enum (PRD §10.3).
//! - [`TaskProfile`] — task-profiler output (PRD §14.2).
//! - [`ReasoningBudget`] — runtime budget envelope (PRD §10.4).
//! - [`ReasoningConfig`] — the `[reasoning]` configuration block (PRD §24).
//!
//! No orchestration logic lives here. These are pure, provider-neutral,
//! serialization-stable contracts that the future Task Profiler, Policy
//! Engine, and Budget Manager (PRD §10) will consume. Field types and the
//! strategy/mode enums follow the PRD verbatim; the only non-PRD values are
//! the conservative baselines for budget fields the PRD §24 config does not
//! pin, and each such value is documented inline and deferred to research
//! phase R3 ("Budget Curves", PRD §20).
//!
//! See `crates/vesper-domain/AGENTS.md` for ownership and contract scope.

use serde::{Deserialize, Serialize};

use crate::BoundedString;

// ---------------------------------------------------------------------------
// Identifiers and free-form labels
// ---------------------------------------------------------------------------

/// Stable verifier identity, e.g. `"cargo_check"`, `"clippy"`, `"schema"`.
///
/// PRD §10.1 example: `["cargo_check", "cargo_test", "clippy"]`.
pub type VerifierId = BoundedString<128>;

/// Free-form task domain label, e.g. `"coding"`, `"math"`, `"research"`.
///
/// PRD §14.2 types `domain` as `TaskDomain`; §10.1 shows `"coding"`.
pub type TaskDomain = BoundedString<128>;

// ---------------------------------------------------------------------------
// ReasoningMode (PRD §8.1)
// ---------------------------------------------------------------------------

/// User-facing reasoning mode.
///
/// | Variant   | Behavior (PRD §8.1)                                              |
/// |-----------|------------------------------------------------------------------|
/// | `Auto`    | VRO profiles the request and selects workflow + budget. Default.|
/// | `Fast`    | Single pass or minimal plan-and-check; strict latency ceiling.  |
/// | `Balanced`| Decomposition plus one verification/repair cycle when needed.   |
/// | `Deep`    | Multiple candidates, stronger verification, bounded search.     |
/// | `Maximum` | Highest configured test-time budget for difficult/high-value work.|
/// | `Off`     | Bypass VRO and use the provider's normal direct response path.  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningMode {
    /// VRO profiles and selects (default).
    #[default]
    Auto,
    /// Minimal plan-and-check; strict latency ceiling.
    Fast,
    /// Decomposition plus one verification/repair cycle.
    Balanced,
    /// Multiple candidates, stronger verification, bounded search.
    Deep,
    /// Highest configured test-time budget.
    Maximum,
    /// Bypass VRO; use the provider's direct response path.
    Off,
}

// ---------------------------------------------------------------------------
// ReasoningStrategy (PRD §10.3 — authoritative 10-variant list)
// ---------------------------------------------------------------------------

/// Policy-engine reasoning strategy.
///
/// Source: PRD §10.3 "Initial strategies". PRD §11 describes the *behavior*
/// of these strategies; §10.3 is the authoritative enum. Reflection after
/// external failure (PRD §11.9) is a *failure-recovery behavior*, not a
/// top-level strategy variant, and is therefore not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningStrategy {
    /// Simple, low-risk tasks (PRD §11.1).
    #[default]
    Direct,
    /// Decomposition helps but external execution is unnecessary (PRD §11.2).
    PlanThenAnswer,
    /// Plan, execute, then verify (PRD §10.3 #3).
    PlanExecuteVerify,
    /// Generate → verify → targeted repair (PRD §11.3).
    GenerateVerifyRepair,
    /// Parallel candidates compared on normalized answers (PRD §11.4).
    ParallelCandidatesConsensus,
    /// Parallel candidates judged by a separate verifier (PRD §11.5).
    ParallelCandidatesJudge,
    /// Environment-grounded ReAct loop (PRD §11.6).
    ToolGroundedReact,
    /// Conservative depth/breadth tree search (PRD §11.7).
    BoundedTreeSearch,
    /// Proposer, critic, adjudicator roles (PRD §11.8).
    ProposerCriticAdjudicator,
    /// Reuse a learned workflow and re-verify it (PRD §10.3 #10).
    WorkflowReplayWithVerification,
}

// ---------------------------------------------------------------------------
// Task classification scalars (PRD §14.2)
// ---------------------------------------------------------------------------

/// Estimated task complexity.
///
/// PRD §14.2 types `complexity` as `Complexity`; §10.1 shows `"high"`.
/// The variants below are the levels demonstrated in the PRD examples and are
/// extensible as later phases require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    /// Low complexity.
    Low,
    /// Moderate complexity (default profile baseline).
    #[default]
    Medium,
    /// High complexity.
    High,
}

/// Consequence/risk level.
///
/// PRD §14.2 types `risk` as `RiskLevel`; §10.1 shows `"medium"`. Also used
/// as `risk_hint` in `ReasoningRequest` (PRD §14.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Low consequence.
    Low,
    /// Moderate consequence (default profile baseline).
    #[default]
    Medium,
    /// High consequence.
    High,
}

// ---------------------------------------------------------------------------
// TaskProfile (PRD §14.2)
// ---------------------------------------------------------------------------

/// Structured output of the Task Profiler (PRD §10.1, contract §14.2).
///
/// The wire contract follows §14.2 exactly. PRD §10.1 additionally lists
/// profiler-internal hints (`requires_environment_grounding`,
/// `requires_plan`, `candidate_count_hint`, expected output type, etc.);
/// those are profiler implementation details and are not part of the durable
/// §14.2 wire shape, so they are intentionally absent here.
///
/// Derives `PartialEq` but **not** `Eq` because `ambiguity: f32` is not `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskProfile {
    /// Task domain, e.g. `"coding"`.
    pub domain: TaskDomain,
    /// Estimated complexity.
    pub complexity: Complexity,
    /// Consequence/risk level.
    pub risk: RiskLevel,
    /// Ambiguity score in `[0.0, 1.0]` (PRD §14.2 types this as `f32`).
    pub ambiguity: f32,
    /// Whether external/environment evidence is required.
    pub requires_grounding: bool,
    /// Whether the task mutates the environment (files, shell state, …).
    pub requires_mutation: bool,
    /// Verifiers available for this task (e.g. `["cargo_test", "clippy"]`).
    pub available_verifiers: Vec<VerifierId>,
    /// Strategy the Policy Engine would recommend for this profile.
    pub recommended_strategy: ReasoningStrategy,
}

// ---------------------------------------------------------------------------
// ReasoningBudget (PRD §10.4 — exact field types)
// ---------------------------------------------------------------------------

/// Runtime reasoning budget envelope.
///
/// Field types follow PRD §10.4 verbatim: `max_parallel_branches`,
/// `max_search_depth`, and `max_repairs` are `u16`; the model/tool counts are
/// `u32`; token and wall-clock ceilings are `u64`.
///
/// The [`Default`] returns the `Balanced` preset (a middle-of-the-road
/// envelope). The [`ReasoningBudget::fast`], [`ReasoningBudget::balanced`],
/// and [`ReasoningBudget::deep`] constructors encode the values pinned by the
/// PRD §24 config example. Fields the PRD does not pin carry **Phase R3
/// calibrated baselines** (PRD §20 "Budget Curves") derived from observed
/// coding-agent behavior:
///
/// - **Output tokens per turn**: GLM-4.6 / Qwen3 coding turns produce
///   1.5–4 KiB output on refactor/verify tasks; we size the per-mode total
///   envelope as `max_model_calls × ~6 KiB` to absorb one repair cycle plus
///   headroom.
/// - **Tool calls**: balanced needs ~6 (one read + one verify + a write);
///   deep doubles that for exploration; maximum triples.
/// - **Wall-clock latency**: Fast is PRD-pinned at 30 s; Balanced at 3 min,
///   Deep at 8 min, Maximum at 15 min — sized for local model servers under
///   shared GPU load.
///
/// All four presets escalate monotonically so the mode surface is consistent
/// (Fast ≤ Balanced ≤ Deep ≤ Maximum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningBudget {
    /// Maximum provider model calls the orchestrator may issue.
    #[serde(default = "default_budget_max_model_calls")]
    pub max_model_calls: u32,
    /// Maximum total output tokens across all calls in the turn.
    #[serde(default = "default_budget_max_total_output_tokens")]
    pub max_total_output_tokens: u64,
    /// Maximum tool invocations across the turn.
    #[serde(default = "default_budget_max_tool_calls")]
    pub max_tool_calls: u32,
    /// Maximum parallel candidate/search branches.
    #[serde(default = "default_budget_max_parallel_branches")]
    pub max_parallel_branches: u16,
    /// Maximum search-tree depth for bounded-tree-search strategies.
    #[serde(default = "default_budget_max_search_depth")]
    pub max_search_depth: u16,
    /// Maximum verification→repair cycles.
    #[serde(default = "default_budget_max_repairs")]
    pub max_repairs: u16,
    /// Soft wall-clock ceiling in milliseconds.
    #[serde(default = "default_budget_max_wall_time_ms")]
    pub max_wall_time_ms: u64,
}

impl Default for ReasoningBudget {
    fn default() -> Self {
        Self::balanced()
    }
}

impl ReasoningBudget {
    /// `Fast` preset.
    ///
    /// PRD §24 pins: `max_model_calls = 1`, `max_repairs = 0`,
    /// `max_wall_time_ms = 30000`. The remaining fields are Phase R3
    /// calibrated baselines (PRD §20) — single pass, no parallelism, no
    /// search, a tight output envelope sized for one completion.
    #[must_use]
    pub const fn fast() -> Self {
        Self {
            max_model_calls: 1,
            // Phase R3 baseline: 1 call × 4 KiB headroom.
            max_total_output_tokens: 4_096,
            // Phase R3 baseline: single-pass fast allows one read + one write.
            max_tool_calls: 4,
            // Phase R3 baseline: fast is single-pass.
            max_parallel_branches: 1,
            // Phase R3 baseline: fast does not search.
            max_search_depth: 1,
            max_repairs: 0,
            max_wall_time_ms: 30_000,
        }
    }

    /// `Balanced` preset (also the [`Default`]).
    ///
    /// PRD §24 pins: `max_model_calls = 4`, `max_repairs = 1`,
    /// `max_parallel_branches = 2`. The remaining fields are Phase R3
    /// calibrated baselines (PRD §20) — output envelope sized for 4 model
    /// calls × ~6 KiB headroom, tool-call budget sized for a single
    /// read→verify→write cycle, wall-clock at 3 minutes for shared-GPU
    /// local servers.
    #[must_use]
    pub const fn balanced() -> Self {
        Self {
            max_model_calls: 4,
            // Phase R3 baseline: 4 calls × ~6 KiB.
            max_total_output_tokens: 24_576,
            // Phase R3 baseline: read + verify + one repair + write.
            max_tool_calls: 12,
            max_parallel_branches: 2,
            // Phase R3 baseline: balanced does not search.
            max_search_depth: 1,
            max_repairs: 1,
            // Phase R3 baseline: 3 minutes (shared-GPU headroom).
            max_wall_time_ms: 180_000,
        }
    }

    /// `Deep` preset.
    ///
    /// PRD §24 pins: `max_model_calls = 10`, `max_repairs = 2`,
    /// `max_parallel_branches = 3`, `max_search_depth = 3`. The remaining
    /// fields are Phase R3 calibrated baselines (PRD §20) — output envelope
    /// for 10 model calls × ~6.5 KiB, tool budget doubled vs. Balanced for
    /// exploration, wall-clock at 8 minutes.
    #[must_use]
    pub const fn deep() -> Self {
        Self {
            max_model_calls: 10,
            // Phase R3 baseline: 10 calls × ~6.5 KiB.
            max_total_output_tokens: 65_536,
            // Phase R3 baseline: explore + verify + 2 repairs + write.
            max_tool_calls: 32,
            max_parallel_branches: 3,
            max_search_depth: 3,
            max_repairs: 2,
            // Phase R3 baseline: 8 minutes (multi-branch exploration).
            max_wall_time_ms: 480_000,
        }
    }

    /// `Maximum` preset.
    ///
    /// The PRD §24 config block does not pin a `maximum` table. This
    /// constructor provides a conservative escalation above `Deep` so the
    /// [`ReasoningMode::Maximum`] surface has a defined envelope. All values
    /// are Phase R3 calibrated baselines (PRD §20): 16 model calls × 8 KiB,
    /// 60 tools, 4 parallel branches / 4 search depth, 3 repairs, 15-minute
    /// wall-clock ceiling.
    #[must_use]
    pub const fn maximum() -> Self {
        Self {
            max_model_calls: 16,
            // Phase R3 baseline: 16 calls × 8 KiB.
            max_total_output_tokens: 131_072,
            // Phase R3 baseline: full multi-tool exploration.
            max_tool_calls: 60,
            max_parallel_branches: 4,
            max_search_depth: 4,
            max_repairs: 3,
            // Phase R3 baseline: 15 minutes (highest test-time budget).
            max_wall_time_ms: 900_000,
        }
    }

    /// Returns the preset matching a [`ReasoningMode`], or `None` for
    /// `Auto`/`Off` (which defer to the policy engine or direct path).
    #[must_use]
    pub fn for_mode(mode: ReasoningMode) -> Option<Self> {
        match mode {
            ReasoningMode::Auto | ReasoningMode::Off => None,
            ReasoningMode::Fast => Some(Self::fast()),
            ReasoningMode::Balanced => Some(Self::balanced()),
            ReasoningMode::Deep => Some(Self::deep()),
            ReasoningMode::Maximum => Some(Self::maximum()),
        }
    }
}

// Conservative per-field serde defaults used when a config preset table is
// present but omits a field. These mirror the `Fast` baseline so a partial
// `[reasoning.fast]` is safe even before the named constructor runs. They are
// Phase R3 baselines (PRD §20), not PRD-pinned.
fn default_budget_max_model_calls() -> u32 {
    1
}
fn default_budget_max_total_output_tokens() -> u64 {
    4_096
}
fn default_budget_max_tool_calls() -> u32 {
    4
}
fn default_budget_max_parallel_branches() -> u16 {
    1
}
fn default_budget_max_search_depth() -> u16 {
    1
}
fn default_budget_max_repairs() -> u16 {
    0
}
fn default_budget_max_wall_time_ms() -> u64 {
    30_000
}

// ---------------------------------------------------------------------------
// Workflow Memory config (PRD §24 — `[reasoning.workflow_memory]`)
// ---------------------------------------------------------------------------

/// Workflow-learning configuration (PRD §24, `[reasoning.workflow_memory]`).
///
/// Defaults follow PRD §24: `enabled = true`, `require_approval = true`,
/// `revalidate_after_days = 30`. VRO-1 ships this contract only; the workflow
/// memory store is a later phase (PRD §10.10, VRO-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowMemoryConfig {
    /// Whether learned-workflow capture is active.
    #[serde(default = "default_wm_enabled")]
    pub enabled: bool,
    /// Whether captured workflows require explicit user approval before reuse.
    #[serde(default = "default_wm_require_approval")]
    pub require_approval: bool,
    /// Days after which a stored workflow must be revalidated before reuse.
    #[serde(default = "default_wm_revalidate_after_days")]
    pub revalidate_after_days: u32,
}

impl Default for WorkflowMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_approval: true,
            revalidate_after_days: 30,
        }
    }
}

fn default_wm_enabled() -> bool {
    true
}
fn default_wm_require_approval() -> bool {
    true
}
fn default_wm_revalidate_after_days() -> u32 {
    30
}

// ---------------------------------------------------------------------------
// ReasoningConfig (PRD §24 — the `[reasoning]` block)
// ---------------------------------------------------------------------------

/// The `[reasoning]` configuration block (PRD §24).
///
/// Preset values default to the PRD §24 example (`fast`/`balanced`/`deep`).
/// The master `enabled` flag defaults to **`false`** per the VRO-1 contract:
/// when disabled, the composition boundary must route every request through
/// the existing direct execution loop with zero behavior change (PRD §21,
/// VRO-1 exit criteria: "No behavior regression when disabled"). The PRD §24
/// *example* shows `enabled = true`, but that is an illustrative populated
/// config, not the shipped default for a developer-only feature flag (PRD §25,
/// Stage A).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    /// Master feature flag. `false` ⇒ direct execution, no VRO.
    #[serde(default)]
    pub enabled: bool,
    /// Default mode when the caller does not specify one.
    #[serde(default = "default_default_mode")]
    pub default_mode: ReasoningMode,
    /// Whether private/deliberation artifacts may be persisted.
    #[serde(default)]
    pub persist_private_reasoning: bool,
    /// Whether verification may cross providers (privacy boundary).
    #[serde(default)]
    pub allow_cross_provider_verification: bool,
    /// Hard ceiling on parallel branches across the whole orchestrator
    /// (PRD §24: `4`).
    #[serde(default = "default_max_global_parallel_branches")]
    pub max_global_parallel_branches: u16,
    /// `Fast` budget preset (PRD §24).
    #[serde(default = "ReasoningBudget::fast")]
    pub fast: ReasoningBudget,
    /// `Balanced` budget preset (PRD §24).
    #[serde(default = "ReasoningBudget::balanced")]
    pub balanced: ReasoningBudget,
    /// `Deep` budget preset (PRD §24).
    #[serde(default = "ReasoningBudget::deep")]
    pub deep: ReasoningBudget,
    /// Workflow-learning sub-config (PRD §24).
    #[serde(default)]
    pub workflow_memory: WorkflowMemoryConfig,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            // VRO-1 contract: disabled by default (zero behavior regression).
            enabled: false,
            default_mode: ReasoningMode::Auto,
            persist_private_reasoning: false,
            allow_cross_provider_verification: false,
            max_global_parallel_branches: 4,
            fast: ReasoningBudget::fast(),
            balanced: ReasoningBudget::balanced(),
            deep: ReasoningBudget::deep(),
            workflow_memory: WorkflowMemoryConfig::default(),
        }
    }
}

fn default_default_mode() -> ReasoningMode {
    ReasoningMode::Auto
}
fn default_max_global_parallel_branches() -> u16 {
    4
}

impl ReasoningConfig {
    /// Returns the budget preset for a manual mode, or the `Balanced` preset
    /// for `Auto`. `Off` returns the `Balanced` preset as well since no
    /// orchestrated turn runs under `Off` (the composition boundary takes the
    /// direct path before consulting this).
    #[must_use]
    pub fn preset_for(&self, mode: ReasoningMode) -> ReasoningBudget {
        match mode {
            ReasoningMode::Fast => self.fast,
            ReasoningMode::Balanced | ReasoningMode::Auto => self.balanced,
            ReasoningMode::Deep => self.deep,
            ReasoningMode::Maximum => ReasoningBudget::maximum(),
            ReasoningMode::Off => self.balanced,
        }
    }
}

// ---------------------------------------------------------------------------
// Section 14 data contracts (PRD §14.1, §14.3, §14.4, §14.5)
//
// VRO-2.1 ships the §14 wire contracts. Fields whose underlying types are not
// yet defined use documented placeholders (String aliases or
// `serde_json::Value`) per the directive; each is marked and deferred to a
// later phase. Real domain types are reused where they already exist
// (`SessionId`, `RequestId`, `CandidateId`, `RiskLevel`, `ReasoningBudget`,
// `ReasoningMode`). Structs containing `f32` (`VerificationResult.confidence`)
// or `serde_json::Value` derive `PartialEq` only, not `Eq`.
// ---------------------------------------------------------------------------

use crate::{CandidateId, RequestId, SessionId};

/// Privacy boundary for a reasoning request (PRD §14.1, §17).
///
/// Controls how far a request and its artifacts may travel. Defaults to
/// `Private` (the conservative posture: no cross-provider, no persistence of
/// private deliberation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivacyMode {
    /// In-process only; no cross-provider dispatch, no private-artifact persistence.
    #[default]
    Private,
    /// Within a single provider boundary.
    Internal,
    /// Cross-provider verification allowed (subject to
    /// `ReasoningConfig.allow_cross_provider_verification`).
    Public,
}

// ---------------------------------------------------------------------------
// Strict reference / assumption types (VRO-10, PRD §14.3)
//
// These replace the VRO-2.1 free-form `String` aliases with strict newtypes
// so the type system enforces "you cannot pass an arbitrary String where an
// Assumption / EvidenceRef / ContextRef is expected." Each newtype wraps a
// `String` payload but is its own type — the wire shape is preserved via
// `#[serde(transparent)]` so existing serialized artifacts continue to
// deserialize. `From<&str>` / `From<String>` / `AsRef<str>` impls keep
// ergonomic construction (`vec!["text".into()]`) working at every call
// site without weakening the use-site type contract.
// ---------------------------------------------------------------------------

/// What kind of context a [`ContextRef`] points at (VRO-10, PRD §14.1).
///
/// Carries the discrete kind tag the VRO-2.1 free-form string could not
/// express. `Other` retains forward compatibility for caller-defined kinds
/// (e.g. `"db-row"`, `"external-url"`) the orchestrator does not yet name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    /// A workspace file path (e.g. `"src/main.rs"`).
    #[default]
    File,
    /// A prior conversation message id.
    Message,
    /// A reasoning turn id within the session.
    Turn,
    /// A tool invocation result id.
    ToolResult,
    /// An external resource (URL, DB row, …) the orchestrator does not own.
    Other,
}

/// Reference to conversation or workspace context (PRD §14.1).
///
/// VRO-10 promotes this from a `String` placeholder to a strict newtype
/// carrying a [`ContextKind`] tag plus the locator string. Wire shape is
/// unchanged for plain string contexts: a bare `"file:src/main.rs"` JSON
/// string deserializes via [`ContextRef::from_locator`] semantics, while a
/// structured `{"kind":"file","locator":"src/main.rs"}` object round-trips
/// the new fields. Construction ergonomics are preserved via
/// `From<&str>` / `From<String>` (which default to [`ContextKind::Other`]
/// so the existing test fixtures keep working without modification).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct ContextRef {
    /// Discrete kind tag (file / message / turn / tool_result / other).
    #[serde(default)]
    pub kind: ContextKind,
    /// Locator payload (path, id, URL, …) — the former placeholder string.
    pub locator: String,
}

impl ContextRef {
    /// Builds a typed reference from an explicit kind + locator.
    #[must_use]
    pub fn new(kind: ContextKind, locator: impl Into<String>) -> Self {
        Self {
            kind,
            locator: locator.into(),
        }
    }

    /// Builds a reference from a locator only, defaulting the kind to
    /// [`ContextKind::Other`] (the VRO-2.1 wire shape).
    #[must_use]
    pub fn from_locator(locator: impl Into<String>) -> Self {
        Self {
            kind: ContextKind::Other,
            locator: locator.into(),
        }
    }

    /// The locator payload as `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.locator
    }
}

impl From<String> for ContextRef {
    fn from(locator: String) -> Self {
        Self::from_locator(locator)
    }
}

impl From<&str> for ContextRef {
    fn from(locator: &str) -> Self {
        Self::from_locator(locator)
    }
}

impl AsRef<str> for ContextRef {
    fn as_ref(&self) -> &str {
        &self.locator
    }
}

impl std::fmt::Display for ContextRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.locator)
    }
}

/// Status of a stated [`Assumption`] (VRO-10, PRD §14.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssumptionStatus {
    /// Assumed at planning time, not yet verified.
    #[default]
    Open,
    /// Confirmed by subsequent evidence.
    Confirmed,
    /// Contradicted by subsequent evidence.
    Refuted,
}

/// A stated assumption (PRD §14.3).
///
/// VRO-10 promotes this from a `String` placeholder to a strict newtype
/// carrying the assumption `statement`, an optional `confidence` in
/// `[0.0, 1.0]`, and a tri-state [`AssumptionStatus`]. Wire shape is
/// preserved via `From<&str>` / `From<String>` (which build an `Open`
/// assumption with no confidence) so existing call sites and test fixtures
/// keep compiling. Derives `PartialEq` but **not** `Eq` because
/// `confidence: Option<f32>` is not `Eq`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Assumption {
    /// The assumption text.
    pub statement: String,
    /// Optional confidence in `[0.0, 1.0]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Whether the assumption is still open, confirmed, or refuted.
    #[serde(default)]
    pub status: AssumptionStatus,
}

impl Assumption {
    /// Builds an `Open` assumption from a statement (the VRO-2.1 shape).
    #[must_use]
    pub fn open(statement: impl Into<String>) -> Self {
        Self {
            statement: statement.into(),
            confidence: None,
            status: AssumptionStatus::Open,
        }
    }

    /// Builds a fully-specified assumption.
    #[must_use]
    pub fn with_confidence(
        statement: impl Into<String>,
        confidence: f32,
        status: AssumptionStatus,
    ) -> Self {
        Self {
            statement: statement.into(),
            confidence: Some(confidence),
            status,
        }
    }

    /// The assumption text as `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.statement
    }
}

impl From<String> for Assumption {
    fn from(statement: String) -> Self {
        Self::open(statement)
    }
}

impl From<&str> for Assumption {
    fn from(statement: &str) -> Self {
        Self::open(statement)
    }
}

impl AsRef<str> for Assumption {
    fn as_ref(&self) -> &str {
        &self.statement
    }
}

impl std::fmt::Display for Assumption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.statement)
    }
}

/// What kind of evidence an [`EvidenceRef`] points at (VRO-10, PRD §14.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// Evidence captured from a workspace file.
    #[default]
    File,
    /// Evidence from a tool execution result.
    ToolResult,
    /// Evidence from a deterministic verifier (cargo, schema, …).
    Verifier,
    /// Evidence from a model-based verifier / critic.
    ModelVerifier,
    /// Evidence from an external source (URL, DB row, …).
    External,
    /// Caller-defined evidence kind.
    Other,
}

/// Reference to evidence backing a claim (PRD §14.3, §14.4, §10.8).
///
/// VRO-10 promotes this from a `String` placeholder to a strict newtype
/// carrying an [`EvidenceKind`] tag plus the locator string. Wire shape and
/// construction ergonomics are preserved via `From<&str>` / `From<String>`
/// (defaulting to [`EvidenceKind::Other`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// Discrete evidence kind tag.
    #[serde(default)]
    pub kind: EvidenceKind,
    /// Locator payload (e.g. `"tests/foo.rs:L42"`, `"cargo_test:passed"`).
    pub locator: String,
}

impl EvidenceRef {
    /// Builds a typed evidence reference.
    #[must_use]
    pub fn new(kind: EvidenceKind, locator: impl Into<String>) -> Self {
        Self {
            kind,
            locator: locator.into(),
        }
    }

    /// Builds an evidence reference from a locator only.
    #[must_use]
    pub fn from_locator(locator: impl Into<String>) -> Self {
        Self {
            kind: EvidenceKind::Other,
            locator: locator.into(),
        }
    }

    /// The locator payload as `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.locator
    }
}

impl From<String> for EvidenceRef {
    fn from(locator: String) -> Self {
        Self::from_locator(locator)
    }
}

impl From<&str> for EvidenceRef {
    fn from(locator: &str) -> Self {
        Self::from_locator(locator)
    }
}

impl AsRef<str> for EvidenceRef {
    fn as_ref(&self) -> &str {
        &self.locator
    }
}

impl std::fmt::Display for EvidenceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.locator)
    }
}

/// Severity of a single [`VerificationFinding`] (PRD §10.8).
///
/// Ordered low → high: `Info` < `Warning` < `Error` < `Critical`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationSeverity {
    /// Informational; does not affect pass/fail.
    #[default]
    Info,
    /// A non-blocking concern.
    Warning,
    /// A blocking problem.
    Error,
    /// A problem with severe consequence (data loss, security, …).
    Critical,
}

/// A single verification finding (PRD §10.8).
///
/// Upgraded from a `String` placeholder in VRO-2.2 to carry the directive's
/// `message` / `severity` / `location` fields. `location` is optional because
/// some findings (e.g. a missing dependency) are not tied to a source span.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VerificationFinding {
    /// Human-readable description of the finding.
    pub message: String,
    /// How serious this finding is.
    pub severity: VerificationSeverity,
    /// Optional source location such as `"src/lib.rs:42"`.
    pub location: Option<String>,
}

/// Provider-structured output payload (PRD §14.4, §14.5).
///
/// **Placeholder**: the raw provider-structured JSON. A typed
/// `StructuredOutput` (content parts + schema-conformance metadata) replaces
/// this in a later phase.
pub type StructuredOutput = serde_json::Value;

/// Verifier pass/fail status (PRD §10.8).
///
/// VRO-2.2 adds `Error`: the verifier itself could not run (e.g. `cargo` not
/// found, the command crashed, a timeout). This is distinct from `Failed`,
/// which means the verifier ran and found problems with the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationStatus {
    /// The verifier passed.
    #[default]
    Passed,
    /// The verifier ran and found problems with the target.
    Failed,
    /// The verifier itself could not run (cargo missing, crash, timeout).
    Error,
    /// The verifier was skipped (not applicable / unavailable).
    Skipped,
    /// The verifier could not reach a determination.
    Inconclusive,
}

/// Terminal status of a reasoning outcome (PRD §14.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutcomeStatus {
    /// The turn produced a verified final answer.
    #[default]
    Succeeded,
    /// The turn failed to produce a usable answer.
    Failed,
    /// The turn produced a partial answer with unresolved risks.
    Partial,
    /// The turn was cancelled.
    Cancelled,
    /// The budget was exhausted before completion.
    BudgetExceeded,
    /// The provider returned a rate-limit (HTTP 429) signal and the
    /// orchestrator halted rather than retrying into an unbounded busy loop
    /// (VRO-10, PRD §10.4: "account for provider rate limits").
    RateLimitExceeded,
    /// Verification could not reach a determination.
    Inconclusive,
}

/// A verifier response (PRD §10.8).
///
/// Field names follow §10.8 verbatim. Derives `PartialEq` but **not** `Eq`
/// because `confidence: f32` is not `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationResult {
    /// The verifier that produced this result (e.g. `"cargo_test"`).
    pub verifier_id: VerifierId,
    /// Pass/fail/inconclusive status.
    pub status: VerificationStatus,
    /// Verifier confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Human-readable findings.
    pub findings: Vec<VerificationFinding>,
    /// Evidence backing the result.
    pub evidence_refs: Vec<EvidenceRef>,
    /// Whether a failed check is repairable.
    pub repairable: bool,
}

/// What to do when a plan step's verification fails (VRO-10, PRD §10.5
/// "Failure policy").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepFailurePolicy {
    /// Retry the step in place up to `max_attempts` (default).
    #[default]
    RetryInPlace,
    /// Skip the step and continue with the rest of the plan.
    Skip,
    /// Halt the entire plan and surface the failure upstream.
    HaltPlan,
    /// Escalate to a different reasoning strategy.
    EscalateStrategy,
}

/// One step in a VRO workflow plan (PRD §10.5).
///
/// VRO-10 closes the §10.5 PARTIAL gap by adding the 5 fields the VRO-2.1
/// placeholder schema omitted: `expected_output_schema`,
/// `failure_policy`, `max_attempts`, `parallel_allowed`, and
/// `requires_user_approval`. Each maps verbatim to a §10.5 "Each step must
/// define" bullet. The 5 new fields carry conservative serde defaults so
/// existing serialized plans (which omit them) deserialize unchanged.
///
/// This is distinct from `crate::PlanStep` (the TUI plan-display step).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkflowPlanStep {
    /// Stable step identifier.
    pub id: String,
    /// What this step accomplishes.
    pub objective: String,
    /// Step IDs that must complete first.
    pub depends_on: Vec<String>,
    /// Tool names this step may invoke.
    pub tools: Vec<String>,
    /// Verifiers to run on this step's output.
    pub verify_with: Vec<VerifierId>,
    /// Expected output schema (PRD §10.5). Free-form descriptor (JSON-Schema
    /// fragment, type name, or `""` when unconstrained). Defaults to empty.
    #[serde(default)]
    pub expected_output_schema: String,
    /// Failure policy (PRD §10.5). Defaults to retry-in-place.
    #[serde(default)]
    pub failure_policy: StepFailurePolicy,
    /// Maximum attempts before the failure policy engages (PRD §10.5).
    /// Defaults to `1` (one shot, no automatic retry).
    #[serde(default = "default_step_max_attempts")]
    pub max_attempts: u16,
    /// Whether this step may run in parallel with its siblings (PRD §10.5).
    /// Defaults to `false` (sequential) for safety.
    #[serde(default)]
    pub parallel_allowed: bool,
    /// Whether explicit user approval is required before this step executes
    /// (PRD §10.5). Defaults to `false`.
    #[serde(default)]
    pub requires_user_approval: bool,
}

fn default_step_max_attempts() -> u16 {
    1
}

/// Placeholder cost accounting (PRD §14 references `InferenceCost`).
///
/// Minimal observable accounting surface; the concrete schema is deferred to a
/// later phase. Integer-only ⇒ derives `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InferenceCost {
    /// Provider model calls issued.
    pub model_calls: u32,
    /// Total tokens consumed.
    pub total_tokens: u64,
}

/// Observed capabilities of a provider/model combination (PRD §10.2, VRO-3.1).
///
/// Each field is a boolean capability flag (the directive's "boolean flags"
/// for `supports_native_tools` / `supports_structured_output` /
/// `supports_system_prompts`, plus the additional boolean capability axes from
/// PRD §10.2). Capabilities are **observed**, not assumed from a model family
/// name (PRD §10.2: "declare observed capabilities rather than relying only on
/// model family names"). [`ModelCapabilities::local_server_defaults`] returns a
/// reasonable baseline for a typical locally-served model; a real adapter
/// overrides fields from an empirical capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Native tool-call support in the chat template.
    pub supports_native_tools: bool,
    /// Tool calls emulated via prompt scaffolding (when native is absent).
    pub supports_emulated_tools: bool,
    /// Structured (JSON-mode / schema-constrained) output support.
    pub supports_structured_output: bool,
    /// System-prompt role support in the chat template.
    pub supports_system_prompts: bool,
    /// SSE/streaming response support.
    pub supports_streaming: bool,
    /// In-flight request cancellation support.
    pub supports_cancellation: bool,
    /// Multimodal vision input support.
    pub supports_vision: bool,
}

impl ModelCapabilities {
    /// Reasonable defaults for a typical model served by a local model server:
    /// system prompts and streaming are universally supported; structured
    /// output and cancellation are usually available; native tool calls and
    /// vision default off until probed.
    #[must_use]
    pub const fn local_server_defaults() -> Self {
        Self {
            supports_native_tools: false,
            supports_emulated_tools: true,
            supports_structured_output: true,
            supports_system_prompts: true,
            supports_streaming: true,
            supports_cancellation: true,
            supports_vision: false,
        }
    }
}

/// Placeholder verification roll-up (PRD §14 references `VerificationSummary`).
///
/// Minimal aggregate; the concrete schema is deferred to a later phase.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VerificationSummary {
    /// Number of verifiers that passed.
    pub passed: u32,
    /// Number of verifiers that failed.
    pub failed: u32,
    /// Overall verification status.
    pub overall: VerificationStatus,
}

/// A reasoning request entering the orchestrator (PRD §14.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningRequest {
    /// Stable request identity.
    pub request_id: RequestId,
    /// Owning session identity.
    pub session_id: SessionId,
    /// The user's raw message text.
    pub user_message: String,
    /// References to conversation/workspace context.
    pub context_refs: Vec<ContextRef>,
    /// Requested reasoning mode.
    pub mode: ReasoningMode,
    /// Caller-supplied risk hint, overriding the profiler when present.
    pub risk_hint: Option<RiskLevel>,
    /// Caller-supplied budget override.
    pub budget_override: Option<ReasoningBudget>,
    /// Privacy boundary for this request.
    pub privacy_mode: PrivacyMode,
}

/// A concise operational deliberation record (PRD §14.3).
///
/// This is **not** raw chain-of-thought (PRD §6.7): it is a structured,
/// bounded, auditable artifact.
///
/// Derives `PartialEq` but **not** `Eq` because `assumptions: Vec<Assumption>`
/// may carry `Option<f32>` confidence scores (VRO-10 strict Assumption type).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DeliberationArtifact {
    /// The distilled objective.
    pub objective: String,
    /// Hard constraints the solution must satisfy.
    pub constraints: Vec<String>,
    /// Assumptions made (each is a placeholder `Assumption` string).
    pub assumptions: Vec<Assumption>,
    /// The structured plan (placeholder `WorkflowPlanStep`s).
    pub plan: Vec<WorkflowPlanStep>,
    /// Evidence gathered.
    pub evidence: Vec<EvidenceRef>,
    /// Open questions that could not be resolved.
    pub unresolved_questions: Vec<String>,
}

/// One generated candidate answer (PRD §14.4).
///
/// Derives `PartialEq` but **not** `Eq` because it contains
/// [`StructuredOutput`] (`serde_json::Value`, not `Eq`) and
/// [`VerificationResult`] (`confidence: f32`, not `Eq`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Candidate identity.
    pub candidate_id: CandidateId,
    /// Strategy variant that produced this candidate.
    pub strategy_variant: String,
    /// The structured output payload (placeholder `serde_json::Value`).
    pub output: StructuredOutput,
    /// Evidence supporting this candidate.
    pub evidence: Vec<EvidenceRef>,
    /// Verification results for this candidate.
    pub verification: Vec<VerificationResult>,
    /// Cost consumed producing this candidate.
    pub cost: InferenceCost,
}

/// Terminal outcome of a reasoning turn (PRD §14.5).
///
/// Derives `PartialEq` but **not** `Eq` because `final_output` is
/// [`StructuredOutput`] (`serde_json::Value`, not `Eq`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningOutcome {
    /// Terminal status.
    pub status: OutcomeStatus,
    /// The final answer, when one was produced.
    pub final_output: Option<StructuredOutput>,
    /// The selected candidate, when one was chosen.
    pub selected_candidate: Option<CandidateId>,
    /// Verification roll-up.
    pub verification_summary: VerificationSummary,
    /// Risks that remain unresolved.
    pub unresolved_risks: Vec<String>,
    /// Total cost consumed by the turn.
    pub cost: InferenceCost,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- ReasoningMode ---

    #[test]
    fn reasoning_mode_default_is_auto_and_round_trips() {
        assert_eq!(ReasoningMode::default(), ReasoningMode::Auto);
        for mode in [
            ReasoningMode::Auto,
            ReasoningMode::Fast,
            ReasoningMode::Balanced,
            ReasoningMode::Deep,
            ReasoningMode::Maximum,
            ReasoningMode::Off,
        ] {
            let encoded = serde_json::to_string(&mode).unwrap();
            let decoded: ReasoningMode = serde_json::from_str(&encoded).unwrap();
            assert_eq!(
                decoded, mode,
                "mode {mode:?} did not round-trip via {encoded}"
            );
        }
    }

    #[test]
    fn reasoning_mode_serializes_kebab_case() {
        // PRD §8.1 names; serde rename_all = "kebab-case".
        assert_eq!(
            serde_json::to_string(&ReasoningMode::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&ReasoningMode::Maximum).unwrap(),
            "\"maximum\""
        );
        assert_eq!(
            serde_json::to_string(&ReasoningMode::Off).unwrap(),
            "\"off\""
        );
    }

    // --- ReasoningStrategy ---

    #[test]
    fn reasoning_strategy_default_is_direct_and_has_ten_variants() {
        assert_eq!(ReasoningStrategy::default(), ReasoningStrategy::Direct);
        // PRD §10.3 authoritative list — exactly these ten, snake_case.
        let all = [
            ReasoningStrategy::Direct,
            ReasoningStrategy::PlanThenAnswer,
            ReasoningStrategy::PlanExecuteVerify,
            ReasoningStrategy::GenerateVerifyRepair,
            ReasoningStrategy::ParallelCandidatesConsensus,
            ReasoningStrategy::ParallelCandidatesJudge,
            ReasoningStrategy::ToolGroundedReact,
            ReasoningStrategy::BoundedTreeSearch,
            ReasoningStrategy::ProposerCriticAdjudicator,
            ReasoningStrategy::WorkflowReplayWithVerification,
        ];
        for variant in all {
            let encoded = serde_json::to_string(&variant).unwrap();
            let decoded: ReasoningStrategy = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, variant, "strategy {variant:?} did not round-trip");
        }
    }

    #[test]
    fn reasoning_strategy_serializes_snake_case_per_prd_10_3() {
        assert_eq!(
            serde_json::to_string(&ReasoningStrategy::PlanExecuteVerify).unwrap(),
            "\"plan_execute_verify\""
        );
        assert_eq!(
            serde_json::to_string(&ReasoningStrategy::GenerateVerifyRepair).unwrap(),
            "\"generate_verify_repair\""
        );
        assert_eq!(
            serde_json::to_string(&ReasoningStrategy::WorkflowReplayWithVerification).unwrap(),
            "\"workflow_replay_with_verification\""
        );
    }

    // --- TaskProfile ---

    #[test]
    fn task_profile_round_trips_with_prd_example_shape() {
        let profile = TaskProfile {
            domain: TaskDomain::new("coding").unwrap(),
            complexity: Complexity::High,
            risk: RiskLevel::Medium,
            ambiguity: 0.6,
            requires_grounding: true,
            requires_mutation: false,
            available_verifiers: vec![
                VerifierId::new("cargo_check").unwrap(),
                VerifierId::new("cargo_test").unwrap(),
                VerifierId::new("clippy").unwrap(),
            ],
            recommended_strategy: ReasoningStrategy::PlanExecuteVerify,
        };
        let encoded = serde_json::to_string(&profile).unwrap();
        let decoded: TaskProfile = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, profile);
        // ambiguity is f32 — survives serialization.
        assert!((decoded.ambiguity - 0.6_f32).abs() < f32::EPSILON);
        // JSON shape sanity (camelCase field names).
        assert!(encoded.contains("\"requires_grounding\""));
        assert!(encoded.contains("\"requires_mutation\""));
        assert!(encoded.contains("\"recommended_strategy\":\"plan_execute_verify\""));
    }

    // --- ReasoningBudget: PRD §24 pinned values ---

    #[test]
    fn budget_fast_preset_matches_prd_24_pinned_values() {
        let fast = ReasoningBudget::fast();
        // PRD §24 [reasoning.fast].
        assert_eq!(fast.max_model_calls, 1);
        assert_eq!(fast.max_repairs, 0);
        assert_eq!(fast.max_wall_time_ms, 30_000);
    }

    #[test]
    fn budget_balanced_preset_matches_prd_24_pinned_values() {
        let balanced = ReasoningBudget::balanced();
        // PRD §24 [reasoning.balanced].
        assert_eq!(balanced.max_model_calls, 4);
        assert_eq!(balanced.max_repairs, 1);
        assert_eq!(balanced.max_parallel_branches, 2);
    }

    #[test]
    fn budget_deep_preset_matches_prd_24_pinned_values() {
        let deep = ReasoningBudget::deep();
        // PRD §24 [reasoning.deep].
        assert_eq!(deep.max_model_calls, 10);
        assert_eq!(deep.max_repairs, 2);
        assert_eq!(deep.max_parallel_branches, 3);
        assert_eq!(deep.max_search_depth, 3);
    }

    #[test]
    fn budget_presets_escalate_monotonically() {
        // Sanity for the VRO-1 baseline (not PRD-pinned) fields: they must
        // escalate fast < balanced < deep < maximum so the mode surface is
        // monotonic. R3 will replace these with measured curves.
        let f = ReasoningBudget::fast();
        let b = ReasoningBudget::balanced();
        let d = ReasoningBudget::deep();
        let m = ReasoningBudget::maximum();
        assert!(f.max_model_calls <= b.max_model_calls);
        assert!(b.max_model_calls <= d.max_model_calls);
        assert!(d.max_model_calls <= m.max_model_calls);
        assert!(f.max_total_output_tokens < b.max_total_output_tokens);
        assert!(b.max_total_output_tokens < d.max_total_output_tokens);
        assert!(d.max_total_output_tokens < m.max_total_output_tokens);
        assert!(f.max_tool_calls <= b.max_tool_calls);
        assert!(b.max_tool_calls <= d.max_tool_calls);
        assert!(d.max_tool_calls <= m.max_tool_calls);
    }

    #[test]
    fn budget_default_is_balanced() {
        assert_eq!(ReasoningBudget::default(), ReasoningBudget::balanced());
    }

    #[test]
    fn budget_for_mode_resolves_presets() {
        assert_eq!(
            ReasoningBudget::for_mode(ReasoningMode::Fast),
            Some(ReasoningBudget::fast())
        );
        assert_eq!(
            ReasoningBudget::for_mode(ReasoningMode::Deep),
            Some(ReasoningBudget::deep())
        );
        assert_eq!(
            ReasoningBudget::for_mode(ReasoningMode::Maximum),
            Some(ReasoningBudget::maximum())
        );
        // Auto/Off defer — no fixed preset.
        assert_eq!(ReasoningBudget::for_mode(ReasoningMode::Auto), None);
        assert_eq!(ReasoningBudget::for_mode(ReasoningMode::Off), None);
    }

    #[test]
    fn budget_round_trips() {
        let budget = ReasoningBudget::deep();
        let encoded = serde_json::to_string(&budget).unwrap();
        let decoded: ReasoningBudget = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, budget);
    }

    // --- ReasoningConfig: default + partial-parse behavior ---

    #[test]
    fn config_default_is_disabled_with_auto_mode_and_prd_presets() {
        let cfg = ReasoningConfig::default();
        // VRO-1 contract: disabled by default (zero behavior regression).
        assert!(!cfg.enabled);
        assert_eq!(cfg.default_mode, ReasoningMode::Auto);
        assert!(!cfg.persist_private_reasoning);
        assert!(!cfg.allow_cross_provider_verification);
        // PRD §24 global ceiling.
        assert_eq!(cfg.max_global_parallel_branches, 4);
        // Presets wired to PRD §24 values.
        assert_eq!(cfg.fast, ReasoningBudget::fast());
        assert_eq!(cfg.balanced, ReasoningBudget::balanced());
        assert_eq!(cfg.deep, ReasoningBudget::deep());
        // Workflow memory defaults (PRD §24).
        assert!(cfg.workflow_memory.enabled);
        assert!(cfg.workflow_memory.require_approval);
        assert_eq!(cfg.workflow_memory.revalidate_after_days, 30);
    }

    #[test]
    fn config_absent_block_yields_default() {
        // Parsing `{}` (no [reasoning] block at all) must produce the default
        // config, which is disabled with the PRD §24 preset values.
        let cfg: ReasoningConfig = serde_json::from_str("{}").unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.fast.max_model_calls, 1);
        assert_eq!(cfg.balanced.max_model_calls, 4);
        assert_eq!(cfg.deep.max_model_calls, 10);
    }

    #[test]
    fn config_partial_preset_table_fills_field_defaults() {
        // A [reasoning.fast] table that pins only the PRD-named fields must
        // fill the rest with the conservative field defaults (not zero).
        let json = r#"{
            "fast": { "max_model_calls": 1, "max_repairs": 0, "max_wall_time_ms": 30000 }
        }"#;
        let cfg: ReasoningConfig = serde_json::from_str(json).unwrap();
        let fast = cfg.fast;
        assert_eq!(fast.max_model_calls, 1);
        assert_eq!(fast.max_repairs, 0);
        assert_eq!(fast.max_wall_time_ms, 30_000);
        // Unspecified fields take conservative baselines, not zero.
        assert!(fast.max_total_output_tokens > 0);
        assert!(fast.max_tool_calls > 0);
        assert!(fast.max_parallel_branches > 0);
    }

    #[test]
    fn config_full_prd_24_example_parses_and_round_trips() {
        // The PRD §24 example with enabled=true must round-trip exactly.
        let toml_like_json = r#"{
            "enabled": true,
            "default_mode": "auto",
            "persist_private_reasoning": false,
            "allow_cross_provider_verification": false,
            "max_global_parallel_branches": 4,
            "fast": { "max_model_calls": 1, "max_repairs": 0, "max_wall_time_ms": 30000 },
            "balanced": { "max_model_calls": 4, "max_repairs": 1, "max_parallel_branches": 2 },
            "deep": { "max_model_calls": 10, "max_repairs": 2, "max_parallel_branches": 3, "max_search_depth": 3 },
            "workflow_memory": { "enabled": true, "require_approval": true, "revalidate_after_days": 30 }
        }"#;
        let cfg: ReasoningConfig = serde_json::from_str(toml_like_json).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.fast.max_repairs, 0);
        assert_eq!(cfg.balanced.max_parallel_branches, 2);
        assert_eq!(cfg.deep.max_search_depth, 3);
        // Round-trip stability.
        let encoded = serde_json::to_string(&cfg).unwrap();
        let decoded: ReasoningConfig = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, cfg);
    }

    #[test]
    fn config_preset_for_mode() {
        let cfg = ReasoningConfig::default();
        assert_eq!(cfg.preset_for(ReasoningMode::Fast), cfg.fast);
        assert_eq!(cfg.preset_for(ReasoningMode::Balanced), cfg.balanced);
        assert_eq!(cfg.preset_for(ReasoningMode::Deep), cfg.deep);
        assert_eq!(
            cfg.preset_for(ReasoningMode::Maximum),
            ReasoningBudget::maximum()
        );
        // Auto falls back to balanced.
        assert_eq!(cfg.preset_for(ReasoningMode::Auto), cfg.balanced);
    }

    // --- Complexity / RiskLevel serde ---

    #[test]
    fn complexity_and_risk_serialize_lowercase() {
        assert_eq!(
            serde_json::to_string(&Complexity::High).unwrap(),
            "\"high\""
        );
        assert_eq!(
            serde_json::to_string(&RiskLevel::Medium).unwrap(),
            "\"medium\""
        );
        // Defaults.
        assert_eq!(Complexity::default(), Complexity::Medium);
        assert_eq!(RiskLevel::default(), RiskLevel::Medium);
    }

    // --- §14 data contracts ---

    #[test]
    fn reasoning_request_round_trips_and_uses_real_ids() {
        let req = ReasoningRequest {
            request_id: RequestId::new("req-1").unwrap(),
            session_id: SessionId::new("sess-1").unwrap(),
            user_message: "refactor src/main.rs".into(),
            context_refs: vec!["file:src/main.rs".into()],
            mode: ReasoningMode::Auto,
            risk_hint: Some(RiskLevel::Medium),
            budget_override: Some(ReasoningBudget::balanced()),
            privacy_mode: PrivacyMode::Private,
        };
        let encoded = serde_json::to_string(&req).unwrap();
        let decoded: ReasoningRequest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, req);
        // Real domain IDs survive serialization as strings.
        assert!(encoded.contains("\"request_id\":\"req-1\""));
        assert!(encoded.contains("\"session_id\":\"sess-1\""));
    }

    #[test]
    fn privacy_mode_default_is_private_and_round_trips() {
        assert_eq!(PrivacyMode::default(), PrivacyMode::Private);
        for mode in [
            PrivacyMode::Private,
            PrivacyMode::Internal,
            PrivacyMode::Public,
        ] {
            let encoded = serde_json::to_string(&mode).unwrap();
            let decoded: PrivacyMode = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, mode);
        }
    }

    #[test]
    fn verification_result_round_trips_and_is_not_eq_due_to_f32() {
        let vr = VerificationResult {
            verifier_id: VerifierId::new("cargo_test").unwrap(),
            status: VerificationStatus::Failed,
            confidence: 0.12,
            findings: vec![VerificationFinding {
                message: "test foo::bar failed".into(),
                severity: VerificationSeverity::Error,
                location: Some("tests/foo.rs:7".into()),
            }],
            evidence_refs: vec!["file:tests/foo.rs:L7".into()],
            repairable: true,
        };
        let encoded = serde_json::to_string(&vr).unwrap();
        let decoded: VerificationResult = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, vr);
        assert!((decoded.confidence - 0.12_f32).abs() < f32::EPSILON);
    }

    #[test]
    fn candidate_round_trips_with_structured_output_placeholder() {
        let candidate = Candidate {
            candidate_id: CandidateId::new("cand-1").unwrap(),
            strategy_variant: "generate_verify_repair".into(),
            output: serde_json::json!({"answer": "42"}),
            evidence: vec!["file:src/lib.rs:L10".into()],
            verification: vec![VerificationResult {
                verifier_id: VerifierId::new("schema").unwrap(),
                status: VerificationStatus::Passed,
                confidence: 1.0,
                findings: vec![],
                evidence_refs: vec![],
                repairable: false,
            }],
            cost: InferenceCost {
                model_calls: 2,
                total_tokens: 1024,
            },
        };
        let encoded = serde_json::to_string(&candidate).unwrap();
        let decoded: Candidate = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, candidate);
    }

    #[test]
    fn deliberation_artifact_default_is_empty_and_round_trips() {
        let empty = DeliberationArtifact::default();
        assert!(empty.objective.is_empty());
        assert!(empty.constraints.is_empty());
        assert!(empty.plan.is_empty());

        let artifact = DeliberationArtifact {
            objective: "Migrate the session writer".into(),
            constraints: vec!["no behavior regression".into()],
            assumptions: vec!["tests cover the happy path".into()],
            plan: vec![WorkflowPlanStep {
                id: "step-1".into(),
                objective: "Audit the current writer".into(),
                depends_on: vec![],
                tools: vec!["read_file".into()],
                verify_with: vec![VerifierId::new("cargo_test").unwrap()],
                expected_output_schema: "audit_report".into(),
                failure_policy: super::StepFailurePolicy::RetryInPlace,
                max_attempts: 2,
                parallel_allowed: false,
                requires_user_approval: false,
            }],
            evidence: vec!["file:src/sessions/writer.rs".into()],
            unresolved_questions: vec!["Is SQLite required?".into()],
        };
        let encoded = serde_json::to_string(&artifact).unwrap();
        let decoded: DeliberationArtifact = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, artifact);
    }

    #[test]
    fn reasoning_outcome_round_trips() {
        let outcome = ReasoningOutcome {
            status: OutcomeStatus::Succeeded,
            final_output: Some(serde_json::json!({"summary": "done"})),
            selected_candidate: Some(CandidateId::new("cand-1").unwrap()),
            verification_summary: VerificationSummary {
                passed: 3,
                failed: 0,
                overall: VerificationStatus::Passed,
            },
            unresolved_risks: vec![],
            cost: InferenceCost {
                model_calls: 4,
                total_tokens: 8192,
            },
        };
        let encoded = serde_json::to_string(&outcome).unwrap();
        let decoded: ReasoningOutcome = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, outcome);
    }

    #[test]
    fn outcome_and_verification_status_enums_round_trip() {
        for status in [
            OutcomeStatus::Succeeded,
            OutcomeStatus::Failed,
            OutcomeStatus::Partial,
            OutcomeStatus::Cancelled,
            OutcomeStatus::BudgetExceeded,
            OutcomeStatus::Inconclusive,
        ] {
            let encoded = serde_json::to_string(&status).unwrap();
            let decoded: OutcomeStatus = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, status);
        }
        // VRO-2.2: VerificationStatus now includes Error (verifier could not run).
        for status in [
            VerificationStatus::Passed,
            VerificationStatus::Failed,
            VerificationStatus::Error,
            VerificationStatus::Skipped,
            VerificationStatus::Inconclusive,
        ] {
            let encoded = serde_json::to_string(&status).unwrap();
            let decoded: VerificationStatus = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn verification_finding_struct_round_trips_with_severity_and_location() {
        let finding = VerificationFinding {
            message: "cannot find function `foo` in this scope".into(),
            severity: VerificationSeverity::Error,
            location: Some("src/lib.rs:42".into()),
        };
        let encoded = serde_json::to_string(&finding).unwrap();
        let decoded: VerificationFinding = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, finding);
        // Serde shape (kebab-case severity).
        assert!(encoded.contains("\"severity\":\"error\""));
        assert!(encoded.contains("\"location\":\"src/lib.rs:42\""));
        // A finding without a location round-trips too.
        let no_loc = VerificationFinding {
            message: "missing dependency".into(),
            severity: VerificationSeverity::Warning,
            location: None,
        };
        let encoded = serde_json::to_string(&no_loc).unwrap();
        assert_eq!(
            serde_json::from_str::<VerificationFinding>(&encoded).unwrap(),
            no_loc
        );
    }

    #[test]
    fn verification_severity_is_ordered_and_serializes_kebab_case() {
        assert!(VerificationSeverity::Critical > VerificationSeverity::Error);
        assert!(VerificationSeverity::Error > VerificationSeverity::Warning);
        assert!(VerificationSeverity::Warning > VerificationSeverity::Info);
        for sev in [
            VerificationSeverity::Info,
            VerificationSeverity::Warning,
            VerificationSeverity::Error,
            VerificationSeverity::Critical,
        ] {
            let encoded = serde_json::to_string(&sev).unwrap();
            let decoded: VerificationSeverity = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, sev);
        }
        assert_eq!(VerificationSeverity::default(), VerificationSeverity::Info);
    }

    #[test]
    fn model_capabilities_round_trips_and_local_defaults_are_sensible() {
        let caps = ModelCapabilities::local_server_defaults();
        // A typical local model server supports these.
        assert!(caps.supports_system_prompts);
        assert!(caps.supports_streaming);
        assert!(caps.supports_structured_output);
        // Native tool calls / vision must be probed, not assumed.
        assert!(!caps.supports_native_tools);
        assert!(!caps.supports_vision);
        // Serde round-trip stability.
        let encoded = serde_json::to_string(&caps).unwrap();
        let decoded: ModelCapabilities = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, caps);
        assert_eq!(
            ModelCapabilities::default(),
            ModelCapabilities {
                supports_native_tools: false,
                supports_emulated_tools: false,
                supports_structured_output: false,
                supports_system_prompts: false,
                supports_streaming: false,
                supports_cancellation: false,
                supports_vision: false,
            }
        );
    }

    // ======================================================================
    // VRO-10 — strict §14.3 aliases (Assumption / EvidenceRef / ContextRef),
    // §10.5 planner fields, and §10.4 RateLimitExceeded outcome status.
    // ======================================================================

    #[test]
    fn context_ref_round_trips_with_kind_and_locator() {
        let ctx = ContextRef::new(ContextKind::File, "src/main.rs");
        assert_eq!(ctx.kind, ContextKind::File);
        assert_eq!(ctx.as_str(), "src/main.rs");
        let encoded = serde_json::to_string(&ctx).unwrap();
        let decoded: ContextRef = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, ctx);
    }

    #[test]
    fn context_ref_from_string_defaults_to_other_kind() {
        // Existing call sites that build ContextRef from a bare &str / String
        // keep working — the kind defaults to Other.
        let ctx: ContextRef = "file:src/main.rs".into();
        assert_eq!(ctx.kind, ContextKind::Other);
        assert_eq!(ctx.as_str(), "file:src/main.rs");
    }

    #[test]
    fn assumption_round_trips_with_confidence_and_status() {
        let a = Assumption::with_confidence("DB is SQLite", 0.8, AssumptionStatus::Open);
        let encoded = serde_json::to_string(&a).unwrap();
        let decoded: Assumption = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, a);
        assert_eq!(decoded.status, AssumptionStatus::Open);
        assert!((decoded.confidence.unwrap() - 0.8_f32).abs() < f32::EPSILON);
    }

    #[test]
    fn assumption_from_string_defaults_to_open_no_confidence() {
        let a: Assumption = "tests cover happy path".into();
        assert_eq!(a.status, AssumptionStatus::Open);
        assert!(a.confidence.is_none());
        assert_eq!(a.as_str(), "tests cover happy path");
    }

    #[test]
    fn evidence_ref_round_trips_with_kind_and_locator() {
        let er = EvidenceRef::new(EvidenceKind::Verifier, "cargo_test:passed");
        assert_eq!(er.kind, EvidenceKind::Verifier);
        let encoded = serde_json::to_string(&er).unwrap();
        let decoded: EvidenceRef = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, er);
    }

    #[test]
    fn evidence_ref_from_string_defaults_to_other_kind() {
        let er: EvidenceRef = "file:tests/foo.rs:L42".into();
        assert_eq!(er.kind, EvidenceKind::Other);
        assert_eq!(er.as_str(), "file:tests/foo.rs:L42");
    }

    #[test]
    fn workflow_plan_step_round_trips_with_five_new_planner_fields() {
        // VRO-10 §10.5: every "Each step must define" bullet is now present.
        let step = WorkflowPlanStep {
            id: "implement".into(),
            objective: "Apply the approved design".into(),
            depends_on: vec!["design_change".into()],
            tools: vec!["patch".into()],
            verify_with: vec![VerifierId::new("cargo_test").unwrap()],
            expected_output_schema: "diff_patch".into(),
            failure_policy: StepFailurePolicy::RetryInPlace,
            max_attempts: 3,
            parallel_allowed: true,
            requires_user_approval: false,
        };
        let encoded = serde_json::to_string(&step).unwrap();
        let decoded: WorkflowPlanStep = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, step);
        // The five new fields survive the round trip.
        assert_eq!(decoded.expected_output_schema, "diff_patch");
        assert_eq!(decoded.failure_policy, StepFailurePolicy::RetryInPlace);
        assert_eq!(decoded.max_attempts, 3);
        assert!(decoded.parallel_allowed);
        assert!(!decoded.requires_user_approval);
    }

    #[test]
    fn workflow_plan_step_deserializes_vro_2_1_payload_without_new_fields() {
        // A serialized plan from the VRO-2.1 era (which omitted the 5 new
        // fields) must deserialize with conservative defaults.
        let legacy = r#"{
            "id": "old-step",
            "objective": "do something",
            "depends_on": [],
            "tools": [],
            "verify_with": []
        }"#;
        let step: WorkflowPlanStep = serde_json::from_str(legacy).unwrap();
        assert_eq!(step.id, "old-step");
        assert_eq!(step.expected_output_schema, "");
        assert_eq!(step.failure_policy, StepFailurePolicy::RetryInPlace);
        assert_eq!(step.max_attempts, 1);
        assert!(!step.parallel_allowed);
        assert!(!step.requires_user_approval);
    }

    #[test]
    fn outcome_status_includes_rate_limit_exceeded_variant() {
        // VRO-10 §10.4: RateLimitExceeded is a distinct terminal status.
        let encoded = serde_json::to_string(&OutcomeStatus::RateLimitExceeded).unwrap();
        assert_eq!(encoded, "\"rate-limit-exceeded\"");
        let decoded: OutcomeStatus = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, OutcomeStatus::RateLimitExceeded);
        // All seven variants round-trip.
        for status in [
            OutcomeStatus::Succeeded,
            OutcomeStatus::Failed,
            OutcomeStatus::Partial,
            OutcomeStatus::Cancelled,
            OutcomeStatus::BudgetExceeded,
            OutcomeStatus::RateLimitExceeded,
            OutcomeStatus::Inconclusive,
        ] {
            let encoded = serde_json::to_string(&status).unwrap();
            let decoded: OutcomeStatus = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, status);
        }
    }
}
