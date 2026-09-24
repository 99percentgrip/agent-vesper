//! Voice scope configuration and the speech-egress policy (PRD §2.9, D9/D17).
//!
//! Same dependency-free TOML-reader discipline as the web scope
//! (`vesper-config::web_config`): bounded tables, tolerant of unknown
//! keys, malformed known values are errors, a duplicate `[voice]` table
//! is an error. Keys:
//!
//! - `enabled` (bool, default `false`) — the opt-in switch; absent or
//!   false means hosts construct nothing voice-related (R9).
//! - `stt` / `tts` (string provider ids) — selected providers.
//! - `partials` (bool, default `false`).
//! - `voice` (string, optional TTS voice id).
//! - `egress` (string: `on-device` | `self-hosted` | `cloud`).
//!
//! Validation enforces the binding policy rules rather than leaving them
//! to runtime discretion: a configured `cloud`/`self-hosted` provider
//! under an `on-device` egress policy is a **validation error**, not a
//! runtime surprise (failover across egress classes must be configured,
//! never silently performed). Hygiene for third-party cloud synthesis is
//! mandatory and has no disable switch in this schema (a separate
//! local-presentation option may exist later for on-device TTS only).

use std::path::Path;

use serde::{Deserialize, Serialize};
use vesper_domain::ProviderId;

/// Parse/validation failure, honestly named.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceScopeError {
    /// The file could not be read.
    #[error("cannot read {path}: {reason}")]
    Read {
        /// Path attempted.
        path: String,
        /// Underlying reason.
        reason: String,
    },
    /// `[voice]` was defined twice.
    #[error("[voice] table defined twice")]
    DuplicateTable,
    /// A known key carried an unparseable value.
    #[error("malformed value for `{key}` in [voice]: {value}")]
    MalformedValue {
        /// Offending key.
        key: String,
        /// Raw value text.
        value: String,
    },
    /// The combination of selected providers and egress policy is not
    /// permitted (D9/R14: local-only speech never silently reaches a
    /// remote class).
    #[error("egress policy violation: {reason}")]
    EgressPolicy {
        /// Policy explanation.
        reason: String,
    },
}

/// Speech egress classification (mirrors the port-level class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechEgress {
    /// On-device only; neither audio nor speech text leaves the machine.
    OnDevice,
    /// A configured self-hosted endpoint is permitted.
    SelfHostedRemote,
    /// Third-party cloud processing is permitted (with mandatory
    /// pre-cloud hygiene and existing credential mechanisms).
    ThirdPartyCloud,
}

/// Whole-capture and queue budgets (D17: the reference bounded only the
/// partial window; v1 bounds everything).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureBudget {
    /// Maximum captured audio per turn, in bytes of canonical PCM.
    pub max_capture_bytes: u64,
    /// Maximum pending assistant text awaiting segmentation, in bytes.
    pub max_pending_text_bytes: u64,
    /// Maximum synthesized-but-unplayed audio per segment, in bytes.
    pub max_queued_audio_bytes: u64,
}

impl Default for CaptureBudget {
    fn default() -> Self {
        Self {
            // 10 minutes of 16 kHz mono i16 ≈ 19.2 MB; beyond this the
            // capture stops truthfully with a marker rather than growing.
            max_capture_bytes: 10 * 60 * 32_000,
            // 8 KiB pending-speech bound (PRD §2.5).
            max_pending_text_bytes: 8 * 1024,
            // ~30 s of queued synthesis audio before backpressure.
            max_queued_audio_bytes: 30 * 32_000,
        }
    }
}

/// One parsed `[voice]` scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceScope {
    /// The opt-in switch; false means no voice surface at all.
    pub enabled: bool,
    /// Selected STT provider identity.
    pub stt: Option<ProviderId>,
    /// Selected TTS provider identity.
    pub tts: Option<ProviderId>,
    /// Live partials while capturing.
    pub partials: bool,
    /// Selected TTS voice profile id.
    pub voice: Option<String>,
    /// Speech egress policy.
    pub egress: SpeechEgress,
    /// Bounded budgets.
    pub budget: CaptureBudget,
    /// STT execution policy (R16 clarified: `cpu` | `automatic` | `npu`;
    /// default `cpu` preserves every pre-policy selection).
    pub stt_compute: crate::execution::StageExecutionPolicy,
    /// TTS execution policy (independent of STT).
    pub tts_compute: crate::execution::StageExecutionPolicy,
}

impl Default for VoiceScope {
    fn default() -> Self {
        Self {
            enabled: false,
            stt: None,
            tts: None,
            partials: false,
            voice: None,
            egress: SpeechEgress::OnDevice,
            budget: CaptureBudget::default(),
            stt_compute: crate::execution::StageExecutionPolicy::Cpu,
            tts_compute: crate::execution::StageExecutionPolicy::Cpu,
        }
    }
}

/// Selected provider identities with declared egress classes, used for
/// policy validation at composition time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceProviderSelection {
    /// Egress class of the selected STT provider.
    pub stt_egress: crate::ports::SpeechEgressClass,
    /// Egress class of the selected TTS provider.
    pub tts_egress: crate::ports::SpeechEgressClass,
}

impl VoiceScope {
    /// Validates that the selected providers' egress classes are
    /// permitted by this scope's policy (R14).
    ///
    /// # Errors
    ///
    /// [`VoiceScopeError::EgressPolicy`] when an `OnDevice` policy would
    /// reach any remote class, or a `SelfHostedRemote` policy would reach
    /// the third-party cloud.
    pub fn validate_egress(
        &self,
        selection: &VoiceProviderSelection,
    ) -> Result<(), VoiceScopeError> {
        let remote = |class: crate::ports::SpeechEgressClass| {
            matches!(
                class,
                crate::ports::SpeechEgressClass::SelfHostedRemote
                    | crate::ports::SpeechEgressClass::ThirdPartyCloud
            )
        };
        match self.egress {
            SpeechEgress::OnDevice => {
                if remote(selection.stt_egress) || remote(selection.tts_egress) {
                    return Err(VoiceScopeError::EgressPolicy {
                        reason: "on-device policy forbids self-hosted and cloud providers; \
                                 failover across egress classes must be configured explicitly"
                            .into(),
                    });
                }
            }
            SpeechEgress::SelfHostedRemote => {
                if selection.stt_egress == crate::ports::SpeechEgressClass::ThirdPartyCloud
                    || selection.tts_egress == crate::ports::SpeechEgressClass::ThirdPartyCloud
                {
                    return Err(VoiceScopeError::EgressPolicy {
                        reason: "self-hosted policy forbids third-party cloud providers".into(),
                    });
                }
            }
            SpeechEgress::ThirdPartyCloud => {}
        }
        Ok(())
    }
}

/// Parses the `[voice]` table from TOML text (bounded, tolerant of
/// unknown keys, strict about known ones).
///
/// # Errors
///
/// [`VoiceScopeError`] per the variant docs.
pub fn parse_voice_table(text: &str) -> Result<VoiceScope, VoiceScopeError> {
    let mut scope = VoiceScope::default();
    let mut seen = false;
    let mut in_table = false;
    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim().to_owned();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let is_voice = line == "[voice]";
            if is_voice {
                if seen {
                    return Err(VoiceScopeError::DuplicateTable);
                }
                seen = true;
                in_table = true;
            } else {
                in_table = false;
            }
            continue;
        }
        if !in_table {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "enabled" => {
                scope.enabled = parse_bool(key, value)?;
            }
            "partials" => {
                scope.partials = parse_bool(key, value)?;
            }
            "stt" => {
                scope.stt = Some(parse_provider(key, value)?);
            }
            "tts" => {
                scope.tts = Some(parse_provider(key, value)?);
            }
            "voice" => {
                scope.voice = Some(unquote(value).to_owned());
            }
            "egress" => {
                scope.egress = match unquote(value) {
                    "on-device" => SpeechEgress::OnDevice,
                    "self-hosted" => SpeechEgress::SelfHostedRemote,
                    "cloud" => SpeechEgress::ThirdPartyCloud,
                    _ => {
                        return Err(VoiceScopeError::MalformedValue {
                            key: key.into(),
                            value: value.into(),
                        });
                    }
                };
            }
            "stt_compute" | "tts_compute" => {
                let policy =
                    crate::execution::StageExecutionPolicy::from_config_str(unquote(value))
                        .ok_or_else(|| VoiceScopeError::MalformedValue {
                            key: key.into(),
                            value: value.into(),
                        })?;
                if key == "stt_compute" {
                    scope.stt_compute = policy;
                } else {
                    scope.tts_compute = policy;
                }
            }
            _ => {}
        }
    }
    Ok(scope)
}

/// Reads the scope from `.agent-vesper/config.toml` under `root`.
///
/// # Errors
///
/// [`VoiceScopeError`] per the variant docs; a missing file yields the
/// disabled default.
pub fn read_voice_scope(root: &Path) -> Result<VoiceScope, VoiceScopeError> {
    let path = root.join(".agent-vesper").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(VoiceScope::default());
        }
        Err(error) => {
            return Err(VoiceScopeError::Read {
                path: path.display().to_string(),
                reason: error.to_string(),
            });
        }
    };
    parse_voice_table(&text)
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => &line[..index],
        None => line,
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

fn parse_bool(key: &str, value: &str) -> Result<bool, VoiceScopeError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(VoiceScopeError::MalformedValue {
            key: key.into(),
            value: value.into(),
        }),
    }
}

fn parse_provider(key: &str, value: &str) -> Result<ProviderId, VoiceScopeError> {
    ProviderId::new(unquote(value)).map_err(|_| VoiceScopeError::MalformedValue {
        key: key.into(),
        value: value.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::SpeechEgressClass;

    fn temp_root(contents: &str) -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("vesper-voice-cfg-")
            .tempdir()
            .unwrap();
        let target = dir.path().join(".agent-vesper");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("config.toml"), contents).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_disabled_default() {
        let dir = tempfile::Builder::new()
            .prefix("vesper-voice-cfg-")
            .tempdir()
            .unwrap();
        let scope = read_voice_scope(dir.path()).unwrap();
        assert!(!scope.enabled);
        assert_eq!(scope.egress, SpeechEgress::OnDevice);
    }

    #[test]
    fn parses_minimal_enabled_scope() {
        let scope = parse_voice_table("[voice]\nenabled = true\n").unwrap();
        assert!(scope.enabled);
        assert!(!scope.partials);
    }

    #[test]
    fn parses_full_scope() {
        let text = "[voice]\nenabled = true\nstt = \"whisper-local\"\ntts = \"stt-x\"\npartials = true\nvoice = \"alice\"\negress = \"self-hosted\"\n";
        let scope = parse_voice_table(text).unwrap();
        assert_eq!(scope.stt.as_ref().unwrap().as_str(), "whisper-local");
        assert_eq!(scope.egress, SpeechEgress::SelfHostedRemote);
        assert!(scope.partials);
    }

    #[test]
    fn duplicate_table_is_error() {
        let text = "[voice]\nenabled = true\n[voice]\nenabled = false\n";
        assert!(matches!(
            parse_voice_table(text),
            Err(VoiceScopeError::DuplicateTable)
        ));
    }

    #[test]
    fn malformed_bool_is_error() {
        assert!(parse_voice_table("[voice]\nenabled = yes\n").is_err());
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let scope = parse_voice_table("[voice]\nenabled = true\nfuture_key = 7\n").unwrap();
        assert!(scope.enabled);
    }

    #[test]
    fn other_tables_do_not_leak_keys() {
        let scope =
            parse_voice_table("[other]\nenabled = true\n[voice]\nenabled = false\n").unwrap();
        assert!(!scope.enabled);
    }

    #[test]
    fn on_device_policy_rejects_remote_providers() {
        let scope = VoiceScope {
            egress: SpeechEgress::OnDevice,
            ..VoiceScope::default()
        };
        let selection = VoiceProviderSelection {
            stt_egress: SpeechEgressClass::OnDevice,
            tts_egress: SpeechEgressClass::ThirdPartyCloud,
        };
        assert!(matches!(
            scope.validate_egress(&selection),
            Err(VoiceScopeError::EgressPolicy { .. })
        ));
    }

    #[test]
    fn on_device_policy_accepts_on_device_only() {
        let scope = VoiceScope {
            egress: SpeechEgress::OnDevice,
            ..VoiceScope::default()
        };
        let selection = VoiceProviderSelection {
            stt_egress: SpeechEgressClass::OnDevice,
            tts_egress: SpeechEgressClass::OnDevice,
        };
        assert!(scope.validate_egress(&selection).is_ok());
    }

    #[test]
    fn self_hosted_policy_rejects_cloud() {
        let scope = VoiceScope {
            egress: SpeechEgress::SelfHostedRemote,
            ..VoiceScope::default()
        };
        let selection = VoiceProviderSelection {
            stt_egress: SpeechEgressClass::SelfHostedRemote,
            tts_egress: SpeechEgressClass::ThirdPartyCloud,
        };
        assert!(scope.validate_egress(&selection).is_err());
    }

    #[test]
    fn cloud_policy_permits_all_classes() {
        let scope = VoiceScope {
            egress: SpeechEgress::ThirdPartyCloud,
            ..VoiceScope::default()
        };
        for stt in [
            SpeechEgressClass::OnDevice,
            SpeechEgressClass::SelfHostedRemote,
            SpeechEgressClass::ThirdPartyCloud,
        ] {
            let selection = VoiceProviderSelection {
                stt_egress: stt,
                tts_egress: SpeechEgressClass::ThirdPartyCloud,
            };
            assert!(scope.validate_egress(&selection).is_ok());
        }
    }

    #[test]
    fn read_scope_from_disk() {
        let root = temp_root("[voice]\nenabled = true\negress = \"cloud\"\n");
        let scope = read_voice_scope(root.path()).unwrap();
        assert!(scope.enabled);
        assert_eq!(scope.egress, SpeechEgress::ThirdPartyCloud);
    }

    #[test]
    fn budgets_default_bounded() {
        let budget = CaptureBudget::default();
        assert!(budget.max_capture_bytes > 0);
        assert_eq!(budget.max_pending_text_bytes, 8 * 1024);
        assert!(budget.max_queued_audio_bytes > 0);
    }

    #[test]
    fn compute_policy_keys_default_to_cpu() {
        // A pre-policy config (no compute keys) preserves CPU execution
        // for both stages — the binding compatibility default.
        let scope = parse_voice_table("[voice]\nenabled = true\n").unwrap();
        assert_eq!(
            scope.stt_compute,
            crate::execution::StageExecutionPolicy::Cpu
        );
        assert_eq!(
            scope.tts_compute,
            crate::execution::StageExecutionPolicy::Cpu
        );
    }

    #[test]
    fn compute_policy_keys_parse_and_round_trip_independently() {
        let scope = parse_voice_table(
            "[voice]\nenabled = true\nstt_compute = \"automatic\"\ntts_compute = \"npu\"\n",
        )
        .unwrap();
        assert_eq!(
            scope.stt_compute,
            crate::execution::StageExecutionPolicy::AutomaticAccelerator
        );
        assert_eq!(
            scope.tts_compute,
            crate::execution::StageExecutionPolicy::NpuRequired
        );
        // Malformed values are errors, not silent defaults.
        assert!(parse_voice_table("[voice]\nstt_compute = \"gpu\"\n").is_err());
    }
}
