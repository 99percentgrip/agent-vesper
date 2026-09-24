//! Stage execution policy and capability-gated accelerator selection
//! (R16, clarified): CPU, Automatic (verified-compatible acceleration
//! only), and strict NPU-required policies resolved per speech stage
//! from *evidence-based* readiness facts — never from a device label,
//! a vendor name, or a single `npu_available` boolean.
//!
//! Binding semantics (PRD §"Current NPU requirement" and Alex's
//! clarification):
//!
//! - **CPU policy** never initializes, probes, or benchmarks an
//!   optional accelerator, even when one exists and is verified.
//! - **Automatic** dispatches only to a *verified* route for the exact
//!   stage/model on this machine; every lesser readiness fact selects
//!   the authorized compatible CPU route with the actual reason
//!   exposed. No installation is implicit; CPU is an ordinary
//!   supported outcome, not a warning.
//! - **NPU required** dispatches only to the specified verified route.
//!   Any lesser fact is a stage-specific refusal that names the exact
//!   blocker and the Settings action — never a silent CPU dispatch and
//!   never a relabeling of CPU execution as satisfying the request.
//! - STT and TTS resolve **independently**; no shared boolean promotes
//!   both stages.
//!
//! This module is pure: no I/O, no clock, no device access, no vendor
//! names. Hosts supply readiness facts from bounded passive inspection
//! (and, only where a real route is registered, from a bounded real-model
//! verification); the resolver here is the single selection rule.
//! Readiness caching is in-process only — readiness is never persisted,
//! so a copied configuration can carry *intent* (a policy) but never a
//! stale success: every process re-derives readiness locally.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vesper_domain::BoundedString;

/// Which half of the speech pipeline a policy applies to. Stages are
/// independent: STT capability never establishes TTS capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpeechStage {
    /// Speech-to-text (transcription).
    Stt,
    /// Text-to-speech (synthesis).
    Tts,
}

impl SpeechStage {
    /// Plain user-facing stage name (lowercase, sentence use).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Stt => "speech recognition",
            Self::Tts => "speech synthesis",
        }
    }

    /// Title-cased stage name for sentence-initial/row use.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Stt => "Speech recognition",
            Self::Tts => "Speech synthesis",
        }
    }
}

/// Per-stage execution policy, persisted in the `[voice]` scope as
/// `stt_compute` / `tts_compute` (`cpu` | `automatic` | `npu`).
/// The default is `Cpu`, preserving every pre-policy saved selection
/// and today's CPU pipeline byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StageExecutionPolicy {
    /// Use the configured CPU route. Never touch an accelerator.
    #[default]
    Cpu,
    /// Use a verified compatible accelerator route for this stage when
    /// one is ready; otherwise the compatible CPU route, with the
    /// actual selection and reason exposed. No implicit installation.
    AutomaticAccelerator,
    /// Dispatch only to the specified verified accelerator route. A
    /// missing/unready route is a stage-specific refusal, never a CPU
    /// dispatch labeled as success.
    NpuRequired,
}

impl StageExecutionPolicy {
    /// The persisted value token.
    #[must_use]
    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::AutomaticAccelerator => "automatic",
            Self::NpuRequired => "npu",
        }
    }

    /// Parses a persisted token.
    #[must_use]
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value {
            "cpu" => Some(Self::Cpu),
            "automatic" => Some(Self::AutomaticAccelerator),
            "npu" => Some(Self::NpuRequired),
            _ => None,
        }
    }

    /// Plain user-facing policy name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::AutomaticAccelerator => "Automatic (compatible acceleration only)",
            Self::NpuRequired => "NPU required",
        }
    }
}

/// One evidence-based readiness fact for an accelerator route on this
/// machine. These states are deliberately distinct: a label, a device
/// file, or a registered runtime proves none of the later ones. A GPU
/// is not an NPU; stack validation is not model support; support is
/// not verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceleratorReadiness {
    /// No accelerator route for this stage is registered in this build.
    /// The honest absence state: nothing to probe, nothing to offer.
    NoRouteRegistered,
    /// This build/OS/architecture/device combination is not supported
    /// by any known route (e.g. the platform has no compatible route).
    Unsupported {
        /// Bounded, vendor-neutral reason.
        detail: BoundedString<256>,
    },
    /// No matching device was found through bounded passive inspection.
    /// Permission-denied or unknown discovery must NOT use this state.
    DeviceAbsent,
    /// Discovery was denied, timed out, or returned an unusable result.
    /// Never treated as positive eligibility.
    DetectionUnknown {
        /// Bounded, vendor-neutral reason.
        detail: BoundedString<256>,
    },
    /// Device present; a compatible runtime/backend is missing. Guided
    /// setup may be offered only where genuinely supported.
    SetupRequired {
        /// Route backend identity (bounded).
        backend: BoundedString<64>,
        /// Bounded, vendor-neutral reason.
        detail: BoundedString<256>,
    },
    /// The runtime exists but does not support this stage's model or
    /// voice/language.
    ModelUnsupported {
        /// Route backend identity.
        backend: BoundedString<64>,
        /// The unsupported model/voice identity.
        model: BoundedString<128>,
    },
    /// Required assets are absent, or setup consent has not been given.
    /// No download or offload happens in this state.
    AssetsMissing {
        /// Route backend identity.
        backend: BoundedString<64>,
        /// Bounded, vendor-neutral reason.
        detail: BoundedString<256>,
    },
    /// Installation/verification started but has not completed; the
    /// route is not selectable for normal accelerated execution.
    VerificationPending {
        /// Route backend identity.
        backend: BoundedString<64>,
    },
    /// A previously verified route is currently busy, unhealthy, or no
    /// longer compatible. Eligible for pre-dispatch CPU selection under
    /// Automatic; a refusal under strict policy.
    Unavailable {
        /// Route backend identity.
        backend: BoundedString<64>,
        /// Bounded, vendor-neutral reason.
        detail: BoundedString<256>,
    },
    /// A bounded real-model verification for exactly this stage, model,
    /// and machine succeeded. The only state that permits accelerated
    /// dispatch.
    Ready {
        /// Route backend identity.
        backend: BoundedString<64>,
        /// The verified model identity.
        model: BoundedString<128>,
    },
}

impl AcceleratorReadiness {
    /// Whether this fact permits accelerated dispatch. Only
    /// [`Self::Ready`] does.
    #[must_use]
    pub fn permits_acceleration(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// The backend a resolved stage will actually execute on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageBackend {
    /// The compatible local CPU route.
    Cpu,
    /// A verified accelerator route for this stage.
    Accelerator {
        /// Route backend identity.
        backend: BoundedString<64>,
        /// Verified model identity.
        model: BoundedString<128>,
    },
}

impl StageBackend {
    /// Plain user-facing backend name.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Cpu => "CPU",
            // A bounded string is not 'static; expose through Display of
            // the decision instead where the backend id matters.
            Self::Accelerator { backend, .. } => backend.as_str(),
        }
    }
}

/// The resolved decision for one stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRouteDecision {
    /// The stage this decision is for.
    pub stage: SpeechStage,
    /// The user's requested policy (unchanged by resolution).
    pub policy: StageExecutionPolicy,
    /// The backend that will actually execute.
    pub backend: StageBackend,
    /// Why this backend was selected (bounded, metadata only — never
    /// input audio/text).
    pub reason: BoundedString<256>,
}

/// The outcome of resolving one stage under its policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageResolution {
    /// Execute on this backend (CPU or a verified accelerator).
    Execute(StageRouteDecision),
    /// The strict policy cannot run on this machine. The refusal names
    /// the exact blocker and the Settings action; CPU execution must
    /// never be reported as satisfying this request. The saved policy
    /// is explained, not silently rewritten.
    Refused {
        /// The stage that refused.
        stage: SpeechStage,
        /// The strict policy that could not be honored.
        policy: StageExecutionPolicy,
        /// The evidence-based blocker.
        blocker: AcceleratorReadiness,
        /// Stage-specific message including the Settings action.
        message: BoundedString<512>,
    },
}

fn bounded(text: impl AsRef<str>) -> BoundedString<256> {
    // Resolver-internal reasons are all short literals; truncation of a
    // caller-supplied readiness detail is bounded and honest.
    BoundedString::new(text.as_ref()).unwrap_or_else(|_| BoundedString::new("").expect("fits"))
}

fn refusal_message(
    stage: SpeechStage,
    blocker: &AcceleratorReadiness,
    route_hint: Option<&str>,
) -> BoundedString<512> {
    let stage_label = stage.title();
    let route = route_hint
        .map(|hint| format!(" (requested route: {hint})"))
        .unwrap_or_default();
    let blocker_text = match blocker {
        AcceleratorReadiness::NoRouteRegistered => {
            "no speech accelerator route is registered in this build".to_owned()
        }
        AcceleratorReadiness::Unsupported { detail } => {
            format!("this system is not supported: {detail}")
        }
        AcceleratorReadiness::DeviceAbsent => {
            "no compatible speech accelerator was found on this machine".to_owned()
        }
        AcceleratorReadiness::DetectionUnknown { detail } => {
            format!("accelerator detection did not produce a usable result: {detail}")
        }
        AcceleratorReadiness::SetupRequired { backend, detail } => {
            format!("the {backend} runtime is missing: {detail}")
        }
        AcceleratorReadiness::ModelUnsupported { backend, model } => {
            format!("the {backend} runtime does not support this stage's model ({model})")
        }
        AcceleratorReadiness::AssetsMissing { backend, detail } => {
            format!("required assets for {backend} are missing: {detail}")
        }
        AcceleratorReadiness::VerificationPending { backend } => {
            format!("the {backend} route has not finished verification")
        }
        AcceleratorReadiness::Unavailable { backend, detail } => {
            format!("the {backend} route is busy or unhealthy: {detail}")
        }
        AcceleratorReadiness::Ready { .. } => "verified".to_owned(),
    };
    let text = format!(
        "{stage_label} is set to NPU required{route}, but it cannot run here: {blocker_text}. \
         Open Settings → Voice → Execution to choose CPU for this stage, or complete the \
         supported setup when one is available. CPU execution will not be labeled as \
         satisfying this request."
    );
    BoundedString::new(&text).unwrap_or_else(|_| BoundedString::new("").expect("fits"))
}

/// Resolves one stage's execution from its policy and one evidence-based
/// readiness fact. Pure; hosts own how readiness was derived.
///
/// CPU policy ignores readiness entirely — the caller need not even
/// inspect the accelerator (and must not initialize or benchmark it).
#[must_use]
pub fn resolve_stage(
    stage: SpeechStage,
    policy: StageExecutionPolicy,
    readiness: &AcceleratorReadiness,
) -> StageResolution {
    match policy {
        StageExecutionPolicy::Cpu => StageResolution::Execute(StageRouteDecision {
            stage,
            policy,
            backend: StageBackend::Cpu,
            reason: bounded("explicit CPU policy: the accelerator path is not consulted"),
        }),
        StageExecutionPolicy::AutomaticAccelerator => match readiness {
            AcceleratorReadiness::Ready { backend, model } => {
                StageResolution::Execute(StageRouteDecision {
                    stage,
                    policy,
                    backend: StageBackend::Accelerator {
                        backend: backend.clone(),
                        model: model.clone(),
                    },
                    reason: bounded(format!(
                        "automatic: verified accelerator route {backend} for this stage"
                    )),
                })
            }
            other => StageResolution::Execute(StageRouteDecision {
                stage,
                policy,
                backend: StageBackend::Cpu,
                reason: bounded(format!(
                    "automatic: no verified accelerator route ({}) — using the compatible CPU route",
                    automatic_reason(other)
                )),
            }),
        },
        StageExecutionPolicy::NpuRequired => match readiness {
            AcceleratorReadiness::Ready { backend, model } => {
                StageResolution::Execute(StageRouteDecision {
                    stage,
                    policy,
                    backend: StageBackend::Accelerator {
                        backend: backend.clone(),
                        model: model.clone(),
                    },
                    reason: bounded(format!(
                        "strict: verified accelerator route {backend} for this stage"
                    )),
                })
            }
            other => {
                let route_hint = match other {
                    AcceleratorReadiness::SetupRequired { backend, .. }
                    | AcceleratorReadiness::ModelUnsupported { backend, .. }
                    | AcceleratorReadiness::AssetsMissing { backend, .. }
                    | AcceleratorReadiness::VerificationPending { backend }
                    | AcceleratorReadiness::Unavailable { backend, .. }
                    | AcceleratorReadiness::Ready { backend, .. } => Some(backend.as_str()),
                    _ => None,
                };
                StageResolution::Refused {
                    stage,
                    policy,
                    blocker: other.clone(),
                    message: refusal_message(stage, other, route_hint),
                }
            }
        },
    }
}

fn automatic_reason(readiness: &AcceleratorReadiness) -> &'static str {
    match readiness {
        AcceleratorReadiness::NoRouteRegistered => "no route registered",
        AcceleratorReadiness::Unsupported { .. } => "system not supported",
        AcceleratorReadiness::DeviceAbsent => "no compatible device found",
        AcceleratorReadiness::DetectionUnknown { .. } => "detection unusable",
        AcceleratorReadiness::SetupRequired { .. } => "runtime missing",
        AcceleratorReadiness::ModelUnsupported { .. } => "model unsupported",
        AcceleratorReadiness::AssetsMissing { .. } => "assets missing",
        AcceleratorReadiness::VerificationPending { .. } => "verification pending",
        AcceleratorReadiness::Unavailable { .. } => "busy or unhealthy",
        AcceleratorReadiness::Ready { .. } => "ready",
    }
}

/// The identity a cached readiness fact is valid for. A cache hit
/// requires *every* component to match: machine, stage, backend,
/// model, and configuration. Copied success across machines is
/// structurally impossible because hosts never persist this structure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReadinessCacheKey {
    /// Machine identity supplied by the host (never persisted).
    pub machine: BoundedString<128>,
    /// The stage the fact applies to.
    pub stage: SpeechStage,
    /// Route backend identity.
    pub backend: BoundedString<64>,
    /// Model/voice identity the fact was established for.
    pub model: BoundedString<128>,
    /// Configuration identity the fact was established under.
    pub configuration: BoundedString<256>,
}

/// Events that invalidate cached readiness regardless of key match.
/// Detection is not validation: a later setup change, backend failure,
/// environment change, device loss, or resume invalidates prior facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessInvalidation {
    /// Setup completed or changed for some route.
    SetupChanged,
    /// A route's setup/assets were removed.
    SetupRemoved,
    /// A backend failed after verification.
    BackendFailure,
    /// A meaningful environment change (PATH/runtime layout).
    EnvironmentChanged,
    /// The device was lost or disappeared.
    DeviceLost,
    /// Process resume after suspension.
    Resume,
}

/// Bounded in-process readiness cache. Never persisted; never shared
/// across machines. Lookup is exact-key; any recorded invalidation
/// clears everything (the safe, cheap policy — re-deriving readiness
/// from passive checks is bounded and read-only).
#[derive(Debug, Default)]
pub struct ReadinessCache {
    entries: BTreeMap<SpeechStage, (ReadinessCacheKey, AcceleratorReadiness)>,
}

impl ReadinessCache {
    /// Creates an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one fact under its full identity key.
    pub fn record(&mut self, key: ReadinessCacheKey, readiness: AcceleratorReadiness) {
        self.entries.insert(key.stage, (key, readiness));
    }

    /// Returns the cached fact only when the *entire* current key
    /// matches; otherwise `None` (the host re-derives locally).
    #[must_use]
    pub fn lookup(&self, current: &ReadinessCacheKey) -> Option<AcceleratorReadiness> {
        self.entries
            .get(&current.stage)
            .filter(|(cached_key, _)| cached_key == current)
            .map(|(_, readiness)| readiness.clone())
    }

    /// Applies one invalidation event: every fact is cleared (setup
    /// changes, backend failures, device loss, and resume can affect
    /// any route; exact per-route attribution would require trusting
    /// the very evidence being invalidated).
    pub fn invalidate(&mut self, _event: ReadinessInvalidation) {
        self.entries.clear();
    }

    /// Number of cached facts (bounded by the stage count).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no facts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Honest attribution of where a completed accelerated request actually
/// executed. A successful request, a backend's claim, or a device
/// utilization spike is not whole-pipeline proof; a CPU-executed graph
/// behind an NPU-labeled call is `Cpu`/`Hybrid`, never a fabricated
/// full success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffloadPlacement {
    /// The request ran on the CPU path.
    Cpu,
    /// Backend execution evidence covers the whole request for this
    /// stage.
    Full {
        /// Route backend identity.
        backend: BoundedString<64>,
    },
    /// Backend evidence covers only part of the graph (e.g. an
    /// accelerated encoder with a CPU decoder).
    Hybrid {
        /// Route backend identity.
        backend: BoundedString<64>,
        /// Bounded description of the split.
        detail: BoundedString<256>,
    },
    /// The backend claimed acceleration but per-request execution
    /// evidence is absent. The claim is recorded, never promoted.
    Unverified {
        /// Route backend identity.
        backend: BoundedString<64>,
    },
}

/// Evidence used to attribute placement for one completed request.
#[derive(Debug, Clone, Copy)]
pub struct PlacementEvidence<'a> {
    /// The route the request was dispatched to (empty = CPU path).
    pub backend: Option<&'a str>,
    /// The backend's own claim that the graph was offloaded.
    pub backend_claimed_offload: bool,
    /// Per-request evidence that device execution occurred (e.g.
    /// backend execution counters for this request).
    pub device_execution_evidence: bool,
    /// Per-request evidence that part of the graph executed on CPU.
    pub cpu_execution_evidence: bool,
}

/// Derives honest placement from evidence. Pure.
#[must_use]
pub fn attribute_placement(evidence: &PlacementEvidence<'_>) -> OffloadPlacement {
    let Some(backend) = evidence.backend else {
        return OffloadPlacement::Cpu;
    };
    let backend =
        BoundedString::new(backend).unwrap_or_else(|_| BoundedString::new("").expect("fits"));
    if evidence.device_execution_evidence {
        if evidence.cpu_execution_evidence {
            OffloadPlacement::Hybrid {
                backend,
                detail: BoundedString::new(
                    "backend execution evidence covers only part of the graph",
                )
                .unwrap_or_else(|_| BoundedString::new("").expect("fits")),
            }
        } else {
            OffloadPlacement::Full { backend }
        }
    } else if evidence.backend_claimed_offload {
        OffloadPlacement::Unverified { backend }
    } else {
        OffloadPlacement::Cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> AcceleratorReadiness {
        AcceleratorReadiness::Ready {
            backend: BoundedString::new("route-x").unwrap(),
            model: BoundedString::new("model-a").unwrap(),
        }
    }

    fn key(stage: SpeechStage) -> ReadinessCacheKey {
        ReadinessCacheKey {
            machine: BoundedString::new("machine-a").unwrap(),
            stage,
            backend: BoundedString::new("route-x").unwrap(),
            model: BoundedString::new("model-a").unwrap(),
            configuration: BoundedString::new("cfg-1").unwrap(),
        }
    }

    #[test]
    fn cpu_policy_ignores_even_a_verified_route() {
        for stage in [SpeechStage::Stt, SpeechStage::Tts] {
            let resolution = resolve_stage(
                stage,
                StageExecutionPolicy::Cpu,
                &AcceleratorReadiness::Ready {
                    backend: BoundedString::new("route-x").unwrap(),
                    model: BoundedString::new("model-a").unwrap(),
                },
            );
            let StageResolution::Execute(decision) = resolution else {
                panic!("CPU policy must execute");
            };
            assert_eq!(decision.backend, StageBackend::Cpu);
            assert!(decision.reason.as_str().contains("explicit CPU policy"));
        }
    }

    #[test]
    fn automatic_uses_only_verified_routes() {
        let facts = vec![
            AcceleratorReadiness::NoRouteRegistered,
            AcceleratorReadiness::Unsupported {
                detail: BoundedString::new("no route for this platform").unwrap(),
            },
            AcceleratorReadiness::DeviceAbsent,
            AcceleratorReadiness::DetectionUnknown {
                detail: BoundedString::new("probe denied").unwrap(),
            },
            AcceleratorReadiness::SetupRequired {
                backend: BoundedString::new("route-x").unwrap(),
                detail: BoundedString::new("runtime not installed").unwrap(),
            },
            AcceleratorReadiness::ModelUnsupported {
                backend: BoundedString::new("route-x").unwrap(),
                model: BoundedString::new("model-a").unwrap(),
            },
            AcceleratorReadiness::AssetsMissing {
                backend: BoundedString::new("route-x").unwrap(),
                detail: BoundedString::new("model file absent").unwrap(),
            },
            AcceleratorReadiness::VerificationPending {
                backend: BoundedString::new("route-x").unwrap(),
            },
            AcceleratorReadiness::Unavailable {
                backend: BoundedString::new("route-x").unwrap(),
                detail: BoundedString::new("device busy").unwrap(),
            },
        ];
        for fact in facts {
            let resolution = resolve_stage(
                SpeechStage::Stt,
                StageExecutionPolicy::AutomaticAccelerator,
                &fact,
            );
            let StageResolution::Execute(decision) = resolution else {
                panic!("automatic must keep CPU available for {fact:?}");
            };
            assert_eq!(
                decision.backend,
                StageBackend::Cpu,
                "automatic must not accelerate on {fact:?}"
            );
            assert!(
                decision.reason.as_str().contains("no verified accelerator"),
                "reason must expose the actual condition: {}",
                decision.reason
            );
        }
        let resolution = resolve_stage(
            SpeechStage::Stt,
            StageExecutionPolicy::AutomaticAccelerator,
            &ready(),
        );
        let StageResolution::Execute(decision) = resolution else {
            panic!("verified route must execute");
        };
        assert!(matches!(decision.backend, StageBackend::Accelerator { .. }));
    }

    #[test]
    fn strict_policy_refuses_every_unready_fact_and_names_the_blocker() {
        let facts = vec![
            (
                AcceleratorReadiness::NoRouteRegistered,
                "no speech accelerator route is registered",
            ),
            (
                AcceleratorReadiness::DeviceAbsent,
                "no compatible speech accelerator was found",
            ),
            (
                AcceleratorReadiness::DetectionUnknown {
                    detail: BoundedString::new("timeout").unwrap(),
                },
                "detection did not produce a usable result",
            ),
            (
                AcceleratorReadiness::SetupRequired {
                    backend: BoundedString::new("route-x").unwrap(),
                    detail: BoundedString::new("runtime not installed").unwrap(),
                },
                "route-x runtime is missing",
            ),
            (
                AcceleratorReadiness::VerificationPending {
                    backend: BoundedString::new("route-x").unwrap(),
                },
                "has not finished verification",
            ),
            (
                AcceleratorReadiness::Unavailable {
                    backend: BoundedString::new("route-x").unwrap(),
                    detail: BoundedString::new("busy").unwrap(),
                },
                "busy or unhealthy",
            ),
        ];
        for (fact, expected) in facts {
            let resolution =
                resolve_stage(SpeechStage::Tts, StageExecutionPolicy::NpuRequired, &fact);
            let StageResolution::Refused {
                stage,
                policy,
                blocker,
                message,
            } = resolution
            else {
                panic!("strict policy must refuse on {fact:?}");
            };
            assert_eq!(stage, SpeechStage::Tts);
            assert_eq!(policy, StageExecutionPolicy::NpuRequired);
            assert_eq!(blocker, fact);
            assert!(message.as_str().contains(expected), "message: {message}");
            assert!(
                message.as_str().contains("Settings → Voice → Execution"),
                "must name the Settings action: {message}"
            );
        }
    }

    #[test]
    fn strict_policy_executes_only_on_verified_routes() {
        let resolution = resolve_stage(
            SpeechStage::Tts,
            StageExecutionPolicy::NpuRequired,
            &ready(),
        );
        let StageResolution::Execute(decision) = resolution else {
            panic!("verified route must execute under strict policy");
        };
        assert!(matches!(decision.backend, StageBackend::Accelerator { .. }));
    }

    #[test]
    fn stages_resolve_independently() {
        // STT verified + TTS no-route: STT may accelerate while TTS stays
        // on CPU under automatic — no shared boolean promotes both.
        let stt = resolve_stage(
            SpeechStage::Stt,
            StageExecutionPolicy::AutomaticAccelerator,
            &ready(),
        );
        let tts = resolve_stage(
            SpeechStage::Tts,
            StageExecutionPolicy::AutomaticAccelerator,
            &AcceleratorReadiness::NoRouteRegistered,
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
        // And the converse.
        let stt_cpu = resolve_stage(
            SpeechStage::Stt,
            StageExecutionPolicy::AutomaticAccelerator,
            &AcceleratorReadiness::NoRouteRegistered,
        );
        let StageResolution::Execute(stt_cpu_decision) = stt_cpu else {
            panic!()
        };
        assert_eq!(stt_cpu_decision.backend, StageBackend::Cpu);
    }

    #[test]
    fn cache_requires_exact_identity_and_invalidates_on_events() {
        let mut cache = ReadinessCache::new();
        cache.record(key(SpeechStage::Stt), ready());
        assert_eq!(cache.lookup(&key(SpeechStage::Stt)), Some(ready()));
        // Different machine: no stale success.
        let mut other_machine = key(SpeechStage::Stt);
        other_machine.machine = BoundedString::new("machine-b").unwrap();
        assert_eq!(cache.lookup(&other_machine), None);
        // Different model/config: no reuse.
        let mut other_model = key(SpeechStage::Stt);
        other_model.model = BoundedString::new("model-b").unwrap();
        assert_eq!(cache.lookup(&other_model), None);
        let mut other_config = key(SpeechStage::Stt);
        other_config.configuration = BoundedString::new("cfg-2").unwrap();
        assert_eq!(cache.lookup(&other_config), None);
        // Any invalidation event clears everything.
        cache.invalidate(ReadinessInvalidation::BackendFailure);
        assert!(cache.is_empty());
        assert_eq!(cache.lookup(&key(SpeechStage::Stt)), None);
    }

    #[test]
    fn placement_attribution_stays_honest() {
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
        // Claim without per-request device evidence: unverified, never full.
        assert_eq!(
            attribute_placement(&PlacementEvidence {
                backend: Some("route-x"),
                backend_claimed_offload: true,
                device_execution_evidence: false,
                cpu_execution_evidence: false,
            }),
            OffloadPlacement::Unverified {
                backend: BoundedString::new("route-x").unwrap()
            }
        );
        // Device evidence + CPU evidence: hybrid, not full.
        assert!(matches!(
            attribute_placement(&PlacementEvidence {
                backend: Some("route-x"),
                backend_claimed_offload: true,
                device_execution_evidence: true,
                cpu_execution_evidence: true,
            }),
            OffloadPlacement::Hybrid { .. }
        ));
        // Device evidence only: full.
        assert_eq!(
            attribute_placement(&PlacementEvidence {
                backend: Some("route-x"),
                backend_claimed_offload: true,
                device_execution_evidence: true,
                cpu_execution_evidence: false,
            }),
            OffloadPlacement::Full {
                backend: BoundedString::new("route-x").unwrap()
            }
        );
        // Dispatched to the route but ran CPU, no claim: CPU.
        assert_eq!(
            attribute_placement(&PlacementEvidence {
                backend: Some("route-x"),
                backend_claimed_offload: false,
                device_execution_evidence: false,
                cpu_execution_evidence: false,
            }),
            OffloadPlacement::Cpu
        );
    }

    #[test]
    fn policy_tokens_round_trip() {
        for policy in [
            StageExecutionPolicy::Cpu,
            StageExecutionPolicy::AutomaticAccelerator,
            StageExecutionPolicy::NpuRequired,
        ] {
            assert_eq!(
                StageExecutionPolicy::from_config_str(policy.as_config_str()),
                Some(policy)
            );
        }
        assert_eq!(StageExecutionPolicy::from_config_str("gpu"), None);
        assert_eq!(StageExecutionPolicy::default(), StageExecutionPolicy::Cpu);
    }
}
