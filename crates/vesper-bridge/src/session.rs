//! Bridge session core: admission, dispatch validation, revalidation
//! before dispatch, stop, quarantine
//! (VB-PRD-001 §8.1/8.4, BR-04/07/08/14/15/16/17/18/21/30, NF-02/05).
//!
//! One owner per application session. This pure core holds the state
//! machines and derives every decision; the hosted layer performs I/O and
//! drives it. No clock, no network, no capture lives here — callers supply
//! elapsed time and observations.

use serde::{Deserialize, Serialize};

use crate::capability::{Availability, CapabilityManifest, Implementation, Mutability};
use crate::error::BridgeError;
use crate::identity::{ApplicationInstanceId, BridgeSessionId, Generation, ResourceRevision};
use crate::journal::{
    IntentRecord, JournalPort, Settlement, classify_retry, classify_semantic_retry,
};
use crate::lease::{FenceState, InputLeaseState, LeaseDecision, LeaseState};
use crate::observation::Observation;
use crate::operation::{AuthorityRequest, Idempotency, OperationOutcome, OperationRequestId};
/// Session state machine (§8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationSessionState {
    Discovered,
    Connecting,
    Ready,
    Paused,
    Recovering,
    /// Uncertain cleanup/queued writes: no new mutations (BR-18).
    Quarantined,
    Closed,
}

/// Why admission refused a dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DenialReason {
    PlanMode,
    ReadOnlyPermission,
    NotReady(ApplicationSessionState),
    Quarantined,
    AdmissionClosed,
    Capability(BridgeError),
    /// Lease refusal with the actionable detail (H7): WHICH holder owns
    /// the resource and how long until its lease expires — the two facts
    /// a retry decision needs.
    LeaseConflict {
        holder: String,
        expires_in_ms: u64,
    },
    /// Stale fence with the observed/required pair (H7).
    StaleFence {
        observed: u64,
        required: u64,
    },
    Lease(BridgeError),
    StalePrecondition,
    DuplicateSuppressed,
}

/// Admission gate state (stop path closes this, not the model loop).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Open,
    /// Human stop: no new dispatch until explicit resume (BR-17, NF-02).
    Closed,
}

/// Outcome of the independent stop path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopOutcome {
    pub admission: Admission,
    /// Input leases whose driver-owned inputs must be released.
    pub inputs_to_release: Vec<LeaseState>,
    /// Whether release is confirmed (host fills after driver ack).
    pub input_release: InputLeaseState,
    /// Jobs that may still be running in the application (BR-13/17).
    pub outstanding_jobs: Vec<String>,
}

/// Outcome of closing a session (C3: closing must SURFACE unresolved
/// state — a superseded quarantine, inputs pending release and
/// outstanding jobs — instead of silently erasing it with the session).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseReport {
    /// The session was Quarantined when close was requested; that
    /// uncertainty outlives this session object and must be surfaced,
    /// not dropped (§7).
    pub was_quarantined: bool,
    /// Input leases whose driver-owned inputs were never released.
    pub inputs_pending_release: usize,
    /// Jobs that may still be running in the application.
    pub outstanding_jobs: Vec<String>,
}

/// Record of one operation tracked by the session.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationRecord {
    pub request_id: OperationRequestId,
    pub outcome: OperationOutcome,
}

/// The pure Bridge session core.
pub struct BridgeSession {
    pub id: BridgeSessionId,
    pub application: ApplicationInstanceId,
    state: ApplicationSessionState,
    admission: Admission,
    /// Shared lease coordinator: exclusivity is per RESOURCE, not per
    /// session, so two sessions cannot hold one document (BR-14, AT-16).
    fences: std::sync::Arc<std::sync::Mutex<FenceState>>,
    manifest: CapabilityManifest,
    /// Latest observation revision accepted for this session.
    latest_observation_revision: u64,
    /// Highest observation revision already used to ground a MUTATING
    /// dispatch (H5: per-mutation freshness — a mutating dispatch may
    /// never reuse the observation a previous mutation was grounded on).
    latest_mutation_observation_revision: u64,
    /// Current authority epoch (H2: minted fresh on connect/resume so a
    /// pre-renewal approval can never authorize a post-renewal dispatch).
    authority_epoch: u32,
    /// Monotonic request counter (ids must survive record eviction).
    request_sequence: u64,
    /// Jobs reported as possibly still running (render queue etc.).
    outstanding_jobs: Vec<String>,
    /// M6: jobs evicted from the bounded outstanding-jobs window. The
    /// stop report must say the LIST is bounded, not exhaustive — a user
    /// seeing "0 jobs" after 256 pushes must not believe work settled.
    evicted_jobs: u64,
    /// Whether the host has CONFIRMED the driver released its inputs
    /// after a stop (BR-17 settlement). Starts unconfirmed; only an
    /// explicit host confirmation reaches `Released`.
    input_release_confirmed: bool,
    /// Settled operation records (bounded by the core, NF-05/AT-37: the
    /// oldest are evicted past the cap so a runaway caller cannot grow
    /// session memory without limit).
    records: std::collections::BTreeMap<String, OperationRecord>,
}

impl BridgeSession {
    /// Core-side cap on retained operation records (NF-05, AT-37).
    pub const MAX_OPERATION_RECORDS: usize = 512;
    /// Core-side cap on outstanding-job strings (BR-13/18, AT-37).
    pub const MAX_OUTSTANDING_JOBS: usize = 256;
    pub fn new(
        id: BridgeSessionId,
        application: ApplicationInstanceId,
        manifest: CapabilityManifest,
    ) -> Self {
        Self::with_shared_fences(
            id,
            application,
            manifest,
            std::sync::Arc::new(std::sync::Mutex::new(FenceState::new())),
        )
    }

    /// Construct with a shared lease coordinator so separate sessions over
    /// the same resources contend on one fence state (BR-14, AT-16).
    #[must_use]
    pub fn with_shared_fences(
        id: BridgeSessionId,
        application: ApplicationInstanceId,
        manifest: CapabilityManifest,
        fences: std::sync::Arc<std::sync::Mutex<FenceState>>,
    ) -> Self {
        Self {
            id,
            application,
            state: ApplicationSessionState::Discovered,
            admission: Admission::Open,
            fences,
            manifest,
            latest_observation_revision: 0,
            latest_mutation_observation_revision: 0,
            authority_epoch: 1,
            request_sequence: 0,
            input_release_confirmed: false,
            outstanding_jobs: Vec::new(),
            evicted_jobs: 0,
            records: std::collections::BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn state(&self) -> ApplicationSessionState {
        self.state
    }

    #[must_use]
    pub fn admission(&self) -> Admission {
        self.admission
    }

    /// Latest observation revision accepted by this session (test/audit).
    #[must_use]
    pub fn latest_observation_revision(&self) -> u64 {
        self.latest_observation_revision
    }

    /// Current authority epoch (H2). Dispatches must carry the epoch that
    /// was current when the approval was minted; an older epoch is stale.
    #[must_use]
    pub fn authority_epoch(&self) -> u32 {
        self.authority_epoch
    }

    /// Attach transitions Discovered → Connecting → Ready (BR-04).
    /// H2: each attach mints a fresh authority epoch — approvals minted
    /// against a previous session lifetime never carry over.
    pub fn connect(&mut self) -> Result<(), BridgeError> {
        match self.state {
            ApplicationSessionState::Discovered => {
                self.state = ApplicationSessionState::Connecting;
                self.authority_epoch = self
                    .authority_epoch
                    .checked_add(1)
                    .ok_or(BridgeError::ResourceLimit)?;
                Ok(())
            }
            ApplicationSessionState::Connecting => Ok(()),
            // H3: an already-attached (or stopped/closed/quarantined)
            // session is an INVALID STATE for attach — the transport is
            // not the problem, and reporting it as such would send the
            // caller chasing a healthy transport.
            _ => Err(BridgeError::InvalidState),
        }
    }

    pub fn mark_ready(&mut self) {
        if self.state == ApplicationSessionState::Connecting {
            self.state = ApplicationSessionState::Ready;
        }
    }

    /// Quarantine: uncertain cleanup blocks all mutations (BR-18, AT-20/25).
    pub fn quarantine(&mut self) {
        self.state = ApplicationSessionState::Quarantined;
    }

    /// Pause a Ready session (§3.1 host-focus loss / human takeover
    /// preparation): dispatch refuses through the existing readiness
    /// gate, and `resume()` returns the session to a dispatchable state.
    /// Refuses from any state that carries stronger semantics — a
    /// quarantined session stays quarantined (pause is not a resolution
    /// path), and a closed session stays closed.
    pub fn pause(&mut self) -> Result<(), BridgeError> {
        match self.state {
            ApplicationSessionState::Ready | ApplicationSessionState::Recovering => {
                self.state = ApplicationSessionState::Paused;
                Ok(())
            }
            _ => Err(BridgeError::TargetChanged),
        }
    }

    /// Close the session permanently (terminal). Closing is idempotent.
    /// After close, resume refuses (TargetChanged) — a closed session
    /// requires a fresh connect, not a resume.
    ///
    /// C3: closing RETURNS a report of unresolved state instead of
    /// silently erasing it — a superseded quarantine, inputs still
    /// pending release and outstanding jobs all outlive the session
    /// object and must be surfaced to the caller (§7).
    pub fn close(&mut self) -> CloseReport {
        let was_quarantined = self.state == ApplicationSessionState::Quarantined;
        // Emergency-release scan BEFORE the state change: the report must
        // count inputs still pending release (they are not released by
        // closing — the driver must still be told; counting is honest).
        let inputs_pending = {
            let mut fences = self
                .fences
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fences
                .emergency_release_inputs_owned(&self.id.0.clone())
                .len()
        };
        let outstanding_jobs = self.outstanding_jobs.clone();
        self.state = ApplicationSessionState::Closed;
        self.admission = Admission::Closed;
        CloseReport {
            was_quarantined,
            inputs_pending_release: inputs_pending,
            outstanding_jobs,
        }
    }

    /// Emergency-release scan surface for closing composition paths:
    /// exposes the shared fence state so a disconnect can report inputs
    /// still pending release before the session object is dropped.
    pub fn fences_for_release(&mut self) -> std::sync::Arc<std::sync::Mutex<FenceState>> {
        std::sync::Arc::clone(&self.fences)
    }

    /// The pre-dispatch gate. Runs EVERY check the PRD demands before an
    /// adapter is consulted; returns the validated envelope or a denial.
    #[allow(clippy::too_many_arguments)]
    pub fn authorize(
        &mut self,
        journal: &dyn JournalPort,
        spec: &crate::operation::OperationSpec,
        plan_mode: bool,
        read_only_permission: bool,
        current_application_generation: Generation,
        observation: &Observation,
        requested_lease: LeaseState,
        authority_generation: u32,
        elapsed_ms: u64,
    ) -> Result<AuthorityRequest, DenialReason> {
        let _ = elapsed_ms; // enforced at revalidate() (C2/M10), not here
        // 1. Stop/admission (NF-02): closed admission refuses everything.
        if self.admission == Admission::Closed {
            return Err(DenialReason::AdmissionClosed);
        }
        // 2. Session readiness.
        if self.state == ApplicationSessionState::Quarantined {
            return Err(DenialReason::Quarantined);
        }
        if !matches!(
            self.state,
            ApplicationSessionState::Ready | ApplicationSessionState::Recovering
        ) {
            return Err(DenialReason::NotReady(self.state));
        }
        // 3. Plan/read-only mode enforcement (BR-07, AT-06).
        let record = self
            .manifest
            .record(&spec.capability)
            .ok_or(DenialReason::Capability(BridgeError::CapabilityUnavailable))?
            .clone();
        let mutability = record.effective_mutability();
        // Mode enforcement precedes capability resolution detail: a denied
        // mutation is denied regardless of why the capability might also be
        // unavailable — denial outranks diagnostics (BR-07/08).
        if plan_mode && mutability != Mutability::ReadOnly {
            return Err(DenialReason::PlanMode);
        }
        if read_only_permission && mutability != Mutability::ReadOnly {
            return Err(DenialReason::ReadOnlyPermission);
        }
        if !record.dispatchable() {
            let error = if matches!(
                record.implementation,
                Implementation::Unknown | Implementation::Unsupported
            ) {
                BridgeError::CapabilityUnavailable
            } else {
                match record.availability {
                    Availability::PermissionRequired => BridgeError::PermissionRequired,
                    _ => BridgeError::CapabilityUnavailable,
                }
            };
            return Err(DenialReason::Capability(error));
        }
        // 4. Target freshness: a newer generation invalidates the binding
        // (BR-02/15, AT-03).
        if current_application_generation > self.application.generation {
            return Err(DenialReason::StalePrecondition);
        }
        // 5a. SEMANTIC duplicate suppression (§6.6) — checked BEFORE the
        // freshness checks because it is a property of the operation, not
        // the precondition: a retried unsettled mutation must be told to
        // RECONCILE (actionable), not merely to observe again (which
        // would invite a fresh-observation retry loop that re-attempts a
        // possibly-committed mutation). Runs first even when the current
        // observation is stale — the duplicate is the more dangerous fact.
        let idempotency = match mutability {
            Mutability::ReadOnly => Idempotency::ReadOnly,
            Mutability::Mutating | Mutability::Destructive | Mutability::ExternalTransmission => {
                Idempotency::NonIdempotent
            }
        };
        if !matches!(
            classify_semantic_retry(journal, spec, idempotency),
            Settlement::Fresh | Settlement::SafeToRetry
        ) {
            return Err(DenialReason::Capability(BridgeError::UnknownOutcome));
        }
        // 5. Observation freshness: superseded observations refuse (BR-15,
        // AT-13). H5: a MUTATING dispatch additionally requires an
        // observation STRICTLY NEWER than the one its predecessor
        // mutation was grounded on — the same capture may not ground two
        // mutations. Read-only dispatches may reuse the current
        // observation (cheap re-plan is safe for inspection).
        if observation.revision < self.latest_observation_revision || observation.degraded_capture {
            return Err(DenialReason::StalePrecondition);
        }
        if mutability != Mutability::ReadOnly
            && observation.revision <= self.latest_mutation_observation_revision
        {
            return Err(DenialReason::StalePrecondition);
        }
        self.latest_observation_revision = observation.revision;
        if mutability != Mutability::ReadOnly {
            self.latest_mutation_observation_revision = observation.revision;
        }
        // 5b. Authority epoch (H2, §6 renewal): dispatches must carry the
        // CURRENT epoch; an approval minted before the last stop/resume
        // (or connect) no longer authorizes anything.
        if authority_generation != self.authority_epoch {
            return Err(DenialReason::StalePrecondition);
        }
        // 6. Lease admission with fencing (BR-14/15). The session stamps
        // itself as the lease owner (H1) so emergency release is scoped.
        let lease_token = requested_lease.id.0.clone();
        let mut owned_lease = requested_lease;
        owned_lease.owner = Some(self.id.0.clone());
        // M9 (evaluated BEFORE taking the fence lock — the lock is not
        // reentrant): admitting an input-holding lease invalidates any
        // prior release confirmation; the latch covers only the batch
        // that was confirmed, not leases admitted after it.
        let admits_input_lease = owned_lease.holds_input;
        let mut fences = self
            .fences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision_detail = fences.admit(owned_lease);
        match decision_detail {
            LeaseDecision::Granted => {
                if admits_input_lease {
                    self.input_release_confirmed = false;
                }
            }
            LeaseDecision::Conflict { .. } => {
                // H7: surface the actionable detail — the holder and its
                // remaining validity — instead of flattening it away.
                if let LeaseDecision::Conflict {
                    holder,
                    expires_in_ms,
                } = decision_detail
                {
                    return Err(DenialReason::LeaseConflict {
                        holder: holder.0,
                        expires_in_ms,
                    });
                }
                return Err(DenialReason::Lease(BridgeError::LeaseConflict));
            }
            LeaseDecision::StaleFence { .. } => {
                if let LeaseDecision::StaleFence { observed, required } = decision_detail {
                    return Err(DenialReason::StaleFence { observed, required });
                }
                return Err(DenialReason::Lease(BridgeError::TargetChanged));
            }
        }
        // Release the fence lock: steps 7–8 may need it again (C1 revokes
        // the admitted lease on a post-admit denial; the std Mutex is not
        // reentrant).
        drop(fences);
        // 7. Duplicate suppression (BR-16, AT-18).
        let idempotency = match mutability {
            Mutability::ReadOnly => Idempotency::ReadOnly,
            Mutability::Mutating | Mutability::Destructive | Mutability::ExternalTransmission => {
                Idempotency::NonIdempotent
            }
        };
        // Monotonic counter, NOT records.len(): eviction under the
        // NF-05 cap would recycle ids and collide with journal entries
        // (BR-16 duplicate suppression would then misfire).
        self.request_sequence = self
            .request_sequence
            .checked_add(1)
            .ok_or(DenialReason::Capability(BridgeError::ResourceLimit))?;
        let request_id = OperationRequestId(format!("{}/{}", self.id.0, self.request_sequence));
        let request = AuthorityRequest {
            request_id: request_id.clone(),
            bridge_session: self.id.clone(),
            application: self.application.clone(),
            capability_generation: self.manifest.generation,
            authority_generation,
            lease_token: lease_token.clone(),
            spec: spec.clone(),
            preconditions: crate::operation::Preconditions {
                observation: observation.id.clone(),
                resource_revision: ResourceRevision(observation.revision),
                application_generation: current_application_generation,
            },
            timeout: crate::operation::TimeoutPolicy::default(),
            resolved_mutability: mutability,
            idempotency,
        };
        match classify_retry(journal, &request) {
            Settlement::Fresh => {}
            Settlement::DuplicateSuppressed => {
                // C1: the lease admitted at step 6 must be revoked — a
                // suppressed dispatch leaves no live authority behind.
                self.fences
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .revoke_by_token(&lease_token);
                return Err(DenialReason::DuplicateSuppressed);
            }
            Settlement::ReconcileBeforeRetry => {
                self.fences
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .revoke_by_token(&lease_token);
                return Err(DenialReason::Capability(BridgeError::UnknownOutcome));
            }
            Settlement::SafeToRetry => {}
        }
        // 8. Intent recorded BEFORE dispatch (§8.3). Failing to record is
        // fail-closed: no dispatch — and (C1) the admitted lease is
        // revoked so the denial does not wedge the resource.
        if journal
            .record_intent(IntentRecord::new(request.clone()))
            .is_err()
        {
            self.fences
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .revoke_by_token(&lease_token);
            return Err(DenialReason::Capability(BridgeError::CleanupUnconfirmed));
        }
        self.records.insert(
            request_id.0.clone(),
            OperationRecord {
                request_id,
                outcome: OperationOutcome::Dispatched,
            },
        );
        // NF-05/AT-37: the core bounds its own retained records — oldest
        // evicted first. Eviction never refuses dispatch (BR-04).
        while self.records.len() > Self::MAX_OPERATION_RECORDS {
            let oldest = self
                .records
                .keys()
                .next()
                .cloned()
                .expect("len check guarantees a key");
            self.records.remove(&oldest);
        }
        Ok(request)
    }

    /// Revalidate immediately before dispatch (PRD §6, audit C2/M10):
    /// the hosted layer MUST call this between `authorize()` and handing
    /// the envelope to an adapter. It re-checks everything the world can
    /// change in between: the lease must still be live and unfenced-out,
    /// the application generation must not have advanced past the
    /// binding, and the elapsed time must sit inside the timeout policy.
    /// `elapsed_ms` is time since authorization; expiry uses the lease's
    /// remaining validity. A refusal here is a `StalePrecondition` — the
    /// caller must re-authorize against a fresh observation, never
    /// dispatch anyway.
    pub fn revalidate(
        &self,
        lease: &LeaseState,
        current_application_generation: Generation,
        elapsed_ms: u64,
    ) -> Result<(), DenialReason> {
        // Lease liveness: exactly this lease, unfenced-out, unexpired.
        let fences = self
            .fences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !fences.validates(lease) {
            return Err(DenialReason::StalePrecondition);
        }
        // Target freshness: a newer generation than the binding refuses.
        if current_application_generation > self.application.generation {
            return Err(DenialReason::StalePrecondition);
        }
        // Deadline (M10): the timeout policy is enforced here, not stored
        // dead. `elapsed_ms` beyond the policy means the approval aged out.
        if elapsed_ms > crate::operation::TimeoutPolicy::default().timeout_ms {
            return Err(DenialReason::StalePrecondition);
        }
        // Lease expiry against elapsed time: remaining validity counted
        // from authorization cannot survive elapsed time past it.
        if elapsed_ms >= lease.remaining_ms && lease.remaining_ms > 0 {
            return Err(DenialReason::StalePrecondition);
        }
        Ok(())
    }

    /// Test/audit surface: leases currently live for this session's
    /// resources (C1 verification).
    #[must_use]
    pub fn live_leases_for_tests(&self) -> Vec<LeaseState> {
        self.fences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .live_leases()
    }

    /// Test/audit surface: the lease most recently granted by this
    /// session (C2 verification of the clean case).
    #[must_use]
    pub fn granted_lease_for_tests(&self) -> Option<LeaseState> {
        self.live_leases_for_tests().into_iter().next()
    }

    /// Test/audit surface: force the request counter (restart/replay
    /// shapes in tests).
    pub fn set_request_sequence_for_tests(&mut self, sequence: u64) {
        self.request_sequence = sequence;
    }

    /// Test/audit surface: current fence for a resource.
    #[must_use]
    pub fn current_fence_for_tests(&self, resource: &crate::lease::ResourceKey) -> u64 {
        self.fences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fence(resource)
    }

    /// Settle an operation with an observed outcome (driver ack maps to
    /// `Applied`, never `Verified` — verification is a separate act).
    pub fn settle(&mut self, request_id: &OperationRequestId, outcome: OperationOutcome) {
        if let Some(record) = self.records.get_mut(&request_id.0) {
            record.outcome = outcome;
        }
    }

    /// Cancel an in-flight operation (§7): records `Cancelled` as a
    /// terminal settlement. Resource reservations are NOT released by
    /// this call — cancellation retains reservations until work settles;
    /// lease release is the stop/settlement path's business. Returns an
    /// error when no such record exists (cancellation must be honest
    /// about what it cancelled).
    pub fn cancel(&mut self, request_id: &OperationRequestId) -> Result<(), BridgeError> {
        let record = self
            .records
            .get_mut(&request_id.0)
            .ok_or(BridgeError::UnknownOutcome)?;
        if record.outcome.is_settled() {
            // Already terminal: cancellation of settled work is a no-op
            // that reports success honestly (the work is no longer
            // in flight).
            return Ok(());
        }
        record.outcome = OperationOutcome::Cancelled;
        Ok(())
    }

    /// Settle a partially-applied operation (§7: "partial" is one of the
    /// explicit result classes). `evidence` retains what held and what
    /// did not (M7): bounded to 4 KiB and journaled with the record so
    /// audit and verification decisions can see it.
    pub fn settle_partial(
        &mut self,
        journal: &dyn JournalPort,
        request_id: &OperationRequestId,
        evidence: &str,
    ) -> Result<(), BridgeError> {
        let record = self
            .records
            .get_mut(&request_id.0)
            .ok_or(BridgeError::UnknownOutcome)?;
        record.outcome = OperationOutcome::Partial;
        // M7: journal the bounded evidence with the settled record.
        if let Some(intent) = journal.lookup(request_id) {
            let settled = intent.with_partial_evidence(evidence);
            let _ = journal.record_intent(settled);
        }
        Ok(())
    }

    /// Settle verification with INDEPENDENT evidence (BR-12, NF-13). A
    /// driver acknowledgment settled the record to `Applied`; only a
    /// verifier that actually observed the postcondition may pass
    /// `postcondition_holds: true`. Refusing the promotion is the honest
    /// outcome when the application state did not change.
    pub fn verify(
        &mut self,
        request_id: &OperationRequestId,
        postcondition_holds: bool,
    ) -> Result<(), BridgeError> {
        if !postcondition_holds {
            return Err(BridgeError::VerificationFailed);
        }
        let record = self
            .records
            .get_mut(&request_id.0)
            .ok_or(BridgeError::VerificationFailed)?;
        match record.outcome {
            OperationOutcome::Applied | OperationOutcome::Dispatched => {
                record.outcome = OperationOutcome::Verified;
                Ok(())
            }
            OperationOutcome::Verified => Ok(()),
            _ => Err(BridgeError::VerificationFailed),
        }
    }

    /// Independent stop path (BR-17, NF-02/03): closes admission, requests
    /// emergency input release, keeps outstanding jobs visible.
    /// H1: emergency release is scoped to THIS session's leases only — a
    /// shared fence state (BR-14) may hold other sessions' live input
    /// leases, and one session's stop must never yank another's.
    pub fn stop(&mut self, outstanding_jobs: Vec<String>) -> StopOutcome {
        self.admission = Admission::Closed;
        let inputs_to_release = {
            let mut fences = self
                .fences
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            fences.emergency_release_inputs_owned(&self.id.0.clone())
        };
        self.outstanding_jobs = outstanding_jobs;
        let input_release = if self.input_release_confirmed {
            InputLeaseState::Released
        } else {
            InputLeaseState::Unconfirmed
        };
        StopOutcome {
            admission: self.admission,
            inputs_to_release,
            input_release,
            outstanding_jobs: self.outstanding_jobs.clone(),
        }
    }

    /// Host confirms the driver released its inputs (BR-17 settlement).
    /// After confirmation, stops report `Released` instead of re-warning
    /// `Unconfirmed` forever, and the confirmed inputs are no longer
    /// re-reported for emergency release. Confirmation is an explicit
    /// host act after driver acknowledgement — never inferred from a
    /// driver ack string alone (§7: an ack is not a postcondition; the
    /// host owns this transition).
    pub fn confirm_input_release(&mut self) {
        self.input_release_confirmed = true;
        // Settlement clears the input records it covers: the driver has
        // confirmed release, so neither live nor stranded input leases
        // are re-reported by a later stop's emergency-release scan.
        // M9: confirmation is a LATCH, not a permanent state — admitting a
        // new input-holding lease invalidates it, so a later stop reports
        // `Unconfirmed` for the new batch instead of claiming release.
        self.fences
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .settle_input_release_owned(&self.id.0.clone());
    }

    /// Resume after a stop requires fresh observation and renewed authority
    /// (§3.1 human takeover). A QUARANTINED session refuses: quarantine is
    /// resolved by reconciliation with evidence (BR-18), never by resume —
    /// reporting otherwise would be a truthful-reporting lie.
    /// A PAUSED session resumes into `Recovering`: admission reopens and a
    /// fresh observation is required, mirroring the quarantine-resolution
    /// path so the session becomes dispatchable again.
    /// H2: resume MINTS A FRESH AUTHORITY EPOCH — approvals minted before
    /// the stop no longer authorize dispatches after it (§6 renewal).
    pub fn resume(&mut self) -> Result<(), BridgeError> {
        if self.state == ApplicationSessionState::Closed {
            return Err(BridgeError::TargetChanged);
        }
        if self.state == ApplicationSessionState::Quarantined {
            return Err(BridgeError::CleanupUnconfirmed);
        }
        if self.state == ApplicationSessionState::Paused {
            self.state = ApplicationSessionState::Recovering;
        }
        self.admission = Admission::Open;
        // H2: renewed authority is a NEW epoch, not the old one reopened.
        self.authority_epoch = self
            .authority_epoch
            .checked_add(1)
            .ok_or(BridgeError::ResourceLimit)?;
        self.latest_observation_revision += 1; // force a fresh observation
        self.latest_mutation_observation_revision = self.latest_observation_revision;
        Ok(())
    }

    /// Record an outstanding asynchronous job (BR-13, AT-23).
    pub fn add_outstanding_job(&mut self, job: String) {
        // BR-13/18, AT-37: bound the retained job list — the newest
        // window is kept; the oldest entries are dropped first.
        if self.outstanding_jobs.len() >= Self::MAX_OUTSTANDING_JOBS {
            let excess = self.outstanding_jobs.len() + 1 - Self::MAX_OUTSTANDING_JOBS;
            self.outstanding_jobs.drain(..excess);
            // M6: eviction is counted, never silent.
            self.evicted_jobs = self.evicted_jobs.saturating_add(excess as u64);
        }
        self.outstanding_jobs.push(job);
    }

    /// M6: jobs evicted from the bounded window (surfaced by reports).
    #[must_use]
    pub fn evicted_jobs(&self) -> u64 {
        self.evicted_jobs
    }

    /// Reconcile an uncertain operation against observed application
    /// state (BR-18, AT-19/AT-20). Given independent evidence of whether
    /// the uncertain effect actually holds, settle the record:
    ///
    /// - effect holds ⇒ `Applied` (verification may then promote it);
    /// - effect absent ⇒ `Failed` with a reconciliation note;
    /// - evidence unavailable ⇒ stays `UnknownOutcome` and the session
    ///   **must** be quarantined (the caller then refuses new writes).
    ///
    /// Reconciliation never upgrades an unknown to success directly.
    pub fn reconcile(
        &mut self,
        request_id: &OperationRequestId,
        effect_holds: Option<bool>,
    ) -> Result<OperationOutcome, BridgeError> {
        let record = self
            .records
            .get_mut(&request_id.0)
            .ok_or(BridgeError::UnknownOutcome)?;
        if !matches!(
            record.outcome,
            OperationOutcome::Dispatched
                | OperationOutcome::Applied
                | OperationOutcome::UnknownOutcome
        ) {
            // Already settled terminally; reconciliation is a no-op that
            // reports the existing outcome.
            return Ok(record.outcome.clone());
        }
        let settled = match effect_holds {
            Some(true) => OperationOutcome::Applied,
            Some(false) => OperationOutcome::Failed(
                "reconciled: application state shows the effect did not hold".into(),
            ),
            None => {
                // Evidence unavailable: the session must quarantine itself
                // so new writes refuse until a human/reconciler resolves it.
                // The record stays `UnknownOutcome`; quarantine is sticky.
                self.quarantine();
                return Ok(OperationOutcome::UnknownOutcome);
            }
        };
        record.outcome = settled.clone();
        // BR-18: evidence-backed resolution ends the quarantine. The
        // session moves to Recovering (not Ready) — resume + a fresh
        // observation are still required before dispatch, but the
        // uncertainty that justified quarantine is now resolved.
        if self.state == ApplicationSessionState::Quarantined {
            self.state = ApplicationSessionState::Recovering;
        }
        Ok(settled)
    }

    #[must_use]
    pub fn outstanding_jobs(&self) -> &[String] {
        &self.outstanding_jobs
    }

    #[must_use]
    pub fn records(&self) -> Vec<&OperationRecord> {
        self.records.values().collect()
    }
}
