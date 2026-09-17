//! Additional deterministic coverage for contract invariants not yet
//! exercised end-to-end: idempotency classes, error retry mapping, and
//! capture-identity durability rules.

use vesper_bridge::error::{BridgeError, RetryEligibility};
use vesper_bridge::identity::CaptureIdentity;
use vesper_bridge::operation::Idempotency;

#[test]
fn idempotency_classes_are_distinct_and_total() {
    // The four classes must remain distinguishable so the journal can map
    // retry decisions per class (§8.3). Totality = every class round-trips
    // through JSON.
    let classes = [
        Idempotency::ReadOnly,
        Idempotency::NaturallyIdempotent,
        Idempotency::ConditionallyIdempotent,
        Idempotency::NonIdempotent,
    ];
    for class in classes {
        let json = serde_json::to_string(&class).unwrap();
        let parsed: Idempotency = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, class, "{json} must round-trip");
    }
    // Distinctness: pairwise inequality via serialization.
    let encoded: Vec<String> = classes
        .iter()
        .map(|c| serde_json::to_string(c).unwrap())
        .collect();
    let mut sorted = encoded.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        encoded.len(),
        "all four classes must serialize distinctly"
    );
}

#[test]
fn error_retry_matrix_matches_nf10() {
    // Two safe transport retries maximum; everything else never retries.
    assert_eq!(
        BridgeError::TransportUnavailable.retry_eligibility(0),
        RetryEligibility::SafeTransportRetry { used: 0 }
    );
    assert_eq!(
        BridgeError::TransportUnavailable.retry_eligibility(1),
        RetryEligibility::SafeTransportRetry { used: 1 }
    );
    for used in [2, 3, 9] {
        assert_eq!(
            BridgeError::TransportUnavailable.retry_eligibility(used),
            RetryEligibility::Never,
            "NF-10: no more than two safe retries (used={used})"
        );
    }
    for error in [
        BridgeError::UnknownOutcome,
        BridgeError::VerificationFailed,
        BridgeError::PermissionDenied,
        BridgeError::TargetAmbiguous,
        BridgeError::TargetChanged,
        BridgeError::StaleObservation,
        BridgeError::LeaseConflict,
        BridgeError::SchemaMismatch,
        BridgeError::ResourceLimit,
        BridgeError::CleanupUnconfirmed,
        BridgeError::CapabilityUnavailable,
        BridgeError::PermissionRequired,
    ] {
        assert_eq!(
            error.retry_eligibility(0),
            RetryEligibility::Never,
            "{error} must never auto-retry"
        );
    }
}

#[test]
fn capture_identity_durable_rules_cover_the_reuse_cases() {
    // Same handle + same serial + same surface ⇒ same stream.
    let a = CaptureIdentity {
        node_handle: 7,
        object_serial: Some(11),
        surface_key: "win".into(),
    };
    let same = CaptureIdentity {
        node_handle: 7,
        object_serial: Some(11),
        surface_key: "win".into(),
    };
    assert!(a.durable_eq(&same));
    // Same handle REUSED with a different serial ⇒ different stream (the
    // ScreenCast caveat: node IDs are recycled).
    let reused = CaptureIdentity {
        node_handle: 7,
        object_serial: Some(12),
        surface_key: "win".into(),
    };
    assert!(!a.durable_eq(&reused));
    // Serial absent on one side ⇒ incomparable, never equal.
    let no_serial = CaptureIdentity {
        node_handle: 7,
        object_serial: None,
        surface_key: "win".into(),
    };
    assert!(!a.durable_eq(&no_serial));
    assert!(!no_serial.durable_eq(&a));
    // Both without serial ⇒ handle+surface is the best available
    // (callers must flag this weaker identity rather than upgrade it).
    let b = CaptureIdentity {
        node_handle: 9,
        object_serial: None,
        surface_key: "win".into(),
    };
    let b2 = CaptureIdentity {
        node_handle: 9,
        object_serial: None,
        surface_key: "win".into(),
    };
    assert!(b.durable_eq(&b2));
}

#[test]
fn settled_outcomes_are_terminal_and_success_is_verified_only() {
    use vesper_bridge::operation::OperationOutcome;
    for outcome in [
        OperationOutcome::Verified,
        OperationOutcome::Partial,
        OperationOutcome::Failed("x".into()),
        OperationOutcome::Cancelled,
    ] {
        assert!(outcome.is_settled(), "{outcome:?} is terminal");
    }
    for outcome in [
        OperationOutcome::Dispatched,
        OperationOutcome::Applied,
        OperationOutcome::UnknownOutcome,
    ] {
        assert!(!outcome.is_settled(), "{outcome:?} still settles");
        assert!(
            outcome.may_have_committed(),
            "{outcome:?} may have committed"
        );
    }
    assert!(OperationOutcome::Verified.is_success());
    for outcome in [
        OperationOutcome::Dispatched,
        OperationOutcome::Applied,
        OperationOutcome::Partial,
        OperationOutcome::Failed("x".into()),
        OperationOutcome::Cancelled,
        OperationOutcome::UnknownOutcome,
    ] {
        assert!(
            !outcome.is_success(),
            "{outcome:?} must not count as success"
        );
    }
}
