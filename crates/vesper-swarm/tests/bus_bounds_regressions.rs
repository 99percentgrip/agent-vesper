//! Fail-closed bus resource bounds.
use std::time::Duration;
use vesper_swarm::bus::{MessageBus, OutgoingMessage};

#[test]
fn oversized_payload_and_overflow_ttl_are_refused_without_admission() {
    let bus = MessageBus::new(2).unwrap();
    bus.subscribe("w", &[]).unwrap();
    assert!(
        bus.send(OutgoingMessage::new("q", "w").payload("x".repeat(1024 * 1024 + 1)))
            .is_err()
    );
    assert!(
        bus.send(OutgoingMessage::new("q", "w").ttl(Duration::MAX))
            .is_err()
    );
    assert_eq!(bus.queued(), 0);
}

#[test]
fn ack_capacity_refuses_delivery_without_losing_queued_message() {
    let bus = MessageBus::new(1).unwrap();
    bus.subscribe("w", &[]).unwrap();
    bus.send(OutgoingMessage::new("q", "w").requires_ack())
        .unwrap();
    let first = bus.try_recv("w").unwrap().unwrap();
    bus.send(OutgoingMessage::new("q", "w").requires_ack())
        .unwrap();
    assert!(bus.try_recv("w").is_err());
    assert_eq!(bus.queued(), 1);
    bus.ack("w", first.message.id).unwrap();
    assert!(bus.try_recv("w").unwrap().is_some());
}

#[test]
fn oversized_broadcast_is_refused_atomically_by_byte_budget() {
    use vesper_swarm::bus::OutgoingBroadcast;
    let bus = MessageBus::new(1000).unwrap();
    for id in 0..100 {
        bus.subscribe(&format!("w{id}"), &[]).unwrap();
    }
    assert!(
        bus.broadcast(OutgoingBroadcast::new("q").payload("x".repeat(1024 * 1024)))
            .is_err()
    );
    assert_eq!(bus.queued(), 0);
}

#[tokio::test]
async fn delivered_ack_debt_expires_with_its_message_ttl() {
    let bus = MessageBus::new(1).unwrap();
    bus.subscribe("w", &[]).unwrap();
    bus.send(
        OutgoingMessage::new("q", "w")
            .requires_ack()
            .ttl(Duration::from_millis(50)),
    )
    .unwrap();
    bus.try_recv("w").unwrap().unwrap();
    assert_eq!(bus.pending_ack_count(), 1);
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert_eq!(bus.pending_ack_count(), 0);
}
