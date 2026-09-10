//! Bounded bridge for a host's real configured embedding implementation.
use futures_util::future::BoxFuture;
use std::sync::Arc;
use vesper_swarm::ledger::store::{BoundedText, EmbeddingPort, LedgerError};

type Embed = dyn Fn(Vec<String>) -> Result<Vec<Vec<f32>>, String> + Send + Sync;

/// Setup and recall share a process-wide bound, including abandoned blocking
/// requests. Dropping the observer never releases a running request's permit.
pub async fn bounded_setup<T: Send + 'static>(
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    static PERMITS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let permit = PERMITS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Swarm setup capacity is reserved by unfinished requests.".to_owned())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    .map_err(|_| "Swarm setup task failed.".to_owned())
}

/// Cancellation ends observation promptly; an already-started blocking request
/// still owns its global permit until actual completion.
pub async fn cancellable_setup<T: Send + 'static>(
    task: impl FnOnce() -> T + Send + 'static,
    cancellation: vesper_swarm::worker::CancellationSignal,
) -> Result<T, String> {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("Swarm cancelled during setup.".into()),
        result = bounded_setup(task) => result,
    }
}

pub struct ConfiguredEmbedding {
    embed: Arc<Embed>,
    dimensions: usize,
    permits: Arc<tokio::sync::Semaphore>,
}
impl ConfiguredEmbedding {
    pub fn new(dimensions: usize, embed: Arc<Embed>) -> Result<Self, String> {
        if !(1..=4096).contains(&dimensions) {
            return Err("Embedding dimensions must be 1–4096.".into());
        }
        Ok(Self {
            dimensions,
            embed,
            permits: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }
}
impl EmbeddingPort for ConfiguredEmbedding {
    fn embed<'a>(
        &'a self,
        texts: Vec<BoundedText>,
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, LedgerError>> {
        Box::pin(async move {
            if texts.len() > 64
                || texts.iter().map(|text| text.as_str().len()).sum::<usize>() > 1_048_576
            {
                return Err(LedgerError::Embedding(
                    "Embedding batch budget exceeded.".into(),
                ));
            }
            let permit = self.permits.clone().try_acquire_owned().map_err(|_| {
                LedgerError::Embedding(
                    "Embedding capacity is still reserved by unfinished requests.".into(),
                )
            })?;
            let embed = self.embed.clone();
            let count = texts.len();
            let dimensions = self.dimensions;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let vectors = embed(
                    texts
                        .into_iter()
                        .map(|text| text.as_str().to_owned())
                        .collect(),
                )
                .map_err(|_| {
                    LedgerError::Embedding("Configured embedding request failed.".into())
                })?;
                if vectors.len() != count
                    || vectors.iter().any(|vector| {
                        vector.len() != dimensions
                            || vector.iter().any(|value| !value.is_finite())
                            || !vector.iter().any(|value| *value != 0.0)
                    })
                {
                    return Err(LedgerError::Embedding(
                        "Configured embedding returned invalid vectors.".into(),
                    ));
                }
                Ok(vectors)
            })
            .await
            .map_err(|_| LedgerError::Embedding("Configured embedding task failed.".into()))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn abandoned_setup_observers_keep_global_capacity_until_completion() {
        let mut release = Vec::new();
        let mut completed = Vec::new();
        for _ in 0..4 {
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = tokio::sync::oneshot::channel();
            let observer = tokio::spawn(bounded_setup(move || {
                ready_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                let _ = done_tx.send(());
            }));
            ready_rx.await.unwrap();
            observer.abort();
            assert!(observer.await.unwrap_err().is_cancelled());
            release.push(release_tx);
            completed.push(done_rx);
        }
        assert!(
            bounded_setup(|| panic!("over-capacity setup must not execute"))
                .await
                .is_err()
        );
        for sender in release {
            sender.send(()).unwrap();
        }
        for receiver in completed {
            receiver.await.unwrap();
        }
        // Completion notification is inside the blocking closure; its permit
        // drops on return. Bound observation of that final ownership transition.
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if bounded_setup(|| 42).await == Ok(42) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn configured_embedding_rejects_bad_shapes_and_sanitizes_failures() {
        for vectors in [
            vec![],
            vec![vec![1.0]],
            vec![vec![0.0, 0.0]],
            vec![vec![f32::NAN, 1.0]],
        ] {
            let embedding =
                ConfiguredEmbedding::new(2, Arc::new(move |_| Ok(vectors.clone()))).unwrap();
            assert!(
                embedding
                    .embed(vec![BoundedText::new("test").unwrap()])
                    .await
                    .is_err()
            );
        }
        let embedding =
            ConfiguredEmbedding::new(2, Arc::new(|_| Err("synthetic-secret-canary".into())))
                .unwrap();
        let error = embedding
            .embed(vec![BoundedText::new("test").unwrap()])
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("synthetic-secret-canary"));
        assert!(ConfiguredEmbedding::new(0, Arc::new(|_| Ok(vec![]))).is_err());
    }
}
