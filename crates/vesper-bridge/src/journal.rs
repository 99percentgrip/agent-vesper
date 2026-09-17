//! Intent journal port and settlement records (VB-PRD-001 §8.3, BR-16/18).
//!
//! Accepted intent is recorded **before** dispatch; settlement evidence is
//! retained after the operation settles. The port is implemented at the
//! composition boundary — this crate owns the data and the state rules,
//! never the storage medium.

use serde::{Deserialize, Serialize};

use crate::operation::{AuthorityRequest, OperationOutcome};

/// Adapter/verifier/journal port implemented by the hosted layer.
///
/// The core is pure: it asks the port to persist/retrieve records and
/// derives all decisions from returned values. No I/O happens here.
pub trait JournalPort: Send + Sync {
    /// Persist accepted intent before dispatch. Failing to record intent
    /// must abort dispatch (fail-closed).
    fn record_intent(&self, intent: IntentRecord) -> Result<(), String>;
    /// Look up the settled/known outcome for a request id.
    fn lookup(&self, request_id: &crate::operation::OperationRequestId) -> Option<IntentRecord>;
    /// All recorded request ids (bounded; used for duplicate suppression).
    fn known_request_ids(&self) -> Vec<crate::operation::OperationRequestId>;
    /// The newest window of intents (bounded), newest first — the scan
    /// surface for semantic duplicate detection (§6.6). Implementations
    /// must bound the returned window the same way they bound storage.
    fn recent_intents(&self) -> Vec<IntentRecord>;
}

/// Intent recorded before dispatch and settled afterwards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntentRecord {
    pub request: AuthorityRequest,
    /// None until the outcome is observed/classified.
    pub outcome: Option<OperationOutcome>,
    /// Whether reconciliation against application state already ran for an
    /// uncertain settlement (§8.3: reconcile before retry).
    pub reconciled: bool,
    /// M7 (BR-12/NF-13): bounded evidence retained with a `Partial`
    /// settlement — WHAT held and what did not, for audit and
    /// verification decisions. Bounded to 4 KiB at set time.
    pub partial_evidence: Option<String>,
}

impl IntentRecord {
    #[must_use]
    pub fn new(request: AuthorityRequest) -> Self {
        Self {
            request,
            outcome: None,
            reconciled: false,
            partial_evidence: None,
        }
    }

    /// Settle with an observed outcome.
    #[must_use]
    pub fn settle(mut self, outcome: OperationOutcome) -> Self {
        self.outcome = Some(outcome);
        self
    }

    /// M7: attach bounded partial evidence (truncated to the cap).
    #[must_use]
    pub fn with_partial_evidence(mut self, evidence: &str) -> Self {
        const MAX_EVIDENCE_BYTES: usize = 4 * 1024;
        self.partial_evidence = Some(if evidence.len() > MAX_EVIDENCE_BYTES {
            let mut end = MAX_EVIDENCE_BYTES;
            while !evidence.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…[truncated]", &evidence[..end])
        } else {
            evidence.to_string()
        });
        self
    }
}

/// Settlement guidance derived from journal + idempotency for a retry
/// attempt (BR-16, AT-18/19/20).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settlement {
    /// No prior record; dispatch may proceed.
    Fresh,
    /// Exact duplicate of an already-accepted request id: suppressed.
    DuplicateSuppressed,
    /// Prior uncertain outcome on a non-idempotent operation: reconcile
    /// application state before any retry; a blind retry is forbidden.
    ReconcileBeforeRetry,
    /// Prior read-only/idempotent settled outcome: safe to re-run.
    SafeToRetry,
}

/// Decide how to treat a (possibly repeated) request against the journal.
#[must_use]
pub fn classify_retry(journal: &dyn JournalPort, request: &AuthorityRequest) -> Settlement {
    match journal.lookup(&request.request_id) {
        None => Settlement::Fresh,
        Some(record) => match (&record.outcome, request.idempotency) {
            (None, _) => Settlement::DuplicateSuppressed,
            (Some(outcome), idem) => {
                if outcome.may_have_committed() {
                    if matches!(
                        idem,
                        crate::operation::Idempotency::ReadOnly
                            | crate::operation::Idempotency::NaturallyIdempotent
                    ) {
                        Settlement::SafeToRetry
                    } else {
                        Settlement::ReconcileBeforeRetry
                    }
                } else {
                    Settlement::SafeToRetry
                }
            }
        },
    }
}

/// Operation identity for semantic duplicate detection (§6.6 hardening).
///
/// Attempt ids (`OperationRequestId`) are unique per dispatch by design —
/// a retried operation never reuses one, so keying duplicate suppression
/// on them cannot catch real-world retries. Identity is instead the
/// (session, capability, canonical arguments) triple: two dispatches of
/// the SAME operation against the SAME application must be compared.
#[must_use]
pub fn operation_identity(request: &AuthorityRequest) -> String {
    // Canonical arguments: serde_json::Value sorts object keys, so two
    // structurally equal argument objects produce identical strings.
    let canonical = serde_json::to_string(&request.spec.arguments).unwrap_or_default();
    format!(
        "{}/{}|{}",
        request.bridge_session.0, request.spec.capability.0, canonical
    )
}

/// Semantic duplicate detection: is there an unsettled-or-uncertain record
/// of the SAME operation identity in the journal? When the prior attempt
/// may have committed and the operation is not idempotent, the caller must
/// reconcile instead of dispatching again.
#[must_use]
pub fn classify_semantic_retry(
    journal: &dyn JournalPort,
    spec: &crate::operation::OperationSpec,
    idempotency: crate::operation::Idempotency,
) -> Settlement {
    let identity = operation_identity_for_spec(spec);
    for record in journal.recent_intents() {
        if operation_identity_for_spec(&record.request.spec) == identity {
            return match (&record.outcome, idempotency) {
                (None, _) => Settlement::DuplicateSuppressed,
                (Some(outcome), idem) => {
                    if outcome.may_have_committed() {
                        if matches!(
                            idem,
                            crate::operation::Idempotency::ReadOnly
                                | crate::operation::Idempotency::NaturallyIdempotent
                        ) {
                            Settlement::SafeToRetry
                        } else {
                            Settlement::ReconcileBeforeRetry
                        }
                    } else {
                        Settlement::SafeToRetry
                    }
                }
            };
        }
    }
    Settlement::Fresh
}

fn operation_identity_for_spec(spec: &crate::operation::OperationSpec) -> String {
    let canonical = serde_json::to_string(&spec.arguments).unwrap_or_default();
    format!("{}|{}", spec.capability.0, canonical)
}

/// A minimal in-memory journal for tests and hosted fakes. Production
/// persistence lives behind `JournalPort` at the composition boundary.
///
/// NF-05/AT-37: retention is bounded by the journal itself — the newest
/// window of intents is kept, the oldest evicted first. Eviction never
/// refuses a write and never fabricates state: an evicted id reads as
/// `None` (Fresh), which is the honest answer once no intent survives.
#[derive(Default)]
pub struct MemoryJournal {
    records: std::sync::Mutex<std::collections::BTreeMap<String, IntentRecord>>,
}

impl MemoryJournal {
    /// Core-side cap on retained intent records (NF-05, AT-37).
    pub const MAX_RECORDS: usize = 512;

    pub fn new() -> Self {
        Self::default()
    }

    /// Test/audit surface: replace a record in place (settlement shapes
    /// without re-recording a new intent id).
    pub fn reseed_for_tests(&self, record: IntentRecord) {
        if let Ok(mut records) = self.records.lock() {
            records.insert(record.request.request_id.0.clone(), record);
        }
    }
}

impl JournalPort for MemoryJournal {
    fn record_intent(&self, intent: IntentRecord) -> Result<(), String> {
        let mut records = self.records.lock().map_err(|_| "journal poisoned")?;
        // Bound before insert so a write at the cap still lands (newest
        // window retained; oldest evicted first).
        while records.len() >= Self::MAX_RECORDS {
            let oldest = records
                .keys()
                .next()
                .cloned()
                .expect("len check guarantees a key");
            records.remove(&oldest);
        }
        records.insert(intent.request.request_id.0.clone(), intent);
        Ok(())
    }

    fn lookup(&self, request_id: &crate::operation::OperationRequestId) -> Option<IntentRecord> {
        self.records.lock().ok()?.get(&request_id.0).cloned()
    }

    fn known_request_ids(&self) -> Vec<crate::operation::OperationRequestId> {
        self.records
            .lock()
            .map(|records| {
                records
                    .keys()
                    .map(|key| crate::operation::OperationRequestId(key.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn recent_intents(&self) -> Vec<IntentRecord> {
        // Newest first, bounded by the same retention window as storage.
        self.records
            .lock()
            .map(|records| records.values().rev().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{CapabilityId, Mutability};
    use crate::identity::{ApplicationInstanceId, BridgeSessionId, Generation, ResourceRevision};
    use crate::observation::ObservationId;
    use crate::operation::{
        AuthorityRequest, Idempotency, OperationRequestId, OperationSpec, Preconditions,
        TimeoutPolicy,
    };

    fn request(id: &str, idempotency: Idempotency) -> AuthorityRequest {
        AuthorityRequest {
            request_id: OperationRequestId::new(id).unwrap(),
            bridge_session: BridgeSessionId::new("bs").unwrap(),
            application: ApplicationInstanceId::new("app/seat", Generation(1)).unwrap(),
            capability_generation: 1,
            authority_generation: 1,
            lease_token: "t".into(),
            spec: OperationSpec {
                capability: CapabilityId::new("media.timeline.create").unwrap(),
                arguments: serde_json::json!({}),
            },
            preconditions: Preconditions {
                observation: ObservationId("o".into()),
                resource_revision: ResourceRevision(1),
                application_generation: Generation(1),
            },
            timeout: TimeoutPolicy::default(),
            resolved_mutability: Mutability::Mutating,
            idempotency,
        }
    }

    #[test]
    fn duplicate_accepted_request_is_suppressed_locally() {
        let journal = MemoryJournal::new();
        let req = request("r1", Idempotency::NonIdempotent);
        journal
            .record_intent(IntentRecord::new(req.clone()))
            .unwrap();
        assert_eq!(
            classify_retry(&journal, &req),
            Settlement::DuplicateSuppressed
        );
    }

    #[test]
    fn uncertain_non_idempotent_requires_reconciliation_before_retry() {
        let journal = MemoryJournal::new();
        let req = request("r2", Idempotency::NonIdempotent);
        journal
            .record_intent(IntentRecord::new(req.clone()).settle(OperationOutcome::UnknownOutcome))
            .unwrap();
        assert_eq!(
            classify_retry(&journal, &req),
            Settlement::ReconcileBeforeRetry
        );
    }

    #[test]
    fn settled_failure_of_readonly_is_safe_to_retry() {
        let journal = MemoryJournal::new();
        let req = request("r3", Idempotency::ReadOnly);
        journal
            .record_intent(
                IntentRecord::new(req.clone()).settle(OperationOutcome::Failed("transport".into())),
            )
            .unwrap();
        assert_eq!(classify_retry(&journal, &req), Settlement::SafeToRetry);
    }

    #[test]
    fn verified_idempotent_settlement_allows_rerun() {
        let journal = MemoryJournal::new();
        let req = request("r4", Idempotency::NaturallyIdempotent);
        journal
            .record_intent(IntentRecord::new(req.clone()).settle(OperationOutcome::Verified))
            .unwrap();
        assert_eq!(classify_retry(&journal, &req), Settlement::SafeToRetry);
    }

    #[test]
    fn fresh_request_dispatches() {
        let journal = MemoryJournal::new();
        assert_eq!(
            classify_retry(&journal, &request("r5", Idempotency::NonIdempotent)),
            Settlement::Fresh
        );
    }
}
