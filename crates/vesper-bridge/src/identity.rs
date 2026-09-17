//! Generation-bound application identity (VB-PRD-001 §6.1).
//!
//! No single raw integer is durable identity: PIDs and window handles are
//! reused. An application instance binds an OS-stable application identity,
//! executable identity, PID **and process-start evidence**, plus the window
//! and user/desktop session, under a monotonically increasing generation
//! counter. A restart is always a new generation; a binding from an older
//! generation is refused (BR-02, BR-15, AT-03).

use serde::{Deserialize, Serialize};

/// Monotonic binding generation for one logical application slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Generation(pub u64);

impl Generation {
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Host-minted Bridge session identity (distinct from chat/run/tool-call IDs).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BridgeSessionId(pub String);

impl BridgeSessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 {
            return Err("bridge session id must be 1–128 chars".into());
        }
        Ok(Self(value))
    }
}

/// Host-minted, generation-bound application instance identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApplicationInstanceId {
    /// Logical slot this instance occupies (e.g. per application + seat).
    pub slot: String,
    /// Generation within the slot; restarts bump it.
    pub generation: Generation,
}

impl ApplicationInstanceId {
    pub fn new(slot: impl Into<String>, generation: Generation) -> Result<Self, String> {
        let slot = slot.into();
        if slot.is_empty() || slot.len() > 128 {
            return Err("application slot must be 1–128 chars".into());
        }
        Ok(Self { slot, generation })
    }
}

/// The concrete evidence that fixes one running application target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationBinding {
    /// OS-stable application identity where available (bundle id, app id).
    pub application_key: String,
    /// Executable identity (absolute path and/or digest) where available.
    pub executable_identity: String,
    /// PID plus process-start evidence (e.g. start time) so reuse is detectable.
    pub pid: u32,
    pub process_start_evidence: String,
    /// Window identity where the route is window-scoped.
    pub window_identity: Option<String>,
    /// Owning user/desktop session.
    pub session_key: String,
    /// Adapter-specific project/document identifier, if the route has one.
    pub document_ref: Option<String>,
}

/// Result of matching a discovery candidate against a binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetMatch {
    /// Exactly one candidate matches; attachment may proceed.
    Exact,
    /// Multiple candidates match; attachment must wait for explicit choice.
    Ambiguous,
    /// No candidate matches the requested binding.
    NotFound,
}

/// Identity of one capture stream, in the strongest form the negotiated
/// portal/driver revision provides. Node IDs alone are NOT durable identity
/// (they can be reused); a serial/object identity must be present when the
/// platform provides one (VB-PRD-001 §6.1, ScreenCast caveat).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureIdentity {
    /// Driver/portal-provided node or stream handle (may be reused).
    pub node_handle: u64,
    /// Durable object serial where the platform exposes one.
    pub object_serial: Option<u64>,
    /// Logical surface identity (window/resource) the stream targets.
    pub surface_key: String,
}

impl CaptureIdentity {
    /// Durable equality: two capture identities are the same stream only
    /// when their durable components match. If a serial is absent on both
    /// sides, handle+surface equality is the best available and is flagged
    /// as such by the caller, never silently upgraded.
    #[must_use]
    pub fn durable_eq(&self, other: &Self) -> bool {
        match (self.object_serial, other.object_serial) {
            (Some(a), Some(b)) => a == b && self.surface_key == other.surface_key,
            (None, None) => {
                self.node_handle == other.node_handle && self.surface_key == other.surface_key
            }
            _ => false,
        }
    }
}

/// Revision of an application-side resource (project, document, timeline).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceRevision(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_changes_generation_and_old_generation_is_refused_by_ordering() {
        let g1 = Generation(1);
        let g2 = g1.next();
        assert!(g2 > g1);
        let old = ApplicationInstanceId::new("resolve/seat-0", g1).unwrap();
        let new = ApplicationInstanceId::new("resolve/seat-0", g2).unwrap();
        assert_ne!(old, new);
        // The session core refuses dispatch when the observed generation
        // exceeds the bound one (tested in session.rs); ordering is the
        // primitive that makes a PID-reuse restart detectable.
        assert!(new.generation > old.generation);
    }

    #[test]
    fn capture_identity_requires_serial_for_durable_across_handle_reuse() {
        let a = CaptureIdentity {
            node_handle: 7,
            object_serial: Some(1001),
            surface_key: "win-a".into(),
        };
        let reused = CaptureIdentity {
            node_handle: 7,
            object_serial: Some(2002),
            surface_key: "win-a".into(),
        };
        assert!(
            !a.durable_eq(&reused),
            "same reused node handle with a different serial is NOT the same stream"
        );
        let same = CaptureIdentity {
            node_handle: 7,
            object_serial: Some(1001),
            surface_key: "win-a".into(),
        };
        assert!(a.durable_eq(&same));
    }

    #[test]
    fn ids_reject_empty_and_oversized_values() {
        assert!(BridgeSessionId::new("").is_err());
        assert!(BridgeSessionId::new("s").is_ok());
        assert!(ApplicationInstanceId::new("", Generation(1)).is_err());
        assert!(ApplicationInstanceId::new("a".repeat(129), Generation(1)).is_err());
    }
}
