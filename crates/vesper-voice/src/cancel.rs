//! Voice-owned cancellation token (PRD §2.2, decision D2).
//!
//! This is a **locally owned contract**, not a dependency-free re-export
//! and not an import of `vesper-runtime`'s token: hosts construct one
//! child `VoiceCancel` per voice turn and cancel it *alongside* — never
//! instead of — the runtime's own cancellation path.
//!
//! The token is waker-aware: async waiters register their waker through
//! [`VoiceCancel::poll_cancelled`] and are woken exactly once when
//! [`VoiceCancel::cancel`] is called. No busy-spinning, no thread
//! blocking, no async-runtime dependency in the core.

use std::future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

#[derive(Debug, Default)]
struct State {
    cancelled: bool,
    wakers: Vec<Waker>,
    /// Children registered for parent-cancellation propagation. Weak so a
    /// dropped child never keeps its parent's state alive (and vice
    /// versa: parents are held strongly only by their owners).
    children: Vec<std::sync::Weak<Mutex<State>>>,
}

/// Idempotent cancellation scope owned by the voice core.
#[derive(Debug, Clone, Default)]
pub struct VoiceCancel {
    state: Arc<Mutex<State>>,
}

impl VoiceCancel {
    /// An uncancelled scope.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A child scope that observes this scope's cancellation but can also
    /// be cancelled independently.
    ///
    /// Cancelling the child never cancels the parent or its siblings: a
    /// synthesis stream stopping does not stop the turn. Cancelling the
    /// parent cancels every still-alive child.
    #[must_use]
    pub fn child(&self) -> Self {
        let child = Self::default();
        if let Ok(mut parent) = self.state.lock() {
            parent.children.push(Arc::downgrade(&child.state));
            if parent.cancelled {
                // Parent already cancelled: the child starts cancelled.
                drop(parent);
                child.cancel();
            }
        }
        child
    }

    /// Requests cancellation (idempotent); wakes all registered waiters
    /// and propagates to still-alive children.
    pub fn cancel(&self) {
        let (wakers, children) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let wakers = if state.cancelled {
                // Idempotent: second cancel wakes nobody (already woken).
                Vec::new()
            } else {
                state.cancelled = true;
                std::mem::take(&mut state.wakers)
            };
            let children = state.children.clone();
            (wakers, children)
        };
        for waker in wakers {
            waker.wake();
        }
        propagate_cancel(&children);
    }

    /// Whether cancellation was requested (directly or via the parent).
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.cancelled)
            .unwrap_or(true)
    }

    /// Registers the caller's waker and reports cancellation readiness.
    ///
    /// This is the primitive streams and futures use so a `Pending`
    /// result is actually re-polled after [`Self::cancel`]. Returns
    /// `Poll::Ready(())` once cancelled.
    pub fn poll_cancelled(&self, context: &mut Context<'_>) -> Poll<()> {
        let Ok(mut state) = self.state.lock() else {
            return Poll::Ready(());
        };
        if state.cancelled {
            return Poll::Ready(());
        }
        let waker = context.waker();
        if let Some(existing) = state
            .wakers
            .iter_mut()
            .find(|candidate| candidate.will_wake(waker))
        {
            existing.clone_from(waker);
        } else {
            state.wakers.push(waker.clone());
        }
        Poll::Pending
    }

    /// Waits until cancellation is requested.
    ///
    /// Safe to await from any executor: the current task is woken by
    /// [`Self::cancel`]; there is no polling loop and no thread yield.
    pub async fn cancelled(&self) {
        future::poll_fn(|context| self.poll_cancelled(context)).await;
    }
}

/// Cascades cancellation to children, depth-first, waking every waiter.
///
/// Recursion depth equals the scope-hierarchy depth, which is structurally
/// tiny (turn → segment → stream); each level is visited once because a
/// child's `children` list is only appended by [`VoiceCancel::child`] and
/// consumed here via `mem::take`-style traversal of clones.
fn propagate_cancel(children: &[std::sync::Weak<Mutex<State>>]) {
    for child in children {
        let Some(state) = child.upgrade() else {
            continue;
        };
        let Ok(mut child_state) = state.lock() else {
            continue;
        };
        let (already, wakers, grandchildren) = if child_state.cancelled {
            (true, Vec::new(), Vec::new())
        } else {
            child_state.cancelled = true;
            (
                false,
                std::mem::take(&mut child_state.wakers),
                child_state.children.clone(),
            )
        };
        drop(child_state);
        if already {
            // Already cancelled (and therefore already propagated).
            continue;
        }
        for waker in wakers {
            waker.wake();
        }
        propagate_cancel(&grandchildren);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scope_is_live() {
        assert!(!VoiceCancel::new().is_cancelled());
    }

    #[test]
    fn cancel_is_idempotent_and_visible_to_clones() {
        let token = VoiceCancel::new();
        let observer = token.clone();
        token.cancel();
        token.cancel();
        assert!(observer.is_cancelled());
    }

    #[test]
    fn child_shares_parent_cancellation() {
        let parent = VoiceCancel::new();
        let child = parent.child();
        let grandchild = child.child();
        parent.cancel();
        assert!(child.is_cancelled());
        assert!(grandchild.is_cancelled());
    }

    #[test]
    fn child_cancellation_does_not_touch_parent() {
        let parent = VoiceCancel::new();
        let child = parent.child();
        child.cancel();
        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_future_resolves_after_cancel() {
        let token = VoiceCancel::new();
        let waiter = token.clone();
        let task = tokio::spawn(async move {
            waiter.cancelled().await;
        });
        // Give the waiter a chance to register before cancelling so the
        // wake path (not the already-cancelled fast path) is exercised.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(!task.is_finished());
        token.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("waiter must be woken, not spun")
            .unwrap();
    }

    #[tokio::test]
    async fn cancelled_future_resolves_immediately_when_already_cancelled() {
        let token = VoiceCancel::new();
        token.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(2), token.cancelled())
            .await
            .expect("fast path must resolve");
    }

    #[tokio::test]
    async fn parent_cancellation_wakes_child_waiters() {
        let parent = VoiceCancel::new();
        let child = parent.child();
        let waiter = child.clone();
        let task = tokio::spawn(async move {
            waiter.cancelled().await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        parent.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("child waiter must be woken by parent cancel")
            .unwrap();
    }

    #[tokio::test]
    async fn child_created_after_parent_cancel_starts_cancelled() {
        let parent = VoiceCancel::new();
        parent.cancel();
        let child = parent.child();
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn sibling_isolated_from_child_cancellation() {
        let parent = VoiceCancel::new();
        let first = parent.child();
        let second = parent.child();
        first.cancel();
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        assert!(!parent.is_cancelled());
    }

    #[tokio::test]
    async fn many_waiters_all_wake_once() {
        let token = VoiceCancel::new();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let waiter = token.clone();
            tasks.push(tokio::spawn(async move {
                waiter.cancelled().await;
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        token.cancel();
        for task in tasks {
            tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .expect("every waiter must be woken")
                .unwrap();
        }
    }
}
