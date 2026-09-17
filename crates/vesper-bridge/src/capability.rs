//! Versioned capability manifest (VB-PRD-001 §6.2, BR-03).
//!
//! Availability and implementation status are **separate axes**: an
//! operation can be natively implemented yet currently unauthorized, or
//! emulated yet available. `Unknown` implementation status is never
//! dispatchable — the single most important honesty rule of the manifest.

use serde::{Deserialize, Serialize};

/// Namespaced capability id, e.g. `media.timeline.create`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub String);

impl CapabilityId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.split('.').all(|segment| {
                !segment.is_empty()
                    && segment
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            && value.contains('.');
        if !valid {
            return Err("capability id must be dot-separated segments of [A-Za-z0-9_], e.g. media.timeline.create".into());
        }
        Ok(Self(value))
    }
}

/// Whether an operation can currently be used at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Usable now.
    Available,
    /// Implemented, but requires an authority grant first.
    PermissionRequired,
    /// A dependency (application, driver, edition, runtime) is missing.
    DependencyMissing,
    /// Not usable (wrong version/edition/platform/feature).
    Unavailable,
}

/// How the operation is realized on this adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Implementation {
    /// Supported application API / documented native route.
    Native,
    /// Semantic or visual emulation with weaker guarantees.
    Emulated,
    /// Known-absent on this combination.
    Unsupported,
    /// Not established. **Never dispatchable; never reportable as supported.**
    Unknown,
}

/// Mutability class feeding the permission gate's ToolExecutionClass mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutability {
    ReadOnly,
    Mutating,
    Destructive,
    ExternalTransmission,
}

/// Preferred route ladder (AD-02): native API → semantic → bounded visual.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    /// Documented application API / SDK.
    NativeApi = 1,
    /// Semantic surface (DOM, accessibility, structured action).
    Semantic = 2,
    /// Bounded visual input through a driver.
    Visual = 3,
}

/// How the operation is delivered relative to foreground focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Verified background-safe (no foreground input required).
    Background,
    /// Requires foreground/seat input ownership.
    Foreground,
}

/// How the adapter can verify the postcondition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    /// Independent artifact/readback verification available.
    Independent,
    /// Application acknowledgment only (weakest; never "verified").
    AcknowledgmentOnly,
    /// No verification route; success claims are impossible.
    None,
}

/// One capability entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRecord {
    pub id: CapabilityId,
    /// Schema revision/hash of the operation's argument contract.
    pub schema_version: u32,
    /// Availability axis.
    pub availability: Availability,
    /// Implementation axis.
    pub implementation: Implementation,
    pub mutability: Mutability,
    /// Route this adapter implements (AD-02 ladder position).
    pub route: RouteKind,
    pub delivery: DeliveryMode,
    /// Verification quality this capability can offer.
    pub verification: VerificationMethod,
    /// Known limitations text (bounded).
    pub limitations: String,
}

impl CapabilityRecord {
    /// Dispatchability per BR-03: unknown is not supported, unavailable is
    /// not dispatchable, permission gaps are not silent.
    #[must_use]
    pub fn dispatchable(&self) -> bool {
        !matches!(
            self.implementation,
            Implementation::Unknown | Implementation::Unsupported
        ) && matches!(
            self.availability,
            Availability::Available | Availability::PermissionRequired
        )
    }

    /// Effective mutability for the permission gate. Compound tools are
    /// classified by this — the selected operation's class, never the outer
    /// tool name (§8, BR-07).
    #[must_use]
    pub fn effective_mutability(&self) -> Mutability {
        self.mutability
    }
}

/// Manifest revision. A generation bump invalidates cached approvals
/// derived from the previous manifest (BR-28, AT-35).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub schema_version: u32,
    /// Manifest generation; increments on any record change.
    pub generation: u32,
    pub adapter_id: String,
    pub records: Vec<CapabilityRecord>,
}

impl CapabilityManifest {
    pub fn new(
        adapter_id: impl Into<String>,
        generation: u32,
        records: Vec<CapabilityRecord>,
    ) -> Self {
        Self {
            schema_version: 1,
            generation,
            adapter_id: adapter_id.into(),
            records,
        }
    }

    pub fn record(&self, id: &CapabilityId) -> Option<&CapabilityRecord> {
        self.records.iter().find(|record| record.id == *id)
    }

    /// Bump generation with the new record set (approval invalidation hook).
    #[must_use]
    pub fn revise(&self, records: Vec<CapabilityRecord>) -> Self {
        Self {
            schema_version: self.schema_version,
            generation: self.generation + 1,
            adapter_id: self.adapter_id.clone(),
            records,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(implementation: Implementation, availability: Availability) -> CapabilityRecord {
        CapabilityRecord {
            id: CapabilityId::new("media.timeline.create").unwrap(),
            schema_version: 1,
            availability,
            implementation,
            mutability: Mutability::Mutating,
            route: RouteKind::NativeApi,
            delivery: DeliveryMode::Background,
            verification: VerificationMethod::Independent,
            limitations: "none recorded".into(),
        }
    }

    #[test]
    fn unknown_is_never_dispatchable_or_supported() {
        assert!(!record(Implementation::Unknown, Availability::Available).dispatchable());
        assert!(!record(Implementation::Unsupported, Availability::Available).dispatchable());
        assert!(record(Implementation::Native, Availability::Available).dispatchable());
        // Native but unauthorized is dispatchable-pending-authority: the
        // permission gate decides, and PermissionRequired keeps it honest.
        assert!(record(Implementation::Native, Availability::PermissionRequired).dispatchable());
        assert!(!record(Implementation::Native, Availability::DependencyMissing).dispatchable());
        assert!(!record(Implementation::Native, Availability::Unavailable).dispatchable());
    }

    #[test]
    fn capability_ids_are_namespaced_dotted_segments() {
        assert!(CapabilityId::new("media.timeline.create").is_ok());
        assert!(
            CapabilityId::new("shell").is_err(),
            "bare names are not namespaced"
        );
        assert!(CapabilityId::new("media..cut").is_err());
        assert!(CapabilityId::new("media.cut!").is_err());
        assert!(CapabilityId::new("").is_err());
    }

    #[test]
    fn manifest_revision_bumps_generation_for_approval_invalidation() {
        let base = CapabilityManifest::new(
            "resolve-sidecar",
            3,
            vec![record(Implementation::Native, Availability::Available)],
        );
        let revised = base.revise(vec![record(
            Implementation::Native,
            Availability::Unavailable,
        )]);
        assert_eq!(revised.generation, 4);
        assert_ne!(base.generation, revised.generation);
        assert!(revised.generation > base.generation);
        assert!(
            !revised.records[0].dispatchable(),
            "revision that made it unavailable blocks dispatch"
        );
    }

    #[test]
    fn compound_classification_uses_selected_operation_class() {
        // A read-looking wrapper around a destructive op reports destructive.
        let mut destructive = record(Implementation::Native, Availability::Available);
        destructive.mutability = Mutability::Destructive;
        assert_eq!(destructive.effective_mutability(), Mutability::Destructive);
        let mut ro = record(Implementation::Native, Availability::Available);
        ro.mutability = Mutability::ReadOnly;
        assert_eq!(ro.effective_mutability(), Mutability::ReadOnly);
    }
}
