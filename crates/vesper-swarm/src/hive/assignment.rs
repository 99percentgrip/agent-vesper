//! Capability-scored task assignment (VRO-15 PR-5).
//!
//! The scoring function is a faithful port of the swarm oracle's
//! assignment heuristic, expressed over Vesper's capability vectors:
//!
//! ```text
//! score = 100
//!       + (50 * type_match)                        // capability-vector match
//!       - (20 * workload * health)                 // load penalty, scaled by health
//!       + (10 * success_rate)                      // reliability bonus
//!       - (5 * avg_turn_secs / 60)                 // speed penalty, per-minute units
//! ```
//!
//! `type_match` is 1 when the worker declares every capability the task
//! requires (set inclusion over [`WorkerCapabilities::supports`]), else 0.
//!
//! This module is pure: [`score`] and [`select_best`] are deterministic
//! total functions over their inputs. No I/O, no clock, no state
//! mutation, no randomness. Ties resolve to the earliest candidate in
//! slice order (stable), so results are reproducible byte-for-byte.

use crate::bus::MessagePriority;
use crate::worker::{TaskPriority, WorkerCapabilities};

/// Assignment-relevant snapshot of one worker's state.
///
/// All fields are normalized by the caller: `workload` and `health` in
/// `0.0..=1.0`, `success_rate` in `0.0..=1.0`, `avg_turn_secs` a
/// non-negative average of recent turn durations.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerLoad {
    /// Declared capabilities (toolset) of the worker.
    pub capabilities: WorkerCapabilities,
    /// Fractional busyness, `0.0` (idle) to `1.0` (saturated).
    pub workload: f64,
    /// Health score, `0.0` (dead) to `1.0` (perfect).
    pub health: f64,
    /// Fraction of completed turns that succeeded.
    pub success_rate: f64,
    /// Average recent turn duration in seconds.
    pub avg_turn_secs: f64,
}

impl WorkerLoad {
    /// A perfectly healthy, idle, fully reliable worker with no history.
    #[must_use]
    pub fn ideal(capabilities: WorkerCapabilities) -> Self {
        Self {
            capabilities,
            workload: 0.0,
            health: 1.0,
            success_rate: 1.0,
            avg_turn_secs: 0.0,
        }
    }
}

/// Required capabilities for the task being assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRequirements {
    /// Capability names the worker must declare (set inclusion).
    pub required_capabilities: Vec<String>,
}

impl TaskRequirements {
    /// No capability constraints.
    #[must_use]
    pub fn none() -> Self {
        Self {
            required_capabilities: Vec::new(),
        }
    }

    /// The given capability names are required.
    #[must_use]
    pub fn of<I, S>(capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            required_capabilities: capabilities.into_iter().map(Into::into).collect(),
        }
    }
}

/// Pure capability-scored assignment value for one candidate.
///
/// Higher is better. The formula is the upstream oracle's, ported
/// exactly; see the module documentation for the algebra. `type_match`
/// uses [`WorkerCapabilities::supports`] — the worker must declare every
/// required capability for the match bonus.
#[must_use]
pub fn score(candidate: &WorkerLoad, requirements: &TaskRequirements) -> f64 {
    let type_match = f64::from(u8::from(
        candidate
            .capabilities
            .supports(&requirements.required_capabilities),
    ));
    100.0 + (50.0 * type_match) - (20.0 * candidate.workload * candidate.health)
        + (10.0 * candidate.success_rate)
        - (5.0 * (candidate.avg_turn_secs / 60.0))
}

/// Selects the best candidate by [`score`].
///
/// Returns the index of the highest-scoring candidate, or `None` when
/// `candidates` is empty. Ties resolve to the **earliest index** (stable
/// selection), so repeated calls over the same slice order always agree.
#[must_use]
pub fn select_best(candidates: &[WorkerLoad], requirements: &TaskRequirements) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let candidate_score = score(candidate, requirements);
        let better = best.as_ref().is_none_or(|(_, top)| candidate_score > *top);
        if better {
            best = Some((index, candidate_score));
        }
    }
    best.map(|(index, _)| index)
}

/// Maps a task's urgency onto the bus priority tiers (VRO-15 PR-4):
///
/// ```text
/// TaskPriority::Critical  -> MessagePriority::Urgent
/// TaskPriority::High      -> MessagePriority::High
/// TaskPriority::Normal    -> MessagePriority::Normal
/// TaskPriority::Low       -> MessagePriority::Low
/// TaskPriority::Background-> MessagePriority::Low
/// ```
#[must_use]
pub fn bus_priority(task: TaskPriority) -> MessagePriority {
    match task {
        TaskPriority::Critical => MessagePriority::Urgent,
        TaskPriority::High => MessagePriority::High,
        TaskPriority::Normal => MessagePriority::Normal,
        TaskPriority::Low | TaskPriority::Background => MessagePriority::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(tools: &[&str]) -> WorkerCapabilities {
        WorkerCapabilities {
            tools: tools.iter().map(|tool| tool.to_string()).collect(),
            max_concurrent_tasks: 1,
        }
    }

    fn load(workload: f64, health: f64, success_rate: f64, avg_turn_secs: f64) -> WorkerLoad {
        WorkerLoad {
            capabilities: caps(&["read", "write"]),
            workload,
            health,
            success_rate,
            avg_turn_secs,
        }
    }

    // -----------------------------------------------------------------
    // Scoring table: exact arithmetic
    // -----------------------------------------------------------------

    #[test]
    fn ideal_matching_worker_scores_exactly_160() {
        // 100 + 50*1 - 20*0*1 + 10*1 - 5*(0/60) = 160
        let candidate = WorkerLoad::ideal(caps(&["read"]));
        let score = score(&candidate, &TaskRequirements::of(["read"]));
        assert!((score - 160.0).abs() < f64::EPSILON, "got {score}");
    }

    #[test]
    fn capability_mismatch_loses_the_match_bonus() {
        // 100 + 0 - 0 + 10 - 0 = 110
        let candidate = WorkerLoad::ideal(caps(&["read"]));
        let score = score(&candidate, &TaskRequirements::of(["browser"]));
        assert!((score - 110.0).abs() < f64::EPSILON, "got {score}");
    }

    #[test]
    fn partial_capability_match_is_still_a_mismatch() {
        // Set inclusion: possessing one of two required tools is not a match.
        let candidate = WorkerLoad::ideal(caps(&["read"]));
        let score = score(&candidate, &TaskRequirements::of(["read", "browser"]));
        assert!((score - 110.0).abs() < f64::EPSILON, "got {score}");
    }

    #[test]
    fn workload_penalty_scales_with_health() {
        // 100 + 50 - 20*(0.5*1.0) + 10 - 0 = 150
        let half_busy = load(0.5, 1.0, 1.0, 0.0);
        let matched = TaskRequirements::of(["read"]);
        assert!((score(&half_busy, &matched) - 150.0).abs() < f64::EPSILON);
        // A dead worker's workload stops penalizing (0*health... no:
        // workload 0.5 * health 0.0 = 0 penalty): 160 exactly.
        let half_busy_dead = load(0.5, 0.0, 1.0, 0.0);
        assert!((score(&half_busy_dead, &matched) - 160.0).abs() < f64::EPSILON);
        // Full load, full health: 100 + 50 - 20 + 10 = 140.
        let saturated = load(1.0, 1.0, 1.0, 0.0);
        assert!((score(&saturated, &matched) - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn success_rate_contributes_ten_points_per_unit() {
        let matched = TaskRequirements::of(["read"]);
        let never_succeeds = load(0.0, 1.0, 0.0, 0.0);
        // 100 + 50 - 0 + 0 - 0 = 150
        assert!((score(&never_succeeds, &matched) - 150.0).abs() < f64::EPSILON);
        let half = load(0.0, 1.0, 0.5, 0.0);
        // 100 + 50 + 5 = 155
        assert!((score(&half, &matched) - 155.0).abs() < f64::EPSILON);
    }

    #[test]
    fn speed_penalty_is_five_per_minute_of_average_turn() {
        let matched = TaskRequirements::of(["read"]);
        // avg 60s => -5: 160 - 5 = 155
        let minute = load(0.0, 1.0, 1.0, 60.0);
        assert!((score(&minute, &matched) - 155.0).abs() < f64::EPSILON);
        // avg 300s (5 min) => -25: 135
        let five_minutes = load(0.0, 1.0, 1.0, 300.0);
        assert!((score(&five_minutes, &matched) - 135.0).abs() < f64::EPSILON);
        // avg 6s => -0.5: 159.5
        let seconds = load(0.0, 1.0, 1.0, 6.0);
        assert!((score(&seconds, &matched) - 159.5).abs() < 1e-9);
    }

    #[test]
    fn combined_terms_compute_exactly() {
        // All terms active: 100 + 50 - 20*(0.75*0.8) + 10*0.9 - 5*(120/60)
        // = 100 + 50 - 12 + 9 - 10 = 137
        let candidate = load(0.75, 0.8, 0.9, 120.0);
        let matched = TaskRequirements::of(["read"]);
        assert!((score(&candidate, &matched) - 137.0).abs() < 1e-9);
    }

    // -----------------------------------------------------------------
    // Selection
    // -----------------------------------------------------------------

    #[test]
    fn select_best_picks_the_highest_scoring_candidate() {
        let matched = TaskRequirements::of(["read"]);
        let candidates = [
            load(0.9, 1.0, 1.0, 0.0),           // heavily loaded
            WorkerLoad::ideal(caps(&["read"])), // 160
            load(0.0, 1.0, 0.5, 0.0),           // 155
        ];
        assert_eq!(select_best(&candidates, &matched), Some(1));
    }

    #[test]
    fn select_best_breaks_ties_toward_the_earliest_candidate() {
        let matched = TaskRequirements::of(["read"]);
        let candidates = [
            WorkerLoad::ideal(caps(&["read"])),
            WorkerLoad::ideal(caps(&["read", "write"])),
            WorkerLoad::ideal(caps(&["read"])),
        ];
        // All three score 160 for a read-only task: index 0 wins.
        assert_eq!(select_best(&candidates, &matched), Some(0));
    }

    #[test]
    fn select_best_returns_none_for_empty_candidates() {
        assert_eq!(select_best(&[], &TaskRequirements::none()), None);
    }

    #[test]
    fn select_best_prefers_capable_over_faster_incapable() {
        let browser_task = TaskRequirements::of(["browser"]);
        let candidates = [
            WorkerLoad::ideal(caps(&["read", "write"])), // 110, no browser
            WorkerLoad::ideal(caps(&["browser"])),       // 160
        ];
        assert_eq!(select_best(&candidates, &browser_task), Some(1));
    }

    #[test]
    fn score_is_deterministic_across_repeated_calls() {
        let candidate = load(0.4, 0.9, 0.77, 42.0);
        let requirements = TaskRequirements::of(["write"]);
        let first = score(&candidate, &requirements);
        for _ in 0..100 {
            assert_eq!(score(&candidate, &requirements), first);
        }
    }

    // -----------------------------------------------------------------
    // Priority mapping
    // -----------------------------------------------------------------

    #[test]
    fn task_priority_maps_to_bus_tiers_exactly() {
        use crate::worker::TaskPriority;
        assert_eq!(
            bus_priority(TaskPriority::Critical),
            MessagePriority::Urgent
        );
        assert_eq!(bus_priority(TaskPriority::High), MessagePriority::High);
        assert_eq!(bus_priority(TaskPriority::Normal), MessagePriority::Normal);
        assert_eq!(bus_priority(TaskPriority::Low), MessagePriority::Low);
        assert_eq!(bus_priority(TaskPriority::Background), MessagePriority::Low);
    }

    #[test]
    fn priority_mapping_preserves_relative_order() {
        let tiers = [
            TaskPriority::Background,
            TaskPriority::Low,
            TaskPriority::Normal,
            TaskPriority::High,
            TaskPriority::Critical,
        ]
        .map(bus_priority);
        let mut sorted = tiers;
        sorted.sort();
        assert_eq!(tiers, sorted);
        assert_eq!(tiers[0], MessagePriority::Low);
        assert_eq!(tiers[4], MessagePriority::Urgent);
    }
}
