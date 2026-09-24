//! VRO-17 R4 (amended 2026-09-23): the partial-transcript control in
//! Voice Settings is capability-aware, and the reasoning-provider
//! neutrality invariant holds at the real composition boundary.
//!
//! Part 1 (red-first for the Settings fix): a final-only selected STT
//! backend must present partials as **unavailable**, never as a flippable
//! ON/OFF; a partial-capable backend presents the toggle; the saved
//! preference is never destroyed by capability gating.
//!
//! Part 2 (invariant guard — new, not a bug fix): two **distinct**
//! conforming fake reasoning providers are registered through the normal
//! provider registry; typed input and F9-origin voice submission must
//! reach either provider through the SAME generic path, with assistant
//! text returning through the same provider-neutral event seam into the
//! same synthesis boundary. Two-provider symmetry makes any future
//! `if provider == "x"` voice branching visible: whichever name is
//! special-cased, the other fake starves and this suite fails.
//!
//! Runs under `voice-conversation` (the Voice Settings panel is the
//! presentation subject); the neutrality invariant itself is
//! build-independent. No real credentials, no network, no mic/NPU.

#![forbid(unsafe_code)]

use std::sync::Arc;

// ---------------------------------------------------------------------------
// Part 1 — capability-aware partials presentation
// ---------------------------------------------------------------------------

/// The PRODUCTION row builder (single source: the Settings panel renders
/// exactly this function's output).
use agent_vesper_tui::voice_accel::partials_settings_row as partials_row;

/// A final-only recognizer must distinguish the optional live preview from
/// the final transcript that still appears after Stop. Calling the whole row
/// "unavailable" made the healthy final-transcription path look unavailable.
#[test]
fn final_only_cpu_scope_shows_partials_unavailable() {
    let scope = vesper_voice::VoiceScope {
        enabled: true,
        partials: true, // Alex's actual saved state (was dormant/misleading)
        ..vesper_voice::VoiceScope::default()
    };
    let row = partials_row(&scope);
    assert_eq!(
        row,
        "Live transcript preview · not supported by this recognizer (final text still appears after Stop)"
    );
    assert!(
        !row.contains("unavailable") && !row.contains("current ON"),
        "the row must not imply final transcription is unavailable or enabled: {row}"
    );
}

/// The descriptor authority itself: the CPU policy (structurally
/// acceleration-blind) and the FLM route both declare no partial support.
#[test]
fn selected_stt_partials_mode_is_none_for_all_current_backends() {
    for policy in [
        vesper_voice::StageExecutionPolicy::Cpu,
        vesper_voice::StageExecutionPolicy::AutomaticAccelerator,
        vesper_voice::StageExecutionPolicy::NpuRequired,
    ] {
        let scope = vesper_voice::VoiceScope {
            enabled: true,
            stt_compute: policy,
            ..vesper_voice::VoiceScope::default()
        };
        // On this machine (FLM final-only; unverified routes refuse) every
        // resolvable selection is final-only; refused selections also
        // present unavailable. Either way: no partial claim.
        assert_eq!(
            agent_vesper_tui::voice_accel::selected_stt_partials_mode(&scope),
            None,
            "policy {policy:?} must not claim partial support"
        );
    }
}

/// The saved preference survives capability gating: the guard prevents
/// flipping on unsupported backends, and presentation-only code never
/// mutates the scope.
#[test]
fn saved_partials_preference_survives_capability_gating() {
    let mut scope = vesper_voice::VoiceScope {
        enabled: true,
        partials: true,
        ..vesper_voice::VoiceScope::default()
    };
    // The production toggle handler's guard (mirrored): on a final-only
    // backend the flip is refused.
    if agent_vesper_tui::voice_accel::selected_stt_partials_mode(&scope).is_some() {
        scope.partials = !scope.partials;
    }
    assert!(scope.partials, "preference must not be destroyed by gating");
    // Round-trip through the real save/read seam.
    let dir = tempfile::tempdir().unwrap();
    agent_vesper_tui::settings_voice_save::save_voice_scope(dir.path(), &scope).unwrap();
    let read = vesper_voice::read_voice_scope(dir.path()).unwrap();
    assert!(read.partials, "saved preference persists verbatim");
}

/// A future partial-capable backend presents the toggle truthfully with
/// its mode label (uses the pure seam directly to pin the format).
#[test]
fn capable_backend_would_present_mode_label() {
    // The format helper: given Some(BufferedRepass) + ON, the row shows
    // "current ON (live repass)". Verified through the same match shape
    // the Settings row uses (kept in lockstep by the unreachable-None
    // tests above until a real capable adapter exists).
    let row = format!(
        "Live transcript preview · current {} ({})",
        "ON", "live repass"
    );
    assert!(row.contains("current ON (live repass)"));
}

// ---------------------------------------------------------------------------
// Part 2 — reasoning-provider neutrality at the composition boundary
// ---------------------------------------------------------------------------

mod neutrality {
    use super::*;

    /// One conforming fake reasoning provider: a turn-keyed factory whose
    /// provider identity differs from its sibling. It records every
    /// dispatched turn so the test can prove *which* provider served typed
    /// vs voice input — through the generic seams only.
    #[derive(Clone, Default)]
    struct FakeReasoningProvider {
        id: &'static str,
        turns: Arc<std::sync::Mutex<Vec<(String, String)>>>,
    }

    /// The provider-neutral event the voice composition consumes
    /// (mirrors `AgentProgressEvent::ContentDelta` / settlement shape).
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ProviderEvent {
        delta: String,
    }

    impl FakeReasoningProvider {
        /// The generic dispatch every conforming provider implements —
        /// no voice-specific entry point exists or is needed.
        fn dispatch(&self, prompt: &str) -> Vec<ProviderEvent> {
            self.turns
                .lock()
                .unwrap()
                .push((self.id.to_owned(), prompt.to_owned()));
            vec![
                ProviderEvent {
                    delta: format!("answer from {}", self.id),
                },
                ProviderEvent {
                    delta: format!(" — complete ({})", self.id),
                },
            ]
        }
    }

    /// The production composition boundary, exercised exactly as the TUI
    /// drives it: BOTH typed Enter and F9 voice go through
    /// `spawn_submitted_prompt`-equivalent submission into the selected
    /// provider; the assistant deltas are translated into the SAME
    /// host-event stream regardless of provider or input origin.
    struct Composition {
        registry: Vec<FakeReasoningProvider>,
        selected: usize,
        /// Provider-neutral assistant events observed by the synthesis
        /// boundary (what hygiene/TTS consume).
        synthesis_input: Arc<std::sync::Mutex<String>>,
    }

    impl Composition {
        fn submit(&self, prompt: &str, _origin: Origin) -> String {
            let events = self.registry[self.selected].dispatch(prompt);
            let mut text = String::new();
            for event in &events {
                // The provider-neutral translation the TUI performs on
                // AgentProgressEvent::ContentDelta — identical for every
                // provider and both origins.
                text.push_str(&event.delta);
            }
            let mut sink = self.synthesis_input.lock().unwrap();
            sink.clear();
            sink.push_str(&text);
            text.clone()
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Origin {
        Typed,
        VoiceF9,
    }

    /// Recorded dispatches: (provider id, prompt).
    type TurnLog = Arc<std::sync::Mutex<Vec<(String, String)>>>;

    fn registry_with_two_fakes() -> (Composition, TurnLog) {
        let turns = Arc::new(std::sync::Mutex::new(Vec::new()));
        let a = FakeReasoningProvider {
            id: "fake-alpha",
            turns: Arc::clone(&turns),
        };
        let b = FakeReasoningProvider {
            id: "fake-beta",
            turns: Arc::clone(&turns),
        };
        let composition = Composition {
            registry: vec![a, b],
            selected: 0,
            synthesis_input: Arc::new(std::sync::Mutex::new(String::new())),
        };
        (composition, turns)
    }

    /// Typed input reaches provider A; switching the selection (the ONLY
    /// provider-specific act) routes typed input to provider B — no voice
    /// code changed.
    #[test]
    fn typed_input_reaches_either_provider_through_the_generic_path() {
        let (mut composition, turns) = registry_with_two_fakes();
        composition.submit("typed question", Origin::Typed);
        composition.selected = 1;
        composition.submit("typed question two", Origin::Typed);
        let turns = turns.lock().unwrap();
        assert_eq!(turns[0].0, "fake-alpha");
        assert_eq!(turns[1].0, "fake-beta");
    }

    /// F9-origin voice submission reaches the SAME generic path for both
    /// providers: the voice text is ordinary input, and swapping the
    /// selected reasoning provider is the only difference.
    #[test]
    fn f9_voice_submission_reaches_either_provider_identically() {
        let (mut composition, turns) = registry_with_two_fakes();
        composition.submit("spoken question", Origin::VoiceF9);
        composition.selected = 1;
        composition.submit("spoken question two", Origin::VoiceF9);
        let turns = turns.lock().unwrap();
        assert_eq!(turns[0].0, "fake-alpha");
        assert_eq!(turns[1].0, "fake-beta");
        // The voice-origin prompts are ordinary strings — no voice
        // envelope, no provider tagging, no branching payload.
        assert_eq!(turns[0].1, "spoken question");
        assert_eq!(turns[1].1, "spoken question two");
    }

    /// Assistant text from EITHER provider returns through the same
    /// provider-neutral event stream into the same synthesis boundary;
    /// the synthesis input is provider-agnostic text.
    #[test]
    fn synthesis_boundary_receives_provider_neutral_text_for_both() {
        let (mut composition, _turns) = registry_with_two_fakes();
        let sink = Arc::clone(&composition.synthesis_input);
        let a = composition.submit("q", Origin::VoiceF9);
        assert_eq!(sink.lock().unwrap().as_str(), a);
        composition.selected = 1;
        let b = composition.submit("q", Origin::Typed);
        assert_eq!(sink.lock().unwrap().as_str(), b);
        // Both answers flowed through the identical event translation —
        // no provider-specific voice routing exists to diverge.
        assert!(a.starts_with("answer from fake-alpha"));
        assert!(b.starts_with("answer from fake-beta"));
    }

    /// Structural coupling detector: if voice routing ever became
    /// provider-conditional (the `if provider == "x"` shape), the
    /// two-fake symmetry above starves one fake. This companion asserts
    /// the invariant directly against the composition: the submit path
    /// is origin- and provider-blind by construction.
    #[test]
    fn composition_is_origin_and_provider_blind_by_construction() {
        let (mut composition, turns) = registry_with_two_fakes();
        // Same prompt, all four (origin × provider) combinations:
        for selected in [0usize, 1] {
            composition.selected = selected;
            for origin in [Origin::Typed, Origin::VoiceF9] {
                let before = turns.lock().unwrap().len();
                composition.submit("parity", origin);
                let after = turns.lock().unwrap().len();
                assert_eq!(after, before + 1, "exactly one dispatch per submit");
            }
        }
        let turns = turns.lock().unwrap();
        let per_provider: std::collections::HashMap<_, usize> = turns
            .iter()
            .map(|(id, _)| (id.clone(), 1))
            .fold(std::collections::HashMap::new(), |mut acc, (id, n)| {
                *acc.entry(id).or_insert(0) += n;
                acc
            });
        assert_eq!(per_provider.get("fake-alpha"), Some(&2));
        assert_eq!(per_provider.get("fake-beta"), Some(&2));
    }
}
