//! In-memory native turn lineage retained through caller cancellation.
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use vesper_domain::ConversationMessage;

#[derive(Debug, Clone)]
pub struct NativeWorkerRecord {
    pub worker: String,
    pub task: String,
    pub history: Vec<ConversationMessage>,
    pub provider_turns: usize,
}

#[derive(Default)]
pub struct NativeWorkerJournal {
    pending: AtomicUsize,
    records: Mutex<BTreeMap<String, NativeWorkerRecord>>,
    changed: tokio::sync::Notify,
}
impl NativeWorkerJournal {
    pub(crate) fn begin(
        self: &Arc<Self>,
        worker: String,
        task: String,
        history: Arc<Mutex<Vec<ConversationMessage>>>,
        turns: Arc<AtomicUsize>,
    ) -> JournalGuard {
        self.pending.fetch_add(1, Ordering::AcqRel);
        JournalGuard {
            journal: self.clone(),
            worker,
            task,
            history,
            turns,
        }
    }
    pub fn records(&self) -> Vec<NativeWorkerRecord> {
        self.records
            .lock()
            .expect("native journal")
            .values()
            .cloned()
            .collect()
    }
    /// Observation deadline only; owned native work is never aborted here.
    pub async fn settle(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self.pending.load(Ordering::Acquire) == 0 {
                    return;
                }
                changed.await;
            }
        })
        .await
        .is_ok()
    }
}
pub(crate) struct JournalGuard {
    journal: Arc<NativeWorkerJournal>,
    worker: String,
    task: String,
    history: Arc<Mutex<Vec<ConversationMessage>>>,
    turns: Arc<AtomicUsize>,
}
impl Drop for JournalGuard {
    fn drop(&mut self) {
        self.journal.records.lock().expect("native journal").insert(
            self.task.clone(),
            NativeWorkerRecord {
                worker: self.worker.clone(),
                task: self.task.clone(),
                history: self.history.lock().expect("native history").clone(),
                provider_turns: self.turns.load(Ordering::Acquire),
            },
        );
        self.journal.pending.fetch_sub(1, Ordering::AcqRel);
        self.journal.changed.notify_waiters();
    }
}
