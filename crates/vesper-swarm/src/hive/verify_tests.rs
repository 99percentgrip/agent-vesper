//! Verification-gate unit proofs (VRO-16 PR-3 directive 4).
use super::*;

/// Digest determinism and separation: identical content digests equal;
/// any byte change (tamper) diverges.
#[test]
fn digests_are_deterministic_and_tamper_sensitive() {
    assert_eq!(
        artifact_digest("t0", "hello"),
        artifact_digest("t0", "hello")
    );
    assert_ne!(
        artifact_digest("t0", "hello"),
        artifact_digest("t0", "hellp")
    );
    // Task-id/output boundary cannot be forged by concatenation.
    assert_ne!(artifact_digest("ab", "c"), artifact_digest("a", "bc"));
}

/// Digest gate: admission records, verification passes on unchanged
/// output, tampering fails closed with the recorded/recomputed pair.
#[test]
fn tampered_artifacts_are_inadmissible() {
    let mut book = EvidenceBook::default();
    book.admit("t0", "original output", 11).unwrap();
    assert_eq!(
        book.verify("t0", "original output"),
        Ok(artifact_digest("t0", "original output"))
    );
    let failure = book.verify("t0", "tampered output").unwrap_err();
    assert_eq!(
        failure,
        VerificationFailure::DigestMismatch {
            task_id: String::from("t0"),
            recorded: artifact_digest("t0", "original output"),
            recomputed: artifact_digest("t0", "tampered output"),
        }
    );
    // Unknown tasks are refused, not silently admitted.
    assert_eq!(
        book.verify("nope", "x"),
        Err(VerificationFailure::UnknownTask {
            task_id: String::from("nope")
        })
    );
    // Double admission is refused.
    assert!(book.admit("t0", "again", 12).is_err());
}

/// Trace gate: citations resolve only against admitted artifacts;
/// dangling citations fail closed.
#[test]
fn dangling_citations_fail_closed() {
    let mut book = EvidenceBook::default();
    book.admit("t0", "output a", 100).unwrap();
    book.admit("t1", "output b", 101).unwrap();
    let good = "Finding [evidence:100] and [evidence:101] support X.";
    assert_eq!(extract_citations(good), vec![100, 101]);
    assert_eq!(verify_traces(good, &book).unwrap(), vec![100, 101]);
    // A citation to nothing fails closed with the offending id.
    let bad = "Claim [evidence:4242] from nowhere.";
    assert_eq!(
        verify_traces(bad, &book),
        Err(VerificationFailure::DanglingCitation {
            ledger_entry_id: 4242
        })
    );
    // Uncited synthesis passes the trace gate itself (citation coverage
    // policy is the caller's); the gate checks what IS cited.
    assert_eq!(
        verify_traces("no citations here", &book).unwrap(),
        Vec::<u64>::new()
    );
}

/// Budget watchdog: 50/80 emit without stopping; 100 hard-stops; each
/// level fires exactly once; skipped levels emit retroactively in order.
#[test]
fn budget_thresholds_emit_once_and_hard_stop_at_100() {
    let mut watchdog = BudgetWatchdog::new(BudgetCeiling {
        tokens: 1000,
        elapsed_ms: 10_000,
    });
    // 0–49%: nominal, no events.
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 100,
            elapsed_ms: 500,
        },
    );
    assert_eq!(state, BudgetState::Nominal);
    assert!(events.is_empty());
    // 50%: warning + event.
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 500,
            elapsed_ms: 2_000,
        },
    );
    assert_eq!(state, BudgetState::Warning { percent: 50 });
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        crate::hive::governance::AuditEvent::BudgetThreshold { level: 50, .. }
    ));
    // Re-evaluating at 55%: still Warning, no duplicate event.
    let (_, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 550,
            elapsed_ms: 2_000,
        },
    );
    assert!(events.is_empty());
    // 80%: critical + event.
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 800,
            elapsed_ms: 4_000,
        },
    );
    assert_eq!(state, BudgetState::Critical { percent: 80 });
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        crate::hive::governance::AuditEvent::BudgetThreshold { level: 80, .. }
    ));
    // 100%: exhausted + event; the binding axis (tokens) governs.
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 1000,
            elapsed_ms: 5_000,
        },
    );
    assert_eq!(state, BudgetState::Exhausted { percent: 100 });
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        crate::hive::governance::AuditEvent::BudgetThreshold { level: 100, .. }
    ));
    // All three levels fired exactly once, in order.
    assert_eq!(watchdog.fired_levels(), vec![50, 80, 100]);
}

/// A single big jump skips intermediate levels: the skipped ones emit
/// retroactively in ascending order alongside the highest.
#[test]
fn skipped_thresholds_emit_retroactively() {
    let mut watchdog = BudgetWatchdog::new(BudgetCeiling {
        tokens: 100,
        elapsed_ms: 100_000,
    });
    let (state, events) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 95,
            elapsed_ms: 1_000,
        },
    );
    assert_eq!(state, BudgetState::Critical { percent: 95 });
    assert_eq!(events.len(), 2, "50 and 80 both emit");
    let levels: Vec<u8> = events
        .iter()
        .map(|event| match event {
            crate::hive::governance::AuditEvent::BudgetThreshold { level, .. } => *level,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(levels, vec![50, 80]);
}

/// Time axis can govern: an idle-token run still hard-stops on time.
#[test]
fn time_axis_governs_independently() {
    let mut watchdog = BudgetWatchdog::new(BudgetCeiling {
        tokens: 1_000_000,
        elapsed_ms: 1_000,
    });
    let (state, _) = watchdog.evaluate(
        "g",
        BudgetReading {
            tokens: 0,
            elapsed_ms: 1_000,
        },
    );
    assert_eq!(state, BudgetState::Exhausted { percent: 100 });
}
