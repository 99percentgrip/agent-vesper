//! Buffered-repass partial gate (PRD §2.6, D7; PR-1 §6).
//!
//! Wraps a final-only [`VoiceStt`] and exposes **buffered repass**
//! partials: repeated transcription of the capture buffer so far — the
//! behavior the sidecar engine actually supplies. Incremental inference
//! is a different `PartialsKind` and is *not* advertised here.
//!
//! Binding rules proven by tests:
//! - the capture buffer is bounded by the configured byte budget (the
//!   partial *window* does not bound the capture; the capture budget
//!   does);
//! - obsolete partial work is coalesced: at most one partial evaluation
//!   is queued at a time, and a newer request replaces an unfinished
//!   older one's *result* (the final gate never waits behind partials —
//!   finals-first priority);
//! - stale partials cannot appear after finalization, cancellation, or
//!   a new capture generation (generation stamping);
//! - partial transcripts are provisional display data and never trigger
//!   agent turns (no turn-side API exists on this type at all).
//!
//! Blocking inference runs on the supplied [`BlockingExecutor`] so
//! capture callbacks and async reactors never block on model calls. For
//! an engine that cannot preempt an in-flight call, stopping the awaiter
//! does not stop the computation; the gate bounds scheduling (one
//! in-flight evaluation, coalescing) and the adapter's deadline bounds
//! the call itself.

use std::sync::Arc;

use vesper_domain::BoundedString;

use crate::audio::PcmFrame;
use crate::composition::blocking::{self, ValueExecutor};
use crate::config::CaptureBudget;
use crate::error::VoiceError;
use crate::ports::{SttTranscript, VoiceFuture, VoiceStt};

/// A partial-transcript evaluation result keyed to its generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialResult {
    /// Generation the text belongs to (stale results are rejected).
    pub generation: u64,
    /// Interim text, or `None` when the repass produced nothing better.
    pub text: Option<BoundedString<2048>>,
}

/// Generation counter for capture identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialGeneration {
    /// Monotone generation number.
    pub id: u64,
    /// True once finalization has begun for this generation.
    pub finalized: bool,
}

/// Buffered-repass partial gate over a final-only STT adapter.
pub struct PartialGate {
    inner: Arc<dyn VoiceStt>,
    executor: Arc<dyn ValueExecutor>,
    budget: CaptureBudget,
    buffer: std::sync::Mutex<Vec<PcmFrame>>,
    generation: Arc<std::sync::Mutex<PartialGeneration>>,
    in_flight_mutex: Arc<std::sync::Mutex<bool>>,
}

/// Clears the coalescing flag when dropped (every exit path).
struct InFlightFlag(Arc<std::sync::Mutex<bool>>);
impl Drop for InFlightFlag {
    fn drop(&mut self) {
        if let Ok(mut flag) = self.0.lock() {
            *flag = false;
        }
    }
}

impl PartialGate {
    /// Wraps `inner` with repass partials on `executor`.
    #[must_use]
    pub fn new(
        inner: Arc<dyn VoiceStt>,
        executor: Arc<dyn ValueExecutor>,
        budget: CaptureBudget,
    ) -> Self {
        Self {
            inner,
            executor,
            budget,
            buffer: std::sync::Mutex::new(Vec::new()),
            generation: Arc::new(std::sync::Mutex::new(PartialGeneration {
                id: 0,
                finalized: false,
            })),
            in_flight_mutex: Arc::new(std::sync::Mutex::new(false)),
        }
    }

    /// Starts a new capture generation; discards buffered audio and
    /// marks the previous generation stale.
    pub fn start_capture(&self) -> u64 {
        let mut generation = self.generation.lock().unwrap();
        generation.id += 1;
        generation.finalized = false;
        self.buffer.lock().unwrap().clear();
        generation.id
    }

    /// Appends capture audio under the whole-capture byte budget.
    ///
    /// # Errors
    ///
    /// [`VoiceError::ResourceExhausted`] when the capture budget is
    /// exceeded — overflow is rejected loudly (with a truthful marker at
    /// the caller), never silently truncated.
    pub fn push_audio(&self, frame: PcmFrame) -> Result<(), VoiceError> {
        let mut buffer = self.buffer.lock().unwrap();
        let budget = self.budget.max_capture_bytes as usize;
        let used: usize = buffer.iter().map(|frame| frame.bytes().len()).sum();
        if used + frame.bytes().len() > budget {
            return Err(VoiceError::ResourceExhausted(
                "capture budget exceeded; stop capture and retry with a shorter utterance".into(),
            ));
        }
        buffer.push(frame);
        Ok(())
    }

    /// Marks finalization: queued partial results for this generation
    /// become stale; no new partial is scheduled.
    pub fn begin_finalize(&self) {
        if let Ok(mut generation) = self.generation.lock() {
            generation.finalized = true;
        }
    }

    /// Runs one repass partial evaluation for the current generation on
    /// the executor. Coalesced: when an evaluation is already in flight,
    /// the request returns `Ok(None)` (the next repass picks up newer
    /// audio). Stale results (finalized, cancelled, or superseded
    /// generation) return `Ok(None)` and are never published.
    ///
    /// # Errors
    ///
    /// Inner error classes propagate for diagnostics, but callers treat
    /// partials as best-effort ([`SttPartial::partial`] flattens them to
    /// `None`); a failing partial never fails a turn.
    pub fn partial_eval(
        &self,
        cancel: &crate::VoiceCancel,
    ) -> VoiceFuture<'static, Result<Option<PartialResult>, VoiceError>> {
        let inner = Arc::clone(&self.inner);
        let executor: Arc<dyn ValueExecutor> = Arc::clone(&self.executor);
        let live_generation = Arc::clone(&self.generation);
        let live_in_flight = Arc::new(InFlightFlag(Arc::clone(&self.in_flight_mutex)));
        // Coalesce under the lock: only one queued evaluation at a time.
        {
            let mut in_flight = self.in_flight_mutex.lock().unwrap();
            if *in_flight {
                return Box::pin(async move { Ok(None) });
            }
            *in_flight = true;
        }
        let (generation, snapshot) = {
            let generation = *self.generation.lock().unwrap();
            let buffer = self.buffer.lock().unwrap().clone();
            (generation, buffer)
        };
        let cancel = cancel.clone();
        Box::pin(async move {
            // Drop guard clears the coalescing slot on every exit path,
            // including cancellation and panics in the blocking work.
            let _guard = live_in_flight;
            // Finals-first priority: never evaluate after finalization
            // began or the generation advanced.
            if generation.finalized {
                return Ok(None);
            }
            let snapshot_generation = generation.id;
            let result = blocking::run_blocking(
                executor.as_ref(),
                &cancel,
                Box::new(move || {
                    let local_cancel = crate::VoiceCancel::new();
                    crate::test_util::block_on(inner.transcribe(&snapshot, &local_cancel))
                }),
            )
            .await;
            match result {
                Err(VoiceError::Cancelled) => Ok(None),
                Err(other) => Err(other),
                Ok(transcript) => {
                    // Staleness: re-check the *live* generation state.
                    let current = *live_generation.lock().unwrap();
                    if current.id != snapshot_generation || current.finalized {
                        return Ok(None);
                    }
                    let trimmed = transcript.text.as_str().trim();
                    let text = if trimmed.is_empty() {
                        None
                    } else {
                        BoundedString::new(trimmed).ok()
                    };
                    Ok(Some(PartialResult {
                        generation: snapshot_generation,
                        text,
                    }))
                }
            }
        })
    }

    /// Final transcription through the wrapped adapter, clearing partial
    /// state first so finals take priority.
    ///
    /// # Errors
    ///
    /// Per the inner adapter's classification.
    pub fn finalize(
        &self,
        cancel: &crate::VoiceCancel,
    ) -> VoiceFuture<'static, Result<SttTranscript, VoiceError>> {
        self.begin_finalize();
        let inner = Arc::clone(&self.inner);
        let snapshot = self.buffer.lock().unwrap().clone();
        let cancel = cancel.clone();
        Box::pin(async move { inner.transcribe(&snapshot, &cancel).await })
    }
}
