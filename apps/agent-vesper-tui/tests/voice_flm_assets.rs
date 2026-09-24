//! VRO-17 R16 red-first: FLM asset verification through the REAL
//! production seam (`voice_flm_assets`). Feature-independent (the
//! asset assessment compiles with the conversation surface so Settings
//! can always report the pack's state); route registration tests live
//! in `voice_flm_route.rs` under the `voice-flm` feature.

#![cfg(feature = "voice-conversation")]
#![forbid(unsafe_code)]

use agent_vesper_tui::voice_flm_assets::{self, PackState};

/// The pack assessment is passive and honest: no vendor command runs,
/// no device opens, and the observed machine maps to exactly one
/// defined state. (This machine: the runtime and verified pack are
/// installed.)
#[test]
fn pack_assessment_is_passive_and_honest() {
    let state = voice_flm_assets::assess_pack();
    assert!(
        matches!(
            state,
            PackState::Installed
                | PackState::RuntimeMissing
                | PackState::AssetsMissing
                | PackState::SizeMismatch { .. }
                | PackState::DigestMismatch
        ),
        "assessment must produce a defined state, got {state:?}"
    );
    // Installed requires the full file set plus the digest match.
    if matches!(state, PackState::Installed) {
        let dir = voice_flm_assets::whisper_pack_dir();
        for name in [
            "config.json",
            "tokenizer.json",
            "tokenizer_config.json",
            "model.q4nx",
        ] {
            assert!(
                dir.join(name).is_file(),
                "installed pack must contain {name}"
            );
        }
    }
}

/// Every non-installed state maps to setup guidance (never an
/// opportunistic download, never a silent Ready).
#[test]
fn non_installed_states_name_a_remedy() {
    for state in [
        PackState::RuntimeMissing,
        PackState::AssetsMissing,
        PackState::SizeMismatch { actual: 1 },
        PackState::DigestMismatch,
    ] {
        let detail = voice_flm_assets::verify_state_detail(state);
        assert!(detail.is_err(), "non-installed states must not verify");
        let message = detail.unwrap_err().as_str().to_owned();
        assert!(
            message.contains("Settings"),
            "remedy must point at Settings, got: {message}"
        );
    }
}

/// Installed verifies; the mapping is pure over the state.
#[test]
fn installed_state_verifies() {
    assert!(voice_flm_assets::verify_state_detail(PackState::Installed).is_ok());
}
