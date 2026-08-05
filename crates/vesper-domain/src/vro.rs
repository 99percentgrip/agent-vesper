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
/// PRD §24 config example; fields the PRD does not pin carry conservative
/// VRO-1 baselines (documented per-constructor) to be tuned in research
/// phase R3 ("Budget Curves", PRD §20).
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
    /// `max_wall_time_ms = 30000`. The remaining fields are VRO-1 conservative
    /// baselines (single pass, no parallelism, no search) — not PRD-pinned;
    /// tune in research phase R3.
    #[must_use]
    pub const fn fast() -> Self {
        Self {
            max_model_calls: 1,
            // VRO-1 baseline (not PRD-pinned).
            max_total_output_tokens: 8_192,
            // VRO-1 baseline (not PRD-pinned).
            max_tool_calls: 8,
            // VRO-1 baseline (not PRD-pinned): fast is single-pass.
            max_parallel_branches: 1,
            // VRO-1 baseline (not PRD-pinned): fast does not search.
            max_search_depth: 1,
            max_repairs: 0,
            max_wall_time_ms: 30_000,
        }
    }

    /// `Balanced` preset (also the [`Default`]).
    ///
    /// PRD §24 pins: `max_model_calls = 4`, `max_repairs = 1`,
    /// `max_parallel_branches = 2`. The remaining fields are VRO-1
    /// conservative baselines — not PRD-pinned; tune in research phase R3.
    #[must_use]
    pub const fn balanced() -> Self {
        Self {
            max_model_calls: 4,
            // VRO-1 baseline (not PRD-pinned).
            max_total_output_tokens: 16_384,
            // VRO-1 baseline (not PRD-pinned).
            max_tool_calls: 20,
            max_parallel_branches: 2,
            // VRO-1 baseline (not PRD-pinned).
            max_search_depth: 1,
            max_repairs: 1,
            // VRO-1 baseline (not PRD-pinned).
            max_wall_time_ms: 120_000,
        }
    }

    /// `Deep` preset.
    ///
    /// PRD §24 pins: `max_model_calls = 10`, `max_repairs = 2`,
    /// `max_parallel_branches = 3`, `max_search_depth = 3`. The remaining
    /// fields are VRO-1 conservative baselines — not PRD-pinned; tune in
    /// research phase R3.
    #[must_use]
    pub const fn deep() -> Self {
        Self {
            max_model_calls: 10,
            // VRO-1 baseline (not PRD-pinned).
            max_total_output_tokens: 32_768,
            // VRO-1 baseline (not PRD-pinned).
            max_tool_calls: 40,
            max_parallel_branches: 3,
            max_search_depth: 3,
            max_repairs: 2,
            // VRO-1 baseline (not PRD-pinned).
            max_wall_time_ms: 300_000,
        }
    }

    /// `Maximum` preset.
    ///
    /// The PRD §24 config block does not pin a `maximum` table. This
    /// constructor provides a conservative escalation above `Deep` so the
    /// [`ReasoningMode::Maximum`] surface has a defined envelope. All values
    /// are VRO-1 baselines — not PRD-pinned; tune in research phase R3.
    #[must_use]
    pub const fn maximum() -> Self {
        Self {
            max_model_calls: 16,
            max_total_output_tokens: 65_536,
            max_tool_calls: 60,
            max_parallel_branches: 4,
            max_search_depth: 4,
            max_repairs: 3,
            max_wall_time_ms: 600_000,
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
// VRO-1 baselines, not PRD-pinned.
fn default_budget_max_model_calls() -> u32 {
    1
}
fn default_budget_max_total_output_tokens() -> u64 {
    8_192
}
fn default_budget_max_tool_calls() -> u32 {
    8
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
            assert_eq!(decoded, mode, "mode {mode:?} did not round-trip via {encoded}");
        }
    }

    #[test]
    fn reasoning_mode_serializes_kebab_case() {
        // PRD §8.1 names; serde rename_all = "kebab-case".
        assert_eq!(serde_json::to_string(&ReasoningMode::Auto).unwrap(), "\"auto\"");
        assert_eq!(
            serde_json::to_string(&ReasoningMode::Maximum).unwrap(),
            "\"maximum\""
        );
        assert_eq!(serde_json::to_string(&ReasoningMode::Off).unwrap(), "\"off\"");
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
        assert_eq!(cfg.preset_for(ReasoningMode::Maximum), ReasoningBudget::maximum());
        // Auto falls back to balanced.
        assert_eq!(cfg.preset_for(ReasoningMode::Auto), cfg.balanced);
    }

    // --- Complexity / RiskLevel serde ---

    #[test]
    fn complexity_and_risk_serialize_lowercase() {
        assert_eq!(serde_json::to_string(&Complexity::High).unwrap(), "\"high\"");
        assert_eq!(serde_json::to_string(&RiskLevel::Medium).unwrap(), "\"medium\"");
        // Defaults.
        assert_eq!(Complexity::default(), Complexity::Medium);
        assert_eq!(RiskLevel::default(), RiskLevel::Medium);
    }
}
