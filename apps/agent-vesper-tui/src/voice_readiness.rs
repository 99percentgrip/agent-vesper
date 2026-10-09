//! VRO-17 PR-4: the single voice readiness assessment shared by the
//! F9 gate AND the Settings → Voice panel (one interpretation, never
//! two). Configuration enablement stays SEPARATE from operational
//! readiness; an enabled-but-blocked state reports the actual missing
//! prerequisite by name.
//!
//! Path resolution is absolute-or-PATH: a bare executable name is
//! resolved against `PATH` exactly once here, so existence checks never
//! depend on the process CWD (the original defect:
//! `PathBuf::from("espeak-ng").is_file()` evaluated relative to the
//! workspace and always failed).
//!
//! VRO-17 R3: the Natural Voice pack assessment joins the same shared
//! report when the saved speech engine is the neural voice — one
//! interpretation across Settings, dependency setup, preview and F9.

use std::path::{Path, PathBuf};

/// One readiness prerequisite with its observed result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessCheck {
    /// Human name for the prerequisite.
    pub name: &'static str,
    /// Observed result on this machine (read-only checks only; a
    /// `true` here is NOT device acceptance).
    pub ok: bool,
    /// What to do about it when not ok (actionable, no auto-install).
    pub remedy: &'static str,
}

/// Resolves an executable: absolute paths used as-is; bare names
/// resolved through `PATH` (or the supplied search path — tests inject
/// a fixture directory; production passes `None` for the real PATH).
/// Returns `None` when absent. Never depends on the process CWD.
#[must_use]
pub fn resolve_executable_in(name: &str, search: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if name == "espeak-ng" && search.is_none() {
        return vesper_harness::dependency_setup::voice_phonemizer();
    }
    crate::platform_voice::resolve_executable_in(name, search)
}

/// Production resolution through the real `PATH`.
#[must_use]
pub fn resolve_executable(name: &str) -> Option<PathBuf> {
    resolve_executable_in(name, None)
}

/// The shared readiness assessment. Every check is read-only existence
/// probing — no devices are opened, no processes spawned, nothing
/// installed. Tests can supply both external inputs without depending on
/// machine-owned setup; production supplies the harness voice interpreter
/// and the real `PATH`.
#[must_use]
pub fn voice_readiness_with_interpreter(
    venv_python: &Path,
    search: Option<&std::ffi::OsStr>,
) -> Vec<ReadinessCheck> {
    vec![
        ReadinessCheck {
            name: "voice backend (dictation venv interpreter)",
            ok: venv_python.is_file(),
            remedy: "use Settings → Voice → Set up local voice dependencies",
        },
        ReadinessCheck {
            name: "speech engine (espeak-ng)",
            ok: resolve_executable_in("espeak-ng", search).is_some(),
            remedy: "use Settings → Voice → Set up local voice dependencies",
        },
        ReadinessCheck {
            name: if cfg!(any(windows, target_os = "macos")) {
                "native audio backend (device checked on activation)"
            } else {
                "audio player (aplay)"
            },
            ok: cfg!(any(windows, target_os = "macos"))
                || resolve_executable_in("aplay", search).is_some(),
            remedy: "use Settings → Voice → Set up local voice dependencies",
        },
    ]
}

/// The shared readiness assessment with an injectable executable search
/// path. The voice interpreter remains the production harness path.
#[must_use]
pub fn voice_readiness_in(search: Option<&std::ffi::OsStr>) -> Vec<ReadinessCheck> {
    let root = crate::voice_venv_root();
    let venv_python = if root.is_absolute() {
        crate::platform_voice::venv_python(&root)
    } else {
        PathBuf::new()
    };
    voice_readiness_with_interpreter(&venv_python, search)
}

/// The production assessment (real `PATH`).
#[must_use]
pub fn voice_readiness() -> Vec<ReadinessCheck> {
    voice_readiness_in(None)
}

/// The first failing prerequisite, if any (the actual blocker).
#[must_use]
pub fn first_blocker_in(search: Option<&std::ffi::OsStr>) -> Option<ReadinessCheck> {
    voice_readiness_in(search)
        .into_iter()
        .find(|check| !check.ok)
}

/// The first failing prerequisite when both filesystem inputs are supplied.
/// This is the hermetic counterpart to [`first_blocker_in`].
#[must_use]
pub fn first_blocker_with_interpreter(
    venv_python: &Path,
    search: Option<&std::ffi::OsStr>,
) -> Option<ReadinessCheck> {
    voice_readiness_with_interpreter(venv_python, search)
        .into_iter()
        .find(|check| !check.ok)
}

/// The production first blocker (real `PATH`).
#[must_use]
pub fn first_blocker() -> Option<ReadinessCheck> {
    first_blocker_in(None)
}

/// Whether every prerequisite is present.
#[must_use]
pub fn all_ready() -> bool {
    first_blocker().is_none()
}

/// The F9 refusal message for an enabled-but-blocked state: names the
/// verified blocker, never tells the user to enable what is already on.
#[must_use]
pub fn blocked_message(blocker: &ReadinessCheck) -> String {
    format!(
        "Voice is enabled, but {} is unavailable: {}. Settings shows the same readiness report.",
        blocker.name, blocker.remedy
    )
}

// --- VRO-17 R3: Natural Voice pack readiness (shared assessment) ---

/// The saved scope's chosen TTS provider id, or `None` for the baseline.
/// The neural voice provider id lives in the pack crate; this helper
/// avoids leaking that id into non-kokoro builds.
#[cfg(feature = "voice-kokoro")]
#[must_use]
pub fn neural_voice_selected(tts: Option<&vesper_domain::ProviderId>) -> bool {
    tts.is_some_and(|provider| provider.as_str() == vesper_voice_kokoro::PROVIDER_ID)
}

/// The Natural Voice pack's readiness checks (feature-gated): pack
/// components verified + phonemizer present. Read-only; cached by the
/// caller through pack identity (never re-hashes the model per redraw —
/// `pack_identity` gates the digest work inside the pack crate).
#[cfg(feature = "voice-kokoro")]
#[must_use]
pub fn neural_voice_checks() -> Vec<ReadinessCheck> {
    let root = vesper_voice_kokoro::pack_root();
    let mut checks = Vec::new();
    let assessment = vesper_voice_kokoro::assess_pack(&root);
    let pack_ok = matches!(&assessment, Ok(None));
    let remedy: &'static str = match assessment {
        Ok(None) => "",
        Ok(Some(problem)) => match problem {
            vesper_voice_kokoro::PackProblem::NotInstalled => {
                "install it in Settings → Voice → Install voice pack"
            }
            vesper_voice_kokoro::PackProblem::NotVerified => {
                "an install did not finish verification; choose Install voice pack again"
            }
            vesper_voice_kokoro::PackProblem::ComponentInvalid { .. } => {
                "a pack file is invalid; choose Repair in Settings → Voice"
            }
            vesper_voice_kokoro::PackProblem::PhonemizerMissing => {
                "use Settings → Voice → Set up local voice dependencies"
            }
            vesper_voice_kokoro::PackProblem::InsufficientSpace { .. } => {
                "free disk space at the managed location and Retry"
            }
        },
        Err(error) => {
            // Storage-level failure reading the pack: surface as detail.
            let detail: &'static str = Box::leak(error.to_string().into_boxed_str());
            detail
        }
    };
    checks.push(ReadinessCheck {
        name: "Natural Voice pack (local neural speech)",
        ok: pack_ok,
        remedy: if remedy.is_empty() {
            "install it in Settings → Voice → Install voice pack"
        } else {
            remedy
        },
    });
    let phonemizer_ok = resolve_executable("espeak-ng").is_some();
    checks.push(ReadinessCheck {
        name: "pronunciation engine (espeak-ng)",
        ok: phonemizer_ok,
        remedy: "use Settings → Voice → Set up local voice dependencies",
    });
    checks
}

/// The first Neural-Voice-specific blocker, if any (production PATH).
#[cfg(feature = "voice-kokoro")]
#[must_use]
pub fn first_neural_blocker() -> Option<ReadinessCheck> {
    neural_voice_checks().into_iter().find(|check| !check.ok)
}

#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    fn readiness_uses_native_suffix_and_audio_backend_without_device_claims() {
        let temp = tempfile::tempdir().unwrap();
        let python = temp.path().join("python fixture");
        std::fs::write(&python, []).unwrap();
        std::fs::write(
            temp.path().join(if cfg!(windows) {
                "espeak-ng.exe"
            } else {
                "espeak-ng"
            }),
            [],
        )
        .unwrap();
        let search = std::env::join_paths([temp.path()]).unwrap();
        let checks = voice_readiness_with_interpreter(&python, Some(&search));
        assert!(checks[0].ok && checks[1].ok);
        if cfg!(any(windows, target_os = "macos")) {
            assert!(checks[2].ok);
            assert!(checks[2].name.contains("device checked on activation"));
        } else {
            assert!(!checks[2].ok);
            assert!(checks[2].name.contains("aplay"));
        }
    }
}
