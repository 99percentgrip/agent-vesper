//! Provider-neutral session control surface consumed by the ACP adapter.
//!
//! The adapter must stay provider-neutral (crates/AGENTS.md: `vesper-acp` may
//! depend only on domain/runtime), but client footer controls — model
//! selector, reasoning selector, plan selector — are rendered from ACP
//! `sessionConfigOptions` whose *content* is provider-owned. This module
//! defines the neutral hand-off shape: the composition boundary (e.g.
//! `apps/agent-vesper-acp`) derives it from the owning provider adapter's
//! real surfaces (model catalog, superpower descriptors, frozen
//! configuration values) and injects it through
//! [`AcpAdapterConfig::controls`](crate::AcpAdapterConfig::controls). The
//! adapter never fabricates a model, plan, or provider that was not
//! contributed by the real adapter.

use std::collections::BTreeMap;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{SessionConfigOption, SessionConfigOptionCategory};
use vesper_domain::QualifiedModelId;
use vesper_runtime::{ProviderConfiguration, SessionSnapshot};

/// One dropdown-style control advertised through ACP `sessionConfigOptions`.
///
/// `id` is stable per control kind (e.g. `model`, `thought_level`,
/// `permission_mode`) so clients map each control to the right footer slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSessionControl {
    /// Stable ACP config-option id (e.g. `model`, `thought_level`).
    pub id: String,
    /// Human-readable label rendered by the client.
    pub name: String,
    /// Optional description the client may show.
    pub description: Option<String>,
    /// ACP semantic category (`model`, `thought_level`, `permissions`,
    /// `other`).
    pub category: AcpControlCategory,
    /// Currently selected value id.
    pub current_value: String,
    /// Every selectable value: `(value id, display name, description)`.
    pub options: Vec<AcpControlOption>,
}

/// Semantic category mirrored onto the ACP wire category field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpControlCategory {
    /// Model selector (client footer model picker).
    Model,
    /// Thought/reasoning level selector.
    ThoughtLevel,
    /// Permission mode selector.
    Permissions,
    /// Anything else the provider contributes.
    Other,
}

/// One selectable value of an [`AcpSessionControl`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpControlOption {
    /// Stable value id sent back on `session/set_config_option`.
    pub value: String,
    /// Human-readable option name.
    pub name: String,
    /// Optional per-option description.
    pub description: Option<String>,
}

impl AcpControlOption {
    /// Builds one option with id and name only.
    #[must_use]
    pub fn new(value: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            name: name.into(),
            description: None,
        }
    }

    /// Attaches a description.
    #[must_use]
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Result of applying one config-option selection.
#[derive(Debug, Clone)]
pub struct AppliedSelection {
    /// Updated provider configuration envelope (fully merged, ready for the
    /// runtime `UpdateProviderConfiguration` command).
    pub configuration: ProviderConfiguration,
    /// New acting model when the selection changed it.
    pub model: Option<QualifiedModelId>,
}

/// The full set of provider-routed controls for one provider.
///
/// `controls` is keyed by config-option id. The provider-owned `apply`
/// function — contributed by the composition boundary via [`Self::with_apply`]
/// — maps a validated `(option id, value)` selection onto a new provider
/// envelope and an optional model change, so the adapter stays free of
/// provider logic.
#[derive(Clone, Default)]
pub struct SessionControlSurface {
    controls: BTreeMap<String, AcpSessionControl>,
    /// Insertion order of control ids, so clients render the contributed
    /// order (model first, oracle parity) instead of alphabetical order.
    order: Vec<String>,
    /// Provider-owned live current-value resolvers keyed by control id.
    /// Each resolver reads the session's provider envelope (opaque to this
    /// crate) so re-advertised options reflect the actual session state.
    #[allow(clippy::type_complexity)]
    resolvers:
        BTreeMap<String, Arc<dyn Fn(&ProviderConfiguration) -> Option<String> + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    apply: Option<
        Arc<dyn Fn(&ProviderConfiguration, &str, &str) -> Option<AppliedSelection> + Send + Sync>,
    >,
}

impl std::fmt::Debug for SessionControlSurface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionControlSurface")
            .field("controls", &self.controls)
            .field("has_apply", &self.apply.is_some())
            .finish()
    }
}

impl SessionControlSurface {
    /// Builds a surface from contributed controls without an apply function
    /// (advertising-only; selections are then rejected by the adapter).
    #[must_use]
    pub fn new(controls: Vec<AcpSessionControl>) -> Self {
        let order = controls.iter().map(|control| control.id.clone()).collect();
        Self {
            controls: controls
                .into_iter()
                .map(|control| (control.id.clone(), control))
                .collect(),
            order,
            resolvers: BTreeMap::new(),
            apply: None,
        }
    }

    /// Attaches the provider-owned apply function mapping a validated
    /// `(option id, value)` selection onto a fully merged provider envelope
    /// and an optional new model.
    #[must_use]
    pub fn with_apply<F>(mut self, apply: F) -> Self
    where
        F: Fn(&ProviderConfiguration, &str, &str) -> Option<AppliedSelection>
            + Send
            + Sync
            + 'static,
    {
        self.apply = Some(Arc::new(apply));
        self
    }

    /// Attaches a provider-owned resolver returning the live current value
    /// of control `id` from the session's provider envelope. When the
    /// resolver returns `None` (key absent) the contributed static
    /// `current_value` is advertised.
    #[must_use]
    pub fn with_current_resolver<F>(mut self, id: impl Into<String>, resolver: F) -> Self
    where
        F: Fn(&ProviderConfiguration) -> Option<String> + Send + Sync + 'static,
    {
        self.resolvers.insert(id.into(), Arc::new(resolver));
        self
    }

    /// Applies one selection against the session's current configuration.
    /// Returns `None` when no apply function was contributed or the option
    /// is not provider-routed.
    #[must_use]
    pub fn apply(
        &self,
        configuration: &ProviderConfiguration,
        option_id: &str,
        value: &str,
    ) -> Option<AppliedSelection> {
        self.apply
            .as_ref()
            .and_then(|apply| apply(configuration, option_id, value))
    }

    /// Looks up one control by option id.
    #[must_use]
    pub fn control(&self, id: &str) -> Option<&AcpSessionControl> {
        self.controls.get(id)
    }

    /// Every contributed control, in contribution order.
    pub fn all(&self) -> impl Iterator<Item = &AcpSessionControl> {
        self.order.iter().filter_map(|id| self.controls.get(id))
    }

    /// Whether `value` is a selectable option of control `id`.
    #[must_use]
    pub fn accepts(&self, id: &str, value: &str) -> bool {
        self.controls
            .get(id)
            .is_some_and(|control| control.options.iter().any(|option| option.value == value))
    }

    /// Sets the `current_value` of control `id` to `value` in place.
    ///
    /// No-op when `id` is unknown or `value` is not one of its options —
    /// callers validate through [`Self::accepts`] first.
    pub fn set_current(&mut self, id: &str, value: &str) {
        if self.accepts(id, value)
            && let Some(control) = self.controls.get_mut(id)
        {
            control.current_value = value.to_owned();
        }
    }

    /// Renders the surface as ACP config options for one session snapshot.
    ///
    /// Provider-neutral: reads only the runtime snapshot (reasoning override,
    /// permission mode) plus contributed descriptors. The current value of
    /// provider-owned controls (e.g. `model`) is taken from the snapshot's
    /// provider configuration so the advertised dropdown reflects the actual
    /// session state, not a static default.
    #[must_use]
    pub fn acp_config_options(&self, snapshot: &SessionSnapshot) -> Vec<SessionConfigOption> {
        use agent_client_protocol::schema::v1::{SessionConfigSelectOption, SessionConfigValueId};

        let mut options = Vec::new();
        for control in self.all() {
            let current = SessionConfigValueId::new(self.current_for(control, snapshot));
            let mut option = SessionConfigOption::select(
                control.id.clone(),
                control.name.clone(),
                current,
                control
                    .options
                    .iter()
                    .map(|choice| {
                        let mut select = SessionConfigSelectOption::new(
                            choice.value.clone(),
                            choice.name.clone(),
                        );
                        if let Some(description) = &choice.description {
                            select = select.description(description.clone());
                        }
                        select
                    })
                    .collect::<Vec<_>>(),
            );
            if let Some(description) = &control.description {
                option = option.description(description.clone());
            }
            option = option.category(match control.category {
                AcpControlCategory::Model => SessionConfigOptionCategory::Model,
                AcpControlCategory::ThoughtLevel => SessionConfigOptionCategory::ThoughtLevel,
                AcpControlCategory::Permissions => {
                    SessionConfigOptionCategory::Other("permissions".to_owned())
                }
                AcpControlCategory::Other => SessionConfigOptionCategory::Other(control.id.clone()),
            });
            options.push(option);
        }
        options
    }

    /// Resolves the advertised current value for one control against the
    /// snapshot: provider-owned resolver first (live envelope state), then
    /// the built-in runtime reasoning override for `thought_level`, then
    /// the contributed static default.
    #[must_use]
    fn current_for(&self, control: &AcpSessionControl, snapshot: &SessionSnapshot) -> String {
        if let Some(resolver) = self.resolvers.get(&control.id)
            && let Some(current) = resolver(&snapshot.provider_configuration)
        {
            return current;
        }
        match control.id.as_str() {
            "thought_level" => snapshot
                .reasoning
                .as_ref()
                .and_then(|reasoning| reasoning.mode.as_ref())
                .map(|mode| mode.as_str().to_owned())
                .unwrap_or_else(|| control.current_value.clone()),
            _ => control.current_value.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_control() -> AcpSessionControl {
        AcpSessionControl {
            id: "model".to_owned(),
            name: "Model".to_owned(),
            description: Some("Provider model to use".to_owned()),
            category: AcpControlCategory::Model,
            current_value: "model-a".to_owned(),
            options: vec![
                AcpControlOption::new("model-a", "Model A"),
                AcpControlOption::new("model-b", "Model B"),
            ],
        }
    }

    fn configuration() -> ProviderConfiguration {
        use vesper_domain::{
            ExtensionMap, ExtensionNamespace, SchemaVersion, VersionedExtensionEnvelope,
        };
        ProviderConfiguration {
            provider_id: vesper_domain::ProviderId::new("test.provider").expect("valid id"),
            values: VersionedExtensionEnvelope {
                namespace: ExtensionNamespace::new("provider.test").expect("bounded namespace"),
                version: SchemaVersion::new(1).expect("static schema version"),
                values: ExtensionMap::default(),
            },
        }
    }

    #[test]
    fn surface_looks_up_and_validates_options() {
        let surface = SessionControlSurface::new(vec![model_control()]);
        assert!(surface.accepts("model", "model-a"));
        assert!(surface.accepts("model", "model-b"));
        assert!(!surface.accepts("model", "model-c"));
        assert!(!surface.accepts("unknown", "model-a"));
        assert_eq!(surface.all().count(), 1);
    }

    #[test]
    fn apply_requires_contributed_closure() {
        let mut surface = SessionControlSurface::new(vec![model_control()]);
        assert!(
            surface
                .apply(&configuration(), "model", "model-a")
                .is_none()
        );
        surface = surface.with_apply(|configuration, _id, _value| {
            Some(AppliedSelection {
                configuration: configuration.clone(),
                model: None,
            })
        });
        let applied = surface
            .apply(&configuration(), "model", "model-a")
            .expect("contributed closure applies");
        assert!(applied.model.is_none());
    }
}
