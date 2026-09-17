//! VB-PRD-001 Phase 2: the shared `/bridge` host-command answers.
//!
//! Host-level, read-only text. Session control (`stop`/`resume`/`release
//! confirmed`/`disconnect`) is EXECUTED by the host against the live
//! service — those verbs never appear here as static text, and this
//! module never panics (audit H4/L3: shared command code must answer
//! truthfully, not `unreachable!()`). Connect and execute flow through
//! the model tool surface so every operation passes the same Bridge gate.

use std::path::Path;

/// The verbs a host executes against the live service (NF-02/BR-21).
pub const EXECUTED_VERBS: [&str; 4] = ["stop", "resume", "release confirmed", "disconnect"];

/// Answer one `/bridge <argument>` command with bounded, truthful text.
///
/// `stop`/`resume`/`release confirmed`/`disconnect` are executed by the
/// host against the live service before this function is consulted; if a
/// host ever routes one here, the honest answer is a pointer to that
/// execution path — never a panic (audit L3).
#[must_use]
pub fn command(argument: &str, root: Option<&Path>) -> String {
    match argument.trim().to_ascii_lowercase().as_str() {
        "" | "status" => status_text(root),
        "discover" => {
            #[cfg(feature = "bridge")]
            {
                crate::bridge_service::BRIDGE_NO_ADAPTER_DISCOVERY.to_string()
            }
            #[cfg(not(feature = "bridge"))]
            {
                "Bridge discovery requires a build with the `bridge` feature enabled; this build has no Bridge surface.".to_string()
            }
        }
        // Executed verbs: a host that reaches this arm did not wire the
        // live-service path. Truthful text, not a panic (H4/L3).
        "stop" => "Bridge stop is executed by the host against the live service; this answer means the host did not wire that path — no state was changed.".into(),
        "resume" => "Bridge resume is executed by the host against the live service; this answer means the host did not wire that path — no state was changed.".into(),
        "disconnect" => "Bridge disconnect is executed by the host against the live service; this answer means the host did not wire that path — no state was changed.".into(),
        "release confirmed" | "confirm release" => "Bridge input-release confirmation is executed by the host against the live service; this answer means the host did not wire that path — no state was changed.".into(),
        other => format!(
            "Unknown /bridge argument `{other}`. Available: status, discover, stop, resume, release confirmed, disconnect (executed by the host), plus connect/observe/execute via the bridge_* model tools. Matching is case-insensitive."
        ),
    }
}

/// Wrapper hosts use for executed verbs once they have run the operation
/// against the live service: echoes the truthful report.
#[must_use]
pub fn executed_report(report: String) -> String {
    report
}

/// Status text with the effective settings (read-only).
#[must_use]
pub fn status_text(root: Option<&Path>) -> String {
    #[cfg(feature = "bridge")]
    {
        let enabled = root.is_some_and(|root| crate::bridge_settings::holder::shared(root).enabled);
        if enabled {
            "Bridge: enabled (no adapter attached). Capabilities report dependency_missing/unknown truthfully; nothing is controllable yet.".into()
        } else {
            "Bridge: disabled. Enable via .agent-vesper/bridge-settings.json {\"enabled\":true} and restart; the disabled path performs no process, capture, network or model activity.".into()
        }
    }
    #[cfg(not(feature = "bridge"))]
    {
        let _ = root;
        "Bridge: not compiled into this build (feature `bridge` off).".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_disabled_without_a_root() {
        // Feature-off builds honestly say "not compiled"; feature-on
        // builds with no settings root say "disabled". Both are the
        // truthful off-state for their build.
        let text = status_text(None);
        assert!(
            text.starts_with("Bridge: disabled.") || text.starts_with("Bridge: not compiled"),
            "unexpected off-state text: {text}"
        );
    }

    #[test]
    fn unknown_arguments_list_the_surface() {
        let answer = command("explode", None);
        assert!(answer.starts_with("Unknown /bridge argument"));
        assert!(answer.contains("status"));
    }

    #[test]
    fn no_answer_claims_an_application_action() {
        for argument in ["", "status", "discover", "resume", "disconnect", "nonsense"] {
            let answer = command(argument, None);
            assert!(
                !answer.to_lowercase().contains("connected to application"),
                "host answer must not claim connection: {answer}"
            );
        }
    }

    #[test]
    fn executed_report_passes_through_truthful_reports() {
        assert_eq!(
            executed_report("Bridge stopped: admission closed.".into()),
            "Bridge stopped: admission closed."
        );
    }

    /// H4/L3: shared command code must NEVER panic — every verb answers.
    #[test]
    fn every_documented_verb_answers_without_panicking() {
        for argument in EXECUTED_VERBS {
            let answer = command(argument, None);
            assert!(!answer.is_empty(), "{argument} must answer");
            assert!(
                answer.contains("executed by the host"),
                "{argument} must point at the live-service path: {answer}"
            );
        }
    }

    /// M8: the help text lists only real verbs, and the synonyms match.
    #[test]
    fn help_text_lists_real_verbs_and_synonyms_resolve() {
        let help = command("explode", None);
        assert!(
            !help.contains("connect <app>"),
            "connect is a model-tool route, not a /bridge verb"
        );
        assert_eq!(
            command("CONFIRM RELEASE", None),
            command("release confirmed", None)
        );
        assert_eq!(command("Status", None), command("status", None));
    }
}
