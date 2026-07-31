//! Provider Superpowers adapter (Stage 11b).
//!
//! The composition boundary selects a concrete provider at startup and asks
//! the runtime registry for its advertised [`SuperpowerDescriptor`] set. This
//! module owns the pure projection the TUI uses: it stores the descriptors,
//! exposes them by command alias, and applies validated overrides into a
//! typed [`SuperpowerOverrides`] snapshot that the event loop can hand back
//! to the runtime as part of the next provider request.

use std::collections::BTreeMap;

use vesper_domain::ProviderId;
use vesper_provider::{SuperpowerDescriptor, SuperpowerValue};

/// Currently-active override for each superpower ID.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SuperpowerOverrides {
    values: BTreeMap<String, SuperpowerValue>,
}

impl SuperpowerOverrides {
    /// Records an override for the descriptor with `id`. Returns the previous
    /// value, if any.
    pub fn set(&mut self, id: &str, value: SuperpowerValue) -> Option<SuperpowerValue> {
        self.values.insert(id.to_string(), value)
    }

    /// Returns the override for `id`, or the descriptor default when none.
    #[must_use]
    pub fn get<'a>(
        &self,
        id: &str,
        default: impl Into<Option<&'a SuperpowerValue>>,
    ) -> Option<SuperpowerValue> {
        self.values
            .get(id)
            .cloned()
            .or_else(|| default.into().cloned())
    }

    /// Number of overrides currently recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether any overrides are recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Snapshot of every override (keyed by descriptor ID) in stable order.
    #[must_use]
    pub fn snapshot(&self) -> Vec<(String, SuperpowerValue)> {
        self.values
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}

/// The active provider's superpower surface.
///
/// Created at startup from a runtime query and immutable thereafter; per-turn
/// overrides live in [`SuperpowerOverrides`] so the surface itself can be
/// shared across sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSuperpowerSurface {
    provider_id: ProviderId,
    descriptors: Vec<SuperpowerDescriptor>,
}

impl ProviderSuperpowerSurface {
    /// Builds the surface from the descriptors advertised by the registry.
    #[must_use]
    pub fn new(provider_id: ProviderId, mut descriptors: Vec<SuperpowerDescriptor>) -> Self {
        // Stable order: by descriptor ID so the TUI listing is deterministic.
        descriptors.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            provider_id,
            descriptors,
        }
    }

    /// The owning provider identity.
    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// Advertised descriptors in stable order.
    #[must_use]
    pub fn descriptors(&self) -> &[SuperpowerDescriptor] {
        &self.descriptors
    }

    /// Looks up a descriptor by its command alias (e.g. `effort`).
    #[must_use]
    pub fn by_alias(&self, alias: &str) -> Option<&SuperpowerDescriptor> {
        self.descriptors.iter().find(|descriptor| {
            descriptor
                .command_alias
                .as_ref()
                .map(|value| value.as_str())
                .is_some_and(|value| value == alias)
        })
    }

    /// Builds a default override snapshot seeded from every descriptor's
    /// declared `default_value`. The TUI mutates this in place as the user
    /// issues `/effort`, `/thinking`, or `/model`.
    #[must_use]
    pub fn defaults(&self) -> SuperpowerOverrides {
        let mut overrides = SuperpowerOverrides::default();
        for descriptor in &self.descriptors {
            let key = descriptor.id.as_str().to_string();
            if !key.is_empty() {
                overrides.set(&key, descriptor.default_value.clone());
            }
        }
        overrides
    }

    /// Builds the help-line listing shown by the slash-command palette.
    #[must_use]
    pub fn render_help_lines(&self) -> Vec<String> {
        self.descriptors
            .iter()
            .map(|descriptor| {
                let alias = descriptor
                    .command_alias
                    .as_ref()
                    .map(|value| value.as_str())
                    .unwrap_or("<no-alias>");
                let display = descriptor.display_name.as_str();
                format!("/{alias} — {display}")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    //! Sorting, alias resolution, default seeding, and override application.

    use super::*;
    use vesper_domain::BoundedString;
    use vesper_provider::{SuperpowerKind, SuperpowerScope};

    fn provider() -> ProviderId {
        ProviderId::new("test").unwrap()
    }

    fn descriptor(id: &str, alias: Option<&str>) -> SuperpowerDescriptor {
        SuperpowerDescriptor {
            id: BoundedString::new(id).unwrap(),
            provider_id: provider(),
            display_name: BoundedString::new(id).unwrap(),
            kind: SuperpowerKind::Choice,
            scope: SuperpowerScope::Request,
            default_value: SuperpowerValue::Choice {
                value: BoundedString::new("default").unwrap(),
            },
            allowed_values: Vec::new(),
            command_alias: alias.map(|raw| BoundedString::new(raw).unwrap()),
            help: None,
        }
    }

    #[test]
    fn descriptors_are_sorted_stably() {
        let surface = ProviderSuperpowerSurface::new(
            provider(),
            vec![
                descriptor("zai:effort", Some("effort")),
                descriptor("zai:thinking", Some("thinking")),
                descriptor("zai:effort", None), // duplicate ID becomes order-stable
            ],
        );
        let ids: Vec<_> = surface
            .descriptors()
            .iter()
            .map(|d| d.id.as_str().to_string())
            .collect();
        assert_eq!(ids, vec!["zai:effort", "zai:effort", "zai:thinking"]);
    }

    #[test]
    fn by_alias_resolves_only_when_alias_matches() {
        let surface = ProviderSuperpowerSurface::new(
            provider(),
            vec![
                descriptor("zai:effort", Some("effort")),
                descriptor("zai:thinking", Some("thinking")),
            ],
        );
        assert!(surface.by_alias("effort").is_some());
        assert!(surface.by_alias("thinking").is_some());
        assert!(surface.by_alias("model").is_none());
    }

    #[test]
    fn defaults_seed_every_descriptor() {
        let surface = ProviderSuperpowerSurface::new(
            provider(),
            vec![
                descriptor("zai:effort", Some("effort")),
                descriptor("zai:thinking", Some("thinking")),
            ],
        );
        let defaults = surface.defaults();
        assert_eq!(defaults.len(), 2);
        assert_eq!(
            defaults.get("zai:effort", None),
            Some(SuperpowerValue::Choice {
                value: BoundedString::new("default").unwrap(),
            })
        );
    }

    #[test]
    fn overrides_can_be_updated_and_snapshotted() {
        let mut overrides = SuperpowerOverrides::default();
        let first = overrides.set(
            "zai:effort",
            SuperpowerValue::Choice {
                value: BoundedString::new("high").unwrap(),
            },
        );
        assert!(first.is_none());
        let previous = overrides.set(
            "zai:effort",
            SuperpowerValue::Choice {
                value: BoundedString::new("max").unwrap(),
            },
        );
        assert!(matches!(previous, Some(SuperpowerValue::Choice { .. })));
        let snapshot = overrides.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].0, "zai:effort");
    }

    #[test]
    fn render_help_lines_use_aliases() {
        let surface = ProviderSuperpowerSurface::new(
            provider(),
            vec![
                descriptor("zai:effort", Some("effort")),
                descriptor("zai:thinking", Some("thinking")),
            ],
        );
        let lines = surface.render_help_lines();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().any(|line| line.contains("/effort")));
        assert!(lines.iter().any(|line| line.contains("/thinking")));
    }
}
