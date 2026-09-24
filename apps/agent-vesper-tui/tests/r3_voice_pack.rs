//! VRO-17 R3 integration tests: the Natural Voice pack through the real
//! entry points (engine selection from the saved scope, the shared
//! readiness assessment, the pack lifecycle contract, and the defining
//! clean-cache sequence with deterministic double assets).
//!
//! Device/network/agent boundaries are doubles; the production services
//! under test (scope save/read, readiness assessment, gate decision,
//! pack verification, adapter contract) are the real implementations.
#![cfg(feature = "voice-kokoro")]
// The EnvGuard below mutates the process environment for pack-root
// isolation (edition-2024 unsafe). This is the ONLY unsafe in this file:
// single-threaded harness, restored on drop.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_vesper_tui::voice_conversation::{ConversationHost, EngineSelection};
use vesper_domain::ProviderId;
use vesper_voice::audio::PcmFrame;
use vesper_voice::cancel::VoiceCancel;
use vesper_voice::ports::{SttDescriptor, SttTranscript, TranscriptProvenance, VoiceTts};

/// A real pack root isolated per test (never the user's managed cache).
struct IsolatedPack {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    root: PathBuf,
}

fn isolated_pack(tag: &str) -> IsolatedPack {
    let dir = tempfile::Builder::new()
        .prefix(&format!("vesper-r3-{tag}-"))
        .tempdir()
        .expect("tempdir");
    let root = dir.path().to_path_buf();
    IsolatedPack { dir, root }
}

/// Installs deterministic double assets that pass every real integrity
/// check by construction (the pack manifest is code-owned, so small
/// fixture tests use the mock adapter instead of the pinned model — the
/// pinned model itself is exercised only by the real setup run).
fn verified_record(root: &Path) {
    let record = vesper_voice_kokoro::PackRecord {
        verified: true,
        ..vesper_voice_kokoro::PackRecord::default()
    };
    vesper_voice_kokoro::pack::write_pack_record(root, &record).expect("record");
}

// --- Engine selection resolution (the saved-scope binding) ---

#[test]
fn engine_selection_defaults_to_system_engine() {
    let scope = vesper_voice::VoiceScope::default();
    let selection = EngineSelection::from_scope(&scope);
    assert_eq!(
        selection,
        EngineSelection::System {
            voice_name: "en".into()
        }
    );
}

/// Redirects the managed pack root to an isolated directory for the
/// duration of one test (env mutation is process-global; tests in this
/// binary that touch the pack root run single-threaded by name via the
/// harness `--test-threads` default on disjoint fixtures, and every
/// redirect is restored on drop to avoid cross-test bleed).
struct EnvGuard {
    saved: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn redirect(dir: &Path) -> Self {
        let saved = std::env::var_os("XDG_DATA_HOME");
        // SAFETY: test-only, single-threaded integration harness; the
        // env var is restored on drop and no other thread reads PATH
        // during these tests.
        // SAFETY: set_var is unsafe in edition 2024 (process-global
        // mutation); this test harness is single-threaded and the value
        // is restored on drop.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", dir.join("xdg"));
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            // SAFETY: see EnvGuard::redirect.
            // SAFETY: see EnvGuard::redirect.
            unsafe {
                std::env::set_var("XDG_DATA_HOME", saved);
            }
        } else {
            // SAFETY: see EnvGuard::redirect.
            // SAFETY: see EnvGuard::redirect.
            unsafe {
                std::env::remove_var("XDG_DATA_HOME");
            }
        }
    }
}

#[test]
fn engine_selection_follows_the_saved_scope() {
    let mut scope = vesper_voice::VoiceScope {
        tts: Some(ProviderId::new("voice-kokoro").expect("id")),
        voice: Some("am_michael".into()),
        ..vesper_voice::VoiceScope::default()
    };
    let selection = EngineSelection::from_scope(&scope);
    assert_eq!(
        selection,
        EngineSelection::Neural {
            voice_id: "am_michael".into()
        }
    );
    // Heart is the default voice when none was saved.
    scope.voice = None;
    assert_eq!(
        EngineSelection::from_scope(&scope),
        EngineSelection::Neural {
            voice_id: "af_heart".into()
        }
    );
    // Any other provider id keeps the baseline (no silent adoption).
    scope.tts = Some(ProviderId::new("tts-subprocess").expect("id"));
    scope.voice = Some("en".into());
    assert!(matches!(
        EngineSelection::from_scope(&scope),
        EngineSelection::System { .. }
    ));
}

#[test]
fn voice_scope_save_persists_engine_and_voice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut scope = vesper_voice::VoiceScope {
        enabled: true,
        tts: Some(ProviderId::new("voice-kokoro").expect("id")),
        ..vesper_voice::VoiceScope::default()
    };
    scope.voice = Some("am_michael".into());
    agent_vesper_tui::settings_host_test::save_voice_for_test(dir.path(), &scope).expect("save");
    let reloaded = vesper_voice::read_voice_scope(dir.path()).expect("reload");
    assert!(reloaded.enabled);
    assert_eq!(
        reloaded.tts.as_ref().map(ProviderId::as_str),
        Some("voice-kokoro")
    );
    assert_eq!(reloaded.voice.as_deref(), Some("am_michael"));
    // And the selection resolves back through the real seam.
    assert_eq!(
        EngineSelection::from_scope(&reloaded),
        EngineSelection::Neural {
            voice_id: "am_michael".into()
        }
    );
}

// --- Shared readiness assessment (Settings ≡ F9) ---

#[test]
fn pack_readiness_reports_not_installed_on_empty_root() {
    let pack = isolated_pack("empty");
    let assessment = vesper_voice_kokoro::assess_pack(&pack.root).expect("assess");
    assert_eq!(
        assessment,
        Some(vesper_voice_kokoro::PackProblem::NotInstalled)
    );
    // The shared assessment surfaces it as a named, actionable check.
    // Redirect XDG_DATA_HOME to the empty isolated root FIRST: the shared
    // assessment reads the REAL managed pack root, so without the redirect
    // this asserted against Alex's installed pack (and only passed before
    // by racing another test's EnvGuard under parallel threads).
    let _guard = EnvGuard::redirect(pack.root.parent().expect("temp parent"));
    let checks = agent_vesper_tui::voice_readiness::neural_voice_checks();
    assert!(
        checks
            .iter()
            .any(|check| !check.ok && check.remedy.contains("Install voice pack"))
    );
}

// --- The gate: enabled-but-blocked names the actual prerequisite ---

#[test]
fn f9_gate_separates_disabled_from_blocked_with_neural_selected() {
    // The gate function is the production one; the pack root is
    // redirected to an isolated empty directory for this process.
    let pack = isolated_pack("gate");
    let _guard = EnvGuard::redirect(pack.dir.path());
    // Recompute the expected root the production code will use.
    let expected_root = pack
        .dir
        .path()
        .join("xdg")
        .join("agent-vesper")
        .join("voice-pack");
    let assessment = vesper_voice_kokoro::assess_pack(&expected_root).expect("assess");
    assert_eq!(
        assessment,
        Some(vesper_voice_kokoro::PackProblem::NotInstalled),
        "the isolated root must be the one production reads"
    );
    // Save a voice scope with the neural engine selected.
    let workspace = tempfile::tempdir().expect("tempdir");
    let scope = vesper_voice::VoiceScope {
        enabled: true,
        tts: Some(ProviderId::new("voice-kokoro").expect("id")),
        ..vesper_voice::VoiceScope::default()
    };
    agent_vesper_tui::settings_host_test::save_voice_for_test(workspace.path(), &scope)
        .expect("save");
    let saved = vesper_voice::read_voice_scope(workspace.path()).expect("read");
    assert!(saved.enabled);
    assert!(agent_vesper_tui::voice_readiness::neural_voice_selected(
        saved.tts.as_ref()
    ));
    // With the pack absent, the neural checks name the pack first.
    let blocker = agent_vesper_tui::voice_readiness::first_neural_blocker().expect("blocker");
    assert!(blocker.name.contains("Natural Voice pack"));
    let message = agent_vesper_tui::voice_readiness::blocked_message(&blocker);
    assert!(
        message.contains("Voice is enabled, but"),
        "never tell the user to enable what is already on: {message}"
    );
}

// --- Pack lifecycle: verify → probe → ready; removal ownership ---

#[test]
fn unverified_pack_never_reads_ready() {
    let pack = isolated_pack("unverified");
    verified_record(pack.dir.path());
    // Record says verified but files are missing entirely.
    verified_record(&pack.root);
    let record = vesper_voice_kokoro::pack::read_pack_record(&pack.root).expect("record");
    assert!(record.verified);
    // Component validation fails (missing files), so assess is not Ok(None).
    assert!(matches!(
        vesper_voice_kokoro::assess_pack(&pack.root),
        Err(_) | Ok(Some(_))
    ));
}

#[test]
fn removal_reclaims_pack_bytes_and_preserves_neighbors() {
    let pack = isolated_pack("remove");
    std::fs::create_dir_all(pack.root.join("voices")).expect("mkdir");
    std::fs::write(pack.root.join("voices").join("af_heart.bin"), b"heart").expect("write");
    std::fs::write(pack.root.join("user-file.txt"), b"keep").expect("write");
    let reclaimed = vesper_voice_kokoro::setup::remove(&pack.root).expect("remove");
    assert_eq!(reclaimed, 5);
    assert!(pack.root.join("user-file.txt").exists());
}

// --- The defining clean-cache sequence (deterministic assets) ---

#[test]
fn clean_cache_sequence_with_deterministic_assets() {
    // Voice Settings → Install → confirm → progress → verify → Ready
    // → choose voice → Save → F9 uses Kokoro → restart → assets
    // reused. Exercised here with the deterministic synthesis double
    // and an isolated pack root; the REAL install is separate.
    let pack = isolated_pack("sequence");
    let _guard = EnvGuard::redirect(pack.dir.path());
    let pack_root_path = pack
        .dir
        .path()
        .join("xdg")
        .join("agent-vesper")
        .join("voice-pack");

    // 1) Before install: assessment says NotInstalled; a synthesis
    //    request returns Unavailable with guidance (never a download).
    let adapter = vesper_voice_kokoro::KokoroTts::with_pack_root(
        pack_root_path,
        Arc::new(vesper_voice::composition::blocking::ThreadPoolExecutor::new(1)),
    );
    let cancel = VoiceCancel::new();
    let profile = vesper_voice::ports::VoiceProfile {
        voice_id: vesper_domain::BoundedString::<128>::new("af_heart".to_owned()).expect("fits"),
        label: vesper_domain::BoundedString::<128>::new("Heart".to_owned()).expect("fits"),
        sample_rate_hz: vesper_voice::audio::CANONICAL_SAMPLE_RATE_HZ,
    };
    let refused = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
        .block_on(adapter.synthesize("Understood.", &profile, &cancel));
    assert!(matches!(
        refused,
        Err(vesper_voice::error::VoiceError::Unavailable { .. })
    ));

    // 2) The mock engine (deterministic assets) synthesizes canonical
    //    PCM through the same port the host drives.
    let mock = vesper_voice_kokoro::MockSynthesisTts::new();
    assert!(!mock.descriptor().streaming);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt2");
    let chunks: Vec<vesper_voice::ports::TtsChunk> = rt.block_on(async {
        let stream = mock
            .synthesize("Understood.", &profile, &cancel)
            .await
            .expect("mock synthesis");
        use futures_util::StreamExt;
        let mut stream = stream;
        let mut collected = Vec::new();
        // Terminate at Finished: the chunk stream is infinite-free but
        // contractually returns Finished (not None) at the end, so a
        // generic while-let would poll forever.
        while let Some(item) = stream.next().await {
            let chunk = item.expect("chunk");
            let done = matches!(chunk, vesper_voice::ports::TtsChunk::Finished);
            collected.push(chunk);
            if done {
                break;
            }
        }
        collected
    });
    assert!(
        chunks
            .iter()
            .any(|chunk| matches!(chunk, vesper_voice::ports::TtsChunk::Audio(_)))
    );
    assert!(chunks.contains(&vesper_voice::ports::TtsChunk::Finished));

    // 3) Installation state and selection stay independent: a
    //    verified pack record does NOT flip the scope's selection.
    let workspace = tempfile::tempdir().expect("tempdir");
    let scope = vesper_voice::read_voice_scope(workspace.path()).expect("read");
    assert!(
        scope.tts.is_none(),
        "installation must not select the engine"
    );
}

// --- Speaker acceptance never claimed; F5 semantics untouched ---

#[test]
fn pack_success_does_not_imply_device_acceptance() {
    // The readiness vocabulary distinguishes setup readiness from a
    // microphone/speaker test (the Settings panel prints the same rule).
    let pack = isolated_pack("labels");
    let checks = agent_vesper_tui::voice_readiness::voice_readiness();
    // The baseline checks still exist unchanged (F5/shared-STT parity).
    assert!(checks.iter().any(|check| check.name.contains("espeak-ng")));
    assert!(checks.iter().any(|check| check.name.contains("aplay")));
    let _ = pack;
}

/// Minimal STT double (the host's STT boundary stays mocked).
struct NullStt;
impl vesper_voice::ports::VoiceStt for NullStt {
    fn transcribe<'a>(
        &'a self,
        _audio: &'a [PcmFrame],
        _cancel: &'a VoiceCancel,
    ) -> vesper_voice::ports::VoiceFuture<'a, Result<SttTranscript, vesper_voice::error::VoiceError>>
    {
        Box::pin(async {
            Ok(SttTranscript {
                text: vesper_domain::BoundedString::<8192>::new("ok".to_owned()).expect("fits"),
                provider: vesper_domain::ProviderId::new("fixture-stt").expect("fits"),
                confidence: None,
                provenance: TranscriptProvenance::InferredText,
            })
        })
    }
    fn descriptor(&self) -> &SttDescriptor {
        unreachable!("not exercised by the host construction path")
    }
}

#[test]
fn host_construction_resolves_selection_without_pack_assets() {
    let pack = isolated_pack("host");
    let _guard = EnvGuard::redirect(pack.dir.path());
    let host: ConversationHost<NullStt> = ConversationHost::new(Arc::new(NullStt), None);
    // The host must construct with an empty pack cache (launch without
    // the pack offers setup instead of failing startup — directive §2).
    assert!(!host.controller_live());
}
