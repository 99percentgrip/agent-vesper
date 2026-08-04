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

pub mod auth_hub;
pub mod commands;
pub mod dispatch;
pub mod markdown;
pub mod plan_mode;
pub mod superpowers;
pub mod ui;

pub use auth_hub::{
    AuthHubAction, AuthHubState, AuthProvider, StartupRoute, render_auth_hub, startup_route,
};
pub use commands::{CommandIntent, CommandOutcome, CommandRegistry, MediaOp, PlanGesture};
pub use dispatch::{
    DispatchOutcome, PanelVisibility, SessionControls, SessionState, TaskItem, TerminalAction,
    TerminalPreferences, apply_model_plan, apply_task_plan, dispatch,
};
pub use plan_mode::{PendingQuestion, PlanModeError, PlanPhase, PlanState, PlanTransition};
pub use superpowers::{ProviderSuperpowerSurface, SuperpowerOverrides};
pub use ui::{
    FOOTER_ACTIONS, PermissionChoice, PermissionModal, StubRenderer, TerminalRenderer, ViewModel,
    command_menu_height, render_to_frame,
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
    let method = descriptor.authentication_methods.first()?;
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
            }],
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
            configuration: None,
            metadata: Default::default(),
        };
        assert!(auth_provider_from_descriptor(&bare).is_none());
    }
}
