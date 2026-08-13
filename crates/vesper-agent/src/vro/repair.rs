//! Repair Controller heuristics (VRO-10, PRD §10.9).
//!
//! The VRO-2.3 Generate-Verify-Repair loop fed every failed verifier's raw
//! findings back to the generator as `corrections`. PRD §10.9 demands a
//! richer Repair Controller that classifies each finding by error class and
//! injects a **class-specific correction hint** so the next generation
//! targets the root cause instead of guessing. This module is the pure
//! classification + hint surface; the orchestrator's loop calls
//! [`RepairController::augment_corrections`] before each re-Generate.
//!
//! ## Heuristic map (PRD §10.9 directive)
//!
//! | [`RepairHeuristic`]            | Trigger signature                              | Hint prefix                                          |
//! |--------------------------------|------------------------------------------------|------------------------------------------------------|
//! | `JsonParse`                    | finding message mentions JSON / parse / syntax | "Repair the JSON syntax: ensure the output is valid JSON…" |
//! | `SchemaMismatch`               | finding message mentions schema / type / field | "Repair the schema: align the output with the required schema…" |
//! | `FileNotFound`                 | finding message mentions file / path / not found | "Repair the file references: re-check the paths…"   |
//! | `CompilationError`             | finding location ends with `.rs` and severity ≥ error | "Repair the compilation: address the cargo error…" |
//! | `TestFailure`                  | finding message mentions test / assertion      | "Repair the failing tests: address each assertion…" |
//! | `ConstraintViolation`          | finding message mentions constraint / invariant / policy | "Repair the constraint violation: …"      |
//! | `Generic`                      | (fallback — no signature matched)              | (no hint; the raw finding is already in corrections) |
//!
//! ## Determinism + zero-breakage
//!
//! The classifier is pure: it consumes a `&[VerificationFinding]` slice and
//! returns a deterministic `Vec<(usize, RepairHeuristic)>` (one per finding,
//! in input order). The hint text is a `&'static str` per heuristic. The
//! orchestrator only **appends** these hints; existing corrections remain
//! intact, so behavior with no classifiable findings is byte-identical to
//! VRO-2.3.
//!
//! ## Repetition guard (PRD §10.9: "Avoid repeating an identical failed attempt")
//!
//! [`RepairController::is_repeated_attempt`] compares the new corrections
//! against the previously-injected set; when identical, the orchestrator
//! must escalate the strategy or halt rather than re-issue the same prompt.

use std::collections::HashSet;

use vesper_domain::{VerificationFinding, VerificationSeverity};

/// One repair-controller heuristic class (PRD §10.9 directive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RepairHeuristic {
    /// JSON / parse / syntax error — inject a syntax-correction prompt.
    JsonParse,
    /// Schema / type / field mismatch — inject a schema-alignment prompt.
    SchemaMismatch,
    /// File / path not found — inject a path-checking prompt.
    FileNotFound,
    /// Rust / compiler error — inject a cargo-error correction prompt.
    CompilationError,
    /// Test / assertion failure — inject a test-fixing prompt.
    TestFailure,
    /// Constraint / invariant / policy violation.
    ConstraintViolation,
    /// No classifiable signature matched — no targeted hint.
    #[default]
    Generic,
}

impl RepairHeuristic {
    /// The class-specific correction hint prepended to the next Generate's
    /// corrections. `Generic` returns `None` so the orchestrator's behavior
    /// is byte-identical to VRO-2.3 for findings that do not classify.
    #[must_use]
    pub fn correction_hint(self) -> Option<&'static str> {
        match self {
            Self::JsonParse => Some(
                "Repair the JSON syntax: ensure the output is valid JSON with no trailing commas, \
                 unbalanced braces, or unquoted keys. Re-emit the entire payload.",
            ),
            Self::SchemaMismatch => Some(
                "Repair the schema mismatch: align each field with the required type and name. \
                 Drop unknown fields and add any missing required fields.",
            ),
            Self::FileNotFound => Some(
                "Repair the file references: re-check each path against the workspace tree before \
                 reusing it. Use read-only inspection tools to confirm existence.",
            ),
            Self::CompilationError => Some(
                "Repair the compilation error: address each cargo / rustc diagnostic. Re-run \
                 `cargo check` mentally before emitting the next patch.",
            ),
            Self::TestFailure => Some(
                "Repair the failing tests: address each assertion. Trace the failure back to the \
                 smallest change that flips the assertion green.",
            ),
            Self::ConstraintViolation => Some(
                "Repair the constraint violation: identify which invariant was broken and restore \
                 it. Do not relax the constraint unless explicitly authorized.",
            ),
            Self::Generic => None,
        }
    }
}

/// Classifies a single finding into a [`RepairHeuristic`] by signature
/// matching against the finding's `message`, `severity`, and `location`.
///
/// The classifier is deterministic: the same input always yields the same
/// heuristic. It is intentionally conservative — when in doubt it returns
/// [`RepairHeuristic::Generic`] (no targeted hint) so the orchestrator never
/// mis-routes a repair.
#[must_use]
pub fn classify_finding(finding: &VerificationFinding) -> RepairHeuristic {
    // Order matters: more specific classes are checked before more general
    // ones. A finding that mentions BOTH "test" and "schema" is classified
    // as SchemaMismatch because schema is the more specific root cause
    // (a schema mismatch surfaces in tests, but the schema is what to fix).
    let msg = finding.message.to_ascii_lowercase();
    let mentions = |needle: &str| msg.contains(needle);

    // JSON / parse / syntax errors — the directive's canonical example.
    if mentions("json")
        || mentions("failed to parse")
        || mentions("parse error")
        || mentions("parse failed")
        || mentions("syntax error")
        || mentions("deserialize")
        || mentions("unexpected token")
        || mentions("trailing comma")
        || mentions("expected `{`")
        || mentions("expected `}`")
        || mentions("expected `[`")
        || mentions("expected string")
        || mentions("expected number")
    {
        return RepairHeuristic::JsonParse;
    }
    // Schema / type / field mismatches.
    if mentions("schema")
        || mentions("type mismatch")
        || mentions("missing field")
        || mentions("unknown field")
        || mentions("expected type")
        || mentions("invalid type")
    {
        return RepairHeuristic::SchemaMismatch;
    }
    // File / path not found — the directive's other canonical example.
    if mentions("not found")
        || mentions("no such file")
        || mentions("no such directory")
        || (mentions("file") && mentions("missing"))
        || mentions("path")
    {
        return RepairHeuristic::FileNotFound;
    }
    // Test failures — assertion / panic / failed test signatures.
    if mentions("test")
        || mentions("assertion")
        || mentions("assert")
        || mentions("panic")
        || mentions("expected `")
    {
        return RepairHeuristic::TestFailure;
    }
    // Constraint / invariant / policy violations.
    if mentions("constraint")
        || mentions("invariant")
        || mentions("policy")
        || mentions("permission")
        || mentions("forbidden")
    {
        return RepairHeuristic::ConstraintViolation;
    }
    // Compilation errors — location contains a source file extension
    // (possibly followed by `:line:col`) AND severity is Error or Critical.
    // This catches cargo / rustc diagnostics that do not literally contain
    // "json" or "test" but do point at source.
    if finding.severity >= VerificationSeverity::Error
        && finding.location.as_ref().is_some_and(|loc| {
            let lower = loc.to_ascii_lowercase();
            lower.contains(".rs:")
                || lower.ends_with(".rs")
                || lower.contains(".go:")
                || lower.ends_with(".go")
                || lower.contains(".py:")
                || lower.ends_with(".py")
                || lower.contains(".ts:")
                || lower.ends_with(".ts")
                || lower.contains(".js:")
                || lower.ends_with(".js")
        })
    {
        return RepairHeuristic::CompilationError;
    }
    RepairHeuristic::Generic
}

/// Classifies a slice of findings into per-finding heuristics, in input
/// order. Convenience wrapper over [`classify_finding`].
#[must_use]
pub fn classify_findings(findings: &[VerificationFinding]) -> Vec<RepairHeuristic> {
    findings.iter().map(classify_finding).collect()
}

/// The bounded Repair Controller (PRD §10.9).
///
/// Stateless and cheap to construct. Holds the previously-injected
/// corrections signature so [`Self::is_repeated_attempt`] can detect an
/// identical retry (PRD §10.9: "Avoid repeating an identical failed attempt").
#[derive(Debug, Clone, Default)]
pub struct RepairController {
    /// The set of finding-message hashes injected on the previous repair
    /// attempt. Empty until the first call to
    /// [`Self::augment_corrections`].
    last_signature: Option<HashSet<String>>,
}

impl RepairController {
    /// Constructs a fresh controller with no prior attempt recorded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Augments `corrections` with class-specific hints derived from the
    /// failed verifiers' findings. Returns the [`RepairHeuristic`] assigned
    /// to each input finding (in order) so the caller can record the
    /// classification for telemetry.
    ///
    /// Behavior:
    /// - For each finding, [`classify_finding`] decides its heuristic.
    /// - When the heuristic has a [`RepairHeuristic::correction_hint`], a
    ///   synthetic [`VerificationFinding`] carrying that hint is appended to
    ///   `corrections` (severity `Info`, location `None`).
    /// - `Generic` findings contribute no synthetic correction — the raw
    ///   finding itself was already added by the orchestrator's existing
    ///   feedback path, so the VRO-2.3 behavior is preserved.
    /// - The controller records the set of finding messages it just
    ///   classified so the next call can detect a repeated attempt.
    pub fn augment_corrections(
        &mut self,
        corrections: &mut Vec<VerificationFinding>,
        findings: &[VerificationFinding],
    ) -> Vec<RepairHeuristic> {
        let mut classes = Vec::with_capacity(findings.len());
        let mut signature = HashSet::new();
        for finding in findings {
            let class = classify_finding(finding);
            classes.push(class);
            signature.insert(finding.message.clone());
            if let Some(hint) = class.correction_hint() {
                corrections.push(VerificationFinding {
                    message: hint.to_string(),
                    severity: VerificationSeverity::Info,
                    location: None,
                });
            }
        }
        self.last_signature = Some(signature);
        classes
    }

    /// Returns `true` when the supplied findings are byte-identical (by
    /// message text) to the set classified on the previous
    /// [`Self::augment_corrections`] call. PRD §10.9: "Avoid repeating an
    /// identical failed attempt" — the orchestrator must escalate or halt
    /// instead of re-issuing the same prompt.
    ///
    /// Returns `false` when no prior attempt was recorded (the first
    /// repair is never "repeated") or when the message set differs.
    #[must_use]
    pub fn is_repeated_attempt(&self, findings: &[VerificationFinding]) -> bool {
        let Some(last) = self.last_signature.as_ref() else {
            return false;
        };
        let current: HashSet<String> = findings.iter().map(|f| f.message.clone()).collect();
        current == *last
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use vesper_domain::{VerificationFinding, VerificationSeverity};

    fn finding(
        message: &str,
        severity: VerificationSeverity,
        location: Option<&str>,
    ) -> VerificationFinding {
        VerificationFinding {
            message: message.to_string(),
            severity,
            location: location.map(str::to_string),
        }
    }

    // --- classify_finding directive examples ---

    #[test]
    fn classify_json_parse_errors() {
        let cases = [
            "invalid JSON: unexpected token at column 5",
            "failed to parse payload",
            "syntax error near `}`",
            "deserialize failed: expected string",
            "trailing comma in object literal",
        ];
        for msg in cases {
            assert_eq!(
                classify_finding(&finding(msg, VerificationSeverity::Error, None)),
                RepairHeuristic::JsonParse,
                "expected JsonParse for: {msg}"
            );
        }
    }

    #[test]
    fn classify_file_not_found_errors() {
        let cases = [
            "file `src/main.rs` not found",
            "no such file or directory",
            "missing file: config.toml",
            "path does not exist",
        ];
        for msg in cases {
            assert_eq!(
                classify_finding(&finding(msg, VerificationSeverity::Error, None)),
                RepairHeuristic::FileNotFound,
                "expected FileNotFound for: {msg}"
            );
        }
    }

    #[test]
    fn classify_schema_mismatch_errors() {
        let cases = [
            "schema mismatch: missing field `name`",
            "type mismatch: expected u32 found string",
            "unknown field `extra`",
        ];
        for msg in cases {
            assert_eq!(
                classify_finding(&finding(msg, VerificationSeverity::Error, None)),
                RepairHeuristic::SchemaMismatch,
                "expected SchemaMismatch for: {msg}"
            );
        }
    }

    #[test]
    fn classify_test_failures() {
        let cases = [
            "test foo::bar failed",
            "assertion failed: left == right",
            "panicked at 'assertion'",
        ];
        for msg in cases {
            assert_eq!(
                classify_finding(&finding(msg, VerificationSeverity::Error, None)),
                RepairHeuristic::TestFailure,
                "expected TestFailure for: {msg}"
            );
        }
    }

    #[test]
    fn classify_compilation_errors_via_source_location() {
        // The message does NOT mention JSON/test/schema, but the location
        // ends with .rs and the severity is Error — classify as
        // CompilationError so the orchestrator injects a cargo-error hint.
        let f = finding(
            "cannot find function `foo`",
            VerificationSeverity::Error,
            Some("src/lib.rs:42"),
        );
        assert_eq!(classify_finding(&f), RepairHeuristic::CompilationError);
    }

    #[test]
    fn classify_constraint_violations() {
        let cases = [
            "constraint violation: foreign key",
            "invariant broken: balance negative",
            "permission denied",
        ];
        for msg in cases {
            assert_eq!(
                classify_finding(&finding(msg, VerificationSeverity::Error, None)),
                RepairHeuristic::ConstraintViolation,
                "expected ConstraintViolation for: {msg}"
            );
        }
    }

    #[test]
    fn classify_unrecognized_findings_fall_back_to_generic() {
        let f = finding(
            "something weird happened",
            VerificationSeverity::Warning,
            None,
        );
        assert_eq!(classify_finding(&f), RepairHeuristic::Generic);
    }

    // --- correction_hint ---

    #[test]
    fn correction_hints_are_present_for_classified_classes_and_absent_for_generic() {
        for class in [
            RepairHeuristic::JsonParse,
            RepairHeuristic::SchemaMismatch,
            RepairHeuristic::FileNotFound,
            RepairHeuristic::CompilationError,
            RepairHeuristic::TestFailure,
            RepairHeuristic::ConstraintViolation,
        ] {
            assert!(
                class.correction_hint().is_some(),
                "{class:?} must carry a targeted correction hint"
            );
        }
        assert!(RepairHeuristic::Generic.correction_hint().is_none());
    }

    // --- RepairController ---

    #[test]
    fn augment_corrections_injects_class_specific_hints_in_order() {
        let mut controller = RepairController::new();
        let findings = vec![
            finding("invalid JSON", VerificationSeverity::Error, None),
            finding("file foo.rs not found", VerificationSeverity::Error, None),
        ];
        let mut corrections: Vec<VerificationFinding> = Vec::new();
        let classes = controller.augment_corrections(&mut corrections, &findings);
        assert_eq!(classes.len(), 2);
        assert_eq!(classes[0], RepairHeuristic::JsonParse);
        assert_eq!(classes[1], RepairHeuristic::FileNotFound);
        // Two synthetic hint findings appended (one per classifiable finding).
        assert_eq!(corrections.len(), 2);
        assert!(corrections[0].message.contains("JSON"));
        assert!(corrections[1].message.contains("file references"));
        // Hints are Info severity (not Error) so the generator treats them as
        // guidance, not additional failures.
        assert_eq!(corrections[0].severity, VerificationSeverity::Info);
    }

    #[test]
    fn augment_corrections_skips_generic_findings_unchanged_from_vro_2_3() {
        // A finding that classifies as Generic must NOT inject a synthetic
        // correction — VRO-2.3's behavior (raw finding already in
        // corrections) is preserved.
        let mut controller = RepairController::new();
        let findings = vec![finding("weird thing", VerificationSeverity::Warning, None)];
        let mut corrections: Vec<VerificationFinding> = Vec::new();
        let classes = controller.augment_corrections(&mut corrections, &findings);
        assert_eq!(classes, vec![RepairHeuristic::Generic]);
        assert!(corrections.is_empty(), "Generic findings inject no hints");
    }

    #[test]
    fn is_repeated_attempt_detects_identical_message_set() {
        let mut controller = RepairController::new();
        let findings = vec![
            finding("invalid JSON", VerificationSeverity::Error, None),
            finding("missing semicolon", VerificationSeverity::Error, None),
        ];
        let mut corrections = Vec::new();
        controller.augment_corrections(&mut corrections, &findings);
        // Same set, different order — still repeated.
        let same_set_reordered = vec![
            finding("missing semicolon", VerificationSeverity::Error, None),
            finding("invalid JSON", VerificationSeverity::Error, None),
        ];
        assert!(
            controller.is_repeated_attempt(&same_set_reordered),
            "identical message set must be flagged as repeated"
        );
        // Different set — not repeated.
        let different = vec![finding(
            "different error",
            VerificationSeverity::Error,
            None,
        )];
        assert!(!controller.is_repeated_attempt(&different));
    }

    #[test]
    fn is_repeated_attempt_returns_false_with_no_prior_signature() {
        let controller = RepairController::new();
        assert!(!controller.is_repeated_attempt(&[finding(
            "x",
            VerificationSeverity::Error,
            None
        )]));
    }
}
