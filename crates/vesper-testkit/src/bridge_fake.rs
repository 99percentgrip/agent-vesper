//! Deterministic reference adapter for Bridge tests (VB-PRD-001 Phase 1).
//!
//! This is a **fake driver**: it exists only inside test code and proves
//! protocol/state behavior. It can never appear in production builds
//! (nothing outside tests depends on it), every type is visibly named
//! `Fake*`, and its observations carry `"fixture": true` markers so fake
//! evidence cannot masquerade as real application evidence.

use std::collections::BTreeMap;

use vesper_bridge::capability::{
    Availability, CapabilityId, CapabilityManifest, CapabilityRecord, DeliveryMode, Implementation,
    Mutability, RouteKind, VerificationMethod,
};

/// A visibly-fake application binding for deterministic tests.
pub struct FakeApplication {
    pub generation: u64,
    /// Simulated document revision the fake advances on each applied write.
    pub revision: u64,
    /// Recorded dispatched operations (request id → capability).
    pub dispatched: BTreeMap<String, String>,
}

impl FakeApplication {
    #[must_use]
    pub fn new() -> Self {
        Self {
            generation: 1,
            revision: 1,
            dispatched: BTreeMap::new(),
        }
    }

    /// Restart the fake application: a NEW generation, exactly like a real
    /// process restart invalidating PID-based bindings (BR-02/AT-03).
    pub fn restart(&mut self) {
        self.generation += 1;
        self.revision += 1;
    }

    /// Apply a dispatched mutation: advance the revision (a real app
    /// changes state; the fake models that as revision +1).
    pub fn apply(&mut self, request_id: &str, capability: &str) {
        self.dispatched
            .insert(request_id.to_owned(), capability.to_owned());
        self.revision += 1;
    }

    /// The fake's truthful manifest: a small read/inspect set and one
    /// mutating operation, all natively "implemented" *by the fake*.
    #[must_use]
    pub fn manifest() -> CapabilityManifest {
        let records = vec![
            CapabilityRecord {
                id: CapabilityId::new("fixture.project.inspect").unwrap(),
                schema_version: 1,
                availability: Availability::Available,
                implementation: Implementation::Native,
                mutability: Mutability::ReadOnly,
                route: RouteKind::NativeApi,
                delivery: DeliveryMode::Background,
                verification: VerificationMethod::Independent,
                limitations: "fake driver; test-only evidence".into(),
            },
            CapabilityRecord {
                id: CapabilityId::new("fixture.timeline.create").unwrap(),
                schema_version: 1,
                availability: Availability::Available,
                implementation: Implementation::Native,
                mutability: Mutability::Mutating,
                route: RouteKind::NativeApi,
                delivery: DeliveryMode::Background,
                verification: VerificationMethod::Independent,
                limitations: "fake driver; test-only evidence".into(),
            },
            CapabilityRecord {
                id: CapabilityId::new("fixture.effect.unknown_magic").unwrap(),
                schema_version: 1,
                availability: Availability::Available,
                implementation: Implementation::Unknown,
                mutability: Mutability::Mutating,
                route: RouteKind::NativeApi,
                delivery: DeliveryMode::Background,
                verification: VerificationMethod::None,
                limitations: "deliberately unknown to prove unknown ≠ supported".into(),
            },
        ];
        CapabilityManifest::new("fake-driver", 1, records)
    }
}

impl Default for FakeApplication {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_binds_a_new_generation() {
        let mut app = FakeApplication::new();
        assert_eq!(app.generation, 1);
        app.restart();
        assert_eq!(
            app.generation, 2,
            "restart must produce a new generation, not a reused PID-style identity"
        );
    }

    #[test]
    fn applied_writes_advance_the_revision() {
        let mut app = FakeApplication::new();
        let before = app.revision;
        app.apply("req-1", "fixture.timeline.create");
        assert_eq!(app.revision, before + 1);
        assert_eq!(
            app.dispatched.get("req-1").map(String::as_str),
            Some("fixture.timeline.create")
        );
    }

    #[test]
    fn fake_manifest_keeps_unknown_distinct_from_native() {
        let manifest = FakeApplication::manifest();
        let unknown = manifest
            .record(&CapabilityId::new("fixture.effect.unknown_magic").unwrap())
            .unwrap();
        assert_eq!(unknown.implementation, Implementation::Unknown);
        assert!(!unknown.dispatchable());
        let native = manifest
            .record(&CapabilityId::new("fixture.timeline.create").unwrap())
            .unwrap();
        assert_eq!(native.implementation, Implementation::Native);
        assert!(native.dispatchable());
    }
}
