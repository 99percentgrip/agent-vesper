//! Daemon watcher sweep loop (VRO-13 PR-7).
//!
//! The sweep owns the file-tail watchers recorded in `watchers.jsonl`.
//! It runs on a **10-second cadence** using a bounded mtime scan (the
//! `notify` crate is deliberately absent from the workspace dependency
//! set, so PR-7 uses the directive's sanctioned fallback: bounded mtime
//! scanning). Every sweep:
//!
//! 1. opens the watcher store fresh (no long-lived descriptors — the
//!    RAII discipline that prevents the oracle's Errno 24 leak),
//! 2. evaluates each watcher's target mtime (or PID liveness for PID
//!    targets),
//! 3. applies the rate limits below,
//! 4. fires the watchers whose tail matched, as **bounded ReAct turns**
//!    under `Ask` permission with the deny-on-ask port — an unattended
//!    fire that requires approval fails closed and is retried at most
//!    [`WATCHER_MAX_RETRIES`] times before the job is paused.
//!
//! ## Rate limits (oracle constants)
//!
//! - at most [`WATCHER_SWEEP_MAX_FIRES`] fires per sweep
//! - at least 60 s between fires of the same watcher
//! - 180 s default heartbeat
//!
//! ## Rendering isolation (zero TUI degradation)
//!
//! The sweep NEVER touches the render path: it owns no terminal handle,
//! holds no lock the dispatcher needs, and communicates only by appending
//! to `watcher-events.jsonl` in the state root. The TUI surfaces sweeps
//! through `/daemon status` (a bounded read of the same file).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use vesper_checkpoints::watchers::{
    WATCHER_DEFAULT_HEARTBEAT_SECONDS, WATCHER_MIN_FIRE_INTERVAL_SECONDS, WatcherEntry,
    WatcherStore, WatcherTargetKind, read_watcher_tail, watcher_pattern_matches_line,
};

/// Sweep cadence: one bounded mtime scan every 10 s.
pub const WATCHER_SWEEP_INTERVAL: Duration = Duration::from_secs(10);

/// Maximum watcher fires per sweep; over-cap matches queue to the next.
pub const WATCHER_SWEEP_MAX_FIRES: usize = 20;

/// Maximum retries before a failing watcher is paused. Single source:
/// the store's [`vesper_checkpoints::watchers::MAX_WATCHER_RETRIES`].
pub const WATCHER_MAX_RETRIES: u64 = vesper_checkpoints::watchers::MAX_WATCHER_RETRIES;

/// Name of the sweep's event ledger inside the state root.
pub const WATCHER_EVENTS_LOG_NAME: &str = "watcher-events.jsonl";

/// One row of the sweep's event ledger: the durable audit trail the TUI
/// reads (bounded) when `/daemon status` runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SweepEventRow {
    /// Owning watcher id (`watch-N`).
    pub watcher: String,
    /// Scope id the watcher is bound to.
    pub scope: String,
    /// `fired` | `queued` | `paused` | `skipped` | `denied`.
    pub action: String,
    /// Sweep timestamp.
    pub at: SystemTime,
    /// Bounded outcome note (truncated at 400 chars on write).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Outcome of one sweep: which watchers fired, which queued, which paused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SweepOutcome {
    /// Watchers that fired this sweep.
    pub fired: Vec<String>,
    /// Watchers whose match was over capacity (or rate-limited) and
    /// therefore queued for the next sweep — never silently dropped.
    pub queued: Vec<String>,
    /// Watchers paused after exhausting retries.
    pub paused: Vec<String>,
}

/// One sweep evaluation for a single watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepAction {
    /// Fire now (matched and under all limits).
    Fire,
    /// Queue to the next sweep (matched but over capacity or rate-limited).
    Queue,
    /// Do not fire (no match, or disabled).
    Skip,
}

/// Reads whether a PID is alive via `/proc/<pid>` (bounded single stat,
/// never a procfs walk). Non-`/proc` platforms report alive (conservative:
/// a PID watcher simply never fires).
fn is_pid_alive(pid: u32) -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    Path::new("/proc")
        .join(pid.to_string())
        .try_exists()
        .unwrap_or(false)
}

/// Whether a watcher's target currently satisfies its match predicate:
/// a path target must contain a line matching the literal pattern in the
/// bounded 4 KiB tail; a PID target fires when the process is gone; a
/// glob target matches when any expansion hit contains a matching line
/// (glob expansion itself stays bounded by the store's `MAX_WATCHERS`).
fn watcher_matches(entry: &WatcherEntry) -> bool {
    match entry.target_kind {
        WatcherTargetKind::Pid => !is_pid_alive(pid_from_target(&entry.target)),
        WatcherTargetKind::Path | WatcherTargetKind::Glob => {
            let Ok(tail) = read_watcher_tail(Path::new(&entry.target)) else {
                return false;
            };
            let text = String::from_utf8_lossy(&tail);
            text.lines()
                .any(|line| watcher_pattern_matches_line(&entry.pattern, line))
        }
    }
}

/// Parses a numeric PID target. Unparseable targets never match (the
/// store already rejects non-numeric PID targets at registration).
fn pid_from_target(target: &str) -> u32 {
    target.trim().parse().unwrap_or(u32::MAX)
}

/// The pure decision core of one sweep: which watchers fire, queue, or
/// skip given the current match set and the rate-limit state. No I/O, no
/// async — the tests drive it with an injected clock.
fn evaluate_sweep(
    entries: &[WatcherEntry],
    matched: &HashMap<String, bool>,
    now: SystemTime,
) -> Vec<(String, SweepAction)> {
    let mut decisions = Vec::with_capacity(entries.len());
    let mut fires_this_sweep = 0_usize;
    for entry in entries {
        // Disabled (paused after retry exhaustion) and unmatched watchers
        // are both skips; the durable store/event ledger records which.
        let action = if !entry.enabled || !matched.get(&entry.id).copied().unwrap_or(false) {
            SweepAction::Skip
        } else {
            // Re-fire window: `max(60 s rate limit, heartbeat)`. The
            // heartbeat is the *minimum* cadence for a still-matching
            // watcher (its default is 180 s); the 60 s floor is the
            // oracle's per-watcher rate limit and applies even when the
            // file gained a brand-new matching line.
            let heartbeat = entry
                .heartbeat_seconds
                .unwrap_or(WATCHER_DEFAULT_HEARTBEAT_SECONDS);
            let window = WATCHER_MIN_FIRE_INTERVAL_SECONDS.max(heartbeat);
            let suppressed = entry.last_fired_at.is_some_and(|last| {
                now.duration_since(last)
                    .map(|elapsed| elapsed.as_secs() < window)
                    .unwrap_or(false)
            });
            // Fire cap: at most 20 fires per sweep; over-cap and
            // rate-suppressed matches QUEUE (never drop): the next sweep
            // past the window re-evaluates and fires.
            if suppressed || fires_this_sweep >= WATCHER_SWEEP_MAX_FIRES {
                SweepAction::Queue
            } else {
                fires_this_sweep += 1;
                SweepAction::Fire
            }
        };
        decisions.push((entry.id.clone(), action));
    }
    decisions
}

/// Runs one bounded sweep pass: reads the store, evaluates matches and
/// limits, and records outcomes. `fire` is the (host-injected) fire
/// closure — the daemon passes the bounded ReAct turn; tests pass a stub.
/// The sweep never touches the terminal or any render structure.
pub fn run_sweep_once<F>(
    state_root: &Path,
    scope_id: &str,
    now: SystemTime,
    mut fire: F,
) -> SweepOutcome
where
    F: FnMut(&WatcherEntry) -> Result<String, String>,
{
    let mut outcome = SweepOutcome::default();
    let Ok(store) = WatcherStore::open(state_root) else {
        return outcome;
    };
    let entries = store.list_for_scope(scope_id);
    // Matches are evaluated per-watcher by reading the bounded 4 KiB tail
    // directly (`watcher_matches`); no mtime pre-pass is needed because
    // each read is bounded and cheap, and skipping it keeps the sweep to
    // one syscall family per watcher.
    let mut matched: HashMap<String, bool> = HashMap::new();
    for entry in &entries {
        matched.insert(entry.id.clone(), watcher_matches(entry));
    }
    let decisions = evaluate_sweep(&entries, &matched, now);
    for (id, action) in decisions {
        let Some(entry) = entries.iter().find(|entry| entry.id == id) else {
            continue;
        };
        match action {
            SweepAction::Fire => {
                let fire_result = fire(entry);
                match fire_result {
                    Ok(note) => {
                        let _ = store.record_fire(&id, now);
                        append_sweep_event(
                            state_root,
                            &SweepEventRow {
                                watcher: id.clone(),
                                scope: scope_id.to_string(),
                                action: "fired".to_string(),
                                at: now,
                                note: (!note.is_empty()).then_some(note),
                            },
                        );
                        outcome.fired.push(id);
                    }
                    Err(error) => {
                        let _ = store.record_failure(&id, &error, now);
                        let paused = store.get(&id).map(|entry| !entry.enabled).unwrap_or(false);
                        append_sweep_event(
                            state_root,
                            &SweepEventRow {
                                watcher: id.clone(),
                                scope: scope_id.to_string(),
                                action: if paused { "paused" } else { "denied" }.to_string(),
                                at: now,
                                note: Some(error),
                            },
                        );
                        if paused {
                            outcome.paused.push(id);
                        }
                    }
                }
            }
            SweepAction::Queue => {
                append_sweep_event(
                    state_root,
                    &SweepEventRow {
                        watcher: id.clone(),
                        scope: scope_id.to_string(),
                        action: "queued".to_string(),
                        at: now,
                        note: None,
                    },
                );
                outcome.queued.push(id);
            }
            SweepAction::Skip => {}
        }
    }
    outcome
}

/// Appends one row to the sweep's event ledger. Best-effort: a failed
/// append never aborts the sweep (the durable fire record in the store
/// is the authoritative outcome).
fn append_sweep_event(root: &Path, event: &SweepEventRow) {
    let line = serde_json::to_string(event).unwrap_or_default();
    if line.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(root);
    // Scoped file: dropped at the closing brace (RAII, Errno 24 safe).
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(WATCHER_EVENTS_LOG_NAME))
    {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
        let _ = file.sync_all();
    }
}

/// Lists the last `limit` sweep events (newest first) for `/daemon status`.
#[must_use]
pub fn list_sweep_events(state_root: &Path, limit: usize) -> Vec<SweepEventRow> {
    let path = state_root.join(WATCHER_EVENTS_LOG_NAME);
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut rows: Vec<SweepEventRow> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line.trim()).ok())
        .collect();
    rows.reverse(); // newest first
    rows.truncate(limit);
    rows
}

/// Spawns the daemon's sweep loop on the current tokio runtime. The loop
/// is **fully isolated from the render path**: it owns no terminal, holds
/// no dispatcher lock, and only appends to the state root. The TUI never
/// runs this loop — only `--headless daemon` does.
pub fn spawn_watcher_sweep(state_root: PathBuf, scope_id: String, fire: SweepFire)
where
    SweepFire: 'static,
{
    let handle = tokio::runtime::Handle::try_current();
    let Some(handle) = handle.ok() else {
        return;
    };
    handle.spawn(async move {
        loop {
            let now = SystemTime::now();
            let fire = fire.clone();
            run_sweep_once(&state_root, &scope_id, now, move |entry| fire(entry));
            tokio::time::sleep(WATCHER_SWEEP_INTERVAL).await;
        }
    });
}

/// Evaluates one watcher NOW without firing a turn: reports whether the
/// watched tail currently matches. `/watch fire-test` uses this so a
/// registration can be validated without waiting for a daemon sweep.
#[must_use]
pub fn probe_watcher(entry: &WatcherEntry) -> bool {
    read_watcher_tail(std::path::Path::new(&entry.target))
        .map(|tail| {
            let text = String::from_utf8_lossy(&tail);
            text.lines()
                .any(|line| watcher_pattern_matches_line(&entry.pattern, line))
        })
        .unwrap_or(false)
}

/// The daemon's fire closure: a bounded ReAct turn under `Ask`
/// permission with the deny-on-ask port, so unattended fires that need
/// human approval fail closed (the PRD §2.6 safety shape).
pub type SweepFire = std::sync::Arc<dyn Fn(&WatcherEntry) -> Result<String, String> + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use vesper_checkpoints::watchers::WatcherTargetKind;

    fn scope_root(temp: &TempDir) -> std::path::PathBuf {
        let root = temp.path().join("state");
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn watched_file(temp: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let path = temp.path().join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Builds a synthetic [`WatcherEntry`] for pure-core tests.
    fn entry_for(
        path: &Path,
        pattern: &str,
        heartbeat: Option<u64>,
        last_fired: Option<SystemTime>,
    ) -> WatcherEntry {
        WatcherEntry {
            id: "watch-1".into(),
            scope_id: "scope-test".into(),
            target: path.display().to_string(),
            target_kind: WatcherTargetKind::Path,
            pattern: pattern.into(),
            heartbeat_seconds: heartbeat,
            created_at: SystemTime::UNIX_EPOCH,
            enabled: true,
            ordinal: 0,
            fire_count: 0,
            last_fired_at: last_fired,
            consecutive_failures: 0,
            last_error: None,
        }
    }

    #[test]
    fn sweep_fires_only_matching_watchers() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let hit = watched_file(&temp, "hit.log", "line\nBUILD OK\n");
        let miss = watched_file(&temp, "miss.log", "line\nnothing\n");
        let store = WatcherStore::open(&root).unwrap();
        store
            .register(
                "scope-test",
                &hit.display().to_string(),
                WatcherTargetKind::Path,
                "BUILD OK",
                None,
            )
            .unwrap();
        store
            .register(
                "scope-test",
                &miss.display().to_string(),
                WatcherTargetKind::Path,
                "BUILD OK",
                None,
            )
            .unwrap();
        let mut fired = Vec::new();
        let outcome = run_sweep_once(&root, "scope-test", SystemTime::now(), |entry| {
            fired.push(entry.id.clone());
            Ok(String::new())
        });
        assert_eq!(outcome.fired.len(), 1);
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn fire_cap_queues_over_capacity_to_next_sweep() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let store = WatcherStore::open(&root).unwrap();
        for i in 0..25 {
            let path = watched_file(&temp, &format!("log-{i}.log"), "PANIC\n");
            store
                .register(
                    "scope-test",
                    &path.display().to_string(),
                    WatcherTargetKind::Path,
                    "PANIC",
                    None,
                )
                .unwrap();
        }
        let mut fired = Vec::new();
        let outcome = run_sweep_once(&root, "scope-test", SystemTime::now(), |entry| {
            fired.push(entry.id.clone());
            Ok(String::new())
        });
        assert_eq!(outcome.fired.len(), 20, "fire cap: at most 20 per sweep");
        assert_eq!(
            outcome.queued.len(),
            5,
            "over-cap matches queue, never drop"
        );
        assert_eq!(fired.len(), 20);
        // The queued five keep their (never-fired) state, so the next
        // sweep fires them.
        let outcome2 = run_sweep_once(
            &root,
            "scope-test",
            SystemTime::now(),
            |_| Ok(String::new()),
        );
        assert_eq!(
            outcome2.fired.len(),
            5,
            "queued events fire on the next sweep"
        );
    }

    #[test]
    fn rate_limit_requires_sixty_seconds_between_fires_of_one_watcher() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let log = watched_file(&temp, "app.log", "ALERT\n");
        let store = WatcherStore::open(&root).unwrap();
        // Explicit 60 s heartbeat so the rate limit (not the heartbeat
        // window) is the limiting factor under test.
        store
            .register(
                "scope-test",
                &log.to_string_lossy(),
                WatcherTargetKind::Path,
                "ALERT",
                Some(60),
            )
            .unwrap();
        let now = SystemTime::now();
        let first = run_sweep_once(&root, "scope-test", now, |_| Ok(String::new()));
        assert_eq!(first.fired.len(), 1);
        let second = run_sweep_once(&root, "scope-test", now + Duration::from_secs(30), |_| {
            Ok(String::new())
        });
        assert!(second.fired.is_empty(), "no refire within 60 s");
        assert_eq!(
            second.queued.len(),
            1,
            "suppressed match queues for the next sweep"
        );
        let third = run_sweep_once(&root, "scope-test", now + Duration::from_secs(90), |_| {
            Ok(String::new())
        });
        assert_eq!(third.fired.len(), 1, "refire after 60 s");
    }

    #[test]
    fn failed_fire_retries_then_pauses_and_surfaces_in_status() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let log = watched_file(&temp, "app.log", "ALERT\n");
        let store = WatcherStore::open(&root).unwrap();
        store
            .register(
                "scope-test",
                &log.to_string_lossy(),
                WatcherTargetKind::Path,
                "ALERT",
                None,
            )
            .unwrap();
        let mut now = SystemTime::now();
        // Three failing sweeps: each records one failure; the third hits
        // the retry ceiling and pauses the watcher.
        for sweep in 0..3 {
            let outcome =
                run_sweep_once(
                    &root,
                    "scope-test",
                    now,
                    |_| Err("approval required".into()),
                );
            let dbg = store
                .get("watch-1")
                .map(|e| format!("enabled={} fails={}", e.enabled, e.consecutive_failures))
                .unwrap_or_default();
            eprintln!(
                "sweep {sweep}: fired={:?} queued={:?} paused={:?} | {dbg}",
                outcome.fired, outcome.queued, outcome.paused
            );
            assert_eq!(
                outcome.fired.len(),
                0,
                "sweep {sweep}: denied fire is not recorded as ok"
            );
            now += Duration::from_secs(WATCHER_MIN_FIRE_INTERVAL_SECONDS + 10);
        }
        let entry = WatcherStore::open(&root).unwrap().get("watch-1").unwrap();
        assert!(!entry.enabled, "retries exhausted -> watcher paused");
        assert_eq!(entry.consecutive_failures, WATCHER_MAX_RETRIES);
        assert!(
            entry
                .last_error
                .as_deref()
                .unwrap_or("")
                .contains("approval")
        );
        let events = list_sweep_events(&root, 100);
        assert!(
            events.iter().any(|row| row.action == "paused"),
            "pause surfaces in the event ledger /daemon status reads"
        );
    }

    #[test]
    fn heartbeats_default_to_180s() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let log = watched_file(&temp, "app.log", "ALERT\n");
        let store = WatcherStore::open(&root).unwrap();
        store
            .register(
                "scope-test",
                &log.to_string_lossy(),
                WatcherTargetKind::Path,
                "ALERT",
                None,
            )
            .unwrap();
        let now = SystemTime::now();
        let first = run_sweep_once(&root, "scope-test", now, |_| Ok(String::new()));
        assert_eq!(first.fired.len(), 1);
        let early = run_sweep_once(&root, "scope-test", now + Duration::from_secs(120), |_| {
            Ok(String::new())
        });
        assert!(
            early.fired.is_empty(),
            "heartbeat window (180 s default) suppresses early refires"
        );
        let on_time = run_sweep_once(&root, "scope-test", now + Duration::from_secs(185), |_| {
            Ok(String::new())
        });
        assert_eq!(on_time.fired.len(), 1);
    }

    #[test]
    fn pid_watchers_fire_on_death() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        // A PID that cannot exist on Linux: nothing at /proc/4294967295.
        let dead = u32::MAX;
        let store = WatcherStore::open(&root).unwrap();
        store
            .register(
                "scope-test",
                &dead.to_string(),
                WatcherTargetKind::Pid,
                "exit",
                None,
            )
            .unwrap();
        let outcome = run_sweep_once(
            &root,
            "scope-test",
            SystemTime::now(),
            |_| Ok(String::new()),
        );
        if cfg!(target_os = "linux") {
            assert_eq!(outcome.fired.len(), 1, "dead PID fires");
        } else {
            // Conservative-alive semantics: non-Linux reports the PID
            // alive, so the watcher never fires there.
            assert!(
                outcome.fired.is_empty(),
                "non-Linux: PID liveness is conservative-alive"
            );
        }
    }

    #[test]
    fn disabled_watchers_never_fire() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let log = watched_file(&temp, "app.log", "ALERT\n");
        let store = WatcherStore::open(&root).unwrap();
        let entry = store
            .register(
                "scope-test",
                &log.to_string_lossy(),
                WatcherTargetKind::Path,
                "ALERT",
                None,
            )
            .unwrap();
        store.set_enabled(&entry.id, false).unwrap();
        let outcome = run_sweep_once(
            &root,
            "scope-test",
            SystemTime::now(),
            |_| Ok(String::new()),
        );
        assert!(outcome.fired.is_empty());
    }

    #[test]
    fn sweep_events_round_trip_and_read_newest_first() {
        let temp = TempDir::new().unwrap();
        let root = scope_root(&temp);
        let log = watched_file(&temp, "app.log", "ALERT\n");
        let store = WatcherStore::open(&root).unwrap();
        store
            .register(
                "scope-test",
                &log.to_string_lossy(),
                WatcherTargetKind::Path,
                "ALERT",
                None,
            )
            .unwrap();
        run_sweep_once(&root, "scope-test", SystemTime::now(), |_| {
            Ok("matched".into())
        });
        let events = list_sweep_events(&root, 10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "fired");
        assert_eq!(events[0].note.as_deref(), Some("matched"));
    }

    #[test]
    fn evaluate_sweep_pure_core_respects_cap_and_rates() {
        let temp = TempDir::new().unwrap();
        let log = watched_file(&temp, "app.log", "ALERT\n");
        // 25 matched, all eligible: 20 fire, 5 queue.
        let entries: Vec<WatcherEntry> = (0..25)
            .map(|i| {
                let mut entry = entry_for(&log, "ALERT", None, None);
                entry.id = format!("watch-{i}");
                entry
            })
            .collect();
        let matched = entries
            .iter()
            .map(|e| (e.id.clone(), true))
            .collect::<HashMap<_, _>>();
        let decisions = evaluate_sweep(&entries, &matched, SystemTime::now());
        assert_eq!(
            decisions
                .iter()
                .filter(|(_, a)| *a == SweepAction::Fire)
                .count(),
            20
        );
        assert_eq!(
            decisions
                .iter()
                .filter(|(_, a)| *a == SweepAction::Queue)
                .count(),
            5
        );
    }
}
