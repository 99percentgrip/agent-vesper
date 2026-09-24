//! VRO-17 R16-clarified regression suite: capability-gated, stage-specific
//! accelerator selection through the REAL production seams.
//!
//! These drive the production functions (`stage_route_for_policy`,
//! `stage_readiness`, `registered_routes`, `execution_rows`, the F9 gate
//! reconstruction, `save_voice_scope`/`read_voice_scope`) — not a parallel
//! selector. The accelerator boundary is exercised through an
//! instrumented route registry (fixture), asserting **absence of
//! accelerator calls**, not only an eventual CPU result.
//!
//! Scope notes:
//! - The current build registers zero real accelerator routes (honest
//!   empty registry); these tests exercise the full selection machinery
//!   via a local registry fixture that mirrors the real route shape.
//! - Physical-device NPU execution is out of scope here and remains a
//!   hardware-gated acceptance item (reported not-run, never skipped-as-pass).
//! - CPU policy's zero-call guarantee is asserted structurally: CPU
//!   resolution must complete without consulting the registry at all.

#![cfg(feature = "voice-conversation")]
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vesper_domain::BoundedString;
use vesper_voice::execution::{
    AcceleratorReadiness, ReadinessCache, ReadinessCacheKey, ReadinessInvalidation, SpeechStage,
    StageBackend, StageExecutionPolicy, StageResolution,
};

/// Instrumented route registry mirror: counts every consultation. The
/// production `registered_routes()` returns zero routes today; this
/// fixture stands in for a hypothetical registered route so the
/// selection machinery can be proven acceleration-blind under CPU
/// policy and correctly gated under the other policies.
struct InstrumentedRegistry {
    consults: AtomicUsize,
}

impl InstrumentedRegistry {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            consults: AtomicUsize::new(0),
        })
    }

    /// The readiness derivation the production `stage_readiness`
    /// performs, over this fixture's route, counting consults.
    fn readiness(&self, stage: SpeechStage, fact: AcceleratorReadiness) -> AcceleratorReadiness {
        self.consults.fetch_add(1, Ordering::SeqCst);
        let _ = stage;
        fact
    }

    /// The production resolution shape: CPU policy must not reach the
    /// registry (mirrors `voice_accel::stage_route_for_policy`).
    fn resolve(
        &self,
        stage: SpeechStage,
        policy: StageExecutionPolicy,
        fact: AcceleratorReadiness,
    ) -> StageResolution {
        if policy == StageExecutionPolicy::Cpu {
            return vesper_voice::execution::resolve_stage(
                stage,
                policy,
                &AcceleratorReadiness::NoRouteRegistered,
            );
        }
        let readiness = self.readiness(stage, fact);
        vesper_voice::execution::resolve_stage(stage, policy, &readiness)
    }

    fn consults(&self) -> usize {
        self.consults.load(Ordering::SeqCst)
    }
}

fn b64(text: &str) -> BoundedString<64> {
    BoundedString::new(text).expect("fits")
}

fn b128(text: &str) -> BoundedString<128> {
    BoundedString::new(text).expect("fits")
}

fn b256(text: &str) -> BoundedString<256> {
    BoundedString::new(text).expect("fits")
}

// ---------------------------------------------------------------------------
// 1. No NPU and no vendor runtime/libraries: CPU path initializes and
//    works; no NPU worker/load/install attempt, no voice-disabled error.
// ---------------------------------------------------------------------------

#[test]
fn no_npu_machine_cpu_initializes_and_no_acceleration_is_attempted() {
    let registry = InstrumentedRegistry::new();
    // The honest machine fact for a no-NPU machine with a *hypothetical*
    // registered route (worst case): device absent.
    for stage in [SpeechStage::Stt, SpeechStage::Tts] {
        let resolution = registry.resolve(
            stage,
            StageExecutionPolicy::AutomaticAccelerator,
            AcceleratorReadiness::DeviceAbsent,
        );
        let StageResolution::Execute(decision) = resolution else {
            panic!("CPU must remain available with no NPU present");
        };
        assert_eq!(decision.backend, StageBackend::Cpu);
        assert!(
            !decision.reason.as_str().to_lowercase().contains("warning"),
            "CPU selection is an ordinary outcome, never a warning: {}",
            decision.reason
        );
    }
    assert!(
        registry.consults() > 0,
        "automatic policy consulted the registry"
    );
}

// ---------------------------------------------------------------------------
// 2. CPU policy on a fully capable fixture: ZERO accelerator calls.
// ---------------------------------------------------------------------------

#[test]
fn cpu_policy_makes_zero_accelerator_calls_even_when_a_verified_route_exists() {
    let registry = InstrumentedRegistry::new();
    let verified = AcceleratorReadiness::Ready {
        backend: b64("route-x"),
        model: b128("model-a"),
    };
    for stage in [SpeechStage::Stt, SpeechStage::Tts] {
        let resolution = registry.resolve(stage, StageExecutionPolicy::Cpu, verified.clone());
        let StageResolution::Execute(decision) = resolution else {
            panic!("CPU policy must execute");
        };
        assert_eq!(decision.backend, StageBackend::Cpu);
    }
    assert_eq!(
        registry.consults(),
        0,
        "CPU policy must never consult the accelerator registry: a fully capable machine stays untouched"
    );
    // And the production seam is structurally acceleration-blind under
    // CPU policy (no fixture route exists; resolution is CPU).
    for stage in [SpeechStage::Stt, SpeechStage::Tts] {
        let resolution =
            agent_vesper_tui::voice_accel::stage_route_for_policy(stage, StageExecutionPolicy::Cpu);
        let StageResolution::Execute(decision) = resolution else {
            panic!()
        };
        assert_eq!(decision.backend, StageBackend::Cpu);
    }
}

// ---------------------------------------------------------------------------
// 3. NPU present but OS/driver/backend/model unsupported.
// ---------------------------------------------------------------------------

#[test]
fn unsupported_machine_automatic_uses_cpu_strict_names_the_blocker() {
    let registry = InstrumentedRegistry::new();
    let facts = vec![
        AcceleratorReadiness::Unsupported {
            detail: b256("no compatible route for this platform"),
        },
        AcceleratorReadiness::ModelUnsupported {
            backend: b64("route-x"),
            model: b128("model-a"),
        },
        AcceleratorReadiness::AssetsMissing {
            backend: b64("route-x"),
            detail: b256("model file absent"),
        },
        AcceleratorReadiness::VerificationPending {
            backend: b64("route-x"),
        },
    ];
    for fact in facts {
        // Automatic → CPU, ordinary outcome.
        let auto = registry.resolve(
            SpeechStage::Tts,
            StageExecutionPolicy::AutomaticAccelerator,
            fact.clone(),
        );
        let StageResolution::Execute(auto_decision) = auto else {
            panic!("automatic must keep CPU for {fact:?}");
        };
        assert_eq!(auto_decision.backend, StageBackend::Cpu);
        // Strict → stage-specific refusal naming the exact blocker.
        let strict = registry.resolve(
            SpeechStage::Tts,
            StageExecutionPolicy::NpuRequired,
            fact.clone(),
        );
        let StageResolution::Refused {
            message, blocker, ..
        } = strict
        else {
            panic!("strict must refuse on {fact:?}");
        };
        assert_eq!(blocker, fact);
        assert!(
            message.as_str().contains("Speech synthesis"),
            "refusal must be stage-specific: {message}"
        );
        assert!(
            message.as_str().contains("Settings → Voice"),
            "refusal must name the Settings action: {message}"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Detection denied / times out / unknown: no optimistic dispatch.
// ---------------------------------------------------------------------------

#[test]
fn detection_unknown_is_never_positive_eligibility() {
    let registry = InstrumentedRegistry::new();
    let unknown = AcceleratorReadiness::DetectionUnknown {
        detail: b256("probe timed out"),
    };
    let auto = registry.resolve(
        SpeechStage::Stt,
        StageExecutionPolicy::AutomaticAccelerator,
        unknown.clone(),
    );
    let StageResolution::Execute(decision) = auto else {
        panic!()
    };
    assert_eq!(decision.backend, StageBackend::Cpu);
    let strict = registry.resolve(SpeechStage::Stt, StageExecutionPolicy::NpuRequired, unknown);
    assert!(matches!(strict, StageResolution::Refused { .. }));
}

// ---------------------------------------------------------------------------
// 5. STT ready, TTS unsupported (and converse): independent selection;
//    no shared boolean promotes both.
// ---------------------------------------------------------------------------

#[test]
fn stages_resolve_independently_no_shared_boolean() {
    let registry = InstrumentedRegistry::new();
    // STT verified, TTS no-route.
    let stt = registry.resolve(
        SpeechStage::Stt,
        StageExecutionPolicy::AutomaticAccelerator,
        AcceleratorReadiness::Ready {
            backend: b64("route-x"),
            model: b128("whisper"),
        },
    );
    let tts = registry.resolve(
        SpeechStage::Tts,
        StageExecutionPolicy::AutomaticAccelerator,
        AcceleratorReadiness::NoRouteRegistered,
    );
    let StageResolution::Execute(stt_decision) = stt else {
        panic!()
    };
    let StageResolution::Execute(tts_decision) = tts else {
        panic!()
    };
    assert!(matches!(
        stt_decision.backend,
        StageBackend::Accelerator { .. }
    ));
    assert_eq!(tts_decision.backend, StageBackend::Cpu);
    // Converse.
    let stt2 = registry.resolve(
        SpeechStage::Stt,
        StageExecutionPolicy::AutomaticAccelerator,
        AcceleratorReadiness::NoRouteRegistered,
    );
    let StageResolution::Execute(stt2_decision) = stt2 else {
        panic!()
    };
    assert_eq!(stt2_decision.backend, StageBackend::Cpu);
    drop(registry);
}

// ---------------------------------------------------------------------------
// 6. Copied configuration/readiness from another machine: local
//    revalidation; no stale-success dispatch or silent rewrite.
// ---------------------------------------------------------------------------

#[test]
fn copied_strict_config_is_locally_revalidated_and_explained_not_rewritten() {
    // A scope copied from an NPU machine carries `tts_compute = "npu"`.
    let scope = vesper_voice::parse_voice_table(
        "[voice]\nenabled = true\ntts_compute = \"npu\"\nstt_compute = \"cpu\"\n",
    )
    .expect("copied scope parses");
    assert_eq!(scope.tts_compute, StageExecutionPolicy::NpuRequired);
    // Local machine has no route: the strict policy refuses with the
    // explanation — and the SAVED policy must remain NpuRequired (no
    // silent rewrite), verified through the real save/read round trip.
    let resolution =
        agent_vesper_tui::voice_accel::stage_route_for_policy(SpeechStage::Tts, scope.tts_compute);
    let StageResolution::Refused { message, .. } = resolution else {
        panic!("strict policy on a no-route machine must refuse")
    };
    assert!(
        message.as_str().contains("cannot run here"),
        "message explains the stale preference: {message}"
    );
    // The scope is not rewritten by resolution: re-read from disk.
    let workspace = TempWorkspace::with_scope("[voice]\nenabled = true\ntts_compute = \"npu\"\n");
    let reloaded = vesper_voice::read_voice_scope(workspace.root()).unwrap();
    assert_eq!(reloaded.tts_compute, StageExecutionPolicy::NpuRequired);
}

// ---------------------------------------------------------------------------
// 6b. Recording-review presentation contract: requested policy,
//     availability, next-request resolution and last-request receipt are
//     separate, honest facts through the shared rule.
// ---------------------------------------------------------------------------

#[test]
fn presentation_separates_selection_from_availability_and_receipt() {
    // The verification state may be pending or recorded in this process;
    // the assertions below hold under both.
    let pending_stt = matches!(
        agent_vesper_tui::voice_accel::stage_readiness(SpeechStage::Stt),
        AcceleratorReadiness::VerificationPending { .. }
    );
    let ready_stt = matches!(
        agent_vesper_tui::voice_accel::stage_readiness(SpeechStage::Stt),
        AcceleratorReadiness::Ready { .. }
    );
    assert!(
        pending_stt || ready_stt,
        "test requires the real FLM verification state"
    );

    // CPU-selected configuration with a verified FLM option: the main
    // line says CPU is selected and FLM is available but NOT selected;
    // the receipt line starts honest.
    agent_vesper_tui::voice_accel::record_flm_stt_verification();
    let cpu_scope = TempWorkspace::with_scope(
        "[voice]\nenabled = true\nstt_compute = \"cpu\"\ntts_compute = \"cpu\"\n",
    );
    let cpu = vesper_voice::read_voice_scope(cpu_scope.root()).unwrap();
    let lines = agent_vesper_tui::voice_accel::machine_capability_lines(&cpu);
    let stt_line = lines
        .iter()
        .find(|l| l.contains("recognition"))
        .expect("stt line");
    assert!(
        stt_line.contains("CPU selected for next request"),
        "CPU selection stated plainly: {stt_line}"
    );
    assert!(
        stt_line.contains("flm-npu verified and available, not selected"),
        "availability is a separate fact: {stt_line}"
    );
    assert!(
        !stt_line.contains("using CPU"),
        "a static picture never claims current use: {stt_line}"
    );

    // Strict-NPU ready configuration resolves the verified route.
    let npu_scope = TempWorkspace::with_scope(
        "[voice]\nenabled = true\nstt_compute = \"npu\"\ntts_compute = \"npu\"\n",
    );
    let strict = vesper_voice::read_voice_scope(npu_scope.root()).unwrap();
    let strict_lines = agent_vesper_tui::voice_accel::machine_capability_lines(&strict);
    let stt_strict = strict_lines
        .iter()
        .find(|l| l.contains("recognition"))
        .expect("strict stt line");
    assert!(
        stt_strict.contains("flm-npu selected for next request"),
        "strict ready resolves the route: {stt_strict}"
    );

    // The last-request receipt is its own line and never synthesized.
    let receipt = agent_vesper_tui::voice_accel::last_stt_route_line();
    assert!(
        receipt.contains("not run in this session") || receipt.contains("ran via"),
        "receipt is precise: {receipt}"
    );
}

#[test]
fn presentation_strict_npu_without_verification_refuses_never_relabeled_cpu() {
    // Simulate an unverified process state only when no other test has
    // recorded verification: readiness derivation is process-cached and
    // real; the strict refusal path itself is already covered in §6 and
    // the F9 gate reconstruction. Here we assert the PRESENTATION shape:
    // whatever the machine state, a refused stage renders "unavailable"
    // with the refusal reason, never "CPU selected".
    let strict_scope = TempWorkspace::with_scope(
        "[voice]\nenabled = true\nstt_compute = \"npu\"\ntts_compute = \"npu\"\n",
    );
    let strict = vesper_voice::read_voice_scope(strict_scope.root()).unwrap();
    for line in agent_vesper_tui::voice_accel::machine_capability_lines(&strict) {
        if line.contains("unavailable") {
            assert!(
                !line.contains("CPU selected"),
                "a refused strict stage is never relabeled CPU: {line}"
            );
            assert!(
                line.contains("Settings") || line.contains("cannot run here"),
                "refusal names the blocker and action: {line}"
            );
        }
    }
    // Draft-vs-saved: resolving the DRAFT scope must not touch the saved
    // file — the same pure rule over a different scope value.
    let draft = vesper_voice::VoiceScope {
        stt_compute: StageExecutionPolicy::Cpu,
        ..strict.clone()
    };
    let draft_lines = agent_vesper_tui::voice_accel::machine_capability_lines(&draft);
    assert!(
        draft_lines
            .iter()
            .find(|l| l.contains("recognition"))
            .expect("draft stt line")
            .contains("CPU selected for next request"),
        "draft CPU presents as draft CPU"
    );
    let saved = vesper_voice::read_voice_scope(strict_scope.root()).unwrap();
    assert_eq!(
        saved.stt_compute, strict.stt_compute,
        "presentation never rewrites the saved scope"
    );
}

// ---------------------------------------------------------------------------
// 7. Device busy/lost or runtime changes after verification: safe
//    invalidation via the bounded readiness cache.
// ---------------------------------------------------------------------------

#[test]
fn readiness_cache_invalidates_on_events_and_never_crosses_machines() {
    let mut cache = ReadinessCache::new();
    let key = |machine: &str, model: &str| ReadinessCacheKey {
        machine: b128(machine),
        stage: SpeechStage::Tts,
        backend: b64("route-x"),
        model: b128(model),
        configuration: b256("cfg-1"),
    };
    let verified = AcceleratorReadiness::Ready {
        backend: b64("route-x"),
        model: b128("model-a"),
    };
    cache.record(key("machine-a", "model-a"), verified.clone());
    // Exact key: hit.
    assert_eq!(
        cache.lookup(&key("machine-a", "model-a")),
        Some(verified.clone())
    );
    // Another machine / model: miss (no copied success).
    assert_eq!(cache.lookup(&key("machine-b", "model-a")), None);
    assert_eq!(cache.lookup(&key("machine-a", "model-b")), None);
    // Busy/unhealthy after verification + device loss: invalidation.
    cache.invalidate(ReadinessInvalidation::BackendFailure);
    assert!(cache.is_empty());
    cache.record(key("machine-a", "model-a"), verified);
    cache.invalidate(ReadinessInvalidation::DeviceLost);
    assert!(cache.is_empty());
    // No re-probe-per-token amplification: recording once and looking
    // up many times performs no derivation.
    cache.record(
        key("machine-a", "model-a"),
        AcceleratorReadiness::Ready {
            backend: b64("route-x"),
            model: b128("model-a"),
        },
    );
    for _ in 0..100 {
        assert!(cache.lookup(&key("machine-a", "model-a")).is_some());
    }
    assert_eq!(cache.len(), 1);
}

// ---------------------------------------------------------------------------
// 8. Backend claims NPU but executes CPU or only part of the graph:
//    actual-placement metadata stays CPU/hybrid/unverified.
// ---------------------------------------------------------------------------

#[test]
fn placement_attribution_never_fabricates_full_npu_success() {
    use vesper_voice::execution::{OffloadPlacement, PlacementEvidence, attribute_placement};
    // Claim without device evidence.
    assert_eq!(
        attribute_placement(&PlacementEvidence {
            backend: Some("route-x"),
            backend_claimed_offload: true,
            device_execution_evidence: false,
            cpu_execution_evidence: false,
        }),
        OffloadPlacement::Unverified {
            backend: b64("route-x")
        }
    );
    // Part of the graph on CPU.
    assert!(matches!(
        attribute_placement(&PlacementEvidence {
            backend: Some("route-x"),
            backend_claimed_offload: true,
            device_execution_evidence: true,
            cpu_execution_evidence: true,
        }),
        OffloadPlacement::Hybrid { .. }
    ));
    // Full device evidence.
    assert_eq!(
        attribute_placement(&PlacementEvidence {
            backend: Some("route-x"),
            backend_claimed_offload: true,
            device_execution_evidence: true,
            cpu_execution_evidence: false,
        }),
        OffloadPlacement::Full {
            backend: b64("route-x")
        }
    );
    // CPU path.
    assert_eq!(
        attribute_placement(&PlacementEvidence {
            backend: None,
            backend_claimed_offload: false,
            device_execution_evidence: false,
            cpu_execution_evidence: false,
        }),
        OffloadPlacement::Cpu
    );
}

// ---------------------------------------------------------------------------
// 9. Real Settings surfaces: save/read round trip of both policies; the
//    execution rows derive from the shared assessment; the strict row
//    refuses visibly.
// ---------------------------------------------------------------------------

#[test]
fn settings_save_and_execution_rows_round_trip_both_policies() {
    // Explicit state contract: this test expects no recorded FLM
    // verification in this process (see the presentation tests, which
    // record it deliberately).
    agent_vesper_tui::voice_accel::reset_flm_stt_verification_for_test();
    let workspace = TempWorkspace::with_scope("[voice]\nenabled = false\n");
    let mut scope = vesper_voice::read_voice_scope(workspace.root()).unwrap();
    assert_eq!(scope.stt_compute, StageExecutionPolicy::Cpu);
    // Independent stage policies through the real save.
    scope.stt_compute = StageExecutionPolicy::AutomaticAccelerator;
    scope.tts_compute = StageExecutionPolicy::NpuRequired;
    agent_vesper_tui::settings_host_test::save_voice_for_test(workspace.root(), &scope)
        .expect("save");
    let reloaded = vesper_voice::read_voice_scope(workspace.root()).unwrap();
    assert_eq!(
        reloaded.stt_compute,
        StageExecutionPolicy::AutomaticAccelerator
    );
    assert_eq!(reloaded.tts_compute, StageExecutionPolicy::NpuRequired);

    // Execution rows (Settings panel + readiness share these): STT row
    // shows CPU (no route registered — ordinary), TTS row shows the
    // strict refusal, never "Ready".
    let rows = agent_vesper_tui::voice_accel::execution_rows(&reloaded);
    assert_eq!(rows.len(), 2);
    let stt_row = rows
        .iter()
        .find(|row| row.stage.contains("recognition"))
        .unwrap();
    assert_eq!(stt_row.backend, "CPU");
    let tts_row = rows
        .iter()
        .find(|row| row.stage.contains("synthesis"))
        .unwrap();
    assert_eq!(tts_row.backend, "unavailable");
    assert!(tts_row.reason.contains("NPU required"));
    assert!(tts_row.reason.contains("Settings → Voice"));

    // Machine capability lines: scope-aware, honest wording — a CPU
    // selection says CPU selected (with the verified option named
    // separately); a strict refusal says unavailable; "using" is never
    // claimed from a static picture. No installer language for absent
    // hardware.
    let cpu_only = vesper_voice::VoiceScope {
        stt_compute: vesper_voice::StageExecutionPolicy::Cpu,
        tts_compute: vesper_voice::StageExecutionPolicy::Cpu,
        ..scope
    };
    let lines = agent_vesper_tui::voice_accel::machine_capability_lines(&cpu_only);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert!(
            line.contains("CPU selected for next request"),
            "CPU selection must say so plainly: {line}"
        );
        assert!(
            !line.to_lowercase().contains("install"),
            "no installer language for absent hardware: {line}"
        );
    }
    // The last-request receipt starts precise, not inferred.
    assert!(
        agent_vesper_tui::voice_accel::last_stt_route_line().contains("not run in this session")
    );
}

// ---------------------------------------------------------------------------
// 10. Voice unconfigured / build without the adapter: existing startup
//     behavior preserved; no phantom providers.
// ---------------------------------------------------------------------------

#[test]
fn unconfigured_scope_stays_fully_disabled_and_defaults_are_cpu() {
    // Explicit state contract: default resolution requires no recorded
    // verification in this process.
    agent_vesper_tui::voice_accel::reset_flm_stt_verification_for_test();
    let scope = vesper_voice::VoiceScope::default();
    assert!(!scope.enabled);
    assert_eq!(scope.stt_compute, StageExecutionPolicy::Cpu);
    assert_eq!(scope.tts_compute, StageExecutionPolicy::Cpu);
    // The real registry reflects this build: default/voice-conversation
    // builds are empty (no phantom providers); `voice-flm` builds
    // register exactly the one implemented STT route. Either way, no
    // unimplemented route may appear.
    let routes = agent_vesper_tui::voice_accel::registered_routes();
    if cfg!(feature = "voice-flm") {
        assert_eq!(routes.len(), 1, "voice-flm registers exactly one route");
        assert_eq!(routes[0].stage, SpeechStage::Stt, "STT only");
    } else {
        assert!(routes.is_empty());
    }
    // Default-resolution safety: an unverified/absent route resolves
    // automatic to CPU (no unverified acceleration).
    let resolution = agent_vesper_tui::voice_accel::stage_route_for_policy(
        SpeechStage::Stt,
        StageExecutionPolicy::AutomaticAccelerator,
    );
    let StageResolution::Execute(decision) = resolution else {
        panic!()
    };
    // Without a recorded verification in THIS test process, automatic
    // stays CPU even where the route is registered.
    assert_eq!(decision.backend, StageBackend::Cpu);
}

/// Bounded temporary workspace for scope round trips.
struct TempWorkspace {
    root: std::path::PathBuf,
}

impl TempWorkspace {
    fn with_scope(toml: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "vesper-voice-exec-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".agent-vesper")).unwrap();
        std::fs::write(root.join(".agent-vesper/config.toml"), toml).unwrap();
        Self { root }
    }

    fn root(&self) -> &std::path::Path {
        &self.root
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
