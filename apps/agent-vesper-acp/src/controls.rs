//! ACP footer-control surface derived from the real Z.ai GLM adapter data.
//!
//! This is the composition boundary (apps/AGENTS.md): the app MAY depend on
//! the concrete provider adapter and derives the ACP session-config option
//! surface from the frozen-oracle-parity catalog and the frozen `zai:`
//! provider-configuration values. The ACP adapter crate stays
//! provider-neutral and only renders what is contributed here.
//!
//! Zed maps `model` → model picker, `thought_level` → reasoning picker, and
//! renders every contributed selector in the chat footer; the token counter
//! comes from `usage_update` notifications, not from this surface.

use vesper_acp::{
    AcpControlCategory, AcpControlOption, AcpSessionControl, AppliedSelection,
    SessionControlSurface,
};
use vesper_domain::{ModelId, ProviderId, QualifiedModelId};
use vesper_provider::ProviderConfiguration;

/// Stable id of the provider-switching control option.
pub(crate) const PROVIDER_CONTROL_ID: &str = "provider";

/// Configuration key tracking the session's acting provider across footer
/// switches. The runtime envelope keeps the INITIAL provider's identity
/// (a config option never migrates it), so the acting provider rides as an
/// explicit value the current-value resolver reads back.
pub(crate) const ACTIVE_PROVIDER_KEY: &str = "vesper:active-provider";

/// Synthetic MoA picker value (oracle `MOA_PICKER_VALUE`). NOT a real model id.
const MOA_PICKER_VALUE: &str = "__moa__";

/// Frozen oracle per-model display data, in oracle `MODELS` order.
struct FrozenModelDisplay {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    context_window: &'static str,
    /// Plans this model is available on (oracle `plans` lists).
    plans: &'static [&'static str],
    vision: bool,
    deep_reasoning: bool,
}

const MODELS: &[FrozenModelDisplay] = &[
    FrozenModelDisplay {
        id: "glm-5.3",
        name: "GLM-5.3 (Flagship)",
        description: "Latest flagship — advanced complex software engineering, agent tasks, and emergent cybersecurity capabilities",
        context_window: "1M",
        plans: &["coding", "standard", "bigmodel"],
        vision: false,
        deep_reasoning: true,
    },
    FrozenModelDisplay {
        id: "glm-5.2",
        name: "GLM-5.2",
        description: "Previous flagship — maximum reasoning, coding, and long-horizon agentic tasks",
        context_window: "1M",
        plans: &["coding", "standard", "bigmodel"],
        vision: false,
        deep_reasoning: true,
    },
    FrozenModelDisplay {
        id: "glm-5-turbo",
        name: "GLM-5-Turbo",
        description: "Flagship model optimized for speed",
        context_window: "200K",
        plans: &["coding", "standard", "bigmodel"],
        vision: false,
        deep_reasoning: false,
    },
    FrozenModelDisplay {
        id: "glm-4.7",
        name: "GLM-4.7",
        description: "Balanced model for daily development and routine tasks",
        context_window: "200K",
        plans: &["coding", "standard", "bigmodel"],
        vision: false,
        deep_reasoning: false,
    },
    FrozenModelDisplay {
        id: "glm-5v-turbo",
        name: "GLM-5V-Turbo (Vision Coding)",
        description: "Multimodal coding model for screenshots, video, UI, and agent workflows",
        context_window: "200K",
        plans: &["standard", "bigmodel"],
        vision: true,
        deep_reasoning: false,
    },
    FrozenModelDisplay {
        id: "glm-4.5v",
        name: "GLM-4.5V (Vision)",
        description: "Vision-capable model for screenshots, diagrams, and charts",
        context_window: "65K",
        plans: &["standard", "bigmodel"],
        vision: true,
        deep_reasoning: false,
    },
    FrozenModelDisplay {
        id: "glm-4.6v",
        name: "GLM-4.6V (Vision)",
        description: "Vision model with improved OCR and image understanding",
        context_window: "131K",
        plans: &["standard", "bigmodel"],
        vision: true,
        deep_reasoning: false,
    },
];

/// Frozen API plans (oracle `API_ENDPOINTS`).
const API_ENDPOINTS: &[(&str, &str, &str)] = &[
    (
        "coding",
        "Coding Plan",
        "Z.ai Coding Plan — GLM-5.3, GLM-5.2, GLM-5-Turbo, GLM-4.7 (default)",
    ),
    (
        "standard",
        "Standard API",
        "Z.ai standard API — pay-as-you-go, broader model access incl. vision",
    ),
    (
        "bigmodel",
        "BigModel (CN)",
        "BigModel open platform (China) — Chinese mainland endpoint",
    ),
];

/// Frozen thought levels (oracle `THOUGHT_LEVELS`); `high`/`max` restricted
/// to deep-reasoning models.
const THOUGHT_LEVELS: &[(&str, &str, &str)] = &[
    (
        "disabled",
        "Off",
        "No reasoning — fast responses for simple tasks",
    ),
    ("enabled", "Standard", "Full reasoning traces streamed live"),
    (
        "high",
        "Deep · High",
        "Deeper multi-step reasoning for complex tasks (GLM-5.3/GLM-5.2 only)",
    ),
    (
        "max",
        "Deep · Max",
        "Maximum reasoning depth — deepest analysis (GLM-5.3/GLM-5.2 only)",
    ),
];

/// Frozen generation profiles (oracle `GENERATION_PROFILES`).
const GENERATION_PROFILES: &[(&str, &str, &str)] = &[
    (
        "balanced",
        "Balanced",
        "Use Z.ai model defaults; recommended for general coding",
    ),
    (
        "precise",
        "Precise",
        "Lower sampling variance for focused fixes and deterministic edits",
    ),
    (
        "exploratory",
        "Exploratory",
        "Broader nucleus sampling for ideation and alternative designs",
    ),
];

/// Returns the frozen model entries available on `plan`.
fn models_for_plan(plan: &str) -> impl Iterator<Item = &FrozenModelDisplay> {
    MODELS
        .iter()
        .filter(move |model| model.plans.contains(&plan))
}

/// Returns the thought levels available for `model` (oracle
/// `thought_levels_for_model`: `high`/`max` only on deep-reasoning models).
fn thought_levels_for_model(
    model: &str,
) -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    let deep = MODELS
        .iter()
        .find(|entry| entry.id == model)
        .is_some_and(|entry| entry.deep_reasoning);
    THOUGHT_LEVELS
        .iter()
        .filter(move |(id, _, _)| matches!(*id, "disabled" | "enabled") || deep)
}

/// Reads one `zai:` string value from the provider configuration envelope.
fn config_str<'a>(configuration: &'a ProviderConfiguration, key: &str) -> Option<&'a str> {
    configuration
        .values
        .values
        .get(key)
        .and_then(|value| value.as_str())
}

/// Builds the full oracle-parity ACP control surface from the frozen
/// provider configuration values.
pub(crate) fn glm_control_surface(configuration: &ProviderConfiguration) -> SessionControlSurface {
    let model = config_str(configuration, "zai:model")
        .or_else(|| config_str(configuration, "zai:model-id"))
        .unwrap_or("glm-5.3");
    let plan = config_str(configuration, "zai:endpoint-plan").unwrap_or("coding");
    let reasoning = config_str(configuration, "zai:reasoning-mode").unwrap_or("enabled");
    let generation = config_str(configuration, "zai:generation-profile").unwrap_or("balanced");
    let auxiliary = config_str(configuration, "zai:auxiliary-model").unwrap_or("glm-5.2");
    let mixture = config_str(configuration, "zai:mixture-mode").unwrap_or("off");

    let mut controls = Vec::new();

    // Model picker (MoA synthetic entry first, oracle parity).
    let mut model_options = vec![AcpControlOption {
        value: MOA_PICKER_VALUE.to_owned(),
        name: "🔬 Mixture of Agents (council)".to_owned(),
        description: Some(
            "Toggle the Mixture-of-Agents layer: the current model stays the \
             aggregator; up to two reference advisers review in parallel"
                .to_owned(),
        ),
    }];
    for entry in models_for_plan(plan) {
        model_options.push(AcpControlOption {
            value: entry.id.to_owned(),
            name: entry.name.to_owned(),
            description: Some(format!(
                "{} ({context} context)",
                entry.description,
                context = entry.context_window
            )),
        });
    }
    let mixture_enabled = mixture == "enabled";
    let current_model = if mixture_enabled {
        MOA_PICKER_VALUE
    } else {
        model
    };
    controls.push(AcpSessionControl {
        id: "model".to_owned(),
        name: "Model".to_owned(),
        description: Some("GLM model to use".to_owned()),
        category: AcpControlCategory::Model,
        current_value: current_model.to_owned(),
        options: model_options,
    });

    // Reasoning dial.
    let thought_options: Vec<AcpControlOption> = thought_levels_for_model(model)
        .map(|(id, name, description)| AcpControlOption {
            value: (*id).to_owned(),
            name: (*name).to_owned(),
            description: Some((*description).to_owned()),
        })
        .collect();
    controls.push(AcpSessionControl {
        id: "thought_level".to_owned(),
        name: "Reasoning".to_owned(),
        description: Some("Live reasoning trace level".to_owned()),
        category: AcpControlCategory::ThoughtLevel,
        current_value: reasoning.to_owned(),
        options: thought_options,
    });

    // API plan selector.
    controls.push(AcpSessionControl {
        id: "api_endpoint".to_owned(),
        name: "API Plan".to_owned(),
        description: Some("Z.ai API plan / endpoint".to_owned()),
        category: AcpControlCategory::Other,
        current_value: plan.to_owned(),
        options: API_ENDPOINTS
            .iter()
            .map(|(id, name, description)| AcpControlOption {
                value: (*id).to_owned(),
                name: (*name).to_owned(),
                description: Some((*description).to_owned()),
            })
            .collect(),
    });

    // Generation style.
    controls.push(AcpSessionControl {
        id: "generation_profile".to_owned(),
        name: "Generation Style".to_owned(),
        description: Some(
            "Sampling profile; changes only one sampling control at a time".to_owned(),
        ),
        category: AcpControlCategory::Other,
        current_value: generation.to_owned(),
        options: GENERATION_PROFILES
            .iter()
            .map(|(id, name, description)| AcpControlOption {
                value: (*id).to_owned(),
                name: (*name).to_owned(),
                description: Some((*description).to_owned()),
            })
            .collect(),
    });

    // Auxiliary model.
    let mut auxiliary_options = vec![AcpControlOption {
        value: "glm-5.2".to_owned(),
        name: "Use main model".to_owned(),
        description: Some(
            "Use the coding model when needed; otherwise use local fallbacks".to_owned(),
        ),
    }];
    for entry in models_for_plan(plan).filter(|entry| !entry.vision) {
        auxiliary_options.push(AcpControlOption {
            value: entry.id.to_owned(),
            name: entry.name.to_owned(),
            description: Some(
                "Use for titles, compression, recall, evaluation, and workers".to_owned(),
            ),
        });
    }
    controls.push(AcpSessionControl {
        id: "auxiliary_model".to_owned(),
        name: "Auxiliary Model".to_owned(),
        description: Some("Optional GLM model for all bounded auxiliary operations".to_owned()),
        category: AcpControlCategory::Other,
        current_value: auxiliary.to_owned(),
        options: auxiliary_options,
    });

    // Mixture of Agents.
    controls.push(AcpSessionControl {
        id: "mixture_mode".to_owned(),
        name: "Mixture of Agents".to_owned(),
        description: Some("Optional independent GLM references before the acting model".to_owned()),
        category: AcpControlCategory::Other,
        current_value: mixture.to_owned(),
        options: vec![
            AcpControlOption {
                value: "off".to_owned(),
                name: "Off".to_owned(),
                description: Some("Use the acting model directly".to_owned()),
            },
            AcpControlOption {
                value: "enabled".to_owned(),
                name: "Reference review".to_owned(),
                description: Some(
                    "Run up to two independent GLM references before each iteration".to_owned(),
                ),
            },
        ],
    });

    SessionControlSurface::new(controls)
        .with_current_resolver("model", |configuration| {
            let mixture = config_str(configuration, "zai:mixture-mode").unwrap_or("off");
            if mixture == "enabled" {
                Some(MOA_PICKER_VALUE.to_owned())
            } else {
                config_str(configuration, "zai:model")
                    .or_else(|| config_str(configuration, "zai:model-id"))
                    .map(str::to_owned)
            }
        })
        .with_current_resolver("api_endpoint", |configuration| {
            config_str(configuration, "zai:endpoint-plan").map(str::to_owned)
        })
        .with_current_resolver("generation_profile", |configuration| {
            config_str(configuration, "zai:generation-profile").map(str::to_owned)
        })
        .with_current_resolver("auxiliary_model", |configuration| {
            config_str(configuration, "zai:auxiliary-model").map(str::to_owned)
        })
        .with_current_resolver("mixture_mode", |configuration| {
            config_str(configuration, "zai:mixture-mode").map(str::to_owned)
        })
        .with_apply(|configuration, option_id, value| {
            apply_glm_config_selection(configuration, option_id, value).map(|configuration| {
                AppliedSelection {
                    model: session_model_override(&configuration),
                    configuration,
                }
            })
        })
}

/// Derives the `QualifiedModelId` for the current `zai:model` value so a
/// model switch updates the runtime session model alongside the envelope
/// (GLM request serialization rejects a model/envelope mismatch).
fn session_model_override(configuration: &ProviderConfiguration) -> Option<QualifiedModelId> {
    let model = config_str(configuration, "zai:model")
        .or_else(|| config_str(configuration, "zai:model-id"))?;
    let model_id = ModelId::new(model).ok()?;
    Some(QualifiedModelId {
        provider_id: configuration.provider_id.clone(),
        model_id,
    })
}

/// Builds the multi-provider surface: a `provider` picker (with auth status)
/// prepended to the active provider's GLM controls.
///
/// Mirrors the TUI's `/provider` switcher + `/auth` gate: descriptions show
/// each provider's authentication status, and switching changes the session's
/// acting model `provider_id` so the next turn dispatches to the selected
/// adapter. LM Studio needs no credential (local server, optional key); GLM
/// requires `ZAI_API_KEY` or a stored credential (set via `--setup`).
pub(crate) fn multi_provider_control_surface(
    configuration: &ProviderConfiguration,
    registered: &[(String, String, bool)],
) -> SessionControlSurface {
    let mut controls = Vec::new();

    // Provider picker first (TUI parity: /provider is the top switcher).
    let current_provider = configuration.provider_id.as_str().to_owned();
    let provider_options = registered
        .iter()
        .map(|(id, name, authenticated)| {
            let description = if *authenticated {
                format!("{name} — authenticated; switching takes effect on the next turn")
            } else {
                format!(
                    "{name} — NOT authenticated; run `agent-vesper-acp --setup` (or set the \
                     provider's env var) before switching"
                )
            };
            AcpControlOption {
                value: (*id).clone(),
                name: (*name).clone(),
                description: Some(description),
            }
        })
        .collect::<Vec<_>>();
    controls.push(AcpSessionControl {
        id: PROVIDER_CONTROL_ID.to_owned(),
        name: "Provider".to_owned(),
        description: Some("Registered model provider (TUI /provider parity)".to_owned()),
        category: AcpControlCategory::Other,
        current_value: current_provider,
        options: provider_options,
    });

    // GLM controls follow (ignored by other providers on dispatch).
    let glm = glm_control_surface(configuration);
    for control in glm.all() {
        controls.push(control.clone());
    }

    let mut surface = SessionControlSurface::new(controls)
        .with_current_resolver(PROVIDER_CONTROL_ID, |configuration| {
            // The acting provider rides as an explicit value once the user
            // has switched; before any switch the envelope identity IS the
            // acting provider.
            Some(
                config_str(configuration, ACTIVE_PROVIDER_KEY)
                    .unwrap_or_else(|| configuration.provider_id.as_str())
                    .to_owned(),
            )
        })
        .with_apply(|configuration, option_id, value| {
            if option_id == PROVIDER_CONTROL_ID {
                return apply_provider_selection(configuration, value);
            }
            apply_glm_config_selection(configuration, option_id, value).map(|configuration| {
                AppliedSelection {
                    model: session_model_override(&configuration),
                    configuration,
                }
            })
        });
    // Re-attach the GLM current-value resolvers on top of the provider one.
    for id in ["model", "api_endpoint", "generation_profile", "auxiliary_model", "mixture_mode"] {
        surface = surface.with_current_resolver(id, move |configuration| {
            let key = match id {
                "model" => "zai:model",
                "api_endpoint" => "zai:endpoint-plan",
                "generation_profile" => "zai:generation-profile",
                "auxiliary_model" => "zai:auxiliary-model",
                "mixture_mode" => "zai:mixture-mode",
                _ => return None,
            };
            config_str(configuration, key).map(str::to_owned)
        });
    }
    surface
}

/// Applies a provider switch onto the session configuration: returns the
/// target provider's default envelope and acting model so the runtime
/// re-routes the next turn to the selected adapter.
///
/// GLM (`zai`): the frozen default envelope + `glm-5.3`.
/// LM Studio (`lmstudio`): the local-server envelope + the discovered or
/// pinned local model id.
pub(crate) fn apply_provider_selection(
    configuration: &ProviderConfiguration,
    provider: &str,
) -> Option<AppliedSelection> {
    match provider {
        "zai" => {
            let mut next = crate::ProviderProfile::for_identity(
                &ProviderId::new("zai").ok()?,
            )
            .ok()?
            .provider_configuration;
            // Preserve any explicit GLM overrides the user set before.
            for key in [
                "zai:model",
                "zai:endpoint-plan",
                "zai:reasoning-mode",
                "zai:generation-profile",
                "zai:auxiliary-model",
                "zai:mixture-mode",
            ] {
                if let Some(value) = configuration.values.values.get(key) {
                    let _ = next.values.values.insert(key.to_owned(), value.clone());
                }
            }
            let _ = next
                .values
                .values
                .insert(ACTIVE_PROVIDER_KEY.to_owned(), serde_json::json!("zai"));
            let model = ModelId::new(
                config_str(&next, "zai:model").unwrap_or("glm-5.3"),
            )
            .ok()?;
            Some(AppliedSelection {
                model: Some(QualifiedModelId {
                    provider_id: ProviderId::new("zai").ok()?,
                    model_id: model,
                }),
                configuration: next,
            })
        }
        "lmstudio" => {
            let mut next = crate::lmstudio_provider::LmStudioFactory::default_configuration();
            let _ = next
                .values
                .values
                .insert(ACTIVE_PROVIDER_KEY.to_owned(), serde_json::json!("lmstudio"));
            let model = ModelId::new("local-model").ok()?;
            Some(AppliedSelection {
                model: Some(QualifiedModelId {
                    provider_id: ProviderId::new("lmstudio").ok()?,
                    model_id: model,
                }),
                configuration: next,
            })
        }
        _ => None,
    }
}

/// Returns the frozen context window for the configured model (oracle
/// `CONTEXT_WINDOW_TOKENS`), used for `usage_update` sizing.
pub(crate) fn glm_context_window(configuration: &ProviderConfiguration) -> u64 {
    let model = config_str(configuration, "zai:model")
        .or_else(|| config_str(configuration, "zai:model-id"))
        .unwrap_or("glm-5.3");
    match model {
        "glm-5.3" | "glm-5.2" => 1_000_000,
        "glm-5v-turbo" => 200_000,
        "glm-4.5v" => 65_536,
        "glm-4.6v" => 131_072,
        _ => 200_000,
    }
}

/// Applies one `(config option id, value)` selection onto the provider
/// configuration envelope, returning the updated envelope. Unhandled ids
/// are returned as `None`.
pub(crate) fn apply_glm_config_selection(
    configuration: &ProviderConfiguration,
    option_id: &str,
    value: &str,
) -> Option<ProviderConfiguration> {
    let mut next = configuration.clone();
    let values = &mut next.values.values;
    match option_id {
        "model" => {
            if value == MOA_PICKER_VALUE {
                // MoA keeps the current model as the aggregator.
                values
                    .insert("zai:mixture-mode".to_owned(), serde_json::json!("enabled"))
                    .ok()?;
            } else {
                // Selecting a bare model turns MoA off (oracle parity) and
                // switches the model.
                values
                    .insert("zai:mixture-mode".to_owned(), serde_json::json!("off"))
                    .ok()?;
                values
                    .insert("zai:model".to_owned(), serde_json::json!(value))
                    .ok()?;
            }
            Some(next)
        }
        "thought_level" => values
            .insert("zai:reasoning-mode".to_owned(), serde_json::json!(value))
            .ok()
            .map(|()| next),
        "api_endpoint" => values
            .insert("zai:endpoint-plan".to_owned(), serde_json::json!(value))
            .ok()
            .map(|()| next),
        "generation_profile" => values
            .insert(
                "zai:generation-profile".to_owned(),
                serde_json::json!(value),
            )
            .ok()
            .map(|()| next),
        "auxiliary_model" => values
            .insert("zai:auxiliary-model".to_owned(), serde_json::json!(value))
            .ok()
            .map(|()| next),
        "mixture_mode" => values
            .insert("zai:mixture-mode".to_owned(), serde_json::json!(value))
            .ok()
            .map(|()| next),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_configuration() -> ProviderConfiguration {
        let profile = crate::ProviderProfile::for_identity(&vesper_provider_glm::provider_id())
            .expect("profile for registered provider");
        profile.provider_configuration
    }

    #[test]
    fn control_surface_matches_oracle_options() {
        let surface = glm_control_surface(&default_configuration());
        let ids: Vec<&str> = surface.all().map(|control| control.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "model",
                "thought_level",
                "api_endpoint",
                "generation_profile",
                "auxiliary_model",
                "mixture_mode"
            ]
        );
        let model = surface.control("model").expect("model control");
        assert_eq!(model.current_value, "glm-5.3");
        assert_eq!(model.options[0].value, "__moa__");
        assert_eq!(model.options[1].name, "GLM-5.3 (Flagship)");
    }

    #[test]
    fn coding_plan_excludes_vision_models() {
        let surface = glm_control_surface(&default_configuration());
        let model = surface.control("model").expect("model control");
        assert!(
            !model
                .options
                .iter()
                .any(|option| option.value == "glm-4.5v")
        );
        assert!(
            !model
                .options
                .iter()
                .any(|option| option.value == "glm-5v-turbo")
        );
    }

    #[test]
    fn deep_reasoning_levels_only_on_deep_models() {
        let surface = glm_control_surface(&default_configuration());
        let thought = surface.control("thought_level").expect("thought control");
        // glm-5.3 is a deep-reasoning model: all four levels are offered.
        assert_eq!(thought.options.len(), 4);

        let turbo = default_configuration();
        let mut turbo = turbo.clone();
        turbo
            .values
            .values
            .insert("zai:model".to_owned(), serde_json::json!("glm-5-turbo"))
            .expect("bounded key");
        let surface = glm_control_surface(&turbo);
        let thought = surface.control("thought_level").expect("thought control");
        // glm-5-turbo is not deep-reasoning: only Off/Standard.
        assert_eq!(thought.options.len(), 2);
    }

    #[test]
    fn applying_model_selection_updates_envelope() {
        let configuration = default_configuration();
        let next =
            apply_glm_config_selection(&configuration, "model", "glm-5-turbo").expect("applies");
        assert_eq!(
            next.values.values.get("zai:model").and_then(|v| v.as_str()),
            Some("glm-5-turbo")
        );
        assert_eq!(
            next.values
                .values
                .get("zai:mixture-mode")
                .and_then(|v| v.as_str()),
            Some("off")
        );
    }

    #[test]
    fn applying_moa_selection_enables_without_model_change() {
        let configuration = default_configuration();
        let next =
            apply_glm_config_selection(&configuration, "model", MOA_PICKER_VALUE).expect("applies");
        assert_eq!(
            next.values
                .values
                .get("zai:mixture-mode")
                .and_then(|v| v.as_str()),
            Some("enabled")
        );
        // The aggregator model is unchanged.
        assert_eq!(
            next.values.values.get("zai:model").and_then(|v| v.as_str()),
            Some("glm-5.3")
        );
    }

    #[test]
    fn applying_thought_level_updates_reasoning() {
        let configuration = default_configuration();
        let next =
            apply_glm_config_selection(&configuration, "thought_level", "max").expect("applies");
        assert_eq!(
            next.values
                .values
                .get("zai:reasoning-mode")
                .and_then(|v| v.as_str()),
            Some("max")
        );
    }

    #[test]
    fn surface_apply_round_trips_model_change() {
        let configuration = default_configuration();
        let surface = glm_control_surface(&configuration);
        let applied = surface
            .apply(&configuration, "model", "glm-5.2")
            .expect("surface applies model");
        assert_eq!(
            applied
                .configuration
                .values
                .values
                .get("zai:model")
                .and_then(|v| v.as_str()),
            Some("glm-5.2")
        );
        let model = applied.model.expect("model override present");
        assert_eq!(model.model_id.as_str(), "glm-5.2");
    }

    #[test]
    fn context_window_follows_model() {
        let configuration = default_configuration();
        assert_eq!(glm_context_window(&configuration), 1_000_000);
    }

    fn registered_providers(authenticated_zai: bool) -> Vec<(String, String, bool)> {
        vec![
            ("zai".to_owned(), "Z.ai".to_owned(), authenticated_zai),
            ("lmstudio".to_owned(), "LM Studio".to_owned(), true),
        ]
    }

    #[test]
    fn multi_provider_surface_lists_providers_with_auth_status() {
        let configuration = default_configuration();
        let surface = multi_provider_control_surface(
            &configuration,
            &registered_providers(false),
        );
        let provider = surface.control("provider").expect("provider control");
        assert_eq!(provider.current_value, "zai");
        assert_eq!(provider.options.len(), 2);
        let zai = &provider.options[0];
        assert!(zai.description.as_deref().unwrap_or("").contains("NOT authenticated"));
        let lmstudio = &provider.options[1];
        assert!(
            lmstudio
                .description
                .as_deref()
                .unwrap_or("")
                .contains("authenticated")
        );
        // GLM controls follow the provider picker.
        assert!(surface.control("model").is_some());
        assert!(surface.control("thought_level").is_some());
    }

    #[test]
    fn multi_provider_surface_switches_to_lmstudio() {
        let configuration = default_configuration();
        let surface = multi_provider_control_surface(
            &configuration,
            &registered_providers(true),
        );
        let applied = surface
            .apply(&configuration, "provider", "lmstudio")
            .expect("provider switch applies");
        let model = applied.model.expect("model override");
        assert_eq!(model.provider_id.as_str(), "lmstudio");
        assert_eq!(model.model_id.as_str(), "local-model");
        assert_eq!(applied.configuration.provider_id.as_str(), "lmstudio");
    }

    #[test]
    fn multi_provider_surface_switches_back_to_zai_preserving_overrides() {
        let configuration = default_configuration();
        let surface = multi_provider_control_surface(
            &configuration,
            &registered_providers(true),
        );
        // Simulate runtime merge semantics: the session config ACCUMULATES
        // value overlays (the runtime merges onto its own envelope; provider
        // switches never remove keys). A switch to lmstudio overlays nothing
        // (empty envelope), so the pre-switch zai: overrides survive.
        let mut session = surface
            .apply(&configuration, "model", "glm-5-turbo")
            .expect("model switch")
            .configuration;
        let lm = surface
            .apply(&session, "provider", "lmstudio")
            .expect("provider switch");
        // Runtime merge: overlay lm's values onto the existing session keys.
        for (key, value) in lm.configuration.values.values.iter() {
            let _ = session.values.values.insert((*key).to_owned(), value.clone());
        }
        let applied = surface
            .apply(&session, "provider", "zai")
            .expect("switch back");
        let model = applied.model.expect("model override");
        assert_eq!(model.provider_id.as_str(), "zai");
        assert_eq!(
            applied
                .configuration
                .values
                .values
                .get("zai:model")
                .and_then(|v| v.as_str()),
            // glm-5-turbo was the explicit pre-switch override; preserved.
            Some("glm-5-turbo")
        );
    }

    #[test]
    fn multi_provider_surface_rejects_unknown_provider() {
        let configuration = default_configuration();
        // Direct apply_provider_selection rejects unknown ids (the adapter's
        // accepts() gate would already filter these, but the apply must stay
        // fail-closed for direct calls).
        assert!(apply_provider_selection(&configuration, "not-a-provider").is_none());
    }
}
