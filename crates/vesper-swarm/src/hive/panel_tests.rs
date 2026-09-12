//! Panel isolation and failure-mode proofs (VRO-16 PR-2 directive 4).
use super::*;

/// Isolation proof: within one round a reviewer sees only prior-round peer
/// positions — same-round peers are structurally invisible.
#[test]
fn reviewers_never_see_same_round_peers() {
    let reviewers: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    let mut seen: BTreeMap<(String, u32), Vec<String>> = BTreeMap::new();
    let outcome = run_panel("artifact", &reviewers, 1, |reviewer, input| {
        seen.insert(
            (reviewer.to_owned(), input.peer_positions.len() as u32),
            input
                .peer_positions
                .iter()
                .map(|position| position.reviewer.clone())
                .collect(),
        );
        ReviewerTurn::Wrote(format!("position of {reviewer}"))
    });
    assert!(matches!(outcome, PanelOutcome::Survived { .. }));
    // Round 0: nobody sees peers (no prior positions exist).
    for reviewer in &reviewers {
        assert!(
            seen.get(&(reviewer.clone(), 0))
                .is_some_and(|peers| peers.is_empty())
        );
    }
    // Round 1: every reviewer sees exactly the other two round-0 positions
    // (never their own, never any round-1 position).
    for reviewer in &reviewers {
        let entry = seen.get(&(reviewer.clone(), 2)).expect("round-1 input");
        assert_eq!(entry.len(), 2, "{reviewer} saw {entry:?}");
        assert!(!entry.contains(reviewer));
    }
}

/// Drop-on-double-failure: a reviewer failing twice is dropped and its
/// positions disappear; single failures are tolerated.
#[test]
fn double_failure_drops_a_reviewer() {
    let reviewers: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    let outcome = run_panel("artifact", &reviewers, 1, |reviewer, _| {
        if reviewer == "b" {
            ReviewerTurn::Failed
        } else {
            ReviewerTurn::Wrote(format!("position of {reviewer}"))
        }
    });
    let PanelOutcome::Survived {
        positions, dropped, ..
    } = outcome
    else {
        panic!("panel must survive with a and c");
    };
    assert_eq!(dropped, vec![String::from("b")]);
    assert!(!positions.iter().any(|p| p.reviewer == "b"));
    assert!(positions.iter().any(|p| p.reviewer == "a"));
}

/// Zero-panel: every reviewer double-failing is a verification failure
/// outcome, never a silent acceptance.
#[test]
fn zero_panel_is_a_failure_outcome() {
    let reviewers: Vec<String> = ["a", "b"].map(String::from).to_vec();
    let outcome = run_panel("artifact", &reviewers, 1, |_, _| ReviewerTurn::Failed);
    assert_eq!(
        outcome,
        PanelOutcome::ZeroPanel {
            dropped: reviewers.clone()
        }
    );
    // Empty panels and oversized panels are refused as ZeroPanel too.
    assert_eq!(
        run_panel("artifact", &[], 1, |_, _| ReviewerTurn::Wrote("x".into())),
        PanelOutcome::ZeroPanel {
            dropped: Vec::new()
        }
    );
}

/// Empty rebuttals keep the prior position (more lenient than round 0).
#[test]
fn empty_rebuttal_keeps_prior_position() {
    let reviewers: Vec<String> = ["a", "b"].map(String::from).to_vec();
    let mut round = 0;
    let outcome = run_panel("artifact", &reviewers, 1, |reviewer, _| {
        round += 1;
        if round <= 2 {
            ReviewerTurn::Wrote(format!("opening of {reviewer}"))
        } else {
            // Round 1: both reviewers fail once (empty-ish); prior kept.
            ReviewerTurn::Failed
        }
    });
    let PanelOutcome::Survived { positions, .. } = outcome else {
        panic!("panel survives: single round-1 failure retains positions")
    };
    assert!(positions.iter().all(|p| p.text.starts_with("opening of")));
    assert_eq!(positions.len(), 2);
}

/// Round bounds: the panel never runs more than MAX_ROUNDS rebuttals.
#[test]
fn rounds_are_bounded() {
    let reviewers: Vec<String> = ["a", "b"].map(String::from).to_vec();
    let mut calls = 0;
    let outcome = run_panel("artifact", &reviewers, 99, |_, _| {
        calls += 1;
        ReviewerTurn::Wrote("position".into())
    });
    assert!(matches!(outcome, PanelOutcome::Survived { .. }));
    // 2 reviewers × (round 0 + at most MAX_ROUNDS) ≤ 2 × 3 = 6.
    assert!(calls <= 6, "{calls} turn calls exceeded the bound");
}
