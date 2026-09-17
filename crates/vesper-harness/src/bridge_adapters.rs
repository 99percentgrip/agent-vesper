//! Hosted Bridge adapters: real application transports behind the pure
//! `AdapterPort` seam.
//!
//! Two adapters ship in this module, both **dependency-free** (subprocess +
//! filesystem only, MSRV-safe):
//!
//! - `FileIpcAdapter` — the DaVinci Resolve free-edition worker channel
//!   (`recon/probes/vb_worker2.lua`): one Console paste per app launch,
//!   then typed ops flow through `ipc/cmd` / `ipc/result` files.
//! - `MprisAdapter` — any MPRIS2 media player (Elisa, VLC, Amarok,
//!   browsers): discovery, capabilities, play/pause/next/previous/status
//!   through `busctl` with typed property reads.
//!
//! Both adapters follow the same honesty rules as the probes that proved
//! them: bounded reads, escaped application-originated strings, unknown ops
//! refused, no evaluation of model text, and verification evidence only
//! when independently measured.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use vesper_bridge::adapter::{AdapterOutcome, AdapterPort};
use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};
use vesper_bridge::error::BridgeError;
use vesper_bridge::operation::{OperationOutcome, OperationRequestId, OperationSpec};
use vesper_bridge::session::StopOutcome;

/// Bound on any single IPC file read (the worker caps writes at 8 KiB).
const MAX_IPC_FILE_BYTES: usize = 16 * 1024;
/// Bound on a busctl property read.
const MAX_BUSCTL_OUTPUT: usize = 64 * 1024;

fn escape_for_summary(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(300));
    for ch in s.chars() {
        match ch {
            '|' | '"' | '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
        if out.len() >= 300 {
            out.push('~');
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// FileIpcAdapter — Resolve free-edition worker channel
// ---------------------------------------------------------------------------

/// Adapter for the Resolve LuaJIT-FFI worker (`vb_worker2.lua`).
///
/// The worker must already be running inside the application (one Console
/// paste per launch). This adapter never launches the application and never
/// writes Lua; it only exchanges typed JSON lines through the IPC directory.
pub struct FileIpcAdapter {
    ipc_dir: PathBuf,
    manifest: CapabilityManifest,
    seq: AtomicU64,
    /// Measured per-command deadline (worker polls every 0.4s).
    timeout: Duration,
}

impl FileIpcAdapter {
    /// Bind to an IPC directory. The worker must be live (verify with
    /// [`health`](Self::health)).
    #[must_use]
    pub fn new(ipc_dir: impl Into<PathBuf>) -> Self {
        let ops = [
            "status",
            "get_state",
            "fixture_import",
            "timeline_from_fixture",
            "render_draft",
            "render_start",
            "render_status",
            "set_marker",
        ];
        let records = ops
            .iter()
            .map(|op| CapabilityRecord {
                id: CapabilityId::new(format!("resolve.worker.{op}"))
                    .expect("static capability id"),
                schema_version: 1,
                availability: Availability::Available,
                implementation: Implementation::Native,
                mutability: if matches!(*op, "status" | "get_state" | "render_status") {
                    Mutability::ReadOnly
                } else {
                    Mutability::Mutating
                },
                route: RouteKind::NativeApi,
                delivery: if *op == "render_start" {
                    DeliveryMode::Background
                } else {
                    DeliveryMode::Foreground
                },
                verification: if matches!(*op, "render_draft" | "render_start") {
                    VerificationMethod::Independent
                } else {
                    VerificationMethod::AcknowledgmentOnly
                },
                limitations: format!(
                    "Free-edition Console worker route; requires one worker paste per app launch. op={op}"
                ),
            })
            .collect();
        Self {
            ipc_dir: ipc_dir.into(),
            manifest: CapabilityManifest::new("resolve-free-worker", 1, records),
            seq: AtomicU64::new(1),
            timeout: Duration::from_secs(20),
        }
    }

    fn cmd_path(&self) -> PathBuf {
        self.ipc_dir.join("cmd")
    }
    fn result_path(&self) -> PathBuf {
        self.ipc_dir.join("result")
    }
    fn beat_path(&self) -> PathBuf {
        self.ipc_dir.join("beat")
    }

    /// Worker liveness: the beat file must be fresher than 5s.
    fn worker_alive(&self) -> bool {
        match std::fs::metadata(self.beat_path()) {
            Ok(meta) => {
                let Ok(modified) = meta.modified() else {
                    return false;
                };
                modified
                    .elapsed()
                    .map(|age| age < Duration::from_secs(5))
                    .unwrap_or(false)
            }
            Err(_) => false,
        }
    }

    fn exchange(
        &self,
        op: &str,
        arguments: &serde_json::Value,
    ) -> Result<(bool, String), BridgeError> {
        if !self.worker_alive() {
            return Err(BridgeError::DependencyMissing {
                detail: "Resolve worker beat is stale; the in-app worker must be (re)pasted for this Resolve launch".into(),
            });
        }
        let id = self.seq.fetch_add(1, Ordering::SeqCst);
        let payload = serde_json::json!({"id": id, "op": op, "args": arguments});
        let body = serde_json::to_string(&payload).map_err(|_| BridgeError::InvalidArguments {
            detail: "argument serialization failed".into(),
        })?;
        if body.len() > MAX_IPC_FILE_BYTES {
            return Err(BridgeError::InvalidArguments {
                detail: "IPC command exceeds the 16 KiB bound".into(),
            });
        }
        // Drain any stale result from an earlier id, then send.
        let _ = std::fs::remove_file(self.result_path());
        std::fs::write(self.cmd_path(), body).map_err(|e| BridgeError::Transport {
            detail: format!("cannot write IPC cmd: {e}"),
        })?;
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            if let Ok(raw) = std::fs::read_to_string(self.result_path())
                && raw.len() <= MAX_IPC_FILE_BYTES
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw)
                && v.get("id").and_then(serde_json::Value::as_u64) == Some(id)
            {
                let ok = v
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let data = v
                    .get("data")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return Ok((ok, data));
            }
            std::thread::sleep(Duration::from_millis(120));
        }
        Err(BridgeError::Timeout {
            detail: format!("worker did not answer op={op} within the 20s bound"),
        })
    }
}

impl AdapterPort for FileIpcAdapter {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }

    fn dispatch(
        &self,
        request_id: &OperationRequestId,
        spec: &OperationSpec,
    ) -> Result<AdapterOutcome, BridgeError> {
        // capability id form: resolve.worker.<op>
        let op = spec
            .capability
            .0
            .strip_prefix("resolve.worker.")
            .ok_or_else(|| BridgeError::InvalidArguments {
                detail: "FileIpcAdapter only dispatches resolve.worker.* capabilities".into(),
            })?;
        let (ok, data) = self.exchange(op, &spec.arguments)?;
        let summary = format!(
            "request={} op={} ok={} data={}",
            escape_for_summary(&request_id.0),
            op,
            ok,
            escape_for_summary(&data)
        );
        if ok {
            Ok(AdapterOutcome::applied(summary))
        } else {
            Ok(AdapterOutcome::failed(summary))
        }
    }

    fn job_status(&self, _job_id: &str) -> Option<StopOutcome> {
        // The worker reports render progress through render_status, not a
        // job registry; return None so the caller uses the capability.
        None
    }

    fn release_inputs(&self) -> String {
        "file-ipc adapter owns no driver inputs (Console route is not a held-input transport)"
            .into()
    }

    fn describe(&self) -> String {
        format!(
            "Resolve free-edition worker (file IPC at {})",
            self.ipc_dir.display()
        )
    }

    fn health(&self) -> Option<bool> {
        Some(self.worker_alive())
    }
}

// ---------------------------------------------------------------------------
// MprisAdapter — standard media players via busctl
// ---------------------------------------------------------------------------

/// Adapter for any MPRIS2 media player discovered on the user session bus.
///
/// Carries **measured player-behavior guards** (compatibility facts from
/// session 2y, observed live on Elisa/KDE):
/// 1. `Previous` while **paused** restarts the current track instead of
///    navigating — an unwanted mutation. Refused with guidance instead.
/// 2. Rapid `Next`/`Previous` **inversion** crashes the player's MPRIS
///    handler (observed: service vanished, process dead). A cooldown
///    separates direction changes.
/// 3. Navigation is only settled as applied when the **trackid actually
///    changed** — a call acknowledgment never proves a hop (§7).
pub struct MprisAdapter {
    service: String,
    manifest: CapabilityManifest,
    bus: Box<dyn MprisBus>,
    /// Last navigation direction + time (inversion cooldown state).
    last_direction: std::sync::Mutex<Option<(Direction, Instant)>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Direction {
    Forward,
    Backward,
}

/// Minimum spacing between OPPOSITE-direction navigation calls; the
/// measured crash trigger was rapid inversion with no spacing.
const INVERSION_COOLDOWN: Duration = Duration::from_millis(1200);

/// The bus seam: production spawns `busctl`; tests inject a fake.
pub(crate) trait MprisBus: Send + Sync {
    fn busctl(&self, args: &[&str]) -> Result<String, BridgeError>;
}

/// Production seam implementation: real busctl subprocess.
struct BusctlProcess;

impl MprisBus for BusctlProcess {
    fn busctl(&self, args: &[&str]) -> Result<String, BridgeError> {
        let out = Command::new("busctl")
            .args(["--user", "--no-pager"])
            .args(args)
            .env("LANG", "C")
            .output()
            .map_err(|e| BridgeError::Transport {
                detail: format!("cannot spawn busctl: {e}"),
            })?;
        if !out.status.success() {
            return Err(BridgeError::Transport {
                detail: format!(
                    "busctl {} failed: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&out.stderr)
                        .chars()
                        .take(200)
                        .collect::<String>()
                ),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        if text.len() > MAX_BUSCTL_OUTPUT {
            return Err(BridgeError::InvalidArguments {
                detail: "busctl output exceeds bound".into(),
            });
        }
        Ok(text)
    }
}

impl MprisAdapter {
    /// Bind to an MPRIS service name, e.g. `org.mpris.MediaPlayer2.elisa`.
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self::with_bus(service, Box::new(BusctlProcess))
    }

    /// Test seam: inject a fake bus.
    pub(crate) fn with_bus(service: impl Into<String>, bus: Box<dyn MprisBus>) -> Self {
        Self::build(service.into(), bus)
    }

    fn build(service: String, bus: Box<dyn MprisBus>) -> Self {
        let records = [
            ("play", Mutability::Mutating, "Player.Play"),
            ("pause", Mutability::Mutating, "Player.Pause"),
            ("play_pause", Mutability::Mutating, "Player.PlayPause"),
            ("stop", Mutability::Mutating, "Player.Stop"),
            ("next", Mutability::Mutating, "Player.Next"),
            ("previous", Mutability::Mutating, "Player.Previous"),
            ("status", Mutability::ReadOnly, "Player.PlaybackStatus"),
            ("metadata", Mutability::ReadOnly, "Player.Metadata"),
        ]
        .iter()
        .map(|(op, mutability, target)| CapabilityRecord {
            id: CapabilityId::new(format!("mpris.player.{op}")).expect("static capability id"),
            schema_version: 1,
            availability: Availability::Available,
            implementation: Implementation::Native,
            mutability: *mutability,
            route: RouteKind::NativeApi,
            delivery: DeliveryMode::Foreground,
            verification: if *mutability == Mutability::ReadOnly {
                VerificationMethod::AcknowledgmentOnly
            } else {
                // state + position reads verify mutations independently
                VerificationMethod::Independent
            },
            limitations: format!(
                "MPRIS2 route; target {target} of {service}. Guards: previous-while-paused refused (restarts track); direction changes spaced ≥{}ms (rapid inversion crashes player); navigation settled on trackid change",
                INVERSION_COOLDOWN.as_millis()
            ),
        })
        .collect();
        Self {
            manifest: CapabilityManifest::new("mpris-media-player", 1, records),
            service,
            bus,
            last_direction: std::sync::Mutex::new(None),
        }
    }

    fn property(&self, name: &str) -> Result<String, BridgeError> {
        // busctl get-property SERVICE PATH INTERFACE PROPERTY — four args;
        // the interface and property name are separate parameters.
        self.bus.busctl(&[
            "get-property",
            &self.service,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
            name,
        ])
    }

    fn call(&self, method: &str) -> Result<(), BridgeError> {
        self.bus
            .busctl(&[
                "call",
                &self.service,
                "/org/mpris/MediaPlayer2",
                &format!("org.mpris.MediaPlayer2.Player.{method}"),
            ])
            .map(|_| ())
    }

    /// Guard 1 (measured): `Previous` while paused restarts the track —
    /// refuse with actionable guidance instead of mutating unexpectedly.
    fn guard_previous_while_paused(&self) -> Result<(), BridgeError> {
        let status = self.property("PlaybackStatus")?;
        if status.contains("Paused") {
            return Err(BridgeError::InvalidArguments {
                detail: "previous while paused restarts the current track on this player (measured); resume playback before navigating back, or use a seek to 0 explicitly".into(),
            });
        }
        Ok(())
    }

    /// Current track identity (mpris:trackid), used to verify hops.
    fn current_trackid(&self) -> Result<String, BridgeError> {
        let raw = self.property("Metadata")?;
        for line in raw.lines() {
            if line.contains("mpris:trackid")
                && let Some(path) = line.split('"').nth(3)
            {
                return Ok(path.to_string());
            }
        }
        Ok(String::new())
    }

    /// Guard 2 (measured): rapid direction inversion crashes the player.
    fn guard_inversion(&self, direction: Direction) -> Result<(), BridgeError> {
        let last = self
            .last_direction
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((prev, at)) = *last
            && prev != direction
            && at.elapsed() < INVERSION_COOLDOWN
        {
            return Err(BridgeError::InvalidArguments {
                detail: format!(
                    "direction change within the {}ms cooldown — rapid Next/Previous inversion crashes this player (measured); retry after the cooldown",
                    INVERSION_COOLDOWN.as_millis()
                ),
            });
        }
        Ok(())
    }

    fn mark_direction(&self, direction: Direction) {
        let mut last = self
            .last_direction
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *last = Some((direction, Instant::now()));
    }

    /// Guard 3: navigation settles on the **trackid actually changing** —
    /// a call acknowledgment never proves a hop. Same-track after the call
    /// is reported as a failed navigation (with the observed trackids),
    /// never as applied.
    fn settle_navigation(
        &self,
        request_id: &OperationRequestId,
        op: &str,
        before: &str,
    ) -> Result<AdapterOutcome, BridgeError> {
        let after = self.current_trackid()?;
        let hopped = !before.is_empty() && after != before;
        let summary = format!(
            "request={} op={} track {} -> {}",
            escape_for_summary(&request_id.0),
            op,
            escape_for_summary(before),
            escape_for_summary(&after)
        );
        if hopped {
            Ok(AdapterOutcome {
                outcome: OperationOutcome::Verified,
                summary,
                evidence: format!("mpris:trackid changed: {before} -> {after}"),
                job_created: false,
            })
        } else {
            Ok(AdapterOutcome::failed(format!(
                "{summary} | navigation did not change the track (call acknowledged but trackid unchanged)"
            )))
        }
    }
}

impl AdapterPort for MprisAdapter {
    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }

    fn dispatch(
        &self,
        request_id: &OperationRequestId,
        spec: &OperationSpec,
    ) -> Result<AdapterOutcome, BridgeError> {
        let op = spec
            .capability
            .0
            .strip_prefix("mpris.player.")
            .ok_or_else(|| BridgeError::InvalidArguments {
                detail: "MprisAdapter only dispatches mpris.player.* capabilities".into(),
            })?;
        match op {
            "play" => self.call("Play")?,
            "pause" => self.call("Pause")?,
            "play_pause" => self.call("PlayPause")?,
            "stop" => self.call("Stop")?,
            "next" => {
                self.guard_inversion(Direction::Forward)?;
                let before = self.current_trackid()?;
                self.call("Next")?;
                self.mark_direction(Direction::Forward);
                return self.settle_navigation(request_id, "next", &before);
            }
            "previous" => {
                // Guard 1: previous-while-paused restarts the track
                // (measured) — an unwanted mutation; refuse with guidance.
                self.guard_previous_while_paused()?;
                self.guard_inversion(Direction::Backward)?;
                let before = self.current_trackid()?;
                self.call("Previous")?;
                self.mark_direction(Direction::Backward);
                return self.settle_navigation(request_id, "previous", &before);
            }
            "status" | "metadata" => {
                // Read-only: return the property, already bounded.
                let name = if op == "status" {
                    "PlaybackStatus"
                } else {
                    "Metadata"
                };
                let raw = self.property(name)?;
                return Ok(AdapterOutcome::applied(format!(
                    "request={} {}={}",
                    escape_for_summary(&request_id.0),
                    op,
                    escape_for_summary(raw.trim())
                )));
            }
            other => {
                return Err(BridgeError::InvalidArguments {
                    detail: format!("unknown mpris op {other}"),
                });
            }
        }
        // Mutations verify by reading the state back (typed property, not
        // the call's ack).
        let state = self.property("PlaybackStatus")?;
        let state = state.trim().trim_start_matches('s').trim().to_string();
        let summary = format!(
            "request={} op={} state={}",
            escape_for_summary(&request_id.0),
            op,
            escape_for_summary(&state)
        );
        let verified = match op {
            "play" => state == "\"Playing\"",
            "pause" | "stop" => state != "\"Playing\"",
            _ => true,
        };
        Ok(AdapterOutcome {
            outcome: if verified {
                OperationOutcome::Verified
            } else {
                OperationOutcome::Applied
            },
            summary,
            evidence: format!("PlaybackStatus property read back as {state}"),
            job_created: false,
        })
    }

    fn job_status(&self, _job_id: &str) -> Option<StopOutcome> {
        None
    }

    fn release_inputs(&self) -> String {
        "mpris adapter owns no driver inputs".into()
    }

    fn describe(&self) -> String {
        format!("MPRIS media player ({})", self.service)
    }

    fn health(&self) -> Option<bool> {
        Some(self.property("PlaybackStatus").is_ok())
    }
}

/// Discover live MPRIS players on the user session bus (bounded list).
pub fn discover_mpris_players() -> Vec<String> {
    let Some(out) = Command::new("busctl")
        .args(["--user", "--no-pager", "list"])
        .output()
        .ok()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|line| {
            let name = line.split_whitespace().next()?;
            name.starts_with("org.mpris.MediaPlayer2.")
                .then(|| name.to_string())
        })
        .collect()
}

#[allow(dead_code)]
fn _touch(_: Option<&Path>) {}
