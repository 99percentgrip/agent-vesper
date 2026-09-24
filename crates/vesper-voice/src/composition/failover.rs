//! Policy-constrained failover composition (PRD §2.6, D8/D12).
//!
//! Frozen semantics, now executable: adapters are tried in configured
//! order; **only `Unavailable` advances**; `NoSpeech`/`Auth`/`Quota`/
//! `InvalidInput`/`Truncated`/`Inference`/`ResourceExhausted` stop the
//! chain with their real classification; `Cancelled` terminates the
//! request immediately. Egress is **rechecked for every attempt** (and
//! every retry): local-only operation can never spill microphone audio
//! to a remote class, and a loopback address alone is not evidence of
//! on-device inference — classification follows the configured service's
//! trust contract via the declared egress class.

use std::sync::Arc;

use vesper_domain::ProviderId;

use crate::config::{SpeechEgress, VoiceScopeError};
use crate::error::VoiceError;
use crate::ports::{SpeechEgressClass, SttDescriptor, SttTranscript, VoiceFuture, VoiceStt};

/// Sanitized fallback trace for [`crate::report`]/metadata: adapter
/// identity + outcome class only — no server bodies, no transcripts, no
/// credentials, no audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailoverAttempt {
    /// Adapter that was attempted.
    pub provider: ProviderId,
    /// Outcome class of the attempt.
    pub outcome: AttemptOutcome,
}

/// Outcome classes exposed in the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// Egress policy forbade this candidate (including retries).
    EgressDenied,
    /// Provider unavailable; chain advanced (if more candidates remain).
    Advanced,
    /// Non-advanceable outcome stopped the chain.
    Terminal,
    /// Cancelled before/at this attempt.
    Cancelled,
    /// Success.
    Succeeded,
}

/// Failover composition over ordered STT adapters.
pub struct FailoverStt {
    chain: Vec<Arc<dyn VoiceStt>>,
    egress: SpeechEgress,
    max_attempts: usize,
}

/// Construction/validation error for the composition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FailoverConfigError {
    /// No adapters were provided.
    #[error("failover chain requires at least one adapter")]
    Empty,
    /// An adapter's declared egress class is not permitted by the
    /// configured policy (fail-closed at construction and per attempt).
    #[error("adapter {provider} egress class {egress:?} not permitted by policy {policy:?}")]
    EgressNotAllowed {
        /// Offending adapter.
        provider: ProviderId,
        /// Its declared class.
        egress: SpeechEgressClass,
        /// The configured policy.
        policy: SpeechEgress,
    },
}

/// Verifies a candidate class against the policy (shared by
/// construction-time validation and per-attempt rechecks).
#[must_use]
pub fn egress_permitted(policy: SpeechEgress, class: SpeechEgressClass) -> bool {
    match policy {
        SpeechEgress::OnDevice => class == SpeechEgressClass::OnDevice,
        SpeechEgress::SelfHostedRemote => {
            matches!(
                class,
                SpeechEgressClass::OnDevice | SpeechEgressClass::SelfHostedRemote
            )
        }
        SpeechEgress::ThirdPartyCloud => true,
    }
}

impl FailoverStt {
    /// Builds the composition, validating every adapter's egress class
    /// against `policy` before any request can run.
    ///
    /// # Errors
    ///
    /// [`FailoverConfigError::Empty`] with no adapters, or
    /// [`FailoverConfigError::EgressNotAllowed`] when a candidate's
    /// class violates the policy (fail-closed: better at construction
    /// than at request time with audio in hand).
    pub fn new(
        chain: Vec<Arc<dyn VoiceStt>>,
        policy: SpeechEgress,
    ) -> Result<Self, FailoverConfigError> {
        if chain.is_empty() {
            return Err(FailoverConfigError::Empty);
        }
        for adapter in &chain {
            let class = adapter.descriptor().egress;
            if !egress_permitted(policy, class) {
                return Err(FailoverConfigError::EgressNotAllowed {
                    provider: adapter.descriptor().provider.clone(),
                    egress: class,
                    policy,
                });
            }
        }
        Ok(Self {
            chain,
            egress: policy,
            max_attempts: 4,
        })
    }

    /// Upper bound on total attempts across the chain (transport-level
    /// retries inside one adapter count as one attempt here; adapters
    /// bound their own retries so the two do not multiply).
    #[must_use]
    pub const fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    /// Overrides the attempt bound (tests + explicit configuration).
    pub fn set_max_attempts(&mut self, max: usize) {
        self.max_attempts = max.max(1);
    }

    /// Descriptor of the *selected* (first) adapter.
    pub fn selected_descriptor(&self) -> &SttDescriptor {
        self.chain[0].descriptor()
    }

    /// Runs the chain under the frozen policy, returning the transcript
    /// and the sanitized attempt trace.
    ///
    /// # Errors
    ///
    /// The terminal error of the last attempted adapter (classification
    /// preserved, never flattened to `Unavailable`), or
    /// [`VoiceError::Cancelled`].
    pub fn transcribe_traced<'a>(
        &'a self,
        audio: &'a [crate::audio::PcmFrame],
        cancel: &'a crate::VoiceCancel,
    ) -> VoiceFuture<'a, Result<(SttTranscript, Vec<FailoverAttempt>), VoiceError>> {
        Box::pin(async move {
            let mut trace: Vec<FailoverAttempt> = Vec::new();
            let mut terminal: Option<VoiceError> = None;
            let mut attempts = 0;
            for adapter in &self.chain {
                if attempts >= self.max_attempts {
                    break;
                }
                if cancel.is_cancelled() {
                    trace.push(FailoverAttempt {
                        provider: adapter.descriptor().provider.clone(),
                        outcome: AttemptOutcome::Cancelled,
                    });
                    return Err(VoiceError::Cancelled);
                }
                // Per-attempt (and per-retry) egress recheck.
                if !egress_permitted(self.egress, adapter.descriptor().egress) {
                    trace.push(FailoverAttempt {
                        provider: adapter.descriptor().provider.clone(),
                        outcome: AttemptOutcome::EgressDenied,
                    });
                    continue;
                }
                attempts += 1;
                match adapter.transcribe(audio, cancel).await {
                    Ok(transcript) => {
                        trace.push(FailoverAttempt {
                            provider: adapter.descriptor().provider.clone(),
                            outcome: AttemptOutcome::Succeeded,
                        });
                        return Ok((transcript, trace));
                    }
                    Err(VoiceError::Cancelled) => {
                        trace.push(FailoverAttempt {
                            provider: adapter.descriptor().provider.clone(),
                            outcome: AttemptOutcome::Cancelled,
                        });
                        return Err(VoiceError::Cancelled);
                    }
                    Err(error) => {
                        let advance = error.failover_eligible();
                        trace.push(FailoverAttempt {
                            provider: adapter.descriptor().provider.clone(),
                            outcome: if advance {
                                AttemptOutcome::Advanced
                            } else {
                                AttemptOutcome::Terminal
                            },
                        });
                        if !advance {
                            return Err(error);
                        }
                        terminal = Some(error);
                    }
                }
            }
            Err(terminal.unwrap_or(VoiceError::Unavailable {
                provider: self.chain[0].descriptor().provider.clone(),
                reason: vesper_domain::BoundedString::new("all candidates denied by egress policy")
                    .expect("static reason fits bound"),
            }))
        })
    }
}

impl VoiceStt for FailoverStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [crate::audio::PcmFrame],
        cancel: &'a crate::VoiceCancel,
    ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
        Box::pin(async move {
            self.transcribe_traced(audio, cancel)
                .await
                .map(|(transcript, _)| transcript)
        })
    }

    fn partial(&self) -> Option<&dyn crate::ports::SttPartial> {
        self.chain[0].partial()
    }

    fn descriptor(&self) -> &SttDescriptor {
        self.selected_descriptor()
    }
}

/// Converts a [`VoiceScopeError::EgressPolicy`] into the corresponding
/// [`VoiceError`] shape for composition boundaries that surface config
/// failures as unavailability (metadata only).
#[must_use]
pub fn scope_error_as_unavailable(error: &VoiceScopeError) -> VoiceError {
    let reason = match error {
        VoiceScopeError::EgressPolicy { reason } => reason.clone(),
        other => other.to_string(),
    };
    VoiceError::Unavailable {
        provider: EXECUTOR_PROVIDER_ID.clone(),
        reason: vesper_domain::BoundedString::new(reason.chars().take(200).collect::<String>())
            .unwrap_or_else(|_| vesper_domain::BoundedString::new("policy error").unwrap()),
    }
}

/// Synthetic provider id used for composition-level errors (not a real
/// provider; metadata only).
static EXECUTOR_PROVIDER_ID: std::sync::LazyLock<ProviderId> =
    std::sync::LazyLock::new(|| ProviderId::new("voice-composition").unwrap());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::PcmFrame;
    use crate::cancel::VoiceCancel;
    use crate::fakes::{FakeStt, FakeSttOutcome};
    use std::sync::Mutex;

    fn frame() -> PcmFrame {
        PcmFrame::from_aligned(vec![0u8; 3200]).unwrap()
    }

    /// Scriptable fake that records the number of calls.
    struct CountingStt {
        inner: FakeStt,
        calls: Mutex<usize>,
    }
    impl VoiceStt for CountingStt {
        fn transcribe<'a>(
            &'a self,
            audio: &'a [PcmFrame],
            cancel: &'a crate::VoiceCancel,
        ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
            *self.calls.lock().unwrap() += 1;
            self.inner.transcribe(audio, cancel)
        }
        fn descriptor(&self) -> &SttDescriptor {
            self.inner.descriptor()
        }
    }

    #[tokio::test]
    async fn unavailable_advances_to_next_adapter() {
        let first = FakeStt::on_device();
        first.set_outcome(FakeSttOutcome::Unavailable);
        let second = FakeStt::on_device();
        second.set_outcome(FakeSttOutcome::Text("fallback result"));
        let chain = FailoverStt::new(
            vec![Arc::new(first), Arc::new(second)],
            SpeechEgress::OnDevice,
        )
        .unwrap();
        let cancel = VoiceCancel::new();
        let (transcript, trace) = chain.transcribe_traced(&[frame()], &cancel).await.unwrap();
        assert_eq!(transcript.text.as_str(), "fallback result");
        assert_eq!(trace[0].outcome, AttemptOutcome::Advanced);
        assert_eq!(trace[1].outcome, AttemptOutcome::Succeeded);
    }

    #[tokio::test]
    async fn no_speech_stops_the_chain() {
        let first = FakeStt::on_device();
        first.set_outcome(FakeSttOutcome::NoSpeech);
        let second = FakeStt::on_device();
        second.set_outcome(FakeSttOutcome::Text("never reached"));
        let calls = Arc::new(CountingStt {
            inner: second,
            calls: Mutex::new(0),
        });
        let chain = FailoverStt::new(
            vec![Arc::new(first), calls.clone() as Arc<dyn VoiceStt>],
            SpeechEgress::OnDevice,
        )
        .unwrap();
        let cancel = VoiceCancel::new();
        let error = chain.transcribe(&[frame()], &cancel).await.unwrap_err();
        assert!(matches!(error, VoiceError::NoSpeech));
        assert_eq!(*calls.calls.lock().unwrap(), 0, "silence is an answer");
    }

    #[tokio::test]
    async fn inference_failure_is_terminal_not_outage() {
        let first = FakeStt::on_device();
        first.set_outcome(FakeSttOutcome::InferenceFailure);
        let chain = FailoverStt::new(vec![Arc::new(first)], SpeechEgress::OnDevice).unwrap();
        let cancel = VoiceCancel::new();
        assert!(matches!(
            chain.transcribe(&[frame()], &cancel).await,
            Err(VoiceError::Inference(_))
        ));
    }

    #[tokio::test]
    async fn cancellation_terminates_immediately() {
        let first = FakeStt::on_device();
        let chain = FailoverStt::new(vec![Arc::new(first)], SpeechEgress::OnDevice).unwrap();
        let cancel = VoiceCancel::new();
        cancel.cancel();
        assert!(matches!(
            chain.transcribe(&[frame()], &cancel).await,
            Err(VoiceError::Cancelled)
        ));
    }

    /// Adapter wrapper with an overridden egress class (test-only).
    struct ClassOverrideStt {
        inner: FakeStt,
        descriptor: SttDescriptor,
    }
    impl VoiceStt for ClassOverrideStt {
        fn transcribe<'a>(
            &'a self,
            audio: &'a [PcmFrame],
            cancel: &'a crate::VoiceCancel,
        ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
            self.inner.transcribe(audio, cancel)
        }
        fn descriptor(&self) -> &SttDescriptor {
            &self.descriptor
        }
    }

    fn with_class(class: SpeechEgressClass) -> Arc<dyn VoiceStt> {
        let inner = FakeStt::on_device();
        let mut descriptor = inner.descriptor().clone();
        descriptor.egress = class;
        Arc::new(ClassOverrideStt { inner, descriptor })
    }

    #[test]
    fn egress_policy_rejected_at_construction() {
        let outcome = FailoverStt::new(
            vec![with_class(SpeechEgressClass::SelfHostedRemote)],
            SpeechEgress::OnDevice,
        );
        assert!(matches!(
            outcome,
            Err(FailoverConfigError::EgressNotAllowed { .. })
        ));
        // On-device policy accepts only on-device candidates.
        assert!(
            FailoverStt::new(
                vec![with_class(SpeechEgressClass::OnDevice)],
                SpeechEgress::OnDevice
            )
            .is_ok()
        );
        // Self-hosted policy accepts on-device and self-hosted, not cloud.
        assert!(
            FailoverStt::new(
                vec![with_class(SpeechEgressClass::SelfHostedRemote)],
                SpeechEgress::SelfHostedRemote
            )
            .is_ok()
        );
        assert!(
            FailoverStt::new(
                vec![with_class(SpeechEgressClass::ThirdPartyCloud)],
                SpeechEgress::SelfHostedRemote
            )
            .is_err()
        );
    }

    #[test]
    fn empty_chain_is_rejected() {
        assert!(matches!(
            FailoverStt::new(vec![], SpeechEgress::OnDevice),
            Err(FailoverConfigError::Empty)
        ));
    }

    #[tokio::test]
    async fn attempt_bound_limits_chain() {
        let first = FakeStt::on_device();
        first.set_outcome(FakeSttOutcome::Unavailable);
        let second = FakeStt::on_device();
        second.set_outcome(FakeSttOutcome::Unavailable);
        let mut chain = FailoverStt::new(
            vec![Arc::new(first), Arc::new(second)],
            SpeechEgress::OnDevice,
        )
        .unwrap();
        chain.set_max_attempts(1);
        let cancel = VoiceCancel::new();
        let error = chain.transcribe(&[frame()], &cancel).await.unwrap_err();
        assert!(matches!(error, VoiceError::Unavailable { .. }));
    }
}
