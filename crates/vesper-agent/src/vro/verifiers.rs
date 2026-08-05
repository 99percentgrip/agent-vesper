//! Deterministic Verifier Registry (VRO-2.2, PRD §10.8).
//!
//! Defines the async, object-safe [`Verifier`] trait, a [`VerifierRegistry`]
//! that routes verification requests by `verifier_id`, and the first two
//! deterministic verifiers: [`CargoCheckVerifier`] and [`CargoTestVerifier`].
//!
//! ## Why a boxed-future trait (not `async_trait`)
//!
//! The registry stores `Box<dyn Verifier>`, so the trait must be object-safe.
//! Native `async fn` in traits is not yet object-safe without a helper crate,
//! and the workspace does not depend on `async_trait` or `trait-variant`.
//! The trait method therefore returns `Pin<Box<dyn Future + Send>>` directly —
//! the same desugaring `async_trait` produces, with no new dependency.
//!
//! ## Blocking work
//!
//! The cargo verifiers shell out to `cargo`, which blocks. To avoid stalling
//! the async executor, [`Verifier::verify`] offloads the command to
//! [`tokio::task::spawn_blocking`]. The pure parsing logic
//! ([`CargoCheckVerifier::parse_findings`]) is sync and unit-testable without
//! invoking cargo. Each verifier also exposes a sync `run_and_build` entry
//! point so the temp-directory tests can run without a tokio runtime.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;

use vesper_domain::{
    EvidenceRef, VerificationFinding, VerificationResult, VerificationSeverity, VerificationStatus,
    VerifierId,
};

// ---------------------------------------------------------------------------
// Verification context
// ---------------------------------------------------------------------------

/// Inputs handed to a [`Verifier`] for one verification run.
#[derive(Debug, Clone)]
pub struct VerificationContext {
    /// Workspace root the verifier operates in (e.g. where `Cargo.toml` lives).
    pub workspace_root: PathBuf,
    /// Evidence gathered so far that the verifier may cite.
    pub evidence_refs: Vec<EvidenceRef>,
}

impl VerificationContext {
    /// Creates a context rooted at `workspace_root` with no prior evidence.
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            evidence_refs: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Verifier trait (async, object-safe via boxed future)
// ---------------------------------------------------------------------------

/// A deterministic verifier (PRD §10.8).
///
/// Implementations run a check against the [`VerificationContext`] and return a
/// [`VerificationResult`]. The trait is object-safe: the registry stores
/// `Box<dyn Verifier>`.
pub trait Verifier: Send + Sync {
    /// Stable verifier identifier (e.g. `"cargo_check"`). Must be unique within
    /// a [`VerifierRegistry`].
    fn id(&self) -> &str;

    /// Verifies the target described by `ctx`.
    ///
    /// Returns a boxed `Send` future so the trait remains object-safe without
    /// an `async_trait` dependency.
    fn verify<'a>(
        &'a self,
        ctx: &'a VerificationContext,
    ) -> Pin<Box<dyn Future<Output = VerificationResult> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// VerifierRegistry
// ---------------------------------------------------------------------------

/// Routes verification requests to registered [`Verifier`]s by `verifier_id`.
#[derive(Default)]
pub struct VerifierRegistry {
    verifiers: BTreeMap<String, Box<dyn Verifier>>,
}

impl VerifierRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry pre-loaded with the default deterministic cargo
    /// verifiers (`cargo_check`, `cargo_test`).
    #[must_use]
    pub fn default_cargo() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(CargoCheckVerifier));
        registry.register(Box::new(CargoTestVerifier));
        registry
    }

    /// Registers a verifier, keyed by its [`Verifier::id`]. Replaces any
    /// existing verifier with the same id.
    pub fn register(&mut self, verifier: Box<dyn Verifier>) {
        let id = verifier.id().to_string();
        self.verifiers.insert(id, verifier);
    }

    /// Returns whether a verifier with the given id is registered.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.verifiers.contains_key(id)
    }

    /// Returns the registered verifier ids in sorted order.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.verifiers.keys().map(String::as_str).collect()
    }

    /// Runs the verifier with the given id against `ctx`, or `None` if no such
    /// verifier is registered.
    pub async fn run(&self, id: &str, ctx: &VerificationContext) -> Option<VerificationResult> {
        let verifier = self.verifiers.get(id)?;
        Some(verifier.verify(ctx).await)
    }
}

impl std::fmt::Debug for VerifierRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifierRegistry")
            .field("ids", &self.ids())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Shared result builders
// ---------------------------------------------------------------------------

fn verifier_id(id: &str) -> VerifierId {
    VerifierId::new(id).unwrap_or_else(|_| {
        panic!("hardcoded verifier id {id:?} must be a valid bounded identifier")
    })
}

fn passed(id: &str) -> VerificationResult {
    VerificationResult {
        verifier_id: verifier_id(id),
        status: VerificationStatus::Passed,
        confidence: 1.0,
        findings: vec![],
        evidence_refs: vec![],
        repairable: false,
    }
}

fn failed(id: &str, findings: Vec<VerificationFinding>) -> VerificationResult {
    VerificationResult {
        verifier_id: verifier_id(id),
        status: VerificationStatus::Failed,
        confidence: 0.0,
        findings,
        evidence_refs: vec![],
        // Compiler/test failures are generally repairable (PRD §10.9).
        repairable: true,
    }
}

fn error_result(id: &str, message: impl Into<String>) -> VerificationResult {
    VerificationResult {
        verifier_id: verifier_id(id),
        status: VerificationStatus::Error,
        confidence: 0.0,
        findings: vec![VerificationFinding {
            message: message.into(),
            severity: VerificationSeverity::Error,
            location: None,
        }],
        evidence_refs: vec![],
        // We cannot tell whether the target is repairable if the verifier
        // itself could not run.
        repairable: false,
    }
}

// ---------------------------------------------------------------------------
// CargoCheckVerifier
// ---------------------------------------------------------------------------

/// Runs `cargo check --message-format=json` in the workspace root and maps
/// compiler diagnostics into [`VerificationFinding`]s (PRD §10.8).
pub struct CargoCheckVerifier;

impl CargoCheckVerifier {
    /// Stable verifier identifier.
    pub const ID: &'static str = "cargo_check";

    /// Pure parsing of `cargo check --message-format=json` stdout (one JSON
    /// object per line) into findings. Only `compiler-message` diagnostics at
    /// `error`/`warning` level are collected; `note`/`help`/`info` are dropped.
    #[must_use]
    pub fn parse_findings(stdout: &str) -> Vec<VerificationFinding> {
        let mut findings = Vec::new();
        for raw in stdout.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                // Non-JSON line (e.g. a stray cargo banner); skip.
                continue;
            };
            if value.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
                continue;
            }
            let Some(message) = value.get("message") else {
                continue;
            };
            let level = message.get("level").and_then(|l| l.as_str()).unwrap_or("");
            let severity = match level {
                "error" => VerificationSeverity::Error,
                "warning" => VerificationSeverity::Warning,
                _ => continue, // note / help / info are not findings.
            };
            let text = message
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(level)
                .to_string();
            let location = message
                .get("spans")
                .and_then(|s| s.as_array())
                .and_then(|arr| arr.first())
                .and_then(|span| {
                    let file = span.get("file_name")?.as_str()?;
                    let line_no = span.get("line_start").and_then(|l| l.as_u64())?;
                    Some(format!("{file}:{line_no}"))
                })
                // Normalize to forward slashes so the location contract is
                // deterministic cross-platform: cargo emits `src\lib.rs:1` on
                // Windows and `src/lib.rs:1` elsewhere. Downstream consumers
                // (repair controller, evidence refs) get one shape.
                .map(|loc| loc.replace('\\', "/"));
            findings.push(VerificationFinding {
                message: text,
                severity,
                location,
            });
        }
        findings
    }

    /// Synchronously runs `cargo check` and builds the [`VerificationResult`].
    /// Testable without a tokio runtime.
    fn run_and_build(workspace: &Path) -> VerificationResult {
        let output = match Command::new("cargo")
            .args(["check", "--message-format=json"])
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(workspace)
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                return error_result(Self::ID, format!("failed to spawn `cargo`: {err}"));
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut findings = Self::parse_findings(&stdout);
        let has_errors = findings
            .iter()
            .any(|f| f.severity >= VerificationSeverity::Error);

        if !output.status.success() && !has_errors {
            // Cargo failed for a non-compilation reason (e.g. a manifest
            // error) and produced no structured compiler findings. Surface
            // stderr so the result is still actionable.
            let stderr = String::from_utf8_lossy(&output.stderr);
            findings.push(VerificationFinding {
                message: format!("cargo check exited non-zero:\n{}", stderr.trim()),
                severity: VerificationSeverity::Error,
                location: None,
            });
        }

        if findings
            .iter()
            .any(|f| f.severity >= VerificationSeverity::Error)
        {
            failed(Self::ID, findings)
        } else {
            // A clean check (no error-severity findings) is a Pass. Warnings
            // are intentionally not surfaced on a passing result; they are only
            // observable alongside errors via the `Failed` path.
            passed(Self::ID)
        }
    }
}

impl Verifier for CargoCheckVerifier {
    fn id(&self) -> &str {
        Self::ID
    }

    fn verify<'a>(
        &'a self,
        ctx: &'a VerificationContext,
    ) -> Pin<Box<dyn Future<Output = VerificationResult> + Send + 'a>> {
        let workspace = ctx.workspace_root.clone();
        Box::pin(async move {
            match tokio::task::spawn_blocking(move || Self::run_and_build(&workspace)).await {
                Ok(result) => result,
                Err(join_err) => {
                    error_result(Self::ID, format!("verifier task panicked: {join_err}"))
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// CargoTestVerifier
// ---------------------------------------------------------------------------

/// Runs `cargo test` in the workspace root and maps test failures into
/// repairable [`VerificationFinding`]s (PRD §10.8).
pub struct CargoTestVerifier;

impl CargoTestVerifier {
    /// Stable verifier identifier.
    pub const ID: &'static str = "cargo_test";

    /// Parses human-readable `cargo test` output for failure markers.
    ///
    /// Recognizes `"... FAILED"` test lines and `panicked at` panics. The
    /// exact cargo output format is intentionally lenient here.
    #[must_use]
    pub fn parse_failures(combined_output: &str) -> Vec<VerificationFinding> {
        let mut findings = Vec::new();
        for raw in combined_output.lines() {
            let line = raw.trim();
            if line.contains("... FAILED") {
                findings.push(VerificationFinding {
                    message: line.to_string(),
                    severity: VerificationSeverity::Error,
                    location: None,
                });
            } else if let Some(rest) = line.strip_prefix("panicked at ") {
                // "panicked at 'msg', src/lib.rs:5:5" — best-effort location.
                // Normalize backslashes (Windows) to forward slashes.
                let location = rest.rsplit(", ").next().map(|s| s.replace('\\', "/"));
                findings.push(VerificationFinding {
                    message: format!("panicked at {rest}"),
                    severity: VerificationSeverity::Error,
                    location,
                });
            }
        }
        findings
    }

    /// Synchronously runs `cargo test` and builds the [`VerificationResult`].
    fn run_and_build(workspace: &Path) -> VerificationResult {
        let output = match Command::new("cargo")
            .args(["test"])
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(workspace)
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                return error_result(Self::ID, format!("failed to spawn `cargo`: {err}"));
            }
        };
        if output.status.success() {
            return passed(Self::ID);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        let findings = Self::parse_failures(&combined);
        let findings = if findings.is_empty() {
            // Non-zero exit with no recognizable test-failure marker: surface
            // stderr so the result is still actionable.
            vec![VerificationFinding {
                message: format!("cargo test exited non-zero:\n{}", stderr.trim()),
                severity: VerificationSeverity::Error,
                location: None,
            }]
        } else {
            findings
        };
        failed(Self::ID, findings)
    }
}

impl Verifier for CargoTestVerifier {
    fn id(&self) -> &str {
        Self::ID
    }

    fn verify<'a>(
        &'a self,
        ctx: &'a VerificationContext,
    ) -> Pin<Box<dyn Future<Output = VerificationResult> + Send + 'a>> {
        let workspace = ctx.workspace_root.clone();
        Box::pin(async move {
            match tokio::task::spawn_blocking(move || Self::run_and_build(&workspace)).await {
                Ok(result) => result,
                Err(join_err) => {
                    error_result(Self::ID, format!("verifier task panicked: {join_err}"))
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Pure parsing (no cargo invocation) ---

    #[test]
    fn parse_findings_extracts_error_and_warning_diagnostics() {
        let json = concat!(
            r#"{"reason":"compiler-message","message":{"level":"error","#,
            r#""message":"cannot find function `foo` in this scope","#,
            r#""spans":[{"file_name":"src/lib.rs","line_start":42}]}}"#,
            "\n",
            r#"{"reason":"compiler-message","message":{"level":"warning","#,
            r#""message":"unused variable `x`","spans":[{"file_name":"src/main.rs","line_start":7}]}}"#,
            "\n",
            r#"{"reason":"compiler-artifact","message":null}"#,
            "\n",
            r#"{"reason":"compiler-message","message":{"level":"note","#,
            r#""message":"an arm is required","spans":[]}}"#,
        );
        let findings = CargoCheckVerifier::parse_findings(json);
        assert_eq!(findings.len(), 2, "only error + warning become findings");
        assert_eq!(findings[0].severity, VerificationSeverity::Error);
        assert_eq!(
            findings[0].message,
            "cannot find function `foo` in this scope"
        );
        assert_eq!(findings[0].location.as_deref(), Some("src/lib.rs:42"));
        assert_eq!(findings[1].severity, VerificationSeverity::Warning);
        assert_eq!(findings[1].location.as_deref(), Some("src/main.rs:7"));
    }

    #[test]
    fn parse_findings_handles_empty_and_non_json_lines() {
        let findings = CargoCheckVerifier::parse_findings("\n  \nnot json at all\n");
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_findings_normalizes_backslash_locations_to_forward_slash() {
        // Regression: on Windows cargo emits `src\lib.rs` in file_name. The
        // location contract must be deterministic (forward slashes) everywhere
        // so downstream consumers and cross-platform tests see one shape.
        let json = concat!(
            r#"{"reason":"compiler-message","message":{"level":"error","#,
            r#""message":"unclosed delimiter","#,
            r#""spans":[{"file_name":"src\\lib.rs","line_start":1}]}}"#,
        );
        let findings = CargoCheckVerifier::parse_findings(json);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].location.as_deref(),
            Some("src/lib.rs:1"),
            "backslash must be normalized to forward slash"
        );
    }

    #[test]
    fn parse_test_failures_recognizes_failed_tests_and_panics() {
        let out = concat!(
            "test foo::bar ... ok\n",
            "test foo::baz ... FAILED\n",
            "panicked at 'assertion failed', src/lib.rs:5:5\n",
            "test result: FAILED. 1 passed; 1 failed;\n",
        );
        let findings = CargoTestVerifier::parse_failures(out);
        assert_eq!(findings.len(), 2);
        assert!(findings[0].message.contains("foo::baz"));
        assert!(findings[1].message.contains("assertion failed"));
        assert_eq!(findings[1].location.as_deref(), Some("src/lib.rs:5:5"));
    }

    // --- Registry ---

    #[test]
    fn default_cargo_registry_registers_both_verifiers() {
        let registry = VerifierRegistry::default_cargo();
        assert!(registry.contains("cargo_check"));
        assert!(registry.contains("cargo_test"));
        let ids = registry.ids();
        assert!(ids.contains(&"cargo_check"));
        assert!(ids.contains(&"cargo_test"));
    }

    #[test]
    fn empty_registry_contains_nothing() {
        let registry = VerifierRegistry::new();
        assert!(!registry.contains("cargo_check"));
        assert!(registry.ids().is_empty());
    }

    #[test]
    fn register_replaces_verifier_with_same_id() {
        let mut registry = VerifierRegistry::new();
        registry.register(Box::new(CargoCheckVerifier));
        registry.register(Box::new(CargoCheckVerifier));
        // Re-registering the same id does not duplicate.
        assert_eq!(registry.ids().len(), 1);
    }

    // --- Temp-directory verifier tests (real cargo invocation) ---
    //
    // These scaffold a minimal Cargo crate in a tempfile and run the actual
    // `cargo check`/`cargo test` against it. They require `cargo` on PATH
    // (always present in CI). The temp crate has zero external dependencies so
    // cargo does not touch the shared registry cache.

    /// Minimal valid crate source.
    const VALID_LIB: &str = "pub fn add(a: i32, b: i32) -> i32 { a + b }\n";

    /// Crate source with an unclosed function — a hard syntax error.
    const BROKEN_LIB: &str = "pub fn broken(\n";

    /// Crate source with a failing test.
    const FAILING_TEST_LIB: &str = "#[test]\nfn it_fails() { assert_eq!(2 + 2, 5); }\n";

    fn scaffold_crate(lib_src: &str) -> tempfile::TempDir {
        use std::fs;
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            dir.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"vro_test_crate\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2021\"\n",
                "\n",
                "[lib]\n",
                "path = \"src/lib.rs\"\n",
            ),
        )
        .expect("write Cargo.toml");
        fs::create_dir_all(dir.path().join("src")).expect("mkdir src");
        fs::write(dir.path().join("src/lib.rs"), lib_src).expect("write lib.rs");
        dir
    }

    #[test]
    fn cargo_check_passes_on_valid_crate() {
        let dir = scaffold_crate(VALID_LIB);
        let result = CargoCheckVerifier::run_and_build(dir.path());
        assert_eq!(
            result.status,
            VerificationStatus::Passed,
            "valid crate must pass cargo check; findings: {:?}",
            result.findings
        );
        assert_eq!(result.verifier_id.as_str(), "cargo_check");
        assert!(!result.repairable);
    }

    #[test]
    fn cargo_check_fails_on_broken_crate() {
        let dir = scaffold_crate(BROKEN_LIB);
        let result = CargoCheckVerifier::run_and_build(dir.path());
        assert_eq!(
            result.status,
            VerificationStatus::Failed,
            "broken crate must fail cargo check"
        );
        assert!(result.repairable, "compiler failures are repairable");
        assert!(
            result
                .findings
                .iter()
                .any(|f| f.severity >= VerificationSeverity::Error),
            "a failed check must carry at least one error finding"
        );
        // The finding should locate the broken file.
        assert!(
            result.findings.iter().any(|f| f
                .location
                .as_deref()
                .is_some_and(|loc| loc.contains("src/lib.rs"))),
            "findings should locate src/lib.rs; got: {:?}",
            result.findings
        );
    }

    #[test]
    fn cargo_test_passes_on_valid_crate() {
        let dir = scaffold_crate(
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[test]\nfn it_works() { assert_eq!(2 + 2, 4); }\n",
        );
        let result = CargoTestVerifier::run_and_build(dir.path());
        assert_eq!(
            result.status,
            VerificationStatus::Passed,
            "passing tests must report Passed; stderr in findings: {:?}",
            result.findings
        );
        assert_eq!(result.verifier_id.as_str(), "cargo_test");
    }

    #[test]
    fn cargo_test_fails_on_failing_test() {
        let dir = scaffold_crate(FAILING_TEST_LIB);
        let result = CargoTestVerifier::run_and_build(dir.path());
        assert_eq!(
            result.status,
            VerificationStatus::Failed,
            "a failing test must report Failed"
        );
        assert!(result.repairable, "test failures are repairable");
    }

    #[tokio::test]
    async fn async_verifier_trait_runs_cargo_check_through_registry() {
        // Exercises the full async path: registry.run() -> Verifier::verify()
        // -> spawn_blocking -> cargo check.
        let dir = scaffold_crate(VALID_LIB);
        let ctx = VerificationContext::new(dir.path().to_path_buf());
        let registry = VerifierRegistry::default_cargo();
        let result = registry.run("cargo_check", &ctx).await;
        let result = result.expect("cargo_check is registered");
        assert_eq!(result.status, VerificationStatus::Passed);
        assert_eq!(result.verifier_id.as_str(), "cargo_check");
    }

    #[tokio::test]
    async fn run_returns_none_for_unknown_verifier() {
        let registry = VerifierRegistry::default_cargo();
        let ctx = VerificationContext::new(PathBuf::from("/nonexistent"));
        assert!(registry.run("json_schema", &ctx).await.is_none());
    }

    #[tokio::test]
    async fn async_verifier_reports_error_when_workspace_is_garbage() {
        // Point cargo check at a path with no Cargo.toml -> cargo fails to run
        // meaningfully. The verifier must not panic; it returns a non-Passed
        // result (Failed or Error) with at least one finding.
        let dir = tempfile::tempdir().expect("temp dir");
        let ctx = VerificationContext::new(dir.path().to_path_buf());
        let registry = VerifierRegistry::default_cargo();
        let result = registry
            .run("cargo_check", &ctx)
            .await
            .expect("cargo_check is registered");
        assert_ne!(result.status, VerificationStatus::Passed);
        assert!(!result.findings.is_empty());
    }
}
