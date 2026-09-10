//! TTL and ACK expiry can run on explicit caller time without sleeping.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use vesper_swarm::bus::{BusClock, MessageBus, OutgoingMessage};

struct ManualClock {
    origin: Instant,
    seconds: AtomicU64,
}
impl BusClock for ManualClock {
    fn now(&self) -> Instant {
        self.origin + Duration::from_secs(self.seconds.load(Ordering::Acquire))
    }
}
#[test]
fn caller_time_drives_queue_and_ack_expiry_across_cloned_handles() {
    let clock = Arc::new(ManualClock {
        origin: Instant::now(),
        seconds: AtomicU64::new(0),
    });
    let bus = MessageBus::with_clock(2, clock.clone()).unwrap();
    bus.subscribe("worker", &[]).unwrap();
    bus.send(
        OutgoingMessage::new("sender", "worker")
            .ttl(Duration::from_secs(10))
            .requires_ack(),
    )
    .unwrap();
    bus.try_recv("worker").unwrap().unwrap();
    assert_eq!(bus.pending_ack_count(), 1);
    bus.send(OutgoingMessage::new("sender", "worker").ttl(Duration::from_secs(10)))
        .unwrap();
    clock.seconds.store(10, Ordering::Release);
    let clone = bus.clone();
    assert!(clone.try_recv("worker").unwrap().is_none());
    assert_eq!(clone.pending_ack_count(), 0);
}
