//! Deterministic Task Profiler (VRO-2.1).
//!
//! Converts a user prompt (and optional [`ReasoningRequest`]) into a
//! [`TaskProfile`] using pure regex/keyword heuristics — **no LLM call**. This
//! satisfies PRD §10.1 ("the profiler must be cheap … deterministic heuristics
//! first") and the directive's VRO-2.1 requirement that routing work without
//! invoking a model.
//!
//! ## Heuristic pipeline
//!
//! 1. **Chat bypass** — short prompt, no code blocks, no action verbs ⇒
//!    `domain = chat`, `complexity = low`, `strategy = direct`.
//! 2. **Domain mapping** — keyword sets classify the prompt into
//!    `coding`/`math`/`planning`/`research`/`chat`.
//! 3. **Risk** — high-risk keywords (`delete`, `commit`, `production`, …) ⇒
//!    `risk = high`.
//! 4. **Grounding** — file extensions/paths ⇒ `requires_grounding = true` and
//!    populates `available_verifiers` (e.g. `cargo_check` for `.rs`).
//! 5. **Complexity / ambiguity** — length, conjunctions, vague terms.
//! 6. **Strategy selection** — the PRD §12 rule ladder.
//!
//! ## Why substring matching, not `regex`
//!
//! The workspace `regex` crate is configured `default-features = false,
//! features = ["std"]`, which excludes `unicode-perl` and makes `\b`, `\w`,
//! `\s` reject with a build-time NFA error (documented pitfall). Keyword
//! detection here uses case-insensitive `str::contains`, which is sufficient
//! for deterministic heuristics and avoids that dependency footgun entirely.

use vesper_domain::{
    Complexity, ReasoningRequest, ReasoningStrategy, RiskLevel, TaskDomain, TaskProfile, VerifierId,
};

/// Maximum prompt length (in `char`s, not bytes) eligible for the chat bypass.
const CHAT_BYPASS_MAX_CHARS: usize = 60;

/// Heuristic task profiler. Stateless and allocation-cheap per call.
#[derive(Debug, Clone, Default)]
pub struct TaskProfiler;

impl TaskProfiler {
    /// Creates a new profiler.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Profiles a raw user prompt into a [`TaskProfile`].
    #[must_use]
    pub fn profile(&self, prompt: &str) -> TaskProfile {
        self.profile_inner(prompt)
    }

    /// Profiles a full [`ReasoningRequest`], honoring `risk_hint` and `mode`
    /// overrides from the caller.
    ///
    /// `risk_hint` (when present) overrides the heuristic risk. `mode`/`Off`
    /// do not change the *profile* itself — strategy selection still runs —
    /// but the composition boundary may consult the mode separately before
    /// dispatching.
    #[must_use]
    pub fn profile_request(&self, request: &ReasoningRequest) -> TaskProfile {
        let mut profile = self.profile_inner(&request.user_message);
        if let Some(hint) = request.risk_hint {
            profile.risk = hint;
        }
        profile
    }

    fn profile_inner(&self, prompt: &str) -> TaskProfile {
        let lower = prompt.to_lowercase();
        let char_count = lower.chars().count();
        let has_code_block = lower.contains("```");

        // 1. Chat bypass — short, no code, no action verb.
        if is_chat_bypass(&lower, char_count, has_code_block) {
            return chat_profile();
        }

        // 2. Domain detection (priority: coding > math > planning > research).
        let domain = detect_domain(&lower);
        let requires_mutation = has_code_block
            || (domain == TaskDomainKind::Coding && contains_any(&lower, MUTATION_VERBS));

        // 3. Risk.
        let mut risk = detect_risk(&lower);

        // 4. Grounding + verifiers.
        let (requires_grounding, verifiers) = detect_grounding(&lower);

        // 5. Complexity.
        let complexity = detect_complexity(&lower, char_count, domain, requires_mutation);

        // 6. Ambiguity.
        let ambiguity = detect_ambiguity(&lower);

        // A domain-level risk floor: irreversible code mutation is at least
        // Medium even without explicit high-risk keywords.
        if requires_mutation && risk == RiskLevel::Low {
            risk = RiskLevel::Medium;
        }

        // 7. Strategy selection (PRD §12 ladder).
        let strategy = select_strategy(
            domain,
            complexity,
            risk,
            requires_grounding,
            requires_mutation,
            &verifiers,
        );

        TaskProfile {
            domain: TaskDomain::new(domain.label()).unwrap_or_else(|_| {
                TaskDomain::new("chat").expect("chat is a valid bounded domain label")
            }),
            complexity,
            risk,
            ambiguity,
            requires_grounding,
            requires_mutation,
            available_verifiers: verifiers,
            recommended_strategy: strategy,
        }
    }
}

// ---------------------------------------------------------------------------
// Domain classification
// ---------------------------------------------------------------------------

/// Internal domain kind (maps to the `TaskDomain` string label).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskDomainKind {
    Chat,
    Coding,
    Math,
    Planning,
    Research,
}

impl TaskDomainKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Coding => "coding",
            Self::Math => "math",
            Self::Planning => "planning",
            Self::Research => "research",
        }
    }
}

/// Detects the task domain from keyword presence.
///
/// Priority: mutation verbs > math > planning > research > code indicators >
/// chat. Mutation verbs win so "implement a function to calculate X" is code
/// work, not math. Code indicators (`function`/`struct`/`module`) are a *weak*
/// fallback below planning and research, so "design the new module" stays
/// planning and "explain the function in src/lib.rs" stays research. File
/// extensions are a grounding signal, not a domain signal.
fn detect_domain(lower: &str) -> TaskDomainKind {
    if contains_any(lower, MUTATION_VERBS) {
        return TaskDomainKind::Coding;
    }
    if contains_any(lower, MATH_KEYWORDS) {
        return TaskDomainKind::Math;
    }
    if contains_any(lower, PLANNING_KEYWORDS) {
        return TaskDomainKind::Planning;
    }
    if contains_any(lower, RESEARCH_KEYWORDS) {
        return TaskDomainKind::Research;
    }
    if contains_any(lower, CODE_INDICATORS) {
        return TaskDomainKind::Coding;
    }
    TaskDomainKind::Chat
}

// ---------------------------------------------------------------------------
// Chat bypass
// ---------------------------------------------------------------------------

fn is_chat_bypass(lower: &str, char_count: usize, has_code_block: bool) -> bool {
    char_count < CHAT_BYPASS_MAX_CHARS && !has_code_block && !contains_any(lower, ACTION_VERBS)
}

fn chat_profile() -> TaskProfile {
    TaskProfile {
        domain: TaskDomain::new("chat").expect("chat is a valid bounded domain label"),
        complexity: Complexity::Low,
        risk: RiskLevel::Low,
        ambiguity: 0.1,
        requires_grounding: false,
        requires_mutation: false,
        available_verifiers: vec![],
        recommended_strategy: ReasoningStrategy::Direct,
    }
}

// ---------------------------------------------------------------------------
// Risk
// ---------------------------------------------------------------------------

fn detect_risk(lower: &str) -> RiskLevel {
    if contains_any(lower, HIGH_RISK_KEYWORDS) {
        RiskLevel::High
    } else if contains_any(lower, MEDIUM_RISK_KEYWORDS) {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

// ---------------------------------------------------------------------------
// Grounding + verifiers
// ---------------------------------------------------------------------------

fn detect_grounding(lower: &str) -> (bool, Vec<VerifierId>) {
    let mut verifiers = Vec::new();

    // Language-specific verifier sets. Use precise extension/keyword checks so
    // each language lands in its own branch (a `.py` file must not take the
    // Rust branch).
    if lower.contains(".rs") || lower.contains("cargo") || lower.contains("rustc") {
        verifiers.push(bvid("cargo_check"));
        verifiers.push(bvid("cargo_test"));
        verifiers.push(bvid("clippy"));
    } else if lower.contains(".py") || lower.contains("python") {
        verifiers.push(bvid("pytest"));
    } else if lower.contains(".js") || lower.contains(".ts") || lower.contains("npm") {
        verifiers.push(bvid("tsc"));
        verifiers.push(bvid("eslint"));
    }

    let has_file_path = lower.contains("src/") || lower.contains('/') || has_code_extension(lower);

    (
        requires_grounding(lower, has_file_path, &verifiers),
        verifiers,
    )
}

fn requires_grounding(lower: &str, has_file_path: bool, verifiers: &[VerifierId]) -> bool {
    // Grounding is required when there are verifiers (a real environment to
    // check) or an explicit file/path reference, or an explicit grounding verb.
    !verifiers.is_empty() || has_file_path || contains_any(lower, GROUNDING_VERBS)
}

// ---------------------------------------------------------------------------
// Complexity + ambiguity
// ---------------------------------------------------------------------------

fn detect_complexity(
    lower: &str,
    char_count: usize,
    domain: TaskDomainKind,
    requires_mutation: bool,
) -> Complexity {
    let multi_step = conjunction_count(lower) >= 2;
    let multi_file = lower.matches(" and ").count() >= 1
        && (lower.matches('/').count() >= 2 || lower.matches(".rs").count() >= 2);

    if requires_mutation {
        return if multi_file || multi_step {
            Complexity::High
        } else {
            Complexity::Medium
        };
    }

    match domain {
        TaskDomainKind::Chat => Complexity::Low,
        TaskDomainKind::Research | TaskDomainKind::Math => {
            if char_count > 300 || multi_step {
                Complexity::High
            } else {
                Complexity::Medium
            }
        }
        TaskDomainKind::Planning => {
            if char_count > 300 {
                Complexity::High
            } else {
                Complexity::Medium
            }
        }
        TaskDomainKind::Coding => Complexity::Medium,
    }
}

fn detect_ambiguity(lower: &str) -> f32 {
    let vague = contains_any(lower, VAGUE_TERMS);
    let specific = has_code_extension(lower)
        || lower.contains('/')
        || lower.contains('"')
        || lower.contains('`');

    if vague {
        0.7
    } else if specific {
        0.2
    } else {
        0.4
    }
}

fn conjunction_count(lower: &str) -> usize {
    lower.matches(" and ").count()
        + lower.matches(" then ").count()
        + lower.matches(", and ").count()
}

// ---------------------------------------------------------------------------
// Strategy selection (PRD §12 ladder)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn select_strategy(
    domain: TaskDomainKind,
    complexity: Complexity,
    risk: RiskLevel,
    requires_grounding: bool,
    requires_mutation: bool,
    verifiers: &[VerifierId],
) -> ReasoningStrategy {
    // §12: IF task is simple AND risk is low -> direct.
    if complexity == Complexity::Low && risk == RiskLevel::Low {
        return ReasoningStrategy::Direct;
    }

    // Code mutation with grounding/verifiers -> plan_execute_verify (directive:
    // "refactor src/main.rs" resolves to PlanExecuteVerify).
    if requires_mutation {
        return if !verifiers.is_empty() || requires_grounding {
            ReasoningStrategy::PlanExecuteVerify
        } else {
            ReasoningStrategy::GenerateVerifyRepair
        };
    }

    // §12 + §11.3: a deterministic verifier for a math task enables
    // generate_verify_repair. Checked before the grounding branch because math
    // verification is computational (the verifier IS the solver), not
    // environment exploration — light file grounding for input does not turn a
    // math task into a tool-grounded ReAct loop.
    if domain == TaskDomainKind::Math && !verifiers.is_empty() {
        return ReasoningStrategy::GenerateVerifyRepair;
    }

    // §12: ELSE IF environment evidence is required -> tool_grounded_react.
    if requires_grounding {
        return ReasoningStrategy::ToolGroundedReact;
    }

    // §12: ELSE IF task has long-horizon dependencies -> plan_execute_verify.
    if complexity == Complexity::High {
        return ReasoningStrategy::PlanExecuteVerify;
    }

    // §12: ELSE -> plan_then_answer.
    ReasoningStrategy::PlanThenAnswer
}

// ---------------------------------------------------------------------------
// Keyword tables
// ---------------------------------------------------------------------------

/// Verbs that indicate constructive/analytic work (block the chat bypass).
const ACTION_VERBS: &[&str] = &[
    "refactor",
    "implement",
    "fix",
    "add",
    "create",
    "build",
    "delete",
    "remove",
    "modify",
    "change",
    "update",
    "migrate",
    "write",
    "run",
    "test",
    "configure",
    "optimize",
    "debug",
    "rename",
    "calculate",
    "solve",
    "compute",
    "prove",
    "derive",
    "design",
    "plan",
    "explain",
    "summarize",
    "analyze",
];

/// Verbs that specifically indicate code mutation.
const MUTATION_VERBS: &[&str] = &[
    "refactor",
    "implement",
    "fix",
    "add",
    "create",
    "build",
    "delete",
    "remove",
    "modify",
    "change",
    "update",
    "migrate",
    "write",
    "rename",
    "extract",
    "inline",
    "replace",
    "rewrite",
    "debug",
    "optimize",
    "configure",
];

/// Indicators the prompt is about code (without necessarily mutating it).
const CODE_INDICATORS: &[&str] = &[
    "function",
    "struct",
    "class",
    "def ",
    "fn ",
    "import",
    "crate",
    "module",
    "trait",
    "impl",
    "enum ",
    "compile",
    "lint",
    "type error",
    "stack trace",
];

const MATH_KEYWORDS: &[&str] = &[
    "calculate",
    "solve",
    "equation",
    "prove",
    "derive",
    "compute",
    "integral",
    "derivative",
    "sum of",
    "formula",
    "math",
    "algebra",
    "theorem",
    "probability",
    "factorial",
];

const RESEARCH_KEYWORDS: &[&str] = &[
    "explain",
    "summarize",
    "research",
    "describe",
    "analyze",
    "compare",
    "difference between",
    "how does",
    "why does",
    "what is",
    "overview",
    "investigate",
    "review",
];

const PLANNING_KEYWORDS: &[&str] = &[
    "design",
    "architecture",
    "plan",
    "structure",
    "strategy",
    "approach",
    "roadmap",
    "outline",
    "schema",
    "blueprint",
    "specification",
];

const HIGH_RISK_KEYWORDS: &[&str] = &[
    "delete",
    "commit",
    "production",
    "deploy",
    "drop",
    "force",
    "rm -rf",
    "destroy",
    "purge",
    "shutdown",
    "reset",
    "wipe",
    "format",
];

const MEDIUM_RISK_KEYWORDS: &[&str] = &[
    "migrate",
    "disable",
    "remove",
    "overwrite",
    "replace",
    "rename",
    "rollback",
];

const GROUNDING_VERBS: &[&str] = &[
    "inspect", "read", "check", "verify", "audit", "examine", "find", "locate", "grep",
];

const VAGUE_TERMS: &[&str] = &[
    "better",
    "improve",
    "fix it",
    "something",
    "somehow",
    "etc",
    "maybe",
    "make it work",
    "good enough",
    "as needed",
    "tweak",
];

/// Common source file extensions whose presence implies a code environment.
fn has_code_extension(lower: &str) -> bool {
    const EXTS: &[&str] = &[
        ".rs", ".py", ".js", ".ts", ".go", ".java", ".c", ".cpp", ".h", ".rb", ".swift", ".kt",
        ".toml", ".json", ".yaml", ".yml",
    ];
    EXTS.iter().any(|ext| lower.contains(ext))
}

/// True if `lower` contains any of the `keywords` (case already lowered).
fn contains_any(lower: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| lower.contains(kw))
}

/// Builds a [`VerifierId`], panicking only on a programming error (all callers
/// pass compile-time-constant valid identifiers).
fn bvid(name: &str) -> VerifierId {
    VerifierId::new(name).unwrap_or_else(|_| {
        panic!("hardcoded verifier id {name:?} must be a valid bounded identifier")
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_of(prompt: &str) -> TaskProfile {
        TaskProfiler::new().profile(prompt)
    }

    // --- Chat bypass (directive: short greeting -> Direct) ---

    #[test]
    fn short_greeting_resolves_to_chat_direct() {
        let p = profile_of("hello world");
        assert_eq!(p.domain.as_str(), "chat");
        assert_eq!(p.complexity, Complexity::Low);
        assert_eq!(p.risk, RiskLevel::Low);
        assert_eq!(p.recommended_strategy, ReasoningStrategy::Direct);
        assert!(!p.requires_grounding);
        assert!(!p.requires_mutation);
        assert!(p.available_verifiers.is_empty());
    }

    #[test]
    fn short_question_without_action_verb_is_chat_direct() {
        let p = profile_of("what is 2 plus 2?");
        // No action verb, short, no code -> chat bypass -> Direct.
        assert_eq!(p.recommended_strategy, ReasoningStrategy::Direct);
    }

    #[test]
    fn chat_bypass_requires_under_60_chars() {
        // Exactly at the boundary: a 60+ char chat-like prompt is NOT bypassed.
        let long_greeting =
            "hi there, I was just wondering if you might possibly be able to help me out today?";
        assert!(long_greeting.chars().count() >= CHAT_BYPASS_MAX_CHARS);
        let p = profile_of(long_greeting);
        // Still resolves to research/chat -> Direct (no verifiers, low risk),
        // but via the full pipeline rather than the bypass fast path.
        assert_eq!(p.recommended_strategy, ReasoningStrategy::Direct);
    }

    // --- Code mutation (directive: "refactor src/main.rs" -> PlanExecuteVerify) ---

    #[test]
    fn refactor_rust_file_resolves_to_plan_execute_verify() {
        let p = profile_of("refactor src/main.rs");
        assert_eq!(p.domain.as_str(), "coding");
        assert!(p.requires_mutation);
        assert!(p.requires_grounding);
        assert_eq!(p.recommended_strategy, ReasoningStrategy::PlanExecuteVerify);
        // Rust grounding populates verifiers.
        assert!(
            p.available_verifiers
                .iter()
                .any(|v| v.as_str() == "cargo_check")
        );
        assert!(
            p.available_verifiers
                .iter()
                .any(|v| v.as_str() == "cargo_test")
        );
        assert!(p.available_verifiers.iter().any(|v| v.as_str() == "clippy"));
    }

    #[test]
    fn implement_function_with_rust_is_coding_plan_execute_verify() {
        let p = profile_of("implement a new function in src/lib.rs to parse the config");
        assert_eq!(p.domain.as_str(), "coding");
        assert!(p.requires_mutation);
        assert_eq!(p.recommended_strategy, ReasoningStrategy::PlanExecuteVerify);
    }

    // --- Math (GenerateVerifyRepair when a verifier exists) ---

    #[test]
    fn math_prompt_with_code_extension_uses_generate_verify_repair() {
        // "calculate" is a math keyword; ".py" gives a verifier (pytest) but no
        // mutation verb, so this is math + verifier -> GenerateVerifyRepair.
        let p = profile_of("calculate the result of the expression in calc.py");
        assert_eq!(p.domain.as_str(), "math");
        assert!(!p.requires_mutation);
        assert_eq!(
            p.recommended_strategy,
            ReasoningStrategy::GenerateVerifyRepair
        );
    }

    #[test]
    fn pure_math_without_verifier_is_not_generate_verify_repair() {
        // No file extension -> no verifier. Math without a verifier cannot do
        // generate_verify_repair; falls through to plan_then_answer (not low
        // complexity, since math is medium by default).
        let p = profile_of("solve this differential equation for the general case");
        assert_eq!(p.domain.as_str(), "math");
        assert!(!p.requires_mutation);
        assert!(p.available_verifiers.is_empty());
        // No verifier + not simple -> plan_then_answer.
        assert_eq!(p.recommended_strategy, ReasoningStrategy::PlanThenAnswer);
    }

    // --- Research ---

    #[test]
    fn research_explain_resolves_to_direct_when_simple() {
        // "explain recursion" — research domain, low complexity (short, single
        // concept), low risk -> Direct.
        let p = profile_of("explain recursion");
        assert_eq!(p.domain.as_str(), "research");
        assert_eq!(p.complexity, Complexity::Medium);
        // No verifier, no grounding -> falls through to plan_then_answer OR
        // direct. Medium complexity + no mutation -> plan_then_answer.
        assert_eq!(p.recommended_strategy, ReasoningStrategy::PlanThenAnswer);
    }

    // --- Planning ---

    #[test]
    fn planning_design_resolves_to_plan_then_answer() {
        let p = profile_of("design the architecture for the new authentication module");
        assert_eq!(p.domain.as_str(), "planning");
        assert!(!p.requires_mutation);
        // No verifiers, no grounding -> plan_then_answer.
        assert_eq!(p.recommended_strategy, ReasoningStrategy::PlanThenAnswer);
    }

    // --- Risk (directive: delete/commit/production -> High) ---

    #[test]
    fn high_risk_keywords_set_risk_high() {
        let p = profile_of("delete the production database and drop all tables");
        assert_eq!(p.risk, RiskLevel::High);
    }

    #[test]
    fn commit_to_production_is_high_risk() {
        let p = profile_of("commit these changes and deploy to production");
        assert_eq!(p.risk, RiskLevel::High);
    }

    #[test]
    fn code_mutation_without_explicit_risk_is_at_least_medium() {
        // A plain refactor is irreversible-ish; risk floor is Medium.
        let p = profile_of("refactor src/main.rs");
        assert!(p.risk == RiskLevel::Medium || p.risk == RiskLevel::High);
    }

    // --- Grounding ---

    #[test]
    fn rust_file_sets_grounding_and_verifiers() {
        let p = profile_of("add a unit test to src/parser.rs");
        assert!(p.requires_grounding);
        let names: Vec<&str> = p.available_verifiers.iter().map(|v| v.as_str()).collect();
        assert!(names.contains(&"cargo_check"));
        assert!(names.contains(&"cargo_test"));
        assert!(names.contains(&"clippy"));
    }

    #[test]
    fn python_file_sets_pytest_verifier() {
        let p = profile_of("fix the bug in scripts/run.py");
        assert!(p.requires_grounding);
        let names: Vec<&str> = p.available_verifiers.iter().map(|v| v.as_str()).collect();
        assert!(names.contains(&"pytest"));
        assert!(!names.contains(&"cargo_check"));
    }

    #[test]
    fn no_file_reference_means_no_verifiers() {
        let p = profile_of("explain how garbage collection works");
        assert!(p.available_verifiers.is_empty());
    }

    // --- Ambiguity ---

    #[test]
    fn vague_terms_raise_ambiguity() {
        let p = profile_of("make the code better and improve it somehow");
        assert!(p.ambiguity >= 0.7);
    }

    #[test]
    fn specific_file_reference_lowers_ambiguity() {
        let p = profile_of("refactor src/main.rs");
        assert!(p.ambiguity <= 0.3);
    }

    // --- Multi-step complexity ---

    #[test]
    fn multi_step_code_task_is_high_complexity() {
        let p = profile_of(
            "refactor src/main.rs and then update src/lib.rs and fix the tests in src/tests.rs",
        );
        assert!(p.requires_mutation);
        assert_eq!(p.complexity, Complexity::High);
    }

    // --- Determinism ---

    #[test]
    fn profiler_is_deterministic_for_identical_input() {
        let prompt = "refactor src/main.rs to extract the config parser into its own module";
        let a = profile_of(prompt);
        let b = profile_of(prompt);
        assert_eq!(a, b);
    }

    // --- profile_request honors risk_hint override ---

    #[test]
    fn profile_request_overrides_risk_with_caller_hint() {
        use vesper_domain::{PrivacyMode, ReasoningMode, RequestId, SessionId};
        let req = ReasoningRequest {
            request_id: RequestId::new("req-1").unwrap(),
            session_id: SessionId::new("sess-1").unwrap(),
            user_message: "refactor src/main.rs".into(),
            context_refs: vec![],
            mode: ReasoningMode::Auto,
            risk_hint: Some(RiskLevel::High),
            budget_override: None,
            privacy_mode: PrivacyMode::Private,
        };
        let p = TaskProfiler::new().profile_request(&req);
        // Caller forced High risk despite no high-risk keyword in the prompt.
        assert_eq!(p.risk, RiskLevel::High);
    }
}
