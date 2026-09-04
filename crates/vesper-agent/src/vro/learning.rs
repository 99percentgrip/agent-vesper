//! Verified Workflow Learning (VRO-7, PRD §11.9).
//!
//! When a complex reasoning strategy succeeds, this module extracts a
//! sanitized, generalized, reusable **procedural memory** from the
//! successful trajectory (or, for non-ReAct strategies, from the
//! [`ReasoningOutcome`]) and persists it through a caller-supplied
//! [`ProceduralMemorySink`].
//!
//! ## Architecture
//!
//! This module lives in `vesper-agent` (which owns the VRO surface and the
//! [`TrajectoryEntry`] type) and owns **only** pure extraction + sanitization
//! logic. It never imports `vesper-cognition`, never opens a SQLite handle,
//! and never makes a network call — persistence is delegated to a
//! [`ProceduralMemorySink`] trait object supplied at the composition
//! boundary (production impl: `apps/agent-vesper-tui/src/main.rs`, backed by
//! [`CognitiveMemory::add_procedural`](vesper_cognition::pipeline::CognitiveMemory::add_procedural)).
//! This honors the architecture rule that `vesper-agent` depends only on
//! domain/provider/runtime (the cognition engine is a peer crate, not a
//! foundational dependency).
//!
//! ## Zero-breakage contract
//!
//! Extraction is **non-blocking and gracefully degrades**:
//!
//! - Every public extraction path returns `Result<_, LearningError>`; the
//!   orchestrator swallows the `Err` into an `unresolved_risks` entry
//!   rather than propagating it as a turn failure
//!   ([`VroOrchestrator::execute_with_learning`]).
//! - The sink port is `Option<&dyn ProceduralMemorySink>` — when `None`,
//!   extraction still runs but persistence is skipped (useful for tests and
//!   hosts that have not wired cognition yet).
//! - If persistence fails, the *outcome* returned to the host is unchanged;
//!   only a risk note is appended so the user sees the learning gap.
//!
//! ## Security: SecretScrubber
//!
//! Every byte of the trajectory (tool names, arguments, observations, the
//! objective, and the final output) MUST pass through [`SecretScrubber`]
//! before it is incorporated into a [`ProceduralMemory`]. The scrubber
//! redacts AWS access keys, AWS secret keys, JWTs, bearer tokens, IP
//! addresses, generic `api_key`/`token`/`secret`/`password` assignments,
//! and high-entropy 32+ char strings to deterministic placeholders, so no
//! secret material is ever persisted to cognitive memory.
//!
//! [`VroOrchestrator::execute_with_learning`]: super::VroOrchestrator::execute_with_learning
//! [`ReasoningOutcome`]: vesper_domain::ReasoningOutcome

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use vesper_domain::{
    InferenceCost, OutcomeStatus, PrivacyMode, ReasoningOutcome, ReasoningRequest,
    ReasoningStrategy, VerificationStatus,
};

use super::react::TrajectoryEntry;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a workflow-learning operation failed.
///
/// All variants are non-fatal: the orchestrator converts them into
/// `unresolved_risks` entries rather than propagating them as turn failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LearningError {
    /// The outcome was not `Succeeded` — there is no successful procedure to
    /// extract.
    #[error("cannot extract a workflow from a non-succeeded outcome ({0:?})")]
    OutcomeNotSucceeded(OutcomeStatus),
    /// The request was marked [`PrivacyMode::Private`] (PRD §17). Private
    /// requests must NOT be persisted to cognitive memory — the extractor
    /// refuses to produce a procedure rather than risk leaking private
    /// deliberation. Internal / Public requests are eligible.
    ///
    /// [`PrivacyMode::Private`]: vesper_domain::PrivacyMode::Private
    #[error("request is PrivacyMode::Private — private requests are not persisted")]
    PrivateRequestRejected,
    /// The trajectory / outcome contained no usable steps to generalize.
    #[error("no extractable steps found in the trajectory")]
    NoStepsToExtract,
    /// The objective (user message) was empty after sanitization.
    #[error("objective is empty after sanitization")]
    EmptyObjective,
    /// The persistence sink rejected the procedure.
    #[error("procedural-memory sink rejected the procedure: {0}")]
    SinkRejected(String),
}

// ---------------------------------------------------------------------------
// Persistence port (the composition-boundary seam)
// ---------------------------------------------------------------------------

/// Persistence seam for verified workflow learning.
///
/// Implementations live at the composition boundary (e.g. the TUI's
/// `CognitionBundle`). The trait is async + object-safe via a boxed `Send`
/// future (the workspace has no `async_trait` dependency, mirroring
/// [`CandidateGenerator`](super::CandidateGenerator) and
/// [`ReactAgent`](super::ReactAgent)).
///
/// On success, returns the storage-assigned memory id (caller-supplied; the
/// default cognition impl returns the generated UUID). On failure, returns
/// [`LearningError::SinkRejected`].
pub trait ProceduralMemorySink: Send + Sync {
    /// Persist a sanitized [`ProceduralMemory`] to the cognitive-memory store.
    /// Implementations MUST treat the procedure text as already-sanitized —
    /// they MUST NOT trust it (re-running [`SecretScrubber`] defensively is
    /// allowed) but MUST NOT mutate the caller's struct.
    fn save_procedure<'a>(
        &'a self,
        procedure: &'a ProceduralMemory,
    ) -> Pin<Box<dyn Future<Output = Result<String, LearningError>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// SecretScrubber
// ---------------------------------------------------------------------------

/// A redaction engine that strips secret-shaped strings from arbitrary text.
///
/// Patterns are compiled once at construction and reused across calls. The
/// scrubber is cheap to clone (the inner regex set is small and immutable).
///
/// ## Detected patterns (priority order)
///
/// 1. **AWS access key id** — `AKIA[0-9A-Z]{16}` (canonical 20-char prefix).
/// 2. **JWT** — `eyJ…\.eyJ…\.…` (three base64url segments).
/// 3. **Bearer token** — `[Bb]earer\s+<token>` (Authorization header value).
/// 4. **Generic credential assignment** — `(api[_-]?key|token|secret|password
///    |passwd|auth|access[_-]?key)\s*[=:]\s*['"]?<32+ char value>`.
/// 5. **AWS secret access key** — 40-char base64-ish string after an
///    `aws_secret`/`secret_access_key`/`aws_secret_access_key` hint.
/// 6. **IPv4 address** — `\d{1,3}(\.\d{1,3}){3}` (redacted because private IPs
///    are sensitive in shared workspaces and a redacted IP can never hurt).
/// 7. **High-entropy string** — any 32+ char run of base64/url-safe chars
///    whose Shannon entropy exceeds 4.0 bits/char (catches API keys,
///    service-account tokens, opaque credentials that no pattern caught).
///
/// Each match is replaced with a deterministic placeholder:
/// `[REDACTED:<KIND>]` (e.g. `[REDACTED:JWT]`, `[REDACTED:AWS_ACCESS_KEY]`).
#[derive(Debug, Clone)]
pub struct SecretScrubber {
    /// All compiled patterns, in priority order. Each `(regex, kind)` pair
    /// is applied in turn; later passes see the output of earlier passes
    /// (so a `JWT` redacted to `[REDACTED:JWT]` is invisible to the
    /// high-entropy scanner, which has no `[`/`]` characters in its class).
    patterns: Vec<(regex::Regex, &'static str)>,
    /// High-entropy token matcher. Compiled ONCE at construction (matches
    /// the public doc claim "compiled once at construction and reused across
    /// calls"); previously this was recompiled per `scrub()` call, which
    /// contradicted the docs and was wasteful at high call volume.
    entropy_re: regex::Regex,
}

impl Default for SecretScrubber {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretScrubber {
    /// Construct the scrubber with the standard pattern set.
    ///
    /// Errors are panic-worthy (a build-time bug) — if a pattern fails to
    /// compile, this is a code defect, not a runtime condition.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "explicit pattern list, one line each is clearer"
    )]
    pub fn new() -> Self {
        // NOTE: `\b`, `\s`, `\w`, `\d` all require `unicode-perl`. The
        // per-crate `regex` override in `crates/vesper-agent/Cargo.toml`
        // enables it (see the VRO-7 pitfall in CLAUDE memory).
        let raw: &[(&str, &str)] = &[
            // AWS access key id: AKIA followed by 16 uppercase-alphanumeric.
            (r"AKIA[0-9A-Z]{16}", "AWS_ACCESS_KEY"),
            // JWT: three base64url segments separated by dots. Min lengths
            // prevent matching short `a.b.c` fragments that happen to start
            // with `eyJ` (the standard JWT header prefix).
            (
                r"eyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
                "JWT",
            ),
            // Bearer token in an Authorization header.
            (r"(?i)\bBearer\s+[A-Za-z0-9\._\-]+", "BEARER_TOKEN"),
            // Generic credential assignment. The value must be at least 16
            // chars of base64/url-safe to avoid redacting short usernames.
            (
                r#"(?i)(api[_\-]?key|apikey|token|secret|password|passwd|auth[_\-]?token|access[_\-]?key)\s*[=:]\s*['"]?[A-Za-z0-9+/=_\-]{16,}['"]?"#,
                "CREDENTIAL",
            ),
            // AWS secret access key: 40 chars of base64-plus after an
            // aws_secret hint. Catches the long-lived secret that
            // accompanies an AWS_ACCESS_KEY_ID.
            (
                r#"(?i)aws[_\-]?secret[_\-]?(access)?[_\-]?key\s*[=:]\s*['"]?[A-Za-z0-9/+]{40}['"]?"#,
                "AWS_SECRET_KEY",
            ),
            // IPv4 addresses (private IPs are sensitive in shared
            // workspaces; redaction is the conservative default).
            (r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b", "IP_ADDRESS"),
        ];
        let patterns = raw
            .iter()
            .map(|(pat, kind)| {
                let re = regex::Regex::new(pat)
                    .unwrap_or_else(|e| panic!("regex `{pat}` failed to compile: {e}"));
                (re, *kind)
            })
            .collect();
        let entropy_re =
            regex::Regex::new(r"[A-Za-z0-9+/=_\-]{32,}").expect("entropy token regex must compile");
        Self {
            patterns,
            entropy_re,
        }
    }

    /// Scrub every secret-shaped substring from `input`, returning a new
    /// owned `String`. The high-entropy pass runs **after** the pattern
    /// passes so redacted placeholders (which contain only `[`, `]`, `:`,
    /// uppercase letters, and underscore) cannot themselves trip the entropy
    /// scanner.
    #[must_use]
    pub fn scrub(&self, input: &str) -> String {
        let mut current = input.to_string();
        for (re, kind) in &self.patterns {
            let placeholder = format!("[REDACTED:{kind}]");
            current = re.replace_all(&current, placeholder.as_str()).into_owned();
        }
        // High-entropy pass runs last so the deterministic placeholders above
        // (which contain only `[`, `]`, `:`, letters, underscore) cannot
        // trip the entropy threshold.
        scrub_high_entropy(&self.entropy_re, &current)
    }

    /// Scrub a JSON value in place. Strings in the value are scrubbed;
    /// numbers/bools/null are left untouched. Returns a new owned value
    /// (the input is not mutated).
    #[must_use]
    pub fn scrub_json(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.scrub(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.scrub_json(v)).collect())
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    // Key names like `api_key` are themselves sensitive when
                    // their value is a 32+ char string — but the value pass
                    // above already redacted that. We scrub the value but
                    // keep the key as-is (the key alone is harmless).
                    out.insert(k.clone(), self.scrub_json(v));
                }
                serde_json::Value::Object(out)
            }
            other => other.clone(),
        }
    }
}

/// Shannon-entropy-based redaction. Any token of 32+ chars drawn from the
/// base64 / url-safe alphabet with Shannon entropy > 4.0 bits/char is treated
/// as an opaque credential and redacted.
///
/// Threshold rationale: random base64 averages ~6.0 bits/char; natural
/// English averages ~4.5 bits/char for words ≥ 8 letters but drops sharply
/// below 4.0 for longer multi-word phrases (which have repetition). A 32-char
/// English phrase with > 4.0 bits/char is overwhelmingly likely to be
/// machine-generated (a key, hash, or token), not natural prose.
fn scrub_high_entropy(entropy_re: &regex::Regex, input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_end = 0;
    for m in entropy_re.find_iter(input) {
        // Preserve everything before this match.
        out.push_str(&input[last_end..m.start()]);
        let token = m.as_str();
        if shannon_entropy(token) > 4.0 {
            out.push_str("[REDACTED:HIGH_ENTROPY]");
        } else {
            out.push_str(token);
        }
        last_end = m.end();
    }
    out.push_str(&input[last_end..]);
    out
}

/// Shannon entropy in bits per character (base-2). Used by the high-entropy
/// scrubber to flag opaque credentials.
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts: std::collections::HashMap<char, u32> = std::collections::HashMap::new();
    for ch in s.chars() {
        *counts.entry(ch).or_insert(0) += 1;
    }
    let total = s.chars().count() as f64;
    let mut h = 0.0_f64;
    for &count in counts.values() {
        let p = f64::from(count) / total;
        h -= p * p.log2();
    }
    h
}

// ---------------------------------------------------------------------------
// ProceduralMemory artifact
// ---------------------------------------------------------------------------

/// One generalized step in a reusable workflow (PRD §11.9).
///
/// A step is a *generalized* observation — `read_file <path>` rather than
/// `read_file /home/alex/secrets/api_key.txt` (which would have been scrubbed
/// to `[REDACTED:IP_ADDRESS]`/`[REDACTED:CREDENTIAL]` placeholders before
/// generalization). The `description` is a short, machine-readable summary;
/// `inputs` and `outputs` are sanitized, generalized excerpts of the actual
/// tool calls and observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProceduralStep {
    /// 0-based step index in the parent procedure.
    pub index: u32,
    /// Action category: a tool name (`read_file`, `grep`, `edit_file`,
    /// `run_command`) or a synthesized action (`generate`, `verify`,
    /// `repair`, `adjudicate`, `propose`, `critic`).
    pub action: String,
    /// One-line human-readable description of what this step does.
    pub description: String,
    /// Sanitized, generalized inputs (tool arguments or generated prompt
    /// fragments). May be empty for `generate`/`verify` synthesized steps.
    pub inputs: Vec<String>,
    /// Sanitized, generalized observed outputs. May be empty if the step
    /// produced no observable output (e.g. a pure model call).
    pub outputs: Vec<String>,
    /// Whether this step succeeded in the source trajectory. `false` for
    /// rejection observations, tool errors, and Read-Before-Write denials.
    pub success: bool,
}

/// A reusable procedural memory extracted from a successful reasoning turn
/// (PRD §11.9). This is the artifact persisted to cognitive memory.
///
/// The `id` is a deterministic SHA-256 of the normalized `(objective,
/// strategy, steps)` triple — two trajectories that produced the same
/// generalized procedure produce the same id, so the cognitive-memory store
/// can dedupe naturally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProceduralMemory {
    /// Deterministic SHA-256 of the normalized procedure (hex).
    pub id: String,
    /// Short human-readable label, derived from the (scrubbed) objective.
    pub title: String,
    /// Generalized objective: the scrubbed user message that this procedure
    /// successfully resolved.
    pub objective: String,
    /// Strategy variant name (snake_case) that produced the source trajectory.
    pub source_strategy: String,
    /// Ordered, generalized steps.
    pub steps: Vec<ProceduralStep>,
    /// One-line summary of how the source turn was verified (`"passed"`,
    /// `"no_verifier"`, `"verifier_error"`).
    pub verification_summary: String,
    /// RFC3339 timestamp the procedure was extracted.
    pub extracted_at: String,
    /// Total model calls consumed by the source turn.
    pub model_calls: u32,
    /// Total tokens consumed by the source turn.
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// WorkflowExtractor
// ---------------------------------------------------------------------------

/// Extracts a [`ProceduralMemory`] from a successful reasoning turn.
///
/// Two entry points:
///
/// - [`extract_from_trajectory`](Self::extract_from_trajectory): used by the
///   ReAct path, which exposes its [`TrajectoryEntry`] sequence.
/// - [`extract_from_outcome`](Self::extract_from_outcome): used by the
///   non-ReAct strategies (parallel, BTS, PCA, GenerateVerifyRepair), which
///   synthesize a single `generate` step from the final output.
///
/// Both paths scrub every byte of the source material through the supplied
/// [`SecretScrubber`] before incorporating it into the procedure. Both fail
/// (return [`LearningError`]) when the source turn did not succeed or when
/// no extractable steps are present.
#[derive(Debug, Clone, Default)]
pub struct WorkflowExtractor {
    scrubber: SecretScrubber,
}

impl WorkflowExtractor {
    /// Construct with the default [`SecretScrubber`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            scrubber: SecretScrubber::new(),
        }
    }

    /// Construct with a caller-supplied scrubber (tests use this to inject
    /// a scrubber that records what it saw).
    #[must_use]
    pub fn with_scrubber(scrubber: SecretScrubber) -> Self {
        Self { scrubber }
    }

    /// Borrow the inner scrubber (used by the orchestrator to scrub the
    /// objective consistently).
    #[must_use]
    pub fn scrubber(&self) -> &SecretScrubber {
        &self.scrubber
    }

    /// Extract a procedural memory from a ReAct [`TrajectoryEntry`] sequence.
    ///
    /// Walks the trajectory: each [`TrajectoryEntry::Action`] becomes a
    /// [`ProceduralStep`] (action = tool name, inputs = scrubbed JSON
    /// arguments); the following [`TrajectoryEntry::Observation`] (if any)
    /// is attached as the step's `outputs`. The objective is the scrubbed
    /// user message. The id is a deterministic SHA-256 over the normalized
    /// `(objective, strategy, steps)` triple.
    ///
    /// Returns [`LearningError::OutcomeNotSucceeded`] if the outcome is not
    /// `Succeeded`, [`LearningError::EmptyObjective`] if the objective
    /// scrubs to empty, or [`LearningError::NoStepsToExtract`] if the
    /// trajectory contains no actions.
    pub fn extract_from_trajectory(
        &self,
        request: &ReasoningRequest,
        outcome: &ReasoningOutcome,
        trajectory: &[TrajectoryEntry],
        source_strategy: ReasoningStrategy,
        extracted_at: &str,
    ) -> Result<ProceduralMemory, LearningError> {
        // PRD §17: PrivacyMode::Private means "no private-artifact
        // persistence". Refusing here is the safest place — the procedure
        // is never built, so no scrubbed-but-still-private bytes can leak
        // through a future sink bug.
        if request.privacy_mode == PrivacyMode::Private {
            return Err(LearningError::PrivateRequestRejected);
        }
        if outcome.status != OutcomeStatus::Succeeded {
            return Err(LearningError::OutcomeNotSucceeded(outcome.status));
        }
        let objective = self
            .scrubber
            .scrub(request.user_message.as_str())
            .trim()
            .to_string();
        if objective.is_empty() {
            return Err(LearningError::EmptyObjective);
        }

        let mut steps: Vec<ProceduralStep> = Vec::new();
        let mut pending_observation: Option<(bool, String)> = None;
        for entry in trajectory {
            match entry {
                TrajectoryEntry::Action { name, arguments } => {
                    // If a previous action is still waiting for its
                    // observation, that means two actions ran back-to-back —
                    // the second one replaces the pending observation slot
                    // (the missing observation is encoded as a failed empty
                    // output on the prior step). This is rare but possible
                    // when the loop constructs synthetic action entries.
                    if let Some((success, text)) = pending_observation.take()
                        && let Some(last) = steps.last_mut()
                    {
                        if !text.is_empty() {
                            last.outputs.push(text);
                        }
                        // success was already set when the step was pushed.
                        let _ = success; // (already recorded)
                    }
                    let sanitized_args = self.scrubber.scrub_json(arguments);
                    let arg_str = serde_json::to_string(&sanitized_args)
                        .unwrap_or_else(|_| "<unprintable>".to_string());
                    let description = format!("Invoke tool `{name}` with sanitized arguments.");
                    steps.push(ProceduralStep {
                        index: u32::try_from(steps.len()).unwrap_or(u32::MAX),
                        action: name.clone(),
                        description,
                        inputs: vec![arg_str],
                        outputs: Vec::new(),
                        success: true, // will be flipped to false by a failed observation
                    });
                }
                TrajectoryEntry::Observation { text, success } => {
                    let sanitized = self.scrubber.scrub(text);
                    if let Some(last) = steps.last_mut() {
                        if !sanitized.is_empty() {
                            // Bound the observation excerpt to a sensible
                            // procedural-memory length (this is a reusable
                            // artifact, not a verbatim transcript).
                            let excerpt = truncate_observation(&sanitized);
                            last.outputs.push(excerpt);
                        }
                        if !*success {
                            last.success = false;
                        }
                    } else {
                        // Observation before any action: record as a note on
                        // the pending slot so a subsequent action picks it up.
                        pending_observation = Some((*success, sanitized));
                    }
                }
            }
        }
        if steps.is_empty() {
            return Err(LearningError::NoStepsToExtract);
        }

        let verification_summary = verification_label(outcome);
        let title = derive_title(&objective);
        let id = deterministic_id(&objective, source_strategy, &steps);
        Ok(ProceduralMemory {
            id,
            title,
            objective,
            source_strategy: strategy_to_str(source_strategy).to_string(),
            steps,
            verification_summary,
            extracted_at: extracted_at.to_string(),
            model_calls: outcome.cost.model_calls,
            total_tokens: outcome.cost.total_tokens,
        })
    }

    /// Extract a procedural memory from a non-ReAct outcome (parallel, BTS,
    /// PCA, GenerateVerifyRepair). Synthesizes a single `generate` step from
    /// the final output, plus a `verify` step when the outcome ran any
    /// verifier.
    ///
    /// Used by strategies that do not expose a [`TrajectoryEntry`] sequence.
    pub fn extract_from_outcome(
        &self,
        request: &ReasoningRequest,
        outcome: &ReasoningOutcome,
        source_strategy: ReasoningStrategy,
        extracted_at: &str,
    ) -> Result<ProceduralMemory, LearningError> {
        // PRD §17: PrivacyMode::Private means "no private-artifact
        // persistence". See extract_from_trajectory for rationale.
        if request.privacy_mode == PrivacyMode::Private {
            return Err(LearningError::PrivateRequestRejected);
        }
        if outcome.status != OutcomeStatus::Succeeded {
            return Err(LearningError::OutcomeNotSucceeded(outcome.status));
        }
        let objective = self
            .scrubber
            .scrub(request.user_message.as_str())
            .trim()
            .to_string();
        if objective.is_empty() {
            return Err(LearningError::EmptyObjective);
        }

        let mut steps: Vec<ProceduralStep> = Vec::new();

        // Step 0: generate the answer.
        let final_excerpt = outcome
            .final_output
            .as_ref()
            .map(|v| {
                let scrubbed = self.scrubber.scrub_json(v);
                let s = serde_json::to_string(&scrubbed)
                    .unwrap_or_else(|_| "<unprintable>".to_string());
                truncate_observation(&s)
            })
            .unwrap_or_default();
        steps.push(ProceduralStep {
            index: 0,
            action: "generate".to_string(),
            description: "Produce the final structured answer.".to_string(),
            inputs: vec![objective.clone()],
            outputs: if final_excerpt.is_empty() {
                Vec::new()
            } else {
                vec![final_excerpt]
            },
            success: true,
        });

        // Step 1 (optional): verify the answer.
        if outcome.verification_summary.passed > 0 || outcome.verification_summary.failed > 0 {
            let label = if outcome.verification_summary.overall == VerificationStatus::Passed {
                "passed"
            } else {
                "completed"
            };
            steps.push(ProceduralStep {
                index: 1,
                action: "verify".to_string(),
                description: format!(
                    "Ran {} verifier(s); overall status: {label}.",
                    outcome.verification_summary.passed + outcome.verification_summary.failed
                ),
                inputs: Vec::new(),
                outputs: vec![format!(
                    "{} passed / {} failed",
                    outcome.verification_summary.passed, outcome.verification_summary.failed
                )],
                success: outcome.verification_summary.overall == VerificationStatus::Passed,
            });
        }

        let verification_summary = verification_label(outcome);
        let title = derive_title(&objective);
        let id = deterministic_id(&objective, source_strategy, &steps);
        Ok(ProceduralMemory {
            id,
            title,
            objective,
            source_strategy: strategy_to_str(source_strategy).to_string(),
            steps,
            verification_summary,
            extracted_at: extracted_at.to_string(),
            model_calls: outcome.cost.model_calls,
            total_tokens: outcome.cost.total_tokens,
        })
    }
}

/// Cap an observation excerpt to a bounded length suitable for a reusable
/// procedural memory. Longer transcripts would bloat cognitive memory without
/// adding recall value (the procedure is meant to be a recipe, not a log).
fn truncate_observation(s: &str) -> String {
    const MAX_OBS_LEN: usize = 240;
    if s.chars().count() <= MAX_OBS_LEN {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_OBS_LEN - 3).collect();
    format!("{head}...")
}

/// Derive a short title from a scrubbed objective.
fn derive_title(objective: &str) -> String {
    const MAX_TITLE_LEN: usize = 80;
    let trimmed = objective.trim();
    if trimmed.chars().count() <= MAX_TITLE_LEN {
        return trimmed.to_string();
    }
    // Cut at the last whitespace within the budget so we don't split a word.
    let head: String = trimmed.chars().take(MAX_TITLE_LEN - 1).collect();
    if let Some(idx) = head.rfind(char::is_whitespace) {
        format!("{}…", &head[..idx])
    } else {
        format!("{head}…")
    }
}

/// One-line verification label for the procedure.
fn verification_label(outcome: &ReasoningOutcome) -> String {
    match outcome.verification_summary.overall {
        VerificationStatus::Passed => "passed".to_string(),
        VerificationStatus::Failed => format!(
            "failed ({} of {} verifiers)",
            outcome.verification_summary.failed,
            outcome.verification_summary.passed + outcome.verification_summary.failed
        ),
        VerificationStatus::Error => "verifier_error".to_string(),
        VerificationStatus::Inconclusive => "inconclusive".to_string(),
        VerificationStatus::Skipped => "no_verifier".to_string(),
    }
}

/// Deterministic SHA-256 hex over the normalized `(objective, strategy,
/// steps)` triple. Two trajectories that generalize to the same procedure
/// produce the same id, so the cognitive-memory store can dedupe naturally.
fn deterministic_id(
    objective: &str,
    strategy: ReasoningStrategy,
    steps: &[ProceduralStep],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(objective.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(strategy_to_str(strategy).as_bytes());
    hasher.update(b"\x1f");
    for step in steps {
        hasher.update(step.action.as_bytes());
        hasher.update(b"\x1e");
        hasher.update(step.description.as_bytes());
        hasher.update(b"\x1e");
        for input in &step.inputs {
            hasher.update(input.as_bytes());
            hasher.update(b"\x1d");
        }
        hasher.update(b"\x1c");
        for output in &step.outputs {
            hasher.update(output.as_bytes());
            hasher.update(b"\x1d");
        }
        hasher.update(b"\x1f");
    }
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Map a [`ReasoningStrategy`] to its PRD §10.3 snake_case string. We
/// serialize the enum directly elsewhere; this helper exists so the
/// deterministic id stays stable even if the serde rename changes.
fn strategy_to_str(strategy: ReasoningStrategy) -> &'static str {
    match strategy {
        ReasoningStrategy::Direct => "direct",
        ReasoningStrategy::PlanThenAnswer => "plan_then_answer",
        ReasoningStrategy::PlanExecuteVerify => "plan_execute_verify",
        ReasoningStrategy::GenerateVerifyRepair => "generate_verify_repair",
        ReasoningStrategy::ParallelCandidatesConsensus => "parallel_candidates_consensus",
        ReasoningStrategy::ParallelCandidatesJudge => "parallel_candidates_judge",
        ReasoningStrategy::ToolGroundedReact => "tool_grounded_react",
        ReasoningStrategy::BoundedTreeSearch => "bounded_tree_search",
        ReasoningStrategy::ProposerCriticAdjudicator => "proposer_critic_adjudicator",
        ReasoningStrategy::WorkflowReplayWithVerification => "workflow_replay_with_verification",
    }
}

/// Helper for orchestrator callers: build the cost summary that the extractor
/// copies into the [`ProceduralMemory`] from the raw [`InferenceCost`].
/// Public so tests can assert the field shape without reconstructing the
/// whole outcome.
#[must_use]
pub fn cost_summary(cost: InferenceCost) -> (u32, u64) {
    (cost.model_calls, cost.total_tokens)
}

/// The set of strategy variants whose successful turns are eligible for
/// workflow learning. The orchestrator consults this to decide whether to
/// run the extractor after a Succeeded outcome. `Direct` turns (plain chat)
/// are intentionally excluded — there is no procedure to memorize.
#[must_use]
pub fn is_learning_eligible(strategy: ReasoningStrategy) -> bool {
    matches!(
        strategy,
        ReasoningStrategy::GenerateVerifyRepair
            | ReasoningStrategy::ParallelCandidatesConsensus
            | ReasoningStrategy::ParallelCandidatesJudge
            | ReasoningStrategy::ToolGroundedReact
            | ReasoningStrategy::BoundedTreeSearch
            | ReasoningStrategy::ProposerCriticAdjudicator
            | ReasoningStrategy::PlanExecuteVerify
            | ReasoningStrategy::PlanThenAnswer
            | ReasoningStrategy::WorkflowReplayWithVerification
    )
}

/// Returns the deduplicated set of distinct action categories in a procedure.
/// Used by tests to assert the recipe covered the expected tool surface.
#[must_use]
pub fn distinct_actions(procedure: &ProceduralMemory) -> HashSet<String> {
    procedure.steps.iter().map(|s| s.action.clone()).collect()
}

// ---------------------------------------------------------------------------
// Test helpers (visible to the orchestrator's wiring tests in mod.rs)
// ---------------------------------------------------------------------------

/// A test fake sink that records every save attempt and either succeeds
/// or fails (configurable). The orchestrator's wiring tests use this to
/// prove the sink contract is honored. Lives outside the inner `mod tests`
/// so sibling test modules can import it.
#[cfg(test)]
pub(crate) struct RecordingSink {
    /// Every procedure this sink has been asked to persist.
    pub saved: std::sync::Mutex<Vec<ProceduralMemory>>,
    /// When `true`, the next `save_procedure` call returns
    /// [`LearningError::SinkRejected`] without recording anything.
    pub fail_next: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl RecordingSink {
    /// Construct a sink that succeeds by default.
    pub fn new() -> Self {
        Self {
            saved: std::sync::Mutex::new(Vec::new()),
            fail_next: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[cfg(test)]
impl ProceduralMemorySink for RecordingSink {
    fn save_procedure<'a>(
        &'a self,
        procedure: &'a ProceduralMemory,
    ) -> Pin<Box<dyn Future<Output = Result<String, LearningError>> + Send + 'a>> {
        let proc = procedure.clone();
        Box::pin(async move {
            if self.fail_next.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(LearningError::SinkRejected(
                    "test-configured failure".to_string(),
                ));
            }
            self.saved.lock().expect("poisoned").push(proc.clone());
            Ok(proc.id)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::{
        CandidateId, InferenceCost, OutcomeStatus, PrivacyMode, ReasoningBudget, ReasoningRequest,
        RequestId, SessionId, VerificationStatus, VerificationSummary,
    };

    // ============================================================
    // SecretScrubber tests
    // ============================================================

    #[test]
    fn scrubber_redacts_aws_access_key() {
        let s = SecretScrubber::new();
        let input = "auth with AKIAIOSFODNN7EXAMPLE and proceed";
        let out = s.scrub(input);
        assert!(
            !out.contains("AKIAIOSFODNN7EXAMPLE"),
            "AWS access key must be redacted, got: {out}"
        );
        assert!(
            out.contains("[REDACTED:AWS_ACCESS_KEY]"),
            "expected AWS_ACCESS_KEY placeholder, got: {out}"
        );
    }

    #[test]
    fn scrubber_redacts_jwt() {
        let s = SecretScrubber::new();
        // A realistic-looking JWT structure (header.payload.signature), all
        // three segments ≥ 8 chars of base64url.
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let input = format!("Authorization: Bearer {jwt}");
        let out = s.scrub(&input);
        assert!(
            !out.contains("eyJhbGciOiJIUzI1NiJ9"),
            "JWT must be redacted, got: {out}"
        );
        // Either the JWT placeholder or the bearer placeholder should appear.
        assert!(
            out.contains("[REDACTED:JWT]") || out.contains("[REDACTED:BEARER_TOKEN]"),
            "expected a JWT/bearer placeholder, got: {out}"
        );
    }

    #[test]
    fn scrubber_redacts_bearer_token() {
        let s = SecretScrubber::new();
        for prefix in ["Bearer", "bearer", "bEaReR"] {
            let input = format!(
                "Authorization: {prefix} ya29.A0ARrda6-YVM46JQ-32CharOrMoreTokenGoesHereEtc"
            );
            let out = s.scrub(&input);
            assert!(
                !out.contains("ya29."),
                "bearer token must be redacted, got: {out}"
            );
            assert!(
                out.contains("[REDACTED:"),
                "expected a placeholder, got: {out}"
            );
        }
    }

    #[test]
    fn production_manifest_enables_every_scrubber_regex_feature() {
        let manifest = include_str!("../../Cargo.toml");
        assert!(
            manifest.contains(r#"features = ["std", "unicode-perl", "unicode-case"]"#),
            "the release graph must compile case-insensitive scrubber patterns"
        );
    }

    #[test]
    fn scrubber_redacts_generic_credential_assignment() {
        let s = SecretScrubber::new();
        let cases = [
            "api_key=ABCDEF0123456789abcdef0123456789",
            "API-KEY: \"ABCDEF0123456789abcdef0123456789\"",
            "token = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ123456'",
            "secret: SUPERSECRETLONGSTRINGVALUE1234567",
            "password=MyVeryLongPasswordThatIsLongEnough1",
            "access_key=AKIAEXAMPLELONGSTRING1234567890",
        ];
        for input in cases {
            let out = s.scrub(input);
            assert!(
                out.contains("[REDACTED:"),
                "expected redaction in `{input}` -> `{out}`"
            );
        }
    }

    #[test]
    fn scrubber_redacts_aws_secret_key() {
        let s = SecretScrubber::new();
        let input = "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let out = s.scrub(input);
        assert!(
            !out.contains("wJalrXUtnFEMI"),
            "AWS secret key must be redacted, got: {out}"
        );
        assert!(
            out.contains("[REDACTED:"),
            "expected a placeholder, got: {out}"
        );
    }

    #[test]
    fn scrubber_redacts_ipv4_addresses() {
        let s = SecretScrubber::new();
        let input = "connect to 10.0.0.1 then 192.168.1.100 finally 8.8.8.8";
        let out = s.scrub(input);
        assert!(!out.contains("10.0.0.1"));
        assert!(!out.contains("192.168.1.100"));
        assert!(!out.contains("8.8.8.8"));
        assert!(out.contains("[REDACTED:IP_ADDRESS]"));
    }

    #[test]
    fn scrubber_redacts_high_entropy_strings() {
        let s = SecretScrubber::new();
        // 40-char random base64 — no pattern catches it, but entropy does.
        let opaque = "Z9hK4mP7vQ3rT1wX8yB2nL5cJ6dF0gH4sA7eR8tU";
        let input = format!("token={opaque}");
        let out = s.scrub(&input);
        // The generic-credential pattern catches `token=<value>` first; if it
        // missed, the entropy scanner catches the bare opaque string.
        assert!(
            !out.contains(opaque),
            "high-entropy string must be redacted, got: {out}"
        );
        assert!(out.contains("[REDACTED:"));
    }

    #[test]
    fn scrubber_redacts_orphan_high_entropy_string_without_keyword_prefix() {
        // Entropy-only fallback path: an opaque string with NO `key=` /
        // `token=` / `secret=` prefix must still be redacted by the entropy
        // scanner alone. This is the "machine-generated credential that no
        // pattern caught" case from the SecretScrubber docs.
        let s = SecretScrubber::new();
        let opaque = "Z9hK4mP7vQ3rT1wX8yB2nL5cJ6dF0gH4sA7eR8tU";
        // No keyword prefix — the only thing that can catch this is the
        // entropy scanner.
        let input = format!("pipeline-output: {opaque} (run-id 42)");
        let out = s.scrub(&input);
        assert!(
            !out.contains(opaque),
            "orphan high-entropy string must be redacted, got: {out}"
        );
        assert!(
            out.contains("[REDACTED:HIGH_ENTROPY]"),
            "expected HIGH_ENTROPY placeholder, got: {out}"
        );
        // Surrounding text is preserved.
        assert!(out.contains("pipeline-output:"));
        assert!(out.contains("(run-id 42)"));
    }

    #[test]
    fn scrubber_preserves_natural_language_prose() {
        // Natural English with words shorter than 32 chars and well below
        // 4.0 bits/char entropy should pass through unchanged.
        let s = SecretScrubber::new();
        let input = "The quick brown fox jumps over the lazy dog.";
        let out = s.scrub(input);
        assert_eq!(out, input);
    }

    #[test]
    fn scrubber_preserves_normal_code_paths() {
        let s = SecretScrubber::new();
        let input = "src/main.rs:42:17 error: cannot find value `x`";
        let out = s.scrub(input);
        assert_eq!(out, input, "normal code-path text must pass through");
    }

    #[test]
    fn scrubber_scrubs_nested_json_values() {
        let s = SecretScrubber::new();
        let v = serde_json::json!({
            "path": "/tmp/file.txt",
            "headers": {
                "Authorization": "Bearer ya29.A0ARrda6LongTokenString32CharsPlus",
                "X-Api-Key": "AKIAIOSFODNN7EXAMPLE"
            },
            "list": ["normal", "Bearer ya29.AnotherLongTokenValueHere32Chars"],
        });
        let out = s.scrub_json(&v);
        let headers = out.get("headers").unwrap().as_object().unwrap();
        let auth = headers.get("Authorization").unwrap().as_str().unwrap();
        assert!(
            auth.contains("[REDACTED:"),
            "nested Authorization must be redacted, got: {auth}"
        );
        let api_key = headers.get("X-Api-Key").unwrap().as_str().unwrap();
        assert!(
            api_key.contains("[REDACTED:"),
            "nested X-Api-Key must be redacted, got: {api_key}"
        );
        // Path and ordinary list entries are preserved.
        assert_eq!(out.get("path").unwrap().as_str().unwrap(), "/tmp/file.txt");
    }

    #[test]
    fn shannon_entropy_basic_values() {
        // Single-character string: 0 entropy.
        assert!((shannon_entropy("aaaa") - 0.0).abs() < 1e-9);
        // Two distinct chars evenly split: 1.0 bit/char.
        assert!((shannon_entropy("abab") - 1.0).abs() < 1e-9);
        // Random base64 average > 5 bits/char for long strings.
        let opaque = "Z9hK4mP7vQ3rT1wX8yB2nL5cJ6dF0gH4sA7eR8tU";
        assert!(shannon_entropy(opaque) > 4.5);
    }

    // ============================================================
    // WorkflowExtractor tests
    // ============================================================

    fn sample_request(msg: &str) -> ReasoningRequest {
        ReasoningRequest {
            request_id: RequestId::new("req-1").unwrap(),
            session_id: SessionId::new("sess-1").unwrap(),
            user_message: msg.to_string(),
            context_refs: Vec::new(),
            mode: vesper_domain::ReasoningMode::Balanced,
            risk_hint: None,
            budget_override: Some(ReasoningBudget::balanced()),
            // Internal: tests want extraction to actually run. The default
            // (Private) is rejected by the extractor per PRD §17 — see
            // extractor_rejects_private_request below for that path.
            privacy_mode: PrivacyMode::Internal,
        }
    }

    fn succeeded_outcome(cost: InferenceCost, verifiers_passed: u32) -> ReasoningOutcome {
        ReasoningOutcome {
            status: OutcomeStatus::Succeeded,
            final_output: Some(serde_json::json!({"answer": "done"})),
            selected_candidate: Some(CandidateId::new("cand-0000").unwrap()),
            verification_summary: VerificationSummary {
                passed: verifiers_passed,
                failed: 0,
                overall: VerificationStatus::Passed,
            },
            unresolved_risks: Vec::new(),
            cost,
        }
    }

    #[test]
    fn extractor_from_trajectory_generalizes_action_observation_pairs() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("find the root cause of the test failure");
        let outcome = succeeded_outcome(
            InferenceCost {
                model_calls: 4,
                total_tokens: 500,
            },
            1,
        );
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "src/lib.rs"}),
            },
            TrajectoryEntry::Observation {
                text: "pub fn main() {}".to_string(),
                success: true,
            },
            TrajectoryEntry::Action {
                name: "grep".to_string(),
                arguments: serde_json::json!({"pattern": "panic"}),
            },
            TrajectoryEntry::Observation {
                text: "src/lib.rs:5: panic!(\"...\")".to_string(),
                success: true,
            },
        ];
        let proc = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .expect("extraction must succeed");
        assert_eq!(proc.source_strategy, "tool_grounded_react");
        assert_eq!(proc.steps.len(), 2, "two actions -> two steps");
        assert_eq!(proc.steps[0].action, "read_file");
        assert_eq!(proc.steps[1].action, "grep");
        // Each step has its observation excerpt as an output.
        assert_eq!(proc.steps[0].outputs.len(), 1);
        assert!(proc.steps[0].outputs[0].contains("pub fn main"));
        assert_eq!(proc.steps[1].outputs.len(), 1);
        assert!(proc.steps[1].outputs[0].contains("panic"));
        // Cost is propagated.
        assert_eq!(proc.model_calls, 4);
        assert_eq!(proc.total_tokens, 500);
        // Verification label.
        assert_eq!(proc.verification_summary, "passed");
        // ID is a stable hex string.
        assert_eq!(proc.id.len(), 64, "SHA-256 hex = 64 chars");
        assert!(proc.id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn extractor_from_trajectory_marks_failed_observations() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("do the work");
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "edit_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            TrajectoryEntry::Observation {
                text: "permission denied".to_string(),
                success: false,
            },
            TrajectoryEntry::Action {
                name: "edit_file".to_string(),
                arguments: serde_json::json!({"path": "b.txt"}),
            },
            TrajectoryEntry::Observation {
                text: "ok".to_string(),
                success: true,
            },
        ];
        let proc = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        assert!(!proc.steps[0].success, "first step failed");
        assert!(proc.steps[1].success, "second step succeeded");
    }

    #[test]
    fn extractor_from_trajectory_rejects_non_succeeded_outcome() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("do the work");
        let outcome = ReasoningOutcome {
            status: OutcomeStatus::Failed,
            ..succeeded_outcome(InferenceCost::default(), 0)
        };
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({}),
        }];
        let err = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .expect_err("non-succeeded must error");
        assert!(matches!(err, LearningError::OutcomeNotSucceeded(_)));
    }

    #[test]
    fn extractor_from_trajectory_rejects_empty_trajectory() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("do the work");
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let err = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &[],
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .expect_err("empty trajectory must error");
        assert!(matches!(err, LearningError::NoStepsToExtract));
    }

    #[test]
    fn extractor_from_trajectory_rejects_private_request_per_prd_17() {
        // PRD §17: PrivacyMode::Private means "no private-artifact
        // persistence". The extractor must refuse BEFORE doing any work so
        // no scrubbed-but-still-private bytes can leak through a future sink
        // bug. The privacy check runs even before the status check.
        let ext = WorkflowExtractor::new();
        let mut req = sample_request("do the work");
        req.privacy_mode = PrivacyMode::Private;
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.rs"}),
        }];
        let err = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .expect_err("Private request must be rejected");
        assert!(matches!(err, LearningError::PrivateRequestRejected));
    }

    #[test]
    fn extractor_from_outcome_rejects_private_request_per_prd_17() {
        // Same privacy guard, non-ReAct path. Must reject regardless of
        // outcome status / verifiers.
        let ext = WorkflowExtractor::new();
        let mut req = sample_request("compare options");
        req.privacy_mode = PrivacyMode::Private;
        let outcome = succeeded_outcome(InferenceCost::default(), 2);
        let err = ext
            .extract_from_outcome(
                &req,
                &outcome,
                ReasoningStrategy::ParallelCandidatesConsensus,
                "2026-01-01T00:00:00Z",
            )
            .expect_err("Private request must be rejected");
        assert!(matches!(err, LearningError::PrivateRequestRejected));
    }

    #[test]
    fn extractor_accepts_internal_and_public_privacy_modes() {
        // PRD §17: Internal (within a single provider boundary) and Public
        // (cross-provider verification allowed) ARE eligible for cognitive-
        // memory persistence. The check rejects ONLY Private.
        let ext = WorkflowExtractor::new();
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.rs"}),
        }];
        for mode in [PrivacyMode::Internal, PrivacyMode::Public] {
            let mut req = sample_request("do the work");
            req.privacy_mode = mode;
            let proc = ext
                .extract_from_trajectory(
                    &req,
                    &outcome,
                    &trajectory,
                    ReasoningStrategy::ToolGroundedReact,
                    "2026-01-01T00:00:00Z",
                )
                .unwrap_or_else(|e| panic!("mode {mode:?} must be eligible: {e:?}"));
            assert_eq!(proc.steps.len(), 1);
        }
    }

    #[test]
    fn extractor_from_trajectory_scrubs_secrets_in_tool_args_and_observations() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("ship the deploy with api_key=AKIAIOSFODNN7EXAMPLE");
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "run_command".to_string(),
                arguments: serde_json::json!({
                    "command": "export AWS_KEY=AKIAIOSFODNN7EXAMPLE"
                }),
            },
            TrajectoryEntry::Observation {
                text: "done with token ya29.SomeLongOpaqueTokenValue32CharsPlusMore".to_string(),
                success: true,
            },
        ];
        let proc = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        // The objective must not contain the literal AWS key.
        assert!(!proc.objective.contains("AKIAIOSFODNN7EXAMPLE"));
        // The tool-argument string must be redacted.
        let arg = &proc.steps[0].inputs[0];
        assert!(!arg.contains("AKIAIOSFODNN7EXAMPLE"));
        // The observation must be redacted.
        let obs = &proc.steps[0].outputs[0];
        assert!(!obs.contains("ya29.SomeLongOpaqueTokenValue"));
    }

    #[test]
    fn extractor_from_outcome_synthesizes_generate_step() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("compare options for the parser");
        let outcome = succeeded_outcome(
            InferenceCost {
                model_calls: 3,
                total_tokens: 200,
            },
            0,
        );
        let proc = ext
            .extract_from_outcome(
                &req,
                &outcome,
                ReasoningStrategy::ParallelCandidatesConsensus,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        assert_eq!(proc.source_strategy, "parallel_candidates_consensus");
        // No verifiers ran -> only one step (generate).
        assert_eq!(proc.steps.len(), 1);
        assert_eq!(proc.steps[0].action, "generate");
        // Final output is scrubbed + attached.
        assert!(!proc.steps[0].outputs.is_empty());
        assert!(proc.steps[0].outputs[0].contains("answer"));
    }

    #[test]
    fn extractor_from_outcome_adds_verify_step_when_verifiers_ran() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("verify the claim");
        let outcome = succeeded_outcome(
            InferenceCost {
                model_calls: 3,
                total_tokens: 200,
            },
            2,
        );
        let proc = ext
            .extract_from_outcome(
                &req,
                &outcome,
                ReasoningStrategy::GenerateVerifyRepair,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        assert_eq!(proc.steps.len(), 2, "generate + verify when verifiers ran");
        assert_eq!(proc.steps[1].action, "verify");
        assert!(proc.steps[1].success);
    }

    #[test]
    fn extractor_deterministic_id_is_stable_across_calls() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("the same prompt");
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.rs"}),
        }];
        let p1 = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        let p2 = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-02-02T00:00:00Z",
            )
            .unwrap();
        // The extracted_at timestamp is NOT part of the id, so two extractions
        // of the same procedure produce the same id (cognitive-memory dedupe).
        assert_eq!(p1.id, p2.id);
    }

    #[test]
    fn extractor_id_changes_when_objective_changes() {
        let ext = WorkflowExtractor::new();
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.rs"}),
        }];
        let p1 = ext
            .extract_from_trajectory(
                &sample_request("objective one"),
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        let p2 = ext
            .extract_from_trajectory(
                &sample_request("objective two"),
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        assert_ne!(p1.id, p2.id);
    }

    #[test]
    fn extractor_truncates_long_observations() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("dump the file");
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let long_text = "x".repeat(500);
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "big.txt"}),
            },
            TrajectoryEntry::Observation {
                text: long_text,
                success: true,
            },
        ];
        let proc = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        let obs = &proc.steps[0].outputs[0];
        assert!(
            obs.chars().count() <= 240,
            "observation must be truncated to <= 240 chars, got {}",
            obs.chars().count()
        );
        assert!(obs.ends_with("..."));
    }

    #[test]
    fn extractor_derives_title_truncating_at_word_boundary() {
        let ext = WorkflowExtractor::new();
        let long_msg = "find the root cause of the very long bug in the deep tree search algorithm that fails intermittently on windows";
        let req = sample_request(long_msg);
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![TrajectoryEntry::Action {
            name: "read_file".to_string(),
            arguments: serde_json::json!({}),
        }];
        let proc = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        assert!(
            proc.title.chars().count() <= 80,
            "title must be <= 80 chars, got {}: {}",
            proc.title.chars().count(),
            proc.title
        );
        assert!(proc.title.ends_with('…'));
    }

    #[test]
    fn is_learning_eligible_covers_complex_strategies_excluding_direct() {
        assert!(!is_learning_eligible(ReasoningStrategy::Direct));
        assert!(is_learning_eligible(ReasoningStrategy::ToolGroundedReact));
        assert!(is_learning_eligible(ReasoningStrategy::BoundedTreeSearch));
        assert!(is_learning_eligible(
            ReasoningStrategy::GenerateVerifyRepair
        ));
        assert!(is_learning_eligible(
            ReasoningStrategy::ParallelCandidatesConsensus
        ));
        assert!(is_learning_eligible(
            ReasoningStrategy::ProposerCriticAdjudicator
        ));
    }

    #[test]
    fn cost_summary_extracts_pair() {
        let pair = cost_summary(InferenceCost {
            model_calls: 7,
            total_tokens: 1234,
        });
        assert_eq!(pair, (7, 1234));
    }

    #[test]
    fn distinct_actions_dedupes() {
        let ext = WorkflowExtractor::new();
        let req = sample_request("do it");
        let outcome = succeeded_outcome(InferenceCost::default(), 0);
        let trajectory = vec![
            TrajectoryEntry::Action {
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
            TrajectoryEntry::Observation {
                text: "ok".to_string(),
                success: true,
            },
            TrajectoryEntry::Action {
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            },
            TrajectoryEntry::Observation {
                text: "ok2".to_string(),
                success: true,
            },
            TrajectoryEntry::Action {
                name: "grep".to_string(),
                arguments: serde_json::json!({}),
            },
        ];
        let proc = ext
            .extract_from_trajectory(
                &req,
                &outcome,
                &trajectory,
                ReasoningStrategy::ToolGroundedReact,
                "2026-01-01T00:00:00Z",
            )
            .unwrap();
        let actions = distinct_actions(&proc);
        assert_eq!(actions.len(), 2);
        assert!(actions.contains("read_file"));
        assert!(actions.contains("grep"));
    }

    // ============================================================
    // ProceduralMemorySink test fake + serialization
    // ============================================================

    #[tokio::test]
    async fn recording_sink_saves_and_returns_id() {
        let sink = RecordingSink::new();
        let proc = ProceduralMemory {
            id: "abc123".to_string(),
            title: "test".to_string(),
            objective: "do it".to_string(),
            source_strategy: "tool_grounded_react".to_string(),
            steps: Vec::new(),
            verification_summary: "passed".to_string(),
            extracted_at: "2026-01-01T00:00:00Z".to_string(),
            model_calls: 0,
            total_tokens: 0,
        };
        let id = sink.save_procedure(&proc).await.unwrap();
        assert_eq!(id, "abc123");
        assert_eq!(sink.saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recording_sink_returns_error_when_configured_to_fail() {
        let sink = RecordingSink::new();
        sink.fail_next
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let proc = ProceduralMemory {
            id: "abc".to_string(),
            title: "t".to_string(),
            objective: "o".to_string(),
            source_strategy: "tool_grounded_react".to_string(),
            steps: Vec::new(),
            verification_summary: "passed".to_string(),
            extracted_at: "t".to_string(),
            model_calls: 0,
            total_tokens: 0,
        };
        let err = sink.save_procedure(&proc).await.expect_err("must fail");
        assert!(matches!(err, LearningError::SinkRejected(_)));
    }

    #[test]
    fn procedural_memory_round_trips_through_serde_json() {
        let proc = ProceduralMemory {
            id: "deadbeef".to_string(),
            title: "title".to_string(),
            objective: "obj".to_string(),
            source_strategy: "tool_grounded_react".to_string(),
            steps: vec![ProceduralStep {
                index: 0,
                action: "read_file".to_string(),
                description: "Read a file.".to_string(),
                inputs: vec!["{\"path\":\"a.rs\"}".to_string()],
                outputs: vec!["contents".to_string()],
                success: true,
            }],
            verification_summary: "passed".to_string(),
            extracted_at: "2026-01-01T00:00:00Z".to_string(),
            model_calls: 1,
            total_tokens: 10,
        };
        let json = serde_json::to_string(&proc).unwrap();
        let back: ProceduralMemory = serde_json::from_str(&json).unwrap();
        assert_eq!(proc, back);
    }
}
