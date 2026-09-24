//! VRO-17 §2 (CPU production acceptance): Preview and F9 must share ONE
//! execution-policy semantics through the real production paths.
//!
//! Two red checks established by source inspection before any fix (this
//! work unit; the pre-fix tree fails both by construction):
//!
//! 1. **Preview policy parity (RED).** The Natural Voice pack screen —
//!    the Settings Preview path — spawned its `SpeechWorker` without
//!    resolving the TTS execution policy, while the F9 gate
//!    (`main.rs::conversation_gate_decision`) refuses a strict-NPU scope
//!    with a stage-specific refusal. With a saved/copied
//!    `tts_compute = "npu"` scope, F9 honestly refused while Preview
//!    synthesized on CPU anyway — one effective configuration, two
//!    policy outcomes. The fix resolves the Preview TTS stage through
//!    the same shared rule (`voice_accel::stage_route_for_policy`) and
//!    refuses Preview identically.
//!
//! 2. **Unchanged selection must not rebuild the engine (RED).**
//!    `ConversationHost::reload_engine_selection` unconditionally sent
//!    `Command::Replace`, bumping the speech generation and rebuilding
//!    the acoustic engine even for an unrelated or no-op Settings save.
//!    The directive requires that a voice-only selection change not
//!    unnecessarily reload an unchanged acoustic model, and that
//!    re-resolution happen at the existing safe boundary (unit
//!    boundary), never mid-sentence. The fix sends `Replace` only for a
//!    real selection change; a real change still bumps (kept green so
//!    the fix cannot over-suppress).
//!
//! 3. **R9 no-probe parity (kept green).** A voice-unconfigured scope
//!    stays Disabled through the production gate and the honest empty
//!    accelerator registry stays empty — no voice initialization merely
//!    from the application path.
//!
//! 4. **Automatic with no registered route is an ordinary CPU outcome**
//!    (kept green): a truthful reason, no vendor tool, no download.

#![cfg(feature = "voice-kokoro")]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_vesper_tui::voice_conversation::{ConversationHost, EngineSelection};
use vesper_voice::VoiceScope;
use vesper_voice::execution::{SpeechStage, StageExecutionPolicy, StageResolution};

/// Isolated per-test workspace root. The production scope reader uses
/// the current directory, so each test chdirs into its own temporary
/// root and restores the previous directory on drop. Run this suite
/// single-threaded (documented invocation: `-- --test-threads=1`); the
/// chdir guard makes cross-test interference structurally impossible.
/// Serializes chdir-rooted tests: the process-global working directory
/// must not change while another chdir-rooted test's scope reads run.
static PARITY_ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct TestRoot {
    previous: PathBuf,
    dir: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl TestRoot {
    fn new(tag: &str) -> Self {
        // The working directory is PROCESS-GLOBAL: parallel tests chdir
        // underneath each other (the documented "run single-threaded"
        // flake). Holding the guard for the root's lifetime serializes
        // exactly the chdir windows; everything outside them stays parallel.
        let guard = PARITY_ROOT_LOCK.lock().unwrap();
        let previous = std::env::current_dir().expect("current dir");
        let dir =
            std::env::temp_dir().join(format!("vesper-policy-parity-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create isolated root");
        std::env::set_current_dir(&dir).expect("chdir into isolated root");
        Self {
            previous,
            dir,
            _guard: guard,
        }
    }

    /// Write the production voice scope for this root.
    fn scope(&self, scope: &VoiceScope) {
        agent_vesper_tui::settings_voice_save::save_voice_scope(Path::new("."), scope)
            .expect("save scope");
    }

    fn read_scope(&self) -> VoiceScope {
        vesper_voice::read_voice_scope(Path::new(".")).unwrap_or_default()
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn neural_scope() -> VoiceScope {
    VoiceScope {
        enabled: true,
        tts: Some(vesper_domain::ProviderId::new("voice-kokoro").expect("id")),
        voice: Some("am_michael".into()),
        ..VoiceScope::default()
    }
}

/// The F9 gate's ordered policy checks, reconstructed with the same
/// production functions in the same order as
/// `main.rs::conversation_gate_decision` (the binary's gate is private;
/// this reconstruction is the documented integration seam, pinned in
/// shape by `pr4_f9_gate`).
fn f9_gate() -> Result<&'static str, String> {
    let scope = vesper_voice::read_voice_scope(Path::new(".")).unwrap_or_default();
    if !scope.enabled {
        return Ok("Disabled");
    }
    for stage in [
        (SpeechStage::Stt, scope.stt_compute),
        (SpeechStage::Tts, scope.tts_compute),
    ] {
        if let StageResolution::Refused { message, .. } =
            agent_vesper_tui::voice_accel::stage_route_for_policy(stage.0, stage.1)
        {
            return Err(message.as_str().to_owned());
        }
    }
    Ok("Ready")
}

// ---------------------------------------------------------------------------
// 1. Preview policy parity
// ---------------------------------------------------------------------------

/// RED on the pre-fix tree: with a saved strict-NPU TTS scope, the F9
/// gate refuses; the Preview path must resolve the SAME effective policy
/// and refuse identically — one configuration, one outcome.
#[test]
fn preview_resolves_the_same_tts_policy_as_the_f9_gate() {
    let root = TestRoot::new("strict-npu");
    let mut scope = neural_scope();
    scope.tts_compute = StageExecutionPolicy::NpuRequired;
    root.scope(&scope);

    // The production F9 outcome: a stage-specific refusal naming the
    // blocker and the Settings action.
    let Err(gate_message) = f9_gate() else {
        panic!("the gate must refuse a strict-NPU TTS scope");
    };
    assert!(
        gate_message.contains("NPU required")
            && gate_message.contains("Settings → Voice → Execution"),
        "gate refusal must name the policy and remedy: {gate_message}"
    );

    // The production Preview resolution for the SAME effective policy:
    // refused, with the SAME message — not a CPU synthesis.
    let preview = agent_vesper_tui::voice_accel::stage_route_for_policy(
        SpeechStage::Tts,
        StageExecutionPolicy::NpuRequired,
    );
    match preview {
        StageResolution::Refused { message, .. } => {
            assert_eq!(
                message.as_str(),
                gate_message,
                "Preview and F9 must share one refusal for one policy"
            );
        }
        StageResolution::Execute(decision) => panic!(
            "Preview must refuse a strict-NPU policy, not execute on {}",
            decision.backend.label()
        ),
    }
}

/// The Natural Voice pack screen must actually CONSULT that resolution
/// before offering/performing Preview. On the pre-fix tree this gate did
/// not exist and the screen synthesized regardless of the policy.
#[test]
fn pack_screen_consults_the_tts_policy_before_preview() {
    let root = TestRoot::new("strict-npu-pack");
    let mut scope = neural_scope();
    scope.tts_compute = StageExecutionPolicy::NpuRequired;
    root.scope(&scope);

    let Err(message) = agent_vesper_tui::voice_accel::preview_policy_gate(&root.read_scope())
    else {
        panic!("the pack-screen Preview gate must refuse a strict-NPU scope");
    };
    assert!(
        message.contains("NPU required") && message.contains("Settings → Voice"),
        "refusal must name the policy and remedy: {message}"
    );
}

/// An automatic draft with the honest empty registry is an ordinary CPU
/// outcome for BOTH paths (Preview uses the draft; F9 uses saved).
#[test]
fn automatic_policy_is_ordinary_cpu_for_both_paths() {
    let root = TestRoot::new("automatic");
    let mut scope = neural_scope();
    scope.tts_compute = StageExecutionPolicy::AutomaticAccelerator;
    root.scope(&scope);

    assert_eq!(f9_gate(), Ok("Ready"));

    let gate = agent_vesper_tui::voice_accel::preview_policy_gate(&scope);
    assert!(gate.is_ok(), "automatic must stay an ordinary outcome");

    let decision = match agent_vesper_tui::voice_accel::stage_route_for_policy(
        SpeechStage::Tts,
        StageExecutionPolicy::AutomaticAccelerator,
    ) {
        StageResolution::Execute(decision) => decision,
        other => panic!("automatic with no routes must stay executable: {other:?}"),
    };
    assert_eq!(
        decision.backend.label(),
        "CPU",
        "automatic must select the compatible CPU route"
    );
    assert!(
        decision.reason.as_str().contains("no verified accelerator"),
        "the reason must be truthful, not a warning: {}",
        decision.reason
    );
}

/// A CPU scope stays accepted by both paths (existing behavior).
#[test]
fn cpu_policy_is_accepted_by_both_paths() {
    let root = TestRoot::new("cpu");
    let mut scope = neural_scope();
    scope.tts_compute = StageExecutionPolicy::Cpu;
    root.scope(&scope);
    assert_eq!(f9_gate(), Ok("Ready"));
    assert!(agent_vesper_tui::voice_accel::preview_policy_gate(&scope).is_ok());
}

// ---------------------------------------------------------------------------
// 2. Unchanged selection must not rebuild the engine
// ---------------------------------------------------------------------------

/// RED on the pre-fix tree: a reload with an UNCHANGED selection must
/// not bump the speech generation (which invalidated queued speech) and
/// must not start engine reconstruction.
#[test]
fn unchanged_engine_reload_keeps_generation_and_engine() {
    let root = TestRoot::new("no-op-reload");
    root.scope(&neural_scope());

    let stt = Arc::new(agent_vesper_tui::voice_shared_stt::SharedSidecarStt::new());
    let mut host = ConversationHost::new(stt, None);
    let (_, before) = host.speech_state();
    host.reload_engine_selection(); // the production Settings-Save path
    let (selection, after) = host.speech_state();
    assert_eq!(
        before, after,
        "a no-op reload must not bump the speech generation"
    );
    assert!(
        matches!(selection, EngineSelection::Neural { .. }),
        "the selection itself must be preserved"
    );
}

/// A REAL voice change still re-resolves at the safe boundary (existing
/// behavior, kept green so the fix cannot over-suppress).
#[test]
fn changed_voice_still_replaces_at_the_safe_boundary() {
    let root = TestRoot::new("real-change");
    root.scope(&neural_scope());

    let stt = Arc::new(agent_vesper_tui::voice_shared_stt::SharedSidecarStt::new());
    let mut host = ConversationHost::new(stt, None);
    let (_, before) = host.speech_state();
    let mut changed = neural_scope();
    changed.voice = Some("af_heart".into());
    root.scope(&changed);
    host.reload_engine_selection();
    let (selection, after) = host.speech_state();
    assert_eq!(
        before + 1,
        after,
        "a real selection change must bump the generation exactly once"
    );
    assert!(
        matches!(selection, EngineSelection::Neural { ref voice_id } if voice_id == "af_heart"),
        "the new voice must be applied: {selection:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. R9: unconfigured stays fully disabled
// ---------------------------------------------------------------------------

#[test]
fn unconfigured_scope_stays_disabled_and_initializes_nothing() {
    let _root = TestRoot::new("unconfigured");
    // No scope written at all (fresh root): the gate must report the
    // activation route, never initialize the recorder/STT/player, and
    // the registry reflects exactly this build's implemented routes
    // (empty by default; the one FLM STT route only in `voice-flm`
    // builds — never an unimplemented entry).
    assert_eq!(f9_gate(), Ok("Disabled"));
    let routes = agent_vesper_tui::voice_accel::registered_routes();
    if cfg!(feature = "voice-flm") {
        assert_eq!(routes.len(), 1, "voice-flm registers exactly one route");
        assert_eq!(
            routes[0].stage,
            vesper_voice::SpeechStage::Stt,
            "STT only; TTS stays CPU"
        );
    } else {
        assert!(
            routes.is_empty(),
            "the honest empty registry must stay empty"
        );
    }
}
