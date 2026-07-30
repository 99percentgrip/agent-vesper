use tokio_util::sync::CancellationToken;
use vesper_provider::CancellationSignal;

/// Cloneable cancellation scope shared with provider sessions.
#[derive(Debug, Clone, Default)]
pub struct RuntimeCancellation {
    token: CancellationToken,
}

impl RuntimeCancellation {
    /// Creates an independent scope.
    #[must_use]
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    /// Creates a child cancelled when this scope is cancelled.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            token: self.token.child_token(),
        }
    }

    /// Requests cancellation idempotently.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Reports whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Waits until cancellation is requested.
    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }
}

impl CancellationSignal for RuntimeCancellation {
    fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}
