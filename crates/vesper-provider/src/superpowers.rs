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
