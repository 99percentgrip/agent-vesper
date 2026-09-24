//! VRO-17 R16 (clarified): the speech accelerator route registry and the
//! shared stage-readiness assessment. This is the host composition seam
//! for capability-gated NPU dispatch — the *only* place speech
//! acceleration is considered — and it enforces the binding rules:
//!
//! - **No registered route, no acceleration**: on this machine and build,
//!   no verified NPU speech route exists, so every stage resolves to CPU
//!   (automatic) or a truthful stage-specific refusal (strict). Nothing
//!   is advertised, offered for setup, or probed.
//! - **Bounded passive detection only** (existence/PATH checks; no
//!   vendor tools spawned, no accelerator libraries loaded, no device
//!   opened) and **only when a policy can use the result**: CPU policy
//!   never consults this module, so optional-NPU machines stay untouched
//!   (see `voice_accel_zero_calls` in the regression suite).
//! - **No fake registries**: a route appears here only behind a real
//!   registered adapter (none exists yet — FLM-Whisper STT is a
//!   *documented lead* pending its own implementation/verification gate
//!   in `docs/foundation/voice-first-speech-and-npu-assessment.md`).
//! - STT and TTS assess **independently**; there is no shared
//!   `npu_available` boolean anywhere.
//!
//! The pure selection rule lives in `vesper_voice::execution`; this
//! module only derives the *facts* (route registration + passive
//! machine evidence) and resolves through the shared rule so Settings,
//! Preview, F9, and dependency setup can never disagree.

use std::path::{Path, PathBuf};

use vesper_voice::execution::{
    AcceleratorReadiness, SpeechStage, StageExecutionPolicy, StageResolution,
};

/// One speech accelerator route candidate for a stage. A route is
/// registered only by real adapter code (feature-gated); today's build
/// registers none, which is the honest empty registry below.
#[derive(Debug, Clone)]
pub struct AcceleratorRoute {
    /// Stage this route accelerates.
    pub stage: SpeechStage,
    /// Vendor-neutral route/backend identity (bounded by the pure core).
    pub backend: &'static str,
    /// Passive device evidence (bounded, read-only). `None` = no
    /// matching device found through passive inspection.
    pub device_present: Option<PathBuf>,
    /// Passive runtime evidence: the backend's runtime entrypoint exists
    /// on this machine (never executed here).
    pub runtime_present: bool,
    /// Model/voice identity this route would use (route-owned metadata).
    pub model: &'static str,
}

/// The registry of registered accelerator routes in this build.
///
/// The FLM-Whisper NPU STT route registers only in `voice-flm` builds
/// (where the real adapter, owned lifecycle, bounded verification and
/// route acceptance exist — see
/// `docs/foundation/voice-npu-stt-implementation-progress.md`). Without
/// the feature the registry is empty: the honest no-route state, never
/// a decorative entry, so default builds offer no phantom option.
#[must_use]
pub fn registered_routes() -> Vec<AcceleratorRoute> {
    #[cfg(feature = "voice-flm")]
    {
        vec![AcceleratorRoute {
            stage: SpeechStage::Stt,
            backend: crate::voice_flm::BACKEND_ID,
            device_present: Path::new("/dev/accel/accel0")
                .exists()
                .then(|| PathBuf::from("/dev/accel/accel0")),
            runtime_present: crate::voice_flm_assets::flm_executable().is_some(),
            model: crate::voice_flm::MODEL_ID,
        }]
    }
    #[cfg(not(feature = "voice-flm"))]
    Vec::new()
}

/// The recorded in-process verification flag for the FLM STT route.
/// Set only by a successful bounded real-model Verify; readiness is
/// never persisted (every process re-derives it locally).
#[cfg(feature = "voice-flm")]
static FLM_VERIFIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Records a successful bounded real-model verification for the FLM
/// STT route (called by the native Verify action).
#[cfg(feature = "voice-flm")]
pub fn record_flm_stt_verification() {
    FLM_VERIFIED.store(true, std::sync::atomic::Ordering::Release);
}

/// Test-support only (`settings_host_test::save_voice_for_test`
/// precedent): restores the never-verified-in-this-process state so a
/// test can establish its own precondition instead of depending on
/// declaration order. Not part of the production surface.
#[doc(hidden)]
#[cfg(feature = "voice-flm")]
pub fn reset_flm_stt_verification_for_test() {
    FLM_VERIFIED.store(false, std::sync::atomic::Ordering::Release);
}

/// The FLM route's evidence chain: device → runtime → pack assets →
/// verification. Each absent layer yields the honest lesser state.
#[cfg(feature = "voice-flm")]
fn flm_stt_readiness() -> AcceleratorReadiness {
    if FLM_VERIFIED.load(std::sync::atomic::Ordering::Acquire) {
        return AcceleratorReadiness::Ready {
            backend: bounded64(crate::voice_flm::BACKEND_ID),
            model: bounded128(crate::voice_flm::MODEL_ID),
        };
    }
    if !Path::new("/dev/accel/accel0").exists() {
        return AcceleratorReadiness::DeviceAbsent;
    }
    if crate::voice_flm_assets::flm_executable().is_none() {
        return AcceleratorReadiness::SetupRequired {
            backend: bounded64(crate::voice_flm::BACKEND_ID),
            detail: bounded256("the local accelerated-speech runtime is not installed"),
        };
    }
    match crate::voice_flm_assets::assess_pack() {
        crate::voice_flm_assets::PackState::Installed => {
            AcceleratorReadiness::VerificationPending {
                backend: bounded64(crate::voice_flm::BACKEND_ID),
            }
        }
        crate::voice_flm_assets::PackState::RuntimeMissing => AcceleratorReadiness::SetupRequired {
            backend: bounded64(crate::voice_flm::BACKEND_ID),
            detail: bounded256(
                "the local accelerated-speech runtime is not installed; see Settings → Voice",
            ),
        },
        crate::voice_flm_assets::PackState::AssetsMissing => AcceleratorReadiness::AssetsMissing {
            backend: bounded64(crate::voice_flm::BACKEND_ID),
            detail: bounded256("the accelerated recognizer's model files are missing"),
        },
        crate::voice_flm_assets::PackState::SizeMismatch { .. } => {
            AcceleratorReadiness::AssetsMissing {
                backend: bounded64(crate::voice_flm::BACKEND_ID),
                detail: bounded256(
                    "the installed model is a different revision than the verified one",
                ),
            }
        }
        crate::voice_flm_assets::PackState::DigestMismatch => AcceleratorReadiness::AssetsMissing {
            backend: bounded64(crate::voice_flm::BACKEND_ID),
            detail: bounded256("the installed model failed its integrity check"),
        },
    }
}

/// Resolves one route's passive machine evidence into an evidence-based
/// readiness fact. Pure over the route's facts; never spawns anything.
fn route_readiness(route: &AcceleratorRoute) -> AcceleratorReadiness {
    #[cfg(feature = "voice-flm")]
    if route.backend == crate::voice_flm::BACKEND_ID {
        return flm_stt_readiness();
    }
    #[cfg(not(feature = "voice-flm"))]
    let _ = route;
    // Ordered gates: device before runtime before assets. Any absent
    // layer yields the honest lesser state — never a collapse upward.
    let Some(_device) = route.device_present.as_ref() else {
        return AcceleratorReadiness::DeviceAbsent;
    };
    if !route.runtime_present {
        return AcceleratorReadiness::SetupRequired {
            backend: bounded64(route.backend),
            detail: bounded256("the backend runtime is not installed on this machine"),
        };
    }
    // Runtime present but no registered verified adapter path exists:
    // assets/verification are pending, never "ready". A real route
    // implementation replaces this with its own bounded verification.
    AcceleratorReadiness::VerificationPending {
        backend: bounded64(route.backend),
    }
}

fn bounded64(text: &str) -> vesper_domain::BoundedString<64> {
    vesper_domain::BoundedString::new(text).expect("static identities fit")
}

#[cfg(feature = "voice-flm")]
fn bounded128(text: &str) -> vesper_domain::BoundedString<128> {
    vesper_domain::BoundedString::new(text).expect("static identities fit")
}

fn bounded256(text: &str) -> vesper_domain::BoundedString<256> {
    vesper_domain::BoundedString::new(text).expect("static detail fits")
}

/// The shared assessment for one stage on this machine: the readiness
/// fact the pure resolver consumes. Settings panels, the F9 gate,
/// Preview, and dependency setup all read this one function.
///
/// Bounded passive inspection only: route registry + existence checks.
/// No device I/O, no vendor tool execution, no library loading, no
/// models. Callers under CPU policy must not call this at all (see
/// [`stage_route_for_policy`], which skips it structurally).
#[must_use]
pub fn stage_readiness(stage: SpeechStage) -> AcceleratorReadiness {
    let routes = registered_routes();
    let mut best: Option<AcceleratorReadiness> = None;
    for route in routes.iter().filter(|route| route.stage == stage) {
        let readiness = route_readiness(route);
        if readiness.permits_acceleration() {
            return readiness;
        }
        if best.as_ref().is_none_or(|current| {
            // Prefer the most-informative blocker (setup-pending over
            // device-absent): the closest-to-working route's blocker.
            matches!(
                current,
                AcceleratorReadiness::DeviceAbsent | AcceleratorReadiness::NoRouteRegistered
            )
        }) {
            best = Some(readiness);
        }
    }
    best.unwrap_or(AcceleratorReadiness::NoRouteRegistered)
}

/// The production selection resolution for one stage under a policy.
///
/// **CPU policy is structurally acceleration-blind**: this returns CPU
/// without consulting the registry or the machine at all — zero
/// accelerator calls anywhere in the process (asserted by the
/// `voice_execution_policy` regression suite).
#[must_use]
pub fn stage_route_for_policy(stage: SpeechStage, policy: StageExecutionPolicy) -> StageResolution {
    if policy == StageExecutionPolicy::Cpu {
        return vesper_voice::execution::resolve_stage(stage, policy, &no_route());
    }
    let readiness = stage_readiness(stage);
    vesper_voice::execution::resolve_stage(stage, policy, &readiness)
}

fn no_route() -> AcceleratorReadiness {
    AcceleratorReadiness::NoRouteRegistered
}

/// One row of the Settings execution panel: stage, requested policy,
/// effective backend, and the bounded reason. Rendered from the same
/// resolution the F9 gate and Preview use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageExecutionRow {
    /// Stage label ("Speech recognition" / "Speech synthesis").
    pub stage: &'static str,
    /// Requested policy label.
    pub policy: &'static str,
    /// Effective backend label ("CPU" or the route id).
    pub backend: String,
    /// Bounded selection/fallback reason.
    pub reason: String,
}

/// Builds both Settings rows from the current scope (the same shared
/// assessment; one interpretation).
#[must_use]
pub fn execution_rows(scope: &vesper_voice::VoiceScope) -> Vec<StageExecutionRow> {
    [
        (SpeechStage::Stt, scope.stt_compute),
        (SpeechStage::Tts, scope.tts_compute),
    ]
    .into_iter()
    .map(|(stage, policy)| {
        let resolution = stage_route_for_policy(stage, policy);
        match resolution {
            StageResolution::Execute(decision) => StageExecutionRow {
                stage: stage_word(stage),
                policy: policy.label(),
                backend: decision.backend.label().to_owned(),
                reason: decision.reason.as_str().to_owned(),
            },
            StageResolution::Refused { message, .. } => StageExecutionRow {
                stage: stage_word(stage),
                policy: policy.label(),
                backend: "unavailable".to_owned(),
                reason: message.as_str().to_owned(),
            },
        }
    })
    .collect()
}

fn stage_word(stage: SpeechStage) -> &'static str {
    match stage {
        SpeechStage::Stt => "Speech recognition",
        SpeechStage::Tts => "Speech synthesis",
    }
}

/// The Preview gate: the TTS execution-policy resolution the Natural
/// Voice pack screen must consult before offering or performing a
/// Preview, using the SAME shared rule the F9 gate uses
/// ([`stage_route_for_policy`]) against the effective configuration in
/// the Preview context (the visible Settings DRAFT; F9 uses the saved
/// scope — the distinction is preserved, the rule is one).
///
/// VRO-17 §2 (red→green): on the pre-fix tree the pack screen spawned
/// its Preview worker without consulting the policy at all, so a saved
/// strict-NPU scope was refused by F9 but synthesized on CPU through
/// Preview — one effective configuration, two policy outcomes. Preview
/// never silently saves its draft; a refusal here only blocks Preview.
///
/// # Errors
///
/// The stage-specific refusal message (blocker + Settings action) when
/// the strict policy cannot run here. CPU and Automatic always pass.
pub fn preview_policy_gate(scope: &vesper_voice::VoiceScope) -> Result<(), String> {
    match stage_route_for_policy(SpeechStage::Tts, scope.tts_compute) {
        StageResolution::Execute(_) => Ok(()),
        StageResolution::Refused { message, .. } => Err(message.as_str().to_owned()),
    }
}

/// Plain-language lines for the readiness panel: what this machine can
/// do for each stage today, without installer language.
///
/// VRO-17 recording review: these lines now resolve through the SAME
/// shared `execution_rows` rule the F9 gate and Preview use, against
/// the caller's effective scope. A CPU-selected configuration says
/// "CPU selected" and reports the verified option separately; a
/// strict-NPU configuration either resolves the verified route or
/// shows its refusal — "using CPU" is reserved for genuine CPU
/// resolutions (Automatic choosing CPU), never a relabeled fallback.
/// "using" is never claimed from a static picture; when callers have a
/// real last-request route it is reported separately through
/// [`record_last_stt_route`]/[`last_stt_route_line`].
/// R4 (amended 2026-09-23): the partial-transcript capability of the STT
/// backend the CURRENT scope would select — the authority Settings uses
/// for the partials control. Derived from the same policy resolution as
/// the F9 gate (`stage_route_for_policy`), from descriptor metadata only:
/// no adapter is constructed and no engine is probed. Every current
/// production adapter is final-only (`None`); a future capable adapter
/// declares `BufferedRepass`/`Incremental` and the toggle becomes real.
#[must_use]
pub fn selected_stt_partials_mode(
    scope: &vesper_voice::VoiceScope,
) -> Option<vesper_voice::ports::PartialsKind> {
    match stage_route_for_policy(vesper_voice::SpeechStage::Stt, scope.stt_compute) {
        vesper_voice::execution::StageResolution::Execute(decision) => {
            match decision.backend {
                // CPU policy: the shared sidecar (final-only today).
                vesper_voice::execution::StageBackend::Cpu => None,
                // Accelerator route: today the only registered speech
                // accelerator is the FLM composition, final-only by its
                // verified endpoint contract. Any future capable route
                // supplies its descriptor here; until then the honest
                // answer is no partial support.
                vesper_voice::execution::StageBackend::Accelerator { .. } => None,
            }
        }
        // Refused/unavailable: nothing is selected; the toggle shows the
        // final-only presentation (Settings also shows the blocker).
        _ => None,
    }
}

/// R4 (amended): the production Voice-Settings partials row text,
/// capability-aware (the single source the panel renders and the
/// regression suite pins).
#[must_use]
pub fn partials_settings_row(scope: &vesper_voice::VoiceScope) -> String {
    match selected_stt_partials_mode(scope) {
        Some(kind) => {
            let label = match kind {
                vesper_voice::ports::PartialsKind::BufferedRepass => "live repass",
                vesper_voice::ports::PartialsKind::Incremental => "incremental",
            };
            format!(
                "Live transcript preview · current {} ({label})",
                if scope.partials { "ON" } else { "OFF" }
            )
        }
        None => "Live transcript preview · not supported by this recognizer (final text still appears after Stop)".to_owned(),
    }
}

pub fn machine_capability_lines(scope: &vesper_voice::VoiceScope) -> Vec<String> {
    execution_rows(scope)
        .into_iter()
        .map(|row| {
            let availability = if row.backend == "CPU" {
                match stage_readiness(if row.stage.contains("recognition") {
                    SpeechStage::Stt
                } else {
                    SpeechStage::Tts
                }) {
                    AcceleratorReadiness::Ready { backend, .. } => {
                        format!(" — {} verified and available, not selected", backend)
                    }
                    _ => String::new(),
                }
            } else {
                String::new()
            };
            if row.backend == "unavailable" {
                format!("{} acceleration: unavailable — {}", row.stage, row.reason)
            } else {
                format!(
                    "{} acceleration: {} selected for next request{}",
                    row.stage, row.backend, availability
                )
            }
        })
        .collect()
}

/// Records the stage route the LAST completed speech-recognition
/// request actually used (one bounded fact; a software receipt of
/// adapter dispatch, never an offload claim).
pub fn record_last_stt_route(route: impl Into<String>) {
    if let Ok(mut cell) = LAST_STT_ROUTE.lock() {
        *cell = Some(route.into());
    }
}

/// One-line presentation of the last recognition request's route, or a
/// precise never-run line when no request completed in this process.
#[must_use]
pub fn last_stt_route_line() -> String {
    let route = LAST_STT_ROUTE
        .lock()
        .ok()
        .and_then(|guard| guard.as_deref().map(str::to_owned));
    match route {
        Some(route) => format!("Last recognition request ran via: {route}"),
        None => "Last recognition request: not run in this session".to_owned(),
    }
}

static LAST_STT_ROUTE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Passive path existence helper reused by future route registrations.
#[must_use]
pub fn passive_executable_present(name: &str) -> bool {
    crate::voice_readiness::resolve_executable(name).is_some()
}

/// Passive file existence helper (no device open).
#[must_use]
pub fn passive_file_present(path: &Path) -> bool {
    path.exists()
}
