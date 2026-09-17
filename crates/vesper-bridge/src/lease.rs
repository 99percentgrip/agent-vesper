//! Exclusive mutation leases with fencing (VB-PRD-001 §8.2, BR-14/15).
//!
//! Two scopes: resource-scoped exclusive mutation leases (a project cannot
//! have two conflicting writers) and a desktop/seat-wide foreground-input
//! lease (the OS focus is shared). Leases carry a fencing generation and
//! expiry; the supervised worker must reject stale generations before
//! dispatch. Revocation of ordinary leases must NOT disable the narrowly
//! scoped emergency input-release path (§8.4).

use serde::{Deserialize, Serialize};

/// Lease identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeaseId(pub String);

/// What a lease exclusively covers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResourceKey {
    /// An application document/project/timeline resource.
    Document(String),
    /// The desktop/seat foreground input (global).
    SeatInput(String),
}

/// Lease admission decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseDecision {
    Granted,
    /// Another live lease holds the resource.
    Conflict {
        holder: LeaseId,
        expires_in_ms: u64,
    },
    /// The requesting fence generation is stale (BR-15/§8.2).
    StaleFence {
        observed: u64,
        required: u64,
    },
}

/// Pure bookkeeping state for one lease (the host owns time).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseState {
    pub id: LeaseId,
    pub resource: ResourceKey,
    /// Fencing generation; dispatch below the current generation is refused.
    pub fence: u64,
    /// Remaining milliseconds of validity at decision time.
    pub remaining_ms: u64,
    /// Whether this lease owns driver-held inputs (keyboard/buttons).
    pub holds_input: bool,
    /// Owning Bridge session (H1): emergency release is scoped to the
    /// stopping session's own leases; a shared fence state never lets
    /// one session's stop sweep another session's live input lease.
    pub owner: Option<BridgeSessionIdRef>,
}

/// Owner identity carried on leases (a small string, not the full session
/// handle) so `serde` and `PartialEq` stay cheap.
pub type BridgeSessionIdRef = String;

impl LeaseState {
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.remaining_ms == 0
    }
}

/// Tracks live leases per resource and decides admission.
#[derive(Default)]
pub struct FenceState {
    resources: std::collections::BTreeMap<ResourceKey, LeaseState>,
    /// Current fencing generation per resource; only increases.
    fences: std::collections::BTreeMap<ResourceKey, u64>,
    /// Latest fence generation whose emergency release already ran.
    emergency_released_through: u64,
    /// Revoked input-holding leases whose driver release was never
    /// confirmed (§8.4/NF-03): emergency release must still be able to
    /// reach them — ordinary revocation may not strand held inputs.
    stranded_inputs: Vec<LeaseState>,
}

impl FenceState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current fence for a resource (0 when never leased).
    #[must_use]
    pub fn fence(&self, resource: &ResourceKey) -> u64 {
        self.fences.get(resource).copied().unwrap_or(0)
    }

    /// Admit or refuse a lease request. One exclusive holder per resource;
    /// a stale fence never wins (NF-05: one in-flight mutation per resource).
    pub fn admit(&mut self, requested: LeaseState) -> LeaseDecision {
        let current_fence = self.fence(&requested.resource);
        if requested.fence < current_fence {
            return LeaseDecision::StaleFence {
                observed: requested.fence,
                required: current_fence,
            };
        }
        if let Some(holder) = self.resources.get(&requested.resource)
            && !holder.is_expired()
            && holder.id != requested.id
        {
            return LeaseDecision::Conflict {
                holder: holder.id.clone(),
                expires_in_ms: holder.remaining_ms,
            };
        }
        let fence = current_fence.max(requested.fence) + 1;
        self.fences.insert(requested.resource.clone(), fence);
        let resource = requested.resource.clone();
        let mut granted = requested;
        granted.fence = fence;
        self.resources.insert(resource, granted.clone());
        LeaseDecision::Granted
    }

    /// Revoke (stop path) a lease. Returns whether input the driver still
    /// holds needs the emergency-release path — revocation must not strand
    /// held keys/buttons (§8.4, NF-03).
    pub fn revoke(&mut self, id: &LeaseId) -> Option<LeaseState> {
        let resource = self
            .resources
            .values()
            .find(|lease| &lease.id == id)?
            .resource
            .clone();
        let removed = self.resources.remove(&resource);
        // §8.4/NF-03: a revoked lease that holds driver inputs must stay
        // reachable for emergency release until that release is confirmed.
        if let Some(lease) = &removed
            && lease.holds_input
        {
            self.stranded_inputs.push(lease.clone());
        }
        removed
    }

    /// Revoke by lease token (C1): the authorize path stores the token at
    /// admission; a post-admit denial revokes by that token without
    /// needing the full `LeaseState`.
    pub fn revoke_by_token(&mut self, token: &str) {
        let target = self
            .resources
            .values()
            .find(|lease| lease.id.0 == token)
            .map(|lease| (lease.resource.clone(), lease.id.clone()));
        if let Some((resource, id)) = target {
            self.revoke(&id);
            let _ = resource;
        }
    }

    /// Live leases across all resources (C1 verification surface).
    #[must_use]
    pub fn live_leases(&self) -> Vec<LeaseState> {
        self.resources.values().cloned().collect()
    }

    /// Owner-scoped emergency input release (H1): releases input-holding
    /// leases owned by `owner` (live and stranded). A shared fence state
    /// may hold OTHER sessions' live input leases — those are never
    /// touched by this call. Leases without an owner attribute to `None`
    /// and are only released by an owner-less stop (legacy tests).
    pub fn emergency_release_inputs_owned(
        &mut self,
        owner: &BridgeSessionIdRef,
    ) -> Vec<LeaseState> {
        let mut to_release = Vec::new();
        let mut still_live = std::collections::BTreeMap::new();
        for (key, lease) in self.resources.iter() {
            let is_owner = lease
                .owner
                .as_ref()
                .is_none_or(|lease_owner| lease_owner == owner);
            if lease.holds_input && is_owner {
                to_release.push(lease.clone());
                // §8.4/NF-03: a released-but-UNCONFIRMED input stays
                // stranded so a later stop can still surface it — the
                // stranded record is cleared only by an explicit
                // settle_input_release (host confirmation), never by the
                // release scan itself.
                self.stranded_inputs.push(lease.clone());
            } else {
                still_live.insert(key.clone(), lease.clone());
            }
        }
        self.resources = still_live;
        // Stranded inputs: same owner scoping. Released entries STAY in
        // the stranded list (reporting an already-requested release is
        // honest; silently forgetting an unconfirmed one is not).
        let mut still_stranded = Vec::new();
        for lease in std::mem::take(&mut self.stranded_inputs) {
            let is_owner = lease
                .owner
                .as_ref()
                .is_none_or(|lease_owner| lease_owner == owner);
            if is_owner {
                to_release.push(lease.clone());
                still_stranded.push(lease);
            }
        }
        self.stranded_inputs = still_stranded;
        to_release
    }

    /// Validate that a dispatch's lease is live and unfenced-out.
    #[must_use]
    pub fn validates(&self, lease: &LeaseState) -> bool {
        self.resources.get(&lease.resource).is_some_and(|live| {
            live.id == lease.id && live.fence == lease.fence && !live.is_expired()
        })
    }

    /// Emergency input release: applies to every input-holding lease at or
    /// below `fence`, regardless of prior revocation, and always remains
    /// available after ordinary revocation (BR-17, NF-03). Returns the
    /// leases whose inputs must be released.
    pub fn emergency_release_inputs(&mut self, fence: u64) -> Vec<LeaseState> {
        let mut to_release = Vec::new();
        for (_, lease) in self.resources.iter() {
            if lease.holds_input && lease.fence <= fence {
                to_release.push(lease.clone());
            }
        }
        // Also include revoked-but-recorded input holders: we keep a
        // monotonically advancing emergency mark so release happens once per
        // fence generation even after revocation removed the live entry.
        let mut still_stranded = Vec::new();
        for lease in self.stranded_inputs.drain(..) {
            if lease.fence <= fence {
                to_release.push(lease);
            } else {
                still_stranded.push(lease);
            }
        }
        self.stranded_inputs = still_stranded;
        if fence > self.emergency_released_through {
            self.emergency_released_through = fence;
        }
        to_release
    }

    /// Whether emergency release already covered this fence generation.
    /// L2: read by the stop report so a repeated stop can honestly say
    /// "release already requested for generation N".
    #[must_use]
    pub fn emergency_released(&self, fence: u64) -> bool {
        fence <= self.emergency_released_through
    }

    /// L2: the highest fence generation emergency release has covered.
    #[must_use]
    pub fn emergency_released_through(&self) -> u64 {
        self.emergency_released_through
    }

    /// Settle input release after the host confirms the driver released
    /// its inputs (BR-17): clears both live input-holding leases and the
    /// stranded-input records, so settled inputs are never re-reported
    /// by a later emergency-release scan. Owner-scoped (H1): only the
    /// confirming session's inputs are settled.
    pub fn settle_input_release_owned(&mut self, owner: &BridgeSessionIdRef) {
        self.resources
            .retain(|_, lease| !(lease.holds_input && lease.owner.as_ref() == Some(owner)));
        self.stranded_inputs
            .retain(|lease| !(lease.holds_input && lease.owner.as_ref() == Some(owner)));
    }

    /// Legacy settle (pre-H1 call sites/tests): clears all input leases.
    pub fn settle_input_release(&mut self) {
        self.resources.retain(|_, lease| !lease.holds_input);
        self.stranded_inputs.clear();
    }
}

/// Input-lease state after a stop (BR-17 settlement reporting).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputLeaseState {
    /// Driver confirmed release of owned inputs.
    Released,
    /// Release could not be confirmed; unsafe-state warning is required.
    Unconfirmed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(resource: ResourceKey, fence: u64, holds_input: bool) -> LeaseState {
        LeaseState {
            id: LeaseId("l1".into()),
            resource,
            fence,
            remaining_ms: 10_000,
            holds_input,
            owner: None,
        }
    }

    #[test]
    fn one_exclusive_writer_per_resource() {
        let mut fences = FenceState::new();
        let doc = ResourceKey::Document("project-a".into());
        assert!(matches!(
            fences.admit(lease(doc.clone(), 0, false)),
            LeaseDecision::Granted
        ));
        let second = LeaseState {
            id: LeaseId("l2".into()),
            resource: doc.clone(),
            fence: 1,
            remaining_ms: 10_000,
            holds_input: false,
            owner: None,
        };
        match fences.admit(second) {
            LeaseDecision::Conflict { .. } => {}
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn stale_fence_never_wins() {
        let mut fences = FenceState::new();
        let seat = ResourceKey::SeatInput("seat-0".into());
        fences.admit(lease(seat.clone(), 5, true)); // fence becomes 6
        match fences.admit(lease(seat, 2, true)) {
            LeaseDecision::StaleFence { observed, required } => {
                assert_eq!((observed, required), (2, 6));
            }
            other => panic!("expected stale fence, got {other:?}"),
        }
    }

    #[test]
    fn revocation_does_not_strand_emergency_release() {
        let mut fences = FenceState::new();
        let seat = ResourceKey::SeatInput("seat-0".into());
        fences.admit(lease(seat.clone(), 0, true));
        let admitted = match fences.resources.get(&seat) {
            Some(state) => state.clone(),
            None => panic!("expected a granted lease"),
        };
        let fence = admitted.fence;
        fences.revoke(&admitted.id); // stop path revokes ordinary authority
        assert!(!fences.validates(&admitted));
        // Emergency release still covers that fence generation: the
        // revoked-but-unconfirmed input holder MUST be reachable (§8.4,
        // NF-03) — ordinary revocation may not strand held inputs.
        assert!(!fences.emergency_released(fence));
        let released = fences.emergency_release_inputs(fence);
        assert!(
            released.len() == 1 && released[0].holds_input,
            "revoked input holder must still be reported for emergency release; got {released:?}"
        );
        assert!(fences.emergency_released(fence));
        // After the emergency release ran, a second call does not repeat it.
        assert!(fences.emergency_release_inputs(fence).is_empty());
    }

    #[test]
    fn live_input_leases_are_returned_for_emergency_release() {
        let mut fences = FenceState::new();
        fences.admit(lease(ResourceKey::SeatInput("seat-0".into()), 0, true));
        let current = fences.fence(&ResourceKey::SeatInput("seat-0".into()));
        let released = fences.emergency_release_inputs(current);
        assert_eq!(released.len(), 1);
        assert!(released[0].holds_input);
    }

    #[test]
    fn expired_lease_does_not_conflict() {
        let mut fences = FenceState::new();
        let doc = ResourceKey::Document("p".into());
        fences.admit(LeaseState {
            id: LeaseId("old".into()),
            resource: doc.clone(),
            fence: 0,
            remaining_ms: 0,
            holds_input: false,
            owner: None,
        });
        assert!(matches!(
            fences.admit(lease(doc, 1, false)),
            LeaseDecision::Granted
        ));
    }

    use std::fmt::Debug;
    fn _assert_debug<T: Debug>(_: &T) {}
    #[test]
    fn decisions_are_debuggable_for_diagnostics() {
        _assert_debug(&LeaseDecision::Granted);
    }
}
