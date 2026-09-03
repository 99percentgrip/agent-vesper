//! Cron registry — `/loop`.
//!
//! Records scheduled prompts in a JSONL log. This crate owns durable schedule
//! state and claim leases; a composed host such as `vesper-harness` owns the
//! optional bounded worker that executes due prompts and records results.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::error::CheckpointError;
use crate::io::{append_line, read_all_jsonl, write_atomic};
use crate::types::{
    CRON_CLAIM_TTL_SECONDS, CRON_SLOTS_LOG_NAME, CronClaim, CronEntry, CronFiredSlot,
    MAX_CRON_JOBS, MAX_CRON_OUTPUT_CHARS, MAX_FIRED_SLOT_FILES, SLOTS_DIR_NAME,
};

/// One scheduler-owned execution lease returned by [`CronRegistry::claim_due`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronRun {
    /// Snapshot of the job at claim time.
    pub entry: CronEntry,
    /// Token required for renew/finish operations.
    pub token: String,
}

/// In-memory cache + on-disk JSONL store of [`CronEntry`]s.
pub struct CronRegistry {
    root: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    entries: Vec<CronEntry>,
    next_id: u64,
}

impl std::fmt::Debug for CronRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CronRegistry")
            .field("root", &self.root)
            .field(
                "entries",
                &self.state.lock().map(|s| s.entries.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

impl CronRegistry {
    /// Opens (or creates) a cron registry rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        Self::validate_root(root)?;
        let log_path = Self::log_path(root);
        let entries = read_all_jsonl::<CronEntry>(&log_path)?;
        let next_id = entries
            .iter()
            .filter_map(|entry| entry.id.strip_prefix("cron-"))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            root: root.to_path_buf(),
            state: Mutex::new(State { entries, next_id }),
        })
    }

    fn validate_root(root: &Path) -> Result<(), CheckpointError> {
        if !root.is_absolute() {
            return Err(CheckpointError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.as_os_str().is_empty() => Ok(()),
            Some(parent) if parent.exists() => Ok(()),
            _ => Err(CheckpointError::InvalidRoot),
        }
    }

    fn log_path(root: &Path) -> PathBuf {
        root.join("cron.jsonl")
    }

    /// Returns the current job count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .expect("cron mutex poisoned")
            .entries
            .len()
    }

    /// Returns true when the registry holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lists every cron entry in append order.
    #[must_use]
    pub fn list(&self) -> Vec<CronEntry> {
        self.state
            .lock()
            .expect("cron mutex poisoned")
            .entries
            .clone()
    }

    /// Registers a new cron job. Returns the persisted entry.
    pub fn register(
        &self,
        name: &str,
        prompt: &str,
        schedule: &str,
    ) -> Result<CronEntry, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        if state.entries.len() >= MAX_CRON_JOBS {
            return Err(CheckpointError::RetentionCapReached);
        }
        let id = format!("cron-{}", state.next_id);
        state.next_id = state.next_id.saturating_add(1);
        let entry = CronEntry {
            id,
            name: name.to_string(),
            prompt: prompt.to_string(),
            schedule: schedule.to_string(),
            created_at: SystemTime::now(),
            enabled: true,
            next_run_at: Some(next_run_at(schedule, SystemTime::now())?),
            claim: None,
            run_count: 0,
            last_status: None,
            last_output: None,
        };
        entry.validate()?;
        let serialized = serde_json::to_string(&entry)?;
        append_line(&Self::log_path(&self.root), &serialized)?;
        state.entries.push(entry.clone());
        Ok(entry)
    }

    /// Removes the entry with the given id. Idempotent.
    pub fn forget(&self, id: &str) -> Result<bool, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let before = state.entries.len();
        state.entries.retain(|entry| entry.id != id);
        let removed = before - state.entries.len();
        if removed == 0 {
            return Ok(false);
        }
        let mut buffer = String::new();
        for entry in &state.entries {
            buffer.push_str(&serde_json::to_string(entry)?);
            buffer.push('\n');
        }
        write_atomic(&Self::log_path(&self.root), buffer.as_bytes())?;
        Ok(true)
    }

    /// Returns one job by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<CronEntry> {
        self.state
            .lock()
            .expect("cron mutex poisoned")
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    /// Updates the mutable schedule fields of one job and persists the
    /// resulting registry atomically.
    pub fn update(
        &self,
        id: &str,
        name: Option<&str>,
        prompt: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<Option<CronEntry>, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(None);
        };
        if let Some(name) = name {
            entry.name = name.to_owned();
        }
        if let Some(prompt) = prompt {
            entry.prompt = prompt.to_owned();
        }
        if let Some(schedule) = schedule {
            entry.schedule = schedule.to_owned();
        }
        entry.validate()?;
        let updated = entry.clone();
        rewrite(&self.root, &state.entries)?;
        Ok(Some(updated))
    }

    /// Pauses or resumes one job without deleting its definition.
    pub fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Option<CronEntry>, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Ok(None);
        };
        entry.enabled = enabled;
        let updated = entry.clone();
        rewrite(&self.root, &state.entries)?;
        Ok(Some(updated))
    }

    /// Atomically claims all due enabled jobs. Expired claims are recovered;
    /// live claims are never stolen. `force` is useful for an explicit
    /// operator-triggered run but still cannot duplicate a live claim.
    pub fn claim_due(&self, now: SystemTime, force: bool) -> Result<Vec<CronRun>, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let mut changed = false;
        let mut runs = Vec::new();
        for entry in &mut state.entries {
            if !entry.enabled {
                continue;
            }
            if let Some(claim) = &entry.claim {
                if claim.expires_at > now {
                    continue;
                }
                entry.claim = None;
                changed = true;
            }
            let due = force || entry.next_run_at.is_none_or(|when| when <= now);
            if !due {
                continue;
            }
            let token = format!(
                "{}-{}-{}",
                entry.id,
                entry.run_count.saturating_add(1),
                std::process::id()
            );
            entry.claim = Some(CronClaim {
                token: token.clone(),
                claimed_at: now,
                expires_at: now + Duration::from_secs(CRON_CLAIM_TTL_SECONDS),
            });
            entry.next_run_at = if is_recurring(&entry.schedule) {
                Some(next_run_at(&entry.schedule, now)?)
            } else {
                None
            };
            let snapshot = entry.clone();
            runs.push(CronRun {
                entry: snapshot,
                token,
            });
            changed = true;
        }
        if changed {
            rewrite(&self.root, &state.entries)?;
        }
        Ok(runs)
    }

    /// Renews a live claim without reviving an expired or different token.
    pub fn renew_claim(
        &self,
        id: &str,
        token: &str,
        now: SystemTime,
    ) -> Result<bool, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Err(CheckpointError::CronJobNotFound(id.to_owned()));
        };
        let Some(claim) = entry.claim.as_mut() else {
            return Ok(false);
        };
        if claim.token != token || claim.expires_at <= now {
            return Ok(false);
        }
        claim.expires_at = now + Duration::from_secs(CRON_CLAIM_TTL_SECONDS);
        rewrite(&self.root, &state.entries)?;
        Ok(true)
    }

    /// Finishes a claimed run, records bounded output, and transitions a
    /// one-shot job to completed while recurring jobs return to scheduled.
    pub fn finish_claim(
        &self,
        id: &str,
        token: &str,
        status: &str,
        output: &str,
    ) -> Result<CronEntry, CheckpointError> {
        if !matches!(status, "ok" | "error" | "cancelled" | "silent") {
            return Err(CheckpointError::BoundsViolated("cron status"));
        }
        let mut state = self.state.lock().expect("cron mutex poisoned");
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == id) else {
            return Err(CheckpointError::CronJobNotFound(id.to_owned()));
        };
        if entry.claim.as_ref().map(|claim| claim.token.as_str()) != Some(token) {
            return Err(CheckpointError::CronClaimLost);
        }
        let bounded_output = output
            .chars()
            .take(MAX_CRON_OUTPUT_CHARS)
            .collect::<String>();
        entry.claim = None;
        entry.run_count = entry.run_count.saturating_add(1);
        entry.last_status = Some(status.to_owned());
        entry.last_output = (!bounded_output.is_empty()).then_some(bounded_output);
        if !is_recurring(&entry.schedule) {
            entry.enabled = false;
            entry.next_run_at = None;
        }
        let updated = entry.clone();
        rewrite(&self.root, &state.entries)?;
        Ok(updated)
    }

    // === VRO-13 PR-6 — exactly-once slot claims =========================

    /// Path of the slot-claim marker directory shared by every coexisting
    /// scheduler (TUI foreground, background daemon, `cron run` force).
    fn slots_dir(root: &Path) -> PathBuf {
        root.join(SLOTS_DIR_NAME)
    }

    /// One slot marker's path. The file NAME is `<job>-<slot>` with any
    /// path/separator characters in the job id neutralised, so a hostile
    /// job id can never escape the markers directory.
    fn slot_marker_path(root: &Path, job_id: &str, slot: u64) -> Result<PathBuf, CheckpointError> {
        let sanitized = sanitize_marker_component(job_id)?;
        Ok(Self::slots_dir(root).join(format!("{sanitized}.{slot}")))
    }

    /// The slot ledger's JSONL path (audit trail of claims and outcomes).
    fn slots_log_path(root: &Path) -> PathBuf {
        root.join(CRON_SLOTS_LOG_NAME)
    }

    /// Claims one (job, slot) exactly once across every coexisting process.
    ///
    /// The arbiter is an `O_CREAT | O_EXCL` kernel-mediated creation of a
    /// per-slot marker file under `<root>/slots/` — the same primitive qm's
    /// `claimSlot` used SQLite transactions for, delivered JSONL-plus-fs
    /// atomic here (ADR 0012 forbids SQLite in this crate). The claim is
    /// atomic across processes by construction: exactly one `create_new`
    /// succeeds, every other contender observes the existing inode.
    /// Pruning keeps the marker set bounded.
    ///
    /// On success the claim is also appended to `cron-slots.jsonl` (audit
    /// ledger) and the job's `run_count` bookkeeping is refreshed, so
    /// `/daemon status` and `/loop` listings stay truthful.
    pub fn claim_slot(
        &self,
        job_id: &str,
        slot: u64,
        now: SystemTime,
    ) -> Result<bool, CheckpointError> {
        let mut state = self.state.lock().expect("cron mutex poisoned");
        if !state.entries.iter().any(|entry| entry.id == job_id) {
            return Err(CheckpointError::CronJobNotFound(job_id.to_owned()));
        }
        let slots_dir = Self::slots_dir(&self.root);
        std::fs::create_dir_all(&slots_dir).map_err(|_| CheckpointError::io("create"))?;
        prune_slot_markers(&slots_dir, slot);
        let marker = Self::slot_marker_path(&self.root, job_id, slot)?;
        let claimed = std::fs::File::create_new(&marker);
        match claimed {
            Ok(file) => {
                let _ = file.sync_all();
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(false);
            }
            Err(_) => return Err(CheckpointError::io("create")),
        }
        let record = CronFiredSlot {
            job: job_id.to_owned(),
            slot,
            pid: std::process::id(),
            fired_at: now,
            status: None,
        };
        let serialized = serde_json::to_string(&record)?;
        append_line(&Self::slots_log_path(&self.root), &serialized)?;
        if let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == job_id) {
            entry.last_status = Some("claimed".to_owned());
        }
        Ok(true)
    }

    /// Records the outcome of a fired slot. The exactly-once arbiter is the
    /// marker file; this ledger row is the durable audit trail surfaced by
    /// `/daemon status` and the fire's transcript entry.
    pub fn mark_fired(
        &self,
        job_id: &str,
        slot: u64,
        status: &str,
        output: &str,
        now: SystemTime,
    ) -> Result<(), CheckpointError> {
        if !matches!(status, "ok" | "error" | "cancelled" | "silent") {
            return Err(CheckpointError::BoundsViolated("cron status"));
        }
        let mut state = self.state.lock().expect("cron mutex poisoned");
        if !state.entries.iter().any(|entry| entry.id == job_id) {
            return Err(CheckpointError::CronJobNotFound(job_id.to_owned()));
        }
        let record = CronFiredSlot {
            job: job_id.to_owned(),
            slot,
            pid: std::process::id(),
            fired_at: now,
            status: Some(status.to_owned()),
        };
        let serialized = serde_json::to_string(&record)?;
        append_line(&Self::slots_log_path(&self.root), &serialized)?;
        if let Some(entry) = state.entries.iter_mut().find(|entry| entry.id == job_id) {
            entry.run_count = entry.run_count.saturating_add(1);
            let bounded_output = output
                .chars()
                .take(MAX_CRON_OUTPUT_CHARS)
                .collect::<String>();
            entry.last_status = Some(status.to_owned());
            entry.last_output = (!bounded_output.is_empty()).then_some(bounded_output);
        }
        rewrite(&self.root, &state.entries)?;
        Ok(())
    }

    /// Lists slot-ledger rows (claims + outcomes), for `/daemon status`
    /// and diagnostics.
    pub fn list_slot_records(&self) -> Vec<CronFiredSlot> {
        read_all_jsonl::<CronFiredSlot>(&Self::slots_log_path(&self.root)).unwrap_or_default()
    }

    /// The next scheduled fire time across enabled jobs, for `/daemon status`.
    #[must_use]
    pub fn next_scheduled_fire(&self) -> Option<SystemTime> {
        self.state
            .lock()
            .expect("cron mutex poisoned")
            .entries
            .iter()
            .filter(|entry| entry.enabled)
            .filter_map(|entry| entry.next_run_at)
            .min()
    }
}

// === VRO-13 PR-6 — exactly-once slot claims (daemon coexistence) =========
//
// The lease discipline above serializes scheduler ticks within one process
// and across TUI↔TUI restarts, but it cannot prevent two independent
// processes (the foreground TUI scheduler and the background daemon) from
// claiming the same time slot: both open their own `CronRegistry`, so
// neither observes the other's in-memory cache until a rewrite lands, and
// the read-check-write window is wide enough to double-fire.
//
// The slot discipline closes that window with a kernel-arbitrated claim:
// each (job, slot) pair owns one marker file created with `O_CREAT|O_EXCL`
// (`File::create_new`). Exactly one process can create it; every other
// contender gets `AlreadyExists` and declines the slot. The marker is
// immutable (no rewrite races), self-describing (the claim ledger records
// the winning PID and timestamp), and confined to `<root>/slots/` so it
// can never collide with `cron.jsonl`.
//
// `cron-slots.jsonl` is the audit ledger: one line per successful claim
// and one per completed fire. It is advisory — exactly-once is enforced by
// the marker files, so a torn ledger line (crash mid-append) can never
// resurrect a double-fire.

/// Sanitizes a job id for use inside a marker file name: rejects empty,
/// NUL, path separators, and leading dots, so a hostile job id cannot
/// escape `slots/`. Returns the `cron-` prefix stripped.
fn sanitize_marker_component(job_id: &str) -> Result<String, CheckpointError> {
    let trimmed = job_id.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
        || trimmed.starts_with('.')
    {
        return Err(CheckpointError::InvalidSlotClaim);
    }
    Ok(trimmed.trim_start_matches("cron-").to_string())
}

/// Keeps the marker set bounded: drops the oldest per-job markers beyond
/// [`MAX_FIRED_SLOT_FILES`]. Never removes a marker for a slot at or after
/// `from_slot` (the just-claimed one) — pruning only reclaims history the
/// schedule has already left behind. Missing directory or unreadable
/// entries are ignored (pruning is best-effort bookkeeping, never a
/// correctness mechanism).
fn prune_slot_markers(slots_dir: &Path, from_slot: u64) {
    let Ok(read_dir) = std::fs::read_dir(slots_dir) else {
        return;
    };
    // (job suffix, slot) pairs parsed from marker names, newest-last.
    let mut markers: Vec<(String, u64)> = Vec::new();
    for entry in read_dir.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some((job, slot)) = parse_slot_file_name(&name) else {
            continue;
        };
        markers.push((job, slot));
    }
    if markers.is_empty() {
        return;
    }
    markers.sort_unstable();
    // Count per job suffix; collect the ones beyond the per-job bound.
    let mut excess: Vec<(String, u64)> = Vec::new();
    let mut last: Option<(String, usize)> = None;
    for (job, slot) in markers {
        if slot >= from_slot {
            continue; // never prune the live claim or anything after it
        }
        let (current_job, count) = match last {
            Some((prior_job, count)) if prior_job == job.clone() => (job.clone(), count + 1),
            _ => (job.clone(), 1),
        };
        last = Some((current_job.clone(), count));
        // The `count`-th oldest marker for this job: prune when the job
        // holds more than the bound and this is one of the overflow ones.
        // `markers` is sorted ascending, so the first entries per job are
        // the oldest — overflow is everything past the newest
        // MAX_FIRED_SLOT_FILES - 1 per job. Simplest correct form: prune
        // when the running per-job count exceeds the bound.
        if count > MAX_FIRED_SLOT_FILES {
            excess.push((job, slot));
        }
    }
    for (job, slot) in excess {
        let _ = std::fs::remove_file(slots_dir.join(format!("{job}.{slot}")));
    }
}

/// Parses a marker file name back into `(job suffix, slot)`. Returns `None`
/// for anything that does not match the `<job>.<slot>` shape with an
/// all-digit slot.
fn parse_slot_file_name(name: &str) -> Option<(String, u64)> {
    let (job, slot) = name.rsplit_once('.')?;
    if job.is_empty() || slot.is_empty() {
        return None;
    }
    if !slot.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((job.to_string(), slot.parse().ok()?))
}

fn is_recurring(schedule: &str) -> bool {
    let normalized = schedule.trim().to_ascii_lowercase();
    normalized.starts_with("every ")
        || normalized == "hourly"
        || normalized == "@hourly"
        || normalized == "daily"
        || normalized == "@daily"
        || normalized.starts_with("daily ")
}

fn next_run_at(schedule: &str, now: SystemTime) -> Result<SystemTime, CheckpointError> {
    let normalized = schedule.trim().to_ascii_lowercase();
    let seconds = if let Some(value) = normalized.strip_prefix("every ") {
        parse_duration_seconds(value)?
    } else if matches!(normalized.as_str(), "hourly" | "@hourly") {
        3_600
    } else if matches!(normalized.as_str(), "daily" | "@daily") || normalized.starts_with("daily ")
    {
        86_400
    } else if let Some(value) = normalized.strip_prefix("in ") {
        parse_duration_seconds(value)?
    } else {
        return Err(CheckpointError::BoundsViolated("schedule format"));
    };
    now.checked_add(Duration::from_secs(seconds))
        .ok_or(CheckpointError::BoundsViolated("schedule overflow"))
}

fn parse_duration_seconds(value: &str) -> Result<u64, CheckpointError> {
    let value = value.trim();
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .ok_or(CheckpointError::BoundsViolated("schedule duration"))?;
    let amount = value[..split]
        .parse::<u64>()
        .map_err(|_| CheckpointError::BoundsViolated("schedule duration"))?;
    let multiplier = match value[split..].trim() {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err(CheckpointError::BoundsViolated("schedule duration")),
    };
    let seconds = amount
        .checked_mul(multiplier)
        .ok_or(CheckpointError::BoundsViolated("schedule duration"))?;
    if seconds == 0 || seconds > 366 * 86_400 {
        return Err(CheckpointError::BoundsViolated("schedule duration"));
    }
    Ok(seconds)
}

fn rewrite(root: &Path, entries: &[CronEntry]) -> Result<(), CheckpointError> {
    let mut buffer = String::new();
    for entry in entries {
        buffer.push_str(&serde_json::to_string(entry)?);
        buffer.push('\n');
    }
    write_atomic(&CronRegistry::log_path(root), buffer.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn register_persists_across_reopen() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let registry = CronRegistry::open(&root).unwrap();
        let entry = registry
            .register("nightly", "run cargo xtask verify", "daily 02:00")
            .unwrap();
        assert_eq!(entry.id, "cron-1");
        let reopened = CronRegistry::open(&root).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.list()[0].name, "nightly");
    }

    #[test]
    fn forget_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let registry = CronRegistry::open(&root).unwrap();
        let entry = registry.register("ephemeral", "noop", "every 1m").unwrap();
        assert!(registry.forget(&entry.id).unwrap());
        assert!(!registry.forget(&entry.id).unwrap());
    }

    #[test]
    fn update_pause_and_resume_persist() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let registry = CronRegistry::open(&root).unwrap();
        let entry = registry.register("nightly", "old", "daily").unwrap();
        let updated = registry
            .update(&entry.id, Some("renamed"), Some("new"), Some("hourly"))
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.prompt, "new");
        assert_eq!(updated.schedule, "hourly");
        assert!(
            !registry
                .set_enabled(&entry.id, false)
                .unwrap()
                .unwrap()
                .enabled
        );
        assert!(
            registry
                .set_enabled(&entry.id, true)
                .unwrap()
                .unwrap()
                .enabled
        );
        let reopened = CronRegistry::open(&root).unwrap();
        assert_eq!(reopened.get(&entry.id).unwrap().name, "renamed");
        assert!(reopened.get(&entry.id).unwrap().enabled);
    }
}
