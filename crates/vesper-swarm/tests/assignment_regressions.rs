//! Desired-behavior regressions from the independent VRO-15 audit.
use vesper_swarm::hive::assignment::{TaskRequirements, WorkerLoad, score, select_best};
use vesper_swarm::worker::WorkerCapabilities;

fn worker() -> WorkerLoad {
    WorkerLoad::ideal(WorkerCapabilities {
        tools: vec!["read".into()],
        max_concurrent_tasks: 1,
    })
}

#[test]
fn health_scales_the_complete_oracle_base_score() {
    // The oracle applies health after base + match - load, before metrics.
    let requirement = TaskRequirements::of(["read"]);
    for (health, expected) in [(1.0, 150.0), (0.5, 80.0), (0.0, 10.0)] {
        let candidate = WorkerLoad {
            workload: 0.5,
            health,
            ..worker()
        };
        assert!((score(&candidate, &requirement) - expected).abs() < 1e-9);
    }
}

#[test]
fn selection_refuses_missing_capabilities_even_if_all_workers_lack_them() {
    assert_eq!(
        select_best(&[worker()], &TaskRequirements::of(["write"])),
        None
    );
}

#[test]
fn selection_refuses_dead_saturated_and_zero_capacity_workers() {
    let candidates = [
        WorkerLoad {
            health: 0.0,
            ..worker()
        },
        WorkerLoad {
            workload: 1.0,
            ..worker()
        },
        WorkerLoad {
            capabilities: WorkerCapabilities {
                max_concurrent_tasks: 0,
                ..worker().capabilities
            },
            ..worker()
        },
    ];
    assert_eq!(select_best(&candidates, &TaskRequirements::none()), None);
}

#[test]
fn invalid_metrics_cannot_poison_selection_or_win_by_nan_ordering() {
    for invalid in [
        WorkerLoad {
            health: f64::NAN,
            ..worker()
        },
        WorkerLoad {
            health: 1.1,
            ..worker()
        },
        WorkerLoad {
            workload: -0.1,
            ..worker()
        },
        WorkerLoad {
            success_rate: f64::INFINITY,
            ..worker()
        },
        WorkerLoad {
            avg_turn_secs: -1.0,
            ..worker()
        },
        WorkerLoad {
            avg_turn_secs: f64::NAN,
            ..worker()
        },
    ] {
        assert_eq!(
            select_best(std::slice::from_ref(&invalid), &TaskRequirements::none()),
            None
        );
        assert_eq!(
            select_best(&[invalid, worker()], &TaskRequirements::none()),
            Some(1)
        );
    }
}

#[test]
fn scorer_matches_captured_execution_of_the_pinned_oracle_method() {
    #[derive(serde::Deserialize)]
    struct Vector {
        type_match: bool,
        workload: f64,
        health: f64,
        success_rate: f64,
        avg_turn_secs: f64,
        expected: f64,
    }
    #[derive(serde::Deserialize)]
    struct Capture {
        source_commit: String,
        method_sha256: String,
        vectors: Vec<Vector>,
    }
    let capture: Capture =
        serde_json::from_str(include_str!("assignment_oracle_vectors.json")).unwrap();
    assert_eq!(
        capture.source_commit,
        "e341ec8c4aba8ea616499180dee53035af7e295c"
    );
    assert_eq!(
        capture.method_sha256,
        "0cdbe2f756098d4c629d15c9345bf060af037928a671d730cca2bf326fa0b052"
    );
    assert_eq!(capture.vectors.len(), 72);
    for vector in capture.vectors {
        let candidate = WorkerLoad {
            capabilities: WorkerCapabilities {
                tools: if vector.type_match {
                    vec!["read".into()]
                } else {
                    Vec::new()
                },
                max_concurrent_tasks: 1,
            },
            workload: vector.workload,
            health: vector.health,
            success_rate: vector.success_rate,
            avg_turn_secs: vector.avg_turn_secs,
        };
        assert!(
            (score(&candidate, &TaskRequirements::of(["read"])) - vector.expected).abs() < 1e-9,
            "{candidate:?}: expected {}",
            vector.expected
        );
    }
}
