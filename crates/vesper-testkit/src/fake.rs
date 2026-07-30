use std::{
    collections::VecDeque,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use futures_core::Stream;
use vesper_domain::{
    CommandId, ErrorCategory, ErrorInfo, EventId, ExtensionMap, MessageId, PermissionOutcome,
    ProviderId, RedactedDiagnostics, Retryability, SafeMessage, SessionId, ToolCallId, TurnId,
};
use vesper_provider::{
    CancellationSignal, ProviderError, ProviderEventStream, ProviderRequest, ProviderSession,
    ProviderStreamEvent,
};

#[derive(Clone, Default)]
pub struct FakeClock(Arc<AtomicU64>);

/// Deterministic identity source confined to test support.
#[derive(Debug, Clone)]
pub struct DeterministicIds {
    prefix: String,
    next: u64,
}

impl DeterministicIds {
    /// Creates a sequence with a bounded static prefix.
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            next: 0,
        }
    }

    fn next_value(&mut self, kind: &str) -> String {
        let value = format!("{}-{kind}-{}", self.prefix, self.next);
        self.next += 1;
        value
    }

    /// Next session ID.
    pub fn session(&mut self) -> SessionId {
        SessionId::new(self.next_value("session")).expect("test ID prefix must be valid")
    }

    /// Next turn ID.
    pub fn turn(&mut self) -> TurnId {
        TurnId::new(self.next_value("turn")).expect("test ID prefix must be valid")
    }

    /// Next message ID.
    pub fn message(&mut self) -> MessageId {
        MessageId::new(self.next_value("message")).expect("test ID prefix must be valid")
    }

    /// Next tool-call ID.
    pub fn tool_call(&mut self) -> ToolCallId {
        ToolCallId::new(self.next_value("tool-call")).expect("test ID prefix must be valid")
    }

    /// Next command ID.
    pub fn command(&mut self) -> CommandId {
        CommandId::new(self.next_value("command")).expect("test ID prefix must be valid")
    }

    /// Next event ID.
    pub fn event(&mut self) -> EventId {
        EventId::new(self.next_value("event")).expect("test ID prefix must be valid")
    }
}

impl FakeClock {
    pub fn now_millis(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    pub fn advance_millis(&self, amount: u64) {
        self.0.fetch_add(amount, Ordering::SeqCst);
    }
}

#[derive(Clone, Default)]
pub struct CancellationProbe(Arc<AtomicBool>);

impl CancellationProbe {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl CancellationSignal for CancellationProbe {
    fn is_cancelled(&self) -> bool {
        CancellationProbe::is_cancelled(self)
    }
}

pub struct FakeProviderStream {
    events: VecDeque<Result<ProviderStreamEvent, ProviderError>>,
    cancellations: Vec<Arc<dyn CancellationSignal>>,
}

/// One scripted provider start result.
pub type ScriptedProviderResponse =
    Result<Vec<Result<ProviderStreamEvent, ProviderError>>, Box<ProviderError>>;

/// Provider session returning deterministic scripted streams without transport I/O.
#[derive(Clone, Default)]
pub struct FakeProviderSession {
    scripts: Arc<Mutex<VecDeque<ScriptedProviderResponse>>>,
    requests: Arc<Mutex<Vec<ProviderRequest>>>,
    cancellation: CancellationProbe,
}

impl FakeProviderSession {
    /// Creates a fake with one script per future request.
    pub fn with_scripts(scripts: impl IntoIterator<Item = ScriptedProviderResponse>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().collect())),
            requests: Arc::new(Mutex::new(Vec::new())),
            cancellation: CancellationProbe::default(),
        }
    }

    /// Returns requests in dispatch order.
    #[must_use]
    pub fn requests(&self) -> Vec<ProviderRequest> {
        self.requests.lock().expect("fake lock poisoned").clone()
    }

    /// Cancels current and future scripted streams.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

impl ProviderSession for FakeProviderSession {
    fn start<'a>(
        &'a self,
        request: ProviderRequest,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> vesper_provider::ProviderFuture<'a, Result<ProviderEventStream, ProviderError>> {
        self.requests
            .lock()
            .expect("fake lock poisoned")
            .push(request);
        let script = self
            .scripts
            .lock()
            .expect("fake lock poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(Box::new(fake_error(
                    ErrorCategory::InvalidRequest,
                    "script exhausted",
                )))
            });
        let probe = self.cancellation.clone();
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(fake_error(ErrorCategory::Cancellation, "request cancelled"));
            }
            let events = script.map_err(|error| *error)?;
            Ok(Box::pin(FakeProviderStream::with_signals(
                events,
                vec![cancellation, Arc::new(probe)],
            )) as ProviderEventStream)
        })
    }
}

fn fake_error(category: ErrorCategory, message: &str) -> ProviderError {
    ProviderError {
        provider_id: ProviderId::new("test.fake").expect("static provider ID"),
        provider_code: None,
        http_status: None,
        continuation_possible: false,
        info: ErrorInfo {
            category,
            retryability: Retryability::Never,
            retry_after_ms: None,
            visible_output_emitted: false,
            safe_message: SafeMessage::new(message).expect("bounded static message"),
            diagnostics: RedactedDiagnostics::default(),
            provider_code: None,
            causes: Vec::new(),
        },
        metadata: ExtensionMap::default(),
    }
}

impl FakeProviderStream {
    pub fn new(
        events: impl IntoIterator<Item = Result<ProviderStreamEvent, ProviderError>>,
        cancellation: CancellationProbe,
    ) -> Self {
        Self {
            events: events.into_iter().collect(),
            cancellations: vec![Arc::new(cancellation)],
        }
    }

    /// Creates a stream cancelled by any supplied hierarchical signal.
    pub fn with_signals(
        events: impl IntoIterator<Item = Result<ProviderStreamEvent, ProviderError>>,
        cancellations: Vec<Arc<dyn CancellationSignal>>,
    ) -> Self {
        Self {
            events: events.into_iter().collect(),
            cancellations,
        }
    }
}

impl Stream for FakeProviderStream {
    type Item = Result<ProviderStreamEvent, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self
            .cancellations
            .iter()
            .any(|cancellation| cancellation.is_cancelled())
        {
            self.events.clear();
            return Poll::Ready(None);
        }
        Poll::Ready(self.events.pop_front())
    }
}

#[derive(Clone, Default)]
pub struct FakePermissionChannel {
    outcomes: Arc<Mutex<VecDeque<PermissionOutcome>>>,
}

impl FakePermissionChannel {
    pub fn with_outcomes(outcomes: impl IntoIterator<Item = PermissionOutcome>) -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(outcomes.into_iter().collect())),
        }
    }

    pub fn next(&self) -> Option<PermissionOutcome> {
        self.outcomes
            .lock()
            .expect("fake lock poisoned")
            .pop_front()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FakeFilesystemDescriptor {
    pub root_token: String,
    pub writable: bool,
}

#[cfg(test)]
mod tests {
    use futures_core::Stream;

    use super::*;

    #[test]
    fn clock_is_deterministic() {
        let clock = FakeClock::default();
        clock.advance_millis(42);
        assert_eq!(clock.now_millis(), 42);
    }

    #[test]
    fn cancellation_probe_is_shared() {
        let probe = CancellationProbe::default();
        let copy = probe.clone();
        probe.cancel();
        assert!(copy.is_cancelled());
    }

    fn assert_stream<T: Stream>(_value: &T) {}

    #[test]
    fn fake_provider_is_a_stream() {
        let stream = FakeProviderStream::new([], CancellationProbe::default());
        assert_stream(&stream);
    }
}
