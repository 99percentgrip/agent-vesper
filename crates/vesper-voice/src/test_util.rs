//! Test/adapter-internal sync executor (park/unpark, runtime-agnostic).
//!
//! Used by contract tests and by adapters that must drive a boxed future
//! to completion inside a blocking closure. Not a production scheduler:
//! no work stealing, no timers — a future that never completes would
//! block forever, which adapters prevent with their own deadlines.

use std::future::Future;
use std::sync::{Arc, Condvar, Mutex};

struct Park {
    notified: Mutex<bool>,
    cond: Condvar,
}

impl std::task::Wake for Park {
    fn wake(self: Arc<Self>) {
        let mut notified = self.notified.lock().unwrap();
        *notified = true;
        self.cond.notify_one();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.clone().wake();
    }
}

/// Drives `future` to completion on the current thread.
///
/// # Panics
///
/// Panics propagate from the future. A future that never completes
/// blocks forever; callers bound their work with deadlines first.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let park = Arc::new(Park {
        notified: Mutex::new(false),
        cond: Condvar::new(),
    });
    let waker = std::task::Waker::from(park.clone());
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => {
                let mut notified = park.notified.lock().unwrap();
                if !*notified {
                    notified = park.cond.wait(notified).unwrap();
                }
                *notified = false;
            }
        }
    }
}
