#![forbid(unsafe_code)]
//! `agent-vesper-tui` — Stage 11b Terminal User Interface for Agent Vesper.
//!
//! The crate owns the provider-superpowers discovery layer, the Plan Mode
//! state machine, the slash-command registry, and the terminal renderer
//! abstraction. The runtime stays provider-neutral; the TUI is a composition
//! boundary that queries the runtime's `ProviderRegistry` for superpowers at
//! startup and renders the active provider's controls natively.
//!
//! ## Layout
//!
//! - [`plan_mode`] — pure 4-phase Plan Mode state machine (NORMAL → PLANNING
//!   → REVIEW → EXECUTING) mirroring the Python oracle's `PLAN_MODE_PROMPT`.
//! - [`capabilities`] — fail-closed per-model capability index
//!   (`ModelCapabilityIndex`) over the active provider's catalog snapshot;
//!   gates image input, adviser eligibility, and advertised effort levels
//!   (PRD `docs/provider-capability-gating-prd.md`).
//! - [`commands`] — slash-command parsing, registry, and resolution.
//! - [`superpowers`] — provider-native superpower surface + override store.
//! - [`dispatch`] — pure event-loop dispatch: the bridge between the command
//!   registry, the Plan Mode state machine, and the override store. Terminal-
//!   free so the full lifecycle is unit-testable.
//! - [`ui`] — `TerminalRenderer` trait + `ratatui`/`crossterm` backend.
//! - [`markdown`] — streaming-safe markdown → `ratatui` `Line` renderer used
//!   by the Conversation and Reasoning panels.
//!
//! ## DOX
//!
//! See `apps/agent-vesper-tui/AGENTS.md` for purpose, ownership, contracts,
//! and verification.

mod activity;
pub mod auth_hub;
pub mod capabilities;
pub mod commands;
pub mod dispatch;
pub mod landing;
pub mod lmstudio_hub;
pub mod lmstudio_provider;
pub mod markdown;
pub mod plan_mode;
mod presentation;
pub mod provider_hub;
pub mod settings_menu;
pub mod superpowers;
#[cfg(feature = "swarm")]
pub mod swarm_hub;
pub mod ui;
#[cfg(feature = "voice-conversation")]
pub mod voice_accel;
// R20 (2026-09-23 repair): the capture store is the shared capture-safety
// primitive for BOTH dictation (F5, default builds) and conversation
// (feature builds) — it is compiled ungated so the default F5 path owns
// its captures through it too. Pure std; no conversation behavior is
// pulled into the default path.
pub mod voice_capture_store;
#[cfg(feature = "voice-conversation")]
pub mod voice_conversation;
#[cfg(feature = "voice-conversation")]
pub mod voice_playback;
#[cfg(feature = "voice-conversation")]
pub mod voice_readiness;
#[cfg(feature = "voice-conversation")]
#[path = "voice_speech_worker.rs"]
pub mod voice_speech_worker;
pub mod web_hub;
pub use auth_hub::{
    AuthHubAction, AuthHubState, AuthProvider, StartupRoute, render_auth_hub, startup_route,
};
pub use capabilities::{CapabilityDenial, ModelCapabilityIndex};
pub use commands::{
    CommandIntent, CommandOutcome, CommandRegistry, DEFAULT_INTERVIEW_QUESTION_LIMIT,
    InterviewQuestionLimit, MAX_INTERVIEW_QUESTIONS, MediaOp, PlanGesture,
};
pub use dispatch::{
    DispatchOutcome, PanelVisibility, SessionControls, SessionState, TaskItem, TerminalAction,
    TerminalPreferences, apply_model_plan, apply_task_plan, dispatch,
};
pub use lmstudio_hub::{
    LmStudioHub, LmStudioSettings, LmStudioSettingsAction, load_lmstudio_settings,
    render_lmstudio_hub, save_lmstudio_settings,
};
pub use lmstudio_provider::LmStudioFactory;
pub use plan_mode::{PendingQuestion, PlanModeError, PlanPhase, PlanState, PlanTransition};
pub use superpowers::{ProviderSuperpowerSurface, SuperpowerOverrides};
pub use ui::{
    FOOTER_ACTIONS, PermissionChoice, PermissionModal, ReasoningDiagnostics, StubRenderer,
    TerminalRenderer, UiLayoutMetrics, ViewModel, command_menu_height, footer_action_at,
    footer_action_rows, render_to_frame, ui_layout_metrics,
};

use vesper_domain::ProviderId;
use vesper_provider::{ProviderDescriptor, SuperpowerDescriptor};
use vesper_runtime::ProviderRegistry;

/// Snapshot of everything the TUI needs to know at startup about the
/// composition boundary's selected provider.
#[derive(Debug, Clone)]
pub struct StartupView {
    /// Active provider identity.
    pub provider_id: ProviderId,
    /// Superpower descriptors the active provider advertised.
    pub superpowers: Vec<SuperpowerDescriptor>,
    /// Provider-routed authentication descriptor projected from the active
    /// provider's advertised `ProviderDescriptor`. `None` when the provider
    /// advertised no API-key auth method. Hosts render the auth UI from this
    /// instead of hardcoding provider match arms.
    pub auth: Option<AuthProvider>,
}

/// Projects a provider's advertised descriptor into the TUI's auth descriptor,
/// using the first API-key method's first secret-reference field as the
/// environment variable. Returns `None` when the provider advertised no auth
/// method or no secret-reference field.
#[must_use]
pub fn auth_provider_from_descriptor(descriptor: &ProviderDescriptor) -> Option<AuthProvider> {
    let method = descriptor
        .authentication_methods
        .iter()
        .find(|method| !method.secret_reference_fields.is_empty())?;
    let environment_variable = method.secret_reference_fields.first()?;
    Some(AuthProvider {
        id: descriptor.provider_id.as_str().to_owned(),
        name: method.display_name.as_str().to_owned(),
        environment_variable: environment_variable.as_str().to_owned(),
        key_url: method
            .key_url
            .as_ref()
            .map(|url| url.as_str().to_owned())
            .unwrap_or_default(),
    })
}

/// Queries the runtime registry for the superpowers advertised by
/// `provider_id`. Returns an empty descriptor list when the provider is
/// unknown or registered without superpowers.
///
/// This is the single integration point between the TUI and the runtime; it
/// keeps the rest of the crate free of any concrete provider dependency.
pub async fn query_startup_view(
    registry: &ProviderRegistry,
    provider_id: &ProviderId,
) -> StartupView {
    let superpowers = registry.superpowers(provider_id).await;
    let auth = registry
        .descriptor(provider_id)
        .await
        .as_ref()
        .and_then(auth_provider_from_descriptor);
    StartupView {
        provider_id: provider_id.clone(),
        superpowers,
        auth,
    }
}

/// Resolve the harness-owned voice backend venv root. Precedence:
/// `$AGENT_VESPER_VOICE_VENV` → `$XDG_DATA_HOME/agent-vesper/voice-venv` →
/// `~/.local/share/agent-vesper/voice-venv` (auto-bootstrapped by the
/// dictation worker on first F5).
pub fn voice_venv_root() -> std::path::PathBuf {
    if let Some(root) = std::env::var_os("AGENT_VESPER_VOICE_VENV") {
        return std::path::PathBuf::from(root);
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg)
            .join("agent-vesper")
            .join("voice-venv");
    }
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".local/share/agent-vesper/voice-venv")
}

/// The audio player path for the preview surface (same PATH-resolution
/// rules as the conversation playback owner; one convention).
#[cfg(feature = "voice-kokoro")]
pub fn resolve_player_for_preview() -> std::path::PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("aplay"))
                .find(|candidate| candidate.is_file())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("aplay"))
}

#[cfg(feature = "voice-conversation")]
pub mod voice_shared_stt;

#[cfg(feature = "voice-flm")]
pub mod voice_flm;
/// VRO-17 R16: the FLM NPU recognition composition (default-off; see
/// the `voice-flm` feature). Asset verification is compiled whenever
/// the conversation surface exists so Settings/readiness can report the
/// pack's state without spawning anything.
#[cfg(feature = "voice-conversation")]
pub mod voice_flm_assets;
#[cfg(feature = "voice-flm")]
pub mod voice_flm_vad;

/// Test seam: the feature-gated Settings menu entries this build
/// offers (the same tuples the settings menu renders). Voice appears
/// only in feature builds; absent otherwise (R9 parity).
pub fn feature_gated_settings_entries() -> Vec<(&'static str, &'static str)> {
    #[cfg(feature = "voice-conversation")]
    {
        vec![(
            "/settings voice",
            "Voice · dictation and conversation modes",
        )]
    }
    #[cfg(not(feature = "voice-conversation"))]
    {
        Vec::new()
    }
}

/// The managed voice-capture namespace root (R20). Lives under the
/// harness data root (same root as the voice venv): private, owned,
/// and reservation-accounted across TUI instances.
pub fn voice_capture_root() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("agent-vesper")
}

/// Test seam for the Settings → Voice save path: the production
/// `save_voice_scope` (no validation bypass; same failure semantics).
#[cfg(feature = "voice-conversation")]
#[path = "settings_voice_save.rs"]
pub mod settings_voice_save;

/// Test seam for the Settings → Voice save path: the production
/// `save_voice_scope` (single implementation; no validation bypass).
#[cfg(feature = "voice-conversation")]
pub mod settings_host_test {
    /// Saves the voice scope exactly as the Settings Save flow does.
    pub fn save_voice_for_test(
        root: &std::path::Path,
        scope: &vesper_voice::VoiceScope,
    ) -> Result<(), String> {
        crate::settings_voice_save::save_voice_scope(root, scope)
    }
}

#[cfg(test)]
mod tests {
    //! Top-level re-export sanity checks.

    use super::*;

    #[test]
    fn re_exports_cover_stage_11b_surface() {
        let registry = CommandRegistry::stage_11b();
        assert!(!registry.names().is_empty());
        let state = PlanState::default();
        assert_eq!(state.phase(), PlanPhase::Normal);
        // The dispatch surface re-exports a default SessionState that begins
        // in NORMAL with no overrides.
        let session = SessionState::new();
        assert_eq!(session.phase(), PlanPhase::Normal);
        assert!(session.overrides.is_empty());
    }

    #[test]
    fn auth_descriptor_is_projected_from_advertised_provider_descriptor() {
        // Provider-routed auth proof: an AuthProvider must be projected purely
        // from the active provider's advertised ProviderDescriptor, with no
        // hardcoded provider match arm. The env var comes from the first
        // secret-reference field; key_url from the advertised field.
        use vesper_domain::BoundedString;
        use vesper_provider::{AuthenticationMethodDescriptor, ProviderDescriptor};
        let descriptor = ProviderDescriptor {
            provider_id: ProviderId::new("zai").unwrap(),
            display_name: BoundedString::new("Z.ai GLM").unwrap(),
            authentication_methods: vec![AuthenticationMethodDescriptor {
                method_id: BoundedString::new("zai-api-key").unwrap(),
                display_name: BoundedString::new("Z.ai API key").unwrap(),
                secret_reference_fields: vec![BoundedString::new("ZAI_API_KEY").unwrap()],
                external_runtime_owned: false,
                key_url: Some(
                    BoundedString::new("https://z.ai/manage-apikey/apikey-list").unwrap(),
                ),
                interactive_login: vec![],
            }],
            hosted_tools: Vec::new(),
            configuration: None,
            metadata: Default::default(),
        };
        let auth = auth_provider_from_descriptor(&descriptor).expect("projected");
        assert_eq!(auth.id, "zai");
        assert_eq!(auth.name, "Z.ai API key");
        assert_eq!(auth.environment_variable, "ZAI_API_KEY");
        assert_eq!(auth.key_url, "https://z.ai/manage-apikey/apikey-list");

        // A descriptor with no auth method projects to None.
        let bare = ProviderDescriptor {
            provider_id: ProviderId::new("bare").unwrap(),
            display_name: BoundedString::new("Bare").unwrap(),
            authentication_methods: Vec::new(),
            hosted_tools: Vec::new(),
            configuration: None,
            metadata: Default::default(),
        };
        assert!(auth_provider_from_descriptor(&bare).is_none());
    }
}
