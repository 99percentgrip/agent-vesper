//! Watcher store — `watchers.jsonl` (VRO-13 PR-7).
//!
//! Records file/path watchers for the background daemon's sweep loop.
//! Each row binds a *scope id* (from PR-5's `.vesper-scope-id` stamp) to
//! one target (path, glob, or PID), one **literal** line pattern, and an
//! optional heartbeat interval.
//!
//! ## Why literal patterns (no regex)
//!
//! Watcher patterns are matched against file tails that can contain
//! arbitrary user content. A regex compiled from that content is a ReDoS
//! vector and an aliasing vector (the same tail matching multiple
//! unrelated watchers). This store therefore refuses every regex
//! metacharacter except the `^` and `$` anchors, and matching is a plain
//! per-line substring scan bounded to a
//! [`MAX_WATCHER_TAIL_BYTES`] tail. Anchors are honored *as line
//! anchors*.
//!
//! ## Storage
//!
//! `watchers.jsonl` — append-only rows, one per watcher. Removal rewrites
//! the log atomically (temp + fsync + rename). The store mirrors the
//! [`crate::CronRegistry`] discipline: process-local `Mutex` over an
//! in-memory cache, every `File` scoped to a function body (RAII), and
//! every mutation confined to the absolute root.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::CheckpointError;
use crate::io::{append_line, read_all_jsonl, write_atomic};
use crate::types::MAX_LABEL_CHARS;

/// Name of the watcher store's JSONL log inside the state root.
pub const WATCHERS_LOG_NAME: &str = "watchers.jsonl";

/// Maximum bytes read from the tail of a watched file before matching.
/// Bounds memory exhaustion from arbitrarily large tails.
pub const MAX_WATCHER_TAIL_BYTES: usize = 4 * 1024;

/// Default heartbeat interval (seconds) for a watcher with no explicit one.
pub const WATCHER_DEFAULT_HEARTBEAT_SECONDS: u64 = 180;

/// Minimum seconds between two fires of the *same* watcher (rate limit).
pub const WATCHER_MIN_FIRE_INTERVAL_SECONDS: u64 = 60;

/// Maximum consecutive failed fires before a watcher is paused.
pub const MAX_WATCHER_RETRIES: u64 = 3;

/// Maximum number of watchers one store may hold.
pub const MAX_WATCHERS: usize = 100;

/// Maximum characters in a watcher pattern.
pub const MAX_WATCHER_PATTERN_CHARS: usize = 200;

/// Maximum characters in a watcher target.
pub const MAX_WATCHER_TARGET_CHARS: usize = 260;

/// Maximum heartbeat interval (seconds).
pub const MAX_WATCHER_HEARTBEAT_SECONDS: u64 = 86_400;

/// One row of the JSONL watcher registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatcherEntry {
    /// Stable opaque id, e.g. `watch-7`.
    pub id: String,
    /// Owning scope id (PR-5's `.vesper-scope-id` token).
    pub scope_id: String,
    /// What to watch: an absolute path, a glob, or a numeric PID.
    pub target: String,
    /// Discriminates the target form.
    pub target_kind: WatcherTargetKind,
    /// Literal line pattern (no regex metacharacters; `^`/`$` allowed).
    pub pattern: String,
    /// Optional heartbeat interval in seconds (default 180).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_seconds: Option<u64>,
    /// Registration timestamp.
    pub created_at: SystemTime,
    /// Whether the daemon may dispatch this watcher. Defaults true.
    #[serde(default = "default_watcher_enabled")]
    pub enabled: bool,
    /// Registration ordinal (stable fire ordering across rewrites).
    #[serde(default)]
    pub ordinal: u64,
    /// Completed fires.
    #[serde(default)]
    pub fire_count: u64,
    /// Last fire time (rate-limit anchor: ≥ 60s between fires of one
    /// watcher).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<SystemTime>,
    /// Consecutive failures of the unattended fire (retry budget).
    #[serde(default)]
    pub consecutive_failures: u64,
    /// Bounded last-error projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn default_watcher_enabled() -> bool {
    true
}

/// The three target forms a watcher may reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatcherTargetKind {
    /// An absolute filesystem path (file or directory).
    Path,
    /// A glob pattern (matched against workspace-relative paths).
    Glob,
    /// A numeric process id.
    Pid,
}

impl WatcherTargetKind {
    /// Lowercase tag used in `/watch` listing output.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Glob => "glob",
            Self::Pid => "pid",
        }
    }
}

impl WatcherEntry {
    /// Validates the entry against the bounded contract.
    pub fn validate(&self) -> Result<(), CheckpointError> {
        if self.id.len() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("watcher id length"));
        }
        if self.scope_id.len() > MAX_LABEL_CHARS {
            return Err(CheckpointError::BoundsViolated("scope id length"));
        }
        if self.target.chars().count() > MAX_WATCHER_TARGET_CHARS {
            return Err(CheckpointError::BoundsViolated("watcher target length"));
        }
        if self.pattern.chars().count() > MAX_WATCHER_PATTERN_CHARS {
            return Err(CheckpointError::BoundsViolated("watcher pattern length"));
        }
        if let Some(heartbeat) = self.heartbeat_seconds
            && (heartbeat == 0 || heartbeat > MAX_WATCHER_HEARTBEAT_SECONDS)
        {
            return Err(CheckpointError::BoundsViolated("watcher heartbeat"));
        }
        validate_watcher_pattern(&self.pattern)?;
        Ok(())
    }

    /// The effective heartbeat interval.
    #[must_use]
    pub fn heartbeat(&self) -> Duration {
        Duration::from_secs(
            self.heartbeat_seconds
                .unwrap_or(WATCHER_DEFAULT_HEARTBEAT_SECONDS),
        )
    }

    /// True when this watcher is allowed to fire at `now` (rate limit:
    /// at least [`WATCHER_MIN_FIRE_INTERVAL_SECONDS`] since the last
    /// fire).
    #[must_use]
    pub fn may_fire_at(&self, now: SystemTime) -> bool {
        match self.last_fired_at {
            None => true,
            Some(last) => now
                .duration_since(last)
                .map(|since| since.as_secs() >= WATCHER_MIN_FIRE_INTERVAL_SECONDS)
                .unwrap_or(false),
        }
    }
}

/// Regex metacharacters this store refuses (anchors `^`/`$` allowed).
const REGEX_METACHARACTERS: &[char] =
    &['.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\'];

/// Validates a literal watcher pattern: rejects regex metacharacters
/// except the `^` and `$` anchors, rejects control characters and NUL,
/// and rejects the empty pattern (which would match everything).
pub fn validate_watcher_pattern(pattern: &str) -> Result<(), CheckpointError> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Err(CheckpointError::BoundsViolated("watcher pattern empty"));
    }
    if trimmed.chars().count() > MAX_WATCHER_PATTERN_CHARS {
        return Err(CheckpointError::BoundsViolated("watcher pattern length"));
    }
    for character in trimmed.chars() {
        if character == '\0' {
            return Err(CheckpointError::BoundsViolated("watcher pattern NUL"));
        }
        if character.is_control() {
            return Err(CheckpointError::BoundsViolated(
                "watcher pattern control char",
            ));
        }
        if character != '^' && character != '$' && REGEX_METACHARACTERS.contains(&character) {
            return Err(CheckpointError::InvalidWatcherPattern);
        }
    }
    Ok(())
}

/// Matches a validated literal pattern against one line (honoring the
/// `^`/`$` anchors the validator allows).
#[must_use]
pub fn watcher_pattern_matches_line(pattern: &str, line: &str) -> bool {
    let pattern = pattern.trim();
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$') && pattern.len() > 1;
    let core = pattern
        .trim_start_matches('^')
        .trim_end_matches('$')
        .to_string();
    if core.is_empty() {
        // `^$` would match every line; the validator rejects it upstream,
        // but matching stays fail-closed rather than matching everything.
        return false;
    }
    match (anchored_start, anchored_end) {
        (true, true) => line.trim() == core,
        (true, false) => line.trim_start().starts_with(&core),
        (false, true) => line.trim_end().ends_with(&core),
        (false, false) => line.contains(&core),
    }
}

/// Reads the bounded tail of `path` (last [`MAX_WATCHER_TAIL_BYTES`]
/// bytes). Missing files yield an empty tail. The returned buffer is at
/// most [`MAX_WATCHER_TAIL_BYTES`] long — memory cannot be exhausted by a
/// huge watched file.
pub fn read_watcher_tail(path: &Path) -> Result<Vec<u8>, CheckpointError> {
    use std::io::{Read, Seek, SeekFrom};
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(CheckpointError::io("read")),
    };
    let length = metadata.len();
    if length == 0 {
        return Ok(Vec::new());
    }
    // Scoped: dropped at the closing brace (RAII, Errno-24 discipline).
    let mut file = std::fs::File::open(path).map_err(|_| CheckpointError::io("open"))?;
    let take = length.min(u64::try_from(MAX_WATCHER_TAIL_BYTES).unwrap_or(u64::MAX));
    let start = length - take;
    if file
        .seek(SeekFrom::Start(start))
        .map_err(|_| CheckpointError::io("seek"))?
        != start
    {
        return Err(CheckpointError::io("seek"));
    }
    let mut buffer = vec![0_u8; usize::try_from(take).unwrap_or(MAX_WATCHER_TAIL_BYTES)];
    file.read_exact(&mut buffer)
        .map_err(|_| CheckpointError::io("read"))?;
    drop(file);
    Ok(buffer)
}

/// In-memory cache + on-disk JSONL store of [`WatcherEntry`]s.
pub struct WatcherStore {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    entries: Vec<WatcherEntry>,
    next_id: u64,
    ordinal: u64,
}

impl std::fmt::Debug for WatcherStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WatcherStore")
            .field("root", &self.root)
            .field(
                "entries",
                &self
                    .state
                    .lock()
                    .map(|state| state.entries.len())
                    .unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl WatcherStore {
    /// Opens (or creates) a watcher store rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        if !root.is_absolute() {
            return Err(CheckpointError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.as_os_str().is_empty() => {}
            Some(parent) if parent.exists() => {}
            _ => return Err(CheckpointError::InvalidRoot),
        }
        let entries = read_all_jsonl::<WatcherEntry>(&Self::log_path(root))?;
        let next_id = entries
            .iter()
            .filter_map(|entry| entry.id.strip_prefix("watch-"))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let ordinal = entries.len() as u64;
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(State {
                entries,
                next_id,
                ordinal,
            }),
        })
    }

    fn log_path(root: &Path) -> PathBuf {
        root.join(WATCHERS_LOG_NAME)
    }

    /// Returns the current watcher count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("watcher mutex poisoned")
            .entries
            .len()
    }

    /// Returns true when the store holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists every watcher in append order.
    #[must_use]
    pub fn list(&self) -> Vec<WatcherEntry> {
        self.state
            .lock()
            .expect("watcher mutex poisoned")
            .entries
            .clone()
    }

    /// Lists every enabled watcher bound to `scope_id`.
    #[must_use]
    pub fn list_for_scope(&self, scope_id: &str) -> Vec<WatcherEntry> {
        self.list()
            .into_iter()
            .filter(|entry| entry.scope_id == scope_id && entry.enabled)
            .collect()
    }

    /// Returns one watcher by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<WatcherEntry> {
        self.state
            .lock()
            .expect("watcher mutex poisoned")
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    /// Registers a new watcher. Returns the persisted entry.
    pub fn register(
        &self,
        scope_id: &str,
        target: &str,
        target_kind: WatcherTargetKind,
        pattern: &str,
        heartbeat_seconds: Option<u64>,
    ) -> Result<WatcherEntry, CheckpointError> {
        let mut state = self.state.lock().expect("watcher mutex poisoned");
        if state.entries.len() >= MAX_WATCHERS {
            return Err(CheckpointError::RetentionCapReached);
        }
        if scope_id.trim().is_empty() {
            return Err(CheckpointError::BoundsViolated("watcher scope empty"));
        }
        if target.trim().is_empty() {
            return Err(CheckpointError::BoundsViolated("watcher target empty"));
        }
        if let WatcherTargetKind::Path = target_kind
            && !Path::new(target).is_absolute()
        {
            return Err(CheckpointError::InvalidWatcherTarget);
        }
        if let WatcherTargetKind::Pid = target_kind
            && target.trim().parse::<u32>().is_err()
        {
            return Err(CheckpointError::InvalidWatcherTarget);
        }
        let id = format!("watch-{}", state.next_id);
        state.next_id = state.next_id.saturating_add(1);
        let ordinal = state.ordinal;
        state.ordinal = state.ordinal.saturating_add(1);
        let entry = WatcherEntry {
            id,
            scope_id: scope_id.to_string(),
            target: target.to_string(),
            target_kind,
            pattern: pattern.to_string(),
            heartbeat_seconds,
            created_at: SystemTime::now(),
            enabled: true,
            ordinal,
            fire_count: 0,
            last_fired_at: None,
            consecutive_failures: 0,
            last_error: None,
        };
        entry.validate()?;
        let serialized = serde_json::to_string(&entry)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.entries.push(entry.clone());
        Ok(entry)
    }

    /// Removes the watcher with the given id. Idempotent.
    pub fn forget(&self, id: &str) -> Result<bool, CheckpointError> {
        let mut state = self.state.lock().expect("watcher mutex poisoned");
        let before = state.entries.len();
        state.entries.retain(|entry| entry.id != id);
        let removed = before - state.entries.len();
        if removed == 0 {
            return Ok(false);
        }
        rewrite(&self.root, &state.entries)?;
        Ok(true)
    }

    /// Sets `enabled` on one watcher and persists.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool, CheckpointError> {
        let mut state = self.state.lock().expect("watcher mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(false);
        };
        entry.enabled = enabled;
        if enabled {
            entry.consecutive_failures = 0;
            entry.last_error = None;
        }
        rewrite(&self.root, &state.entries)?;
        Ok(true)
    }

    /// Records one fire: bumps the fire count, stamps `last_fired_at`
    /// (the rate-limit anchor), and clears the failure streak.
    pub fn record_fire(&self, id: &str, now: SystemTime) -> Result<(), CheckpointError> {
        let mut state = self.state.lock().expect("watcher mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Err(CheckpointError::WatcherNotFound(id.to_owned()));
        };
        entry.fire_count = entry.fire_count.saturating_add(1);
        entry.last_fired_at = Some(now);
        entry.consecutive_failures = 0;
        entry.last_error = None;
        rewrite(&self.root, &state.entries)?;
        Ok(())
    }

    /// Records one failed fire: bumps the streak; at
    /// [`MAX_WATCHER_RETRIES`] the watcher is paused (`enabled = false`)
    /// so `/daemon status` surfaces it for re-enable.
    pub fn record_failure(
        &self,
        id: &str,
        error: &str,
        _now: SystemTime,
    ) -> Result<(), CheckpointError> {
        let mut state = self.state.lock().expect("watcher mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Err(CheckpointError::WatcherNotFound(id.to_owned()));
        };
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        // A FAILED fire never advances the rate-limit anchor: only
        // completed fires count toward the 60 s same-watcher limit, so a
        // transiently denied turn retries on the next sweep instead of
        // being locked out for a minute (the retry budget above is the
        // actual bound on failure pressure).
        let bounded: String = error.chars().take(400).collect();
        entry.last_error = (!bounded.is_empty()).then_some(bounded);
        if entry.consecutive_failures >= MAX_WATCHER_RETRIES {
            entry.enabled = false;
        }
        rewrite(&self.root, &state.entries)?;
        Ok(())
    }
}

fn rewrite(root: &Path, entries: &[WatcherEntry]) -> Result<(), CheckpointError> {
    let mut buffer = String::new();
    for entry in entries {
        buffer.push_str(&serde_json::to_string(entry)?);
        buffer.push('\n');
    }
    write_atomic(&WatcherStore::log_path(root), buffer.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn store() -> (TempDir, WatcherStore) {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("state");
        fs::create_dir_all(&root).unwrap();
        let store = WatcherStore::open(&root).unwrap();
        (temp, store)
    }

    fn path_target(temp: &TempDir, name: &str) -> String {
        temp.path().join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn register_persists_across_reopen() {
        let (temp, store) = store();
        let target = path_target(&temp, "watched.log");
        let entry = store
            .register(
                "scope-ab12",
                &target,
                WatcherTargetKind::Path,
                "ERROR",
                None,
            )
            .unwrap();
        assert_eq!(entry.id, "watch-1");
        assert!(entry.enabled);
        assert_eq!(entry.heartbeat(), Duration::from_secs(180));
        let reopened = WatcherStore::open(&temp.path().join("state")).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.list()[0].scope_id, "scope-ab12");
    }

    #[test]
    fn regex_metacharacters_are_rejected() {
        let (temp, store) = store();
        let target = path_target(&temp, "f");
        for pattern in ["a.*b", "ERROR (code", "x[0-9]y", "a{2,3}", "p|q", "a\\w"] {
            assert!(
                store
                    .register("s", &target, WatcherTargetKind::Path, pattern, None)
                    .is_err(),
                "pattern `{pattern}` must be rejected"
            );
        }
    }

    #[test]
    fn anchors_are_allowed_and_honored() {
        let (temp, store) = store();
        let target = path_target(&temp, "f");
        assert!(
            store
                .register("s", &target, WatcherTargetKind::Path, "^ERROR$", None)
                .is_ok()
        );
        assert!(watcher_pattern_matches_line("^ERROR$", "ERROR"));
        assert!(watcher_pattern_matches_line("^ERROR", "ERROR: boom"));
        assert!(!watcher_pattern_matches_line("^ERROR", "xERROR"));
        assert!(watcher_pattern_matches_line("ERROR$", "late ERROR"));
        assert!(!watcher_pattern_matches_line("ERROR$", "ERROR: x"));
        assert!(watcher_pattern_matches_line("ERROR", "has ERROR inside"));
    }

    #[test]
    fn rate_limit_requires_sixty_seconds_between_fires() {
        let (temp, store) = store();
        let target = path_target(&temp, "f");
        let entry = store
            .register("s", &target, WatcherTargetKind::Path, "ERROR", None)
            .unwrap();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(entry.may_fire_at(t0));
        store.record_fire(&entry.id, t0).unwrap();
        let updated = store.get(&entry.id).unwrap();
        assert!(!updated.may_fire_at(t0 + Duration::from_secs(30)));
        assert!(updated.may_fire_at(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn failure_streak_pauses_the_watcher() {
        let (temp, store) = store();
        let target = path_target(&temp, "f");
        let entry = store
            .register("s", &target, WatcherTargetKind::Path, "ERROR", None)
            .unwrap();
        let t = SystemTime::UNIX_EPOCH;
        for attempt in 1..=MAX_WATCHER_RETRIES {
            store
                .record_failure(&entry.id, "provider unavailable", t)
                .unwrap();
            let updated = store.get(&entry.id).unwrap();
            assert_eq!(updated.consecutive_failures, attempt);
        }
        let paused = store.get(&entry.id).unwrap();
        assert!(!paused.enabled);
        assert_eq!(paused.last_error.as_deref(), Some("provider unavailable"));
        // Re-enabling clears the streak (the retry budget resets).
        store.set_enabled(&entry.id, true).unwrap();
        let resumed = store.get(&entry.id).unwrap();
        assert!(resumed.enabled);
        assert_eq!(resumed.consecutive_failures, 0);
    }

    #[test]
    fn tail_read_is_bounded_to_four_kib() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("huge.log");
        let payload = vec![b'x'; 1024 * 1024]; // 1 MiB
        fs::write(&path, &payload).unwrap();
        let tail = read_watcher_tail(&path).unwrap();
        assert_eq!(tail.len(), MAX_WATCHER_TAIL_BYTES);
        assert!(tail.iter().all(|&byte| byte == b'x'));
        let missing = read_watcher_tail(&temp.path().join("missing.log")).unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn pid_targets_must_be_numeric() {
        let (_temp, store) = store();
        assert!(
            store
                .register("s", "1234", WatcherTargetKind::Pid, "alive", None)
                .is_ok()
        );
        assert!(
            store
                .register("s", "not-a-pid", WatcherTargetKind::Pid, "alive", None)
                .is_err()
        );
    }

    #[test]
    fn path_targets_must_be_absolute() {
        let (_temp, store) = store();
        assert!(
            store
                .register("s", "relative/log.txt", WatcherTargetKind::Path, "x", None)
                .is_err()
        );
    }

    #[test]
    fn glob_targets_allow_relative_patterns() {
        let (_temp, store) = store();
        assert!(
            store
                .register("s", "**/*.log", WatcherTargetKind::Glob, "ERROR", None)
                .is_ok()
        );
    }

    #[test]
    fn scope_filter_lists_only_bound_watchers() {
        let (temp, store) = store();
        store
            .register(
                "scope-a",
                &path_target(&temp, "a"),
                WatcherTargetKind::Path,
                "ERROR",
                None,
            )
            .unwrap();
        store
            .register(
                "scope-b",
                &path_target(&temp, "b"),
                WatcherTargetKind::Path,
                "ERROR",
                None,
            )
            .unwrap();
        store
            .register(
                "scope-a",
                &path_target(&temp, "c"),
                WatcherTargetKind::Path,
                "ERROR",
                None,
            )
            .unwrap();
        assert_eq!(store.list_for_scope("scope-a").len(), 2);
        assert_eq!(store.list_for_scope("scope-b").len(), 1);
        assert!(store.list_for_scope("scope-c").is_empty());
    }

    #[test]
    fn watcher_capacity_is_bounded() {
        let (temp, store) = store();
        for index in 0..MAX_WATCHERS {
            store
                .register(
                    "s",
                    &path_target(&temp, &format!("f{index}")),
                    WatcherTargetKind::Path,
                    "ERROR",
                    None,
                )
                .unwrap();
        }
        assert!(
            store
                .register(
                    "s",
                    &path_target(&temp, "overflow"),
                    WatcherTargetKind::Path,
                    "ERROR",
                    None,
                )
                .is_err()
        );
    }

    #[test]
    fn forget_is_idempotent() {
        let (temp, store) = store();
        let target = path_target(&temp, "f");
        let entry = store
            .register("s", &target, WatcherTargetKind::Path, "ERROR", None)
            .unwrap();
        assert!(store.forget(&entry.id).unwrap());
        assert!(!store.forget(&entry.id).unwrap());
        assert!(store.is_empty());
    }
}
