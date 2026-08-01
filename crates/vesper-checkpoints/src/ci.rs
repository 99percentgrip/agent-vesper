//! CI status reader — `/ci`.
//!
//! Shells out to the `gh` CLI (if present) to fetch the most recent CI
//! runs for the current branch. Falls back to a clear "unavailable"
//! status when `gh` is not on `PATH` or the working directory is not a
//! GitHub repo. No live API calls, no credentials — `gh` resolves its own
//! auth.
//!
//! RAII: every `Command` is scoped; the spawned process and its pipes are
//! reaped when the function returns.

use std::process::{Command, Stdio};

use crate::error::CheckpointError;

/// Maximum bytes of `gh` output captured (the rest is truncated).
const MAX_CAPTURE_BYTES: usize = 8 * 1024;

/// Result of one [`CiStatusReader::status`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiStatus {
    /// True when `gh` ran successfully and produced output.
    pub available: bool,
    /// The captured `gh run list` output (truncated to
    /// [`MAX_CAPTURE_BYTES`]).
    pub output: String,
}

/// Reads CI status by shelling out to `gh`.
pub struct CiStatusReader;

impl CiStatusReader {
    /// Returns the most recent CI runs for the current branch via
    /// `gh run list --limit 5`. When `gh` is missing or the repo is not a
    /// GitHub repo, returns `available: false` with a clear notice.
    #[must_use]
    pub fn status() -> CiStatus {
        if !which("gh") {
            return CiStatus {
                available: false,
                output: "CI status unavailable: `gh` CLI not on PATH.".into(),
            };
        }
        // Scoped: `output` is collected and the `Command` reaped here.
        let result = Command::new("gh")
            .args(["run", "list", "--limit", "5"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = result else {
            return CiStatus {
                available: false,
                output: "CI status unavailable: `gh run list` failed to spawn.".into(),
            };
        };
        if !output.status.success() {
            return CiStatus {
                available: false,
                output: "CI status unavailable: not a GitHub repo, or `gh` not authenticated."
                    .into(),
            };
        }
        let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if stdout.len() > MAX_CAPTURE_BYTES {
            stdout.truncate(MAX_CAPTURE_BYTES);
            stdout.push_str("\n... (truncated)");
        }
        CiStatus {
            available: true,
            output: stdout,
        }
    }
}

/// Returns true when `program` is on `PATH`. Best-effort; never panics.
fn which(program: &str) -> bool {
    let check = if cfg!(target_os = "windows") {
        Command::new("where")
            .arg(program)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    } else {
        Command::new("which")
            .arg(program)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    matches!(check, Ok(status) if status.success())
}

// Touch CheckpointError so the import stays used in case future variants
// need it (e.g. surfaced via a `Result` return on timeout).
const _: fn() = || {
    let _ = CheckpointError::Subprocess;
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_returns_a_clear_notice_when_gh_is_missing() {
        // We cannot assume `gh` is installed in CI; this test asserts that
        // the reader degrades gracefully regardless. When `gh` IS present
        // (developer machine, or a CI runner with `gh`), the reader
        // attempts the call; either way it must not panic.
        let status = CiStatusReader::status();
        // We don't assert `available` because it depends on the host; we
        // only assert the output is non-empty and structurally sound.
        assert!(!status.output.is_empty());
    }
}
