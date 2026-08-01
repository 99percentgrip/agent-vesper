//! Clipboard port — `/copy`.
//!
//! Provides a safe abstraction over the platform clipboard. In a headless
//! terminal (the typical TUI environment) no clipboard is reachable; the
//! port records the would-be copy target so the driver can retrieve it
//! via `/copy` again, and surfaces a clear "clipboard not available"
//! status instead of crashing.
//!
//! Two strategies are tried in order:
//! 1. **Persistence strategy** (always available): the target is appended
//!    to `<root>/clipboard.log` so a subsequent `/copy` or external tool
//!    can read the last value.
//! 2. **Native strategy** (best-effort): on macOS `pbcopy`, on Linux
//!    `xclip` / `xsel` / `wl-copy`, on Windows `clip`. Failure is silent
//!    and the persistence strategy still wins.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::CheckpointError;
use crate::io::append_line;

/// Maximum bytes of one clipboard target.
pub const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

/// Records would-be clipboard targets under `<root>/clipboard.log`.
pub struct ClipboardPort {
    root: PathBuf,
    /// Cache of the last value (for tests; the source of truth is the log).
    last_value: Mutex<Option<String>>,
}

impl ClipboardPort {
    /// Opens a clipboard port rooted at `root`.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        if !root.is_absolute() {
            return Err(CheckpointError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.exists() => Ok(()),
            Some(_) | None => Err(CheckpointError::InvalidRoot),
        }?;
        Ok(Self {
            root: root.to_path_buf(),
            last_value: Mutex::new(None),
        })
    }

    fn log_path(&self) -> PathBuf {
        self.root.join("clipboard.log")
    }

    /// Returns the most recent recorded value, if any.
    #[must_use]
    pub fn last_value(&self) -> Option<String> {
        self.last_value
            .lock()
            .expect("clipboard mutex poisoned")
            .clone()
    }

    /// Records `value` as the would-be clipboard target. Always succeeds
    /// (the persistence strategy cannot fail in normal operation); the
    /// returned [`ClipboardOutcome`] tells the caller whether the native
    /// strategy also fired.
    pub fn copy(&self, value: &str) -> Result<ClipboardOutcome, CheckpointError> {
        if value.len() > MAX_CLIPBOARD_BYTES {
            return Err(CheckpointError::BoundsViolated("clipboard size"));
        }
        // 1. Persistence strategy (authoritative).
        append_line(&self.log_path(), value)?;
        if let Ok(mut last) = self.last_value.lock() {
            *last = Some(value.to_string());
        }
        // 2. Native strategy (best-effort; never fails the call).
        let native = try_native_clipboard(value);
        Ok(ClipboardOutcome {
            persisted: true,
            native,
        })
    }
}

/// Outcome of one [`ClipboardPort::copy`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardOutcome {
    /// True when the value was persisted to `<root>/clipboard.log`.
    pub persisted: bool,
    /// True when the native clipboard (if any) accepted the value.
    pub native: bool,
}

/// Best-effort attempt to push `value` to the platform clipboard. Returns
/// `true` on success, `false` on any failure (including the platform
/// having no clipboard at all). Uses scoped `std::process::Command`
/// invocations so no descriptors are leaked.
fn try_native_clipboard(value: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else if cfg!(target_os = "windows") {
        ("clip", &[])
    } else if std::env::var("WAYLAND_DISPLAY").is_ok() {
        ("wl-copy", &[])
    } else if which("xclip") {
        ("xclip", &["-selection", "clipboard"])
    } else if which("xsel") {
        ("xsel", &["--clipboard", "--input"])
    } else {
        return false;
    };
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    if let Some(stdin) = child.stdin.as_mut() {
        // Best-effort write — failure here just means the native clipboard
        // did not get the value (the persistence strategy still did).
        let _ = stdin.write_all(value.as_bytes());
    }
    // `child` drops here → subprocess resources are reaped.
    matches!(child.wait(), Ok(status) if status.success())
}

/// Returns true when `program` is on `PATH`. Best-effort; never panics.
fn which(program: &str) -> bool {
    use std::process::{Command, Stdio};
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn copy_always_persists_even_without_a_native_clipboard() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let clipboard = ClipboardPort::open(&root).unwrap();
        let outcome = clipboard.copy("hello world").unwrap();
        assert!(outcome.persisted);
        assert_eq!(clipboard.last_value().as_deref(), Some("hello world"));
        // The log file must contain the value.
        let log = fs::read_to_string(root.join("clipboard.log")).unwrap();
        assert!(log.contains("hello world"));
    }

    #[test]
    fn copy_rejects_oversized_value() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let clipboard = ClipboardPort::open(&root).unwrap();
        let huge = "x".repeat(MAX_CLIPBOARD_BYTES + 1);
        let err = clipboard.copy(&huge).unwrap_err();
        assert_eq!(err, CheckpointError::BoundsViolated("clipboard size"));
    }
}
