//! Provider "superpower" discovery.
//!
//! A *superpower* is a provider-specific control that the runtime cannot model
//! through the neutral `ProviderRequest` shape alone — for example the Z.ai GLM
//! effort dial, interleaved-thinking flag, or model selector. Concrete provider
//! adapters expose their superpowers through [`ProviderSuperpowers`]; the
//! runtime registry forwards them to the composition boundary so a frontend
//! (TUI, ACP adapter, …) can render provider-native controls without taking a
//! dependency on any concrete adapter crate.
//!
//! Contracts honoured here:
//! - The crate implements no concrete provider and depends only on
//!   `vesper-domain`.
//! - All values are bounded and serializable so they may cross crate and
//!   process boundaries unchanged.
//! - Capability fallback is typed and observable: a provider that does not
//!   implement [`ProviderSuperpowers`] simply exposes no superpowers.

use serde::{Deserialize, Serialize};
use vesper_domain::{BoundedString, ProviderId};

/// Where a superpower is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuperpowerScope {
    /// Applies to the entire session (e.g. the active model).
    Session,
    /// Applies to a single prompt turn (e.g. effort level).
    Request,
    /// Applies to both session defaults and per-request overrides.
    Both,
}

/// Value type a superpower may take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SuperpowerValue {
    /// One of an enumerated set of choices.
    Choice {
        /// Selected choice (must appear in `allowed` when that set is non-empty).
        value: BoundedString<128>,
    },
    /// Boolean toggle.
    Flag {
        /// On/off state.
        value: bool,
    },
    /// Bounded integer.
    Number {
        /// Selected integer.
        value: i64,
    },
}

/// Structural kind of a superpower, independent of its current value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuperpowerKind {
    /// Pick one value from `allowed_values`.
    Choice,
    /// Boolean on/off.
    Toggle,
    /// Bounded integer.
    Numeric,
}

/// Static description of one provider-native superpower.
///
/// Stable enough to be advertised to a frontend at startup; the frontend uses
/// `command_alias` to bind the descriptor to a slash command (e.g. `effort`
/// maps to `/effort max`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuperpowerDescriptor {
    /// Provider-namespaced stable ID (e.g. `zai:effort`).
    pub id: BoundedString<128>,
    /// Provider that owns this superpower.
    pub provider_id: ProviderId,
    /// User-facing label (e.g. `Effort`).
    pub display_name: BoundedString<256>,
    /// Structural kind.
    pub kind: SuperpowerKind,
    /// Where the superpower applies.
    pub scope: SuperpowerScope,
    /// Current default value.
    pub default_value: SuperpowerValue,
    /// Allowed values for `Choice` superpowers; empty means free-form text.
    #[serde(default)]
    pub allowed_values: Vec<SuperpowerValue>,
    /// Optional slash-command alias (e.g. `effort`). When absent the
    /// superpower has no command surface.
    #[serde(default)]
    pub command_alias: Option<BoundedString<32>>,
    /// Optional safe usage hint shown by frontends.
    #[serde(default)]
    pub help: Option<BoundedString<256>>,
}

/// Implemented by provider factories that expose provider-native controls.
///
/// The trait is **not** a supertrait of [`crate::ProviderFactory`]: providers
/// that have nothing to advertise simply omit the implementation, and the
/// runtime reports an empty superpower set for them.
pub trait ProviderSuperpowers: Send + Sync {
    /// Stable, ordered superpower descriptors.
    fn superpowers(&self) -> Vec<SuperpowerDescriptor>;
}

/// A provider's policy governing its advertised superpowers: which candidate
/// values are valid for the current session state, how a change cascades, and
/// how a reasoning superpower maps to the runtime reasoning mode.
///
/// This moves **provider-specific** model/plan/reasoning logic OUT of the
/// frontend (which must stay provider-neutral — no hardcoded provider match
/// arms) and BEHIND the owning provider adapter. Each provider implements this
/// for its own superpowers; a [`PermissiveSuperpowerPolicy`] default accepts
/// every advertised value with no constraints and no cascades, which suits
/// providers with no inter-superpower rules (e.g. a local model server that
/// exposes only its loaded model).
///
/// The policy is consulted by `command_alias` (e.g. `"model"`, `"thinking"`,
/// `"reasoning"`), not by provider-namespaced descriptor id, so the frontend
/// never names a concrete provider.
pub trait SuperpowerPolicy: Send + Sync {
    /// Filter the advertised candidate values for `alias` down to those valid
    /// for the current session state. `active_plan`/`active_model` are the
    /// current session choices (empty string when unset). Providers with no
    /// constraint return the input unchanged.
    fn valid_choices(
        &self,
        alias: &str,
        advertised: &[SuperpowerValue],
        active_plan: &str,
        active_model: &str,
    ) -> Vec<SuperpowerValue>;

    /// Validate a chosen `value` for `alias` against the current session
    /// state. `Ok(())` accepts; `Err(message)` rejects with a user-facing
    /// reason. Providers with no constraint always accept.
    fn validate(
        &self,
        alias: &str,
        value: &SuperpowerValue,
        active_plan: &str,
        active_model: &str,
    ) -> Result<(), String>;

    /// Side effects to apply when `alias` changes to `value`, given the active
    /// plan. Each effect sets `target_alias` to `new_value`, but only when the
    /// target's current value is one of `apply_only_if_current_in` (empty ⇒
    /// apply unconditionally). Providers with no cascade rule return an empty
    /// vector.
    fn on_change(
        &self,
        alias: &str,
        value: &SuperpowerValue,
        active_plan: &str,
    ) -> Vec<SuperpowerSideEffect>;

    /// Side effects when the endpoint plan changes (e.g. the model must reset
    /// because it is unavailable on the new plan). Returns the model to reset
    /// to (if any), the auxiliary model to reset to (if any), and whether the
    /// provider owns this plan at all. Providers with no plan concept return
    /// [`PlanChangeReaction::none`] (the default).
    fn on_plan_change(
        &self,
        new_plan: &str,
        current_model: &str,
        current_auxiliary: &str,
    ) -> PlanChangeReaction;

    /// Map a reasoning-superpower value to the runtime reasoning-mode label, if
    /// this provider owns a reasoning superpower. `None` otherwise.
    fn reasoning_mode(&self, reasoning_value: &SuperpowerValue) -> Option<BoundedString<64>>;
}

/// What a provider wants to happen when the endpoint plan changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanChangeReaction {
    /// Whether this provider owns endpoint plans at all. `false` ⇒ the plan
    /// change is rejected for this provider.
    pub owns_plans: bool,
    /// The model to reset to when the current model is unavailable on the new
    /// plan; `None` to keep the current model.
    pub reset_model_to: Option<String>,
    /// The auxiliary model to reset to (e.g. `"main"` when the auxiliary is a
    /// vision model unavailable on the new plan); `None` to keep it.
    pub reset_auxiliary_to: Option<String>,
}

impl PlanChangeReaction {
    /// No reaction (for providers with no endpoint-plan concept).
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// A provider-routed side effect of one superpower changing.
///
/// When `alias` changes, a [`SuperpowerPolicy`] may emit these to cascade the
/// change onto other superpowers (e.g. resetting `thinking` when `model`
/// changes). The host applies `new_value` to `target_alias` only when the
/// target's current value is in `apply_only_if_current_in` (empty ⇒ apply
/// unconditionally).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperpowerSideEffect {
    /// Alias of the superpower to change (e.g. `"thinking"`).
    pub target_alias: String,
    /// Value to set on the target.
    pub new_value: SuperpowerValue,
    /// If non-empty, the effect applies only when the target's current value
    /// is one of these. Empty ⇒ apply unconditionally.
    pub apply_only_if_current_in: Vec<SuperpowerValue>,
}

/// A `SuperpowerPolicy` that accepts every advertised value, applies no
/// cascades, and owns no reasoning mapping. The default for providers with no
/// inter-superpower rules.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermissiveSuperpowerPolicy;

impl SuperpowerPolicy for PermissiveSuperpowerPolicy {
    fn valid_choices(
        &self,
        _alias: &str,
        advertised: &[SuperpowerValue],
        _active_plan: &str,
        _active_model: &str,
    ) -> Vec<SuperpowerValue> {
        advertised.to_vec()
    }

    fn validate(
        &self,
        _alias: &str,
        _value: &SuperpowerValue,
        _active_plan: &str,
        _active_model: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_change(
        &self,
        _alias: &str,
        _value: &SuperpowerValue,
        _active_plan: &str,
    ) -> Vec<SuperpowerSideEffect> {
        Vec::new()
    }

    fn on_plan_change(
        &self,
        _new_plan: &str,
        _current_model: &str,
        _current_auxiliary: &str,
    ) -> PlanChangeReaction {
        PlanChangeReaction::none()
    }

    fn reasoning_mode(&self, reasoning_value: &SuperpowerValue) -> Option<BoundedString<64>> {
        // Permissive: accept any Choice value as a reasoning mode label. This
        // makes /thinking work for providers that advertise a reasoning
        // superpower but don't need a custom mode-mapping (e.g. LM Studio
        // passes the label straight through to the model's API).
        match reasoning_value {
            SuperpowerValue::Choice { value } => BoundedString::new(value.as_str()).ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Round-trip and ordering invariants for superpower descriptors.

    use super::*;

    fn provider() -> ProviderId {
        ProviderId::new("test").expect("static provider id")
    }

    #[test]
    fn descriptor_round_trips_through_serde_json() {
        let descriptor = SuperpowerDescriptor {
            id: BoundedString::new("test:effort").unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new("Effort").unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Request,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("high").unwrap(),
            },
            allowed_values: vec![
                SuperpowerValue::Choice {
                    value: BoundedString::new("low").unwrap(),
                },
                SuperpowerValue::Choice {
                    value: BoundedString::new("high").unwrap(),
                },
            ],
            command_alias: Some(BoundedString::new("effort").unwrap()),
            help: Some(BoundedString::new("Set per-request effort.").unwrap()),
        };
        let serialized = serde_json::to_string(&descriptor).expect("serialize");
        let restored: SuperpowerDescriptor =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(descriptor, restored);
    }

    #[test]
    fn flag_value_round_trips() {
        let value = SuperpowerValue::Flag { value: true };
        let serialized = serde_json::to_string(&value).expect("serialize");
        let restored: SuperpowerValue = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(value, restored);
    }

    #[test]
    fn number_value_round_trips() {
        let value = SuperpowerValue::Number { value: 42 };
        let serialized = serde_json::to_string(&value).expect("serialize");
        let restored: SuperpowerValue = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(value, restored);
    }

    #[test]
    fn superpower_kind_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&SuperpowerKind::Choice).unwrap(),
            "\"choice\""
        );
        assert_eq!(
            serde_json::to_string(&SuperpowerKind::Toggle).unwrap(),
            "\"toggle\""
        );
        assert_eq!(
            serde_json::to_string(&SuperpowerKind::Numeric).unwrap(),
            "\"numeric\""
        );
    }
}
