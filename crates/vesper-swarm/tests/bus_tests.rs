//! Integration tests for the priority message bus (VRO-15 PR-4).

use std::time::Duration;

use vesper_swarm::bus::{
    BROADCAST, MessageBus, MessageKind, MessagePriority, OutgoingBroadcast, OutgoingMessage,
};
use vesper_swarm::error::SwarmError;

fn bus(capacity: usize) -> MessageBus {
    MessageBus::new(capacity).expect("valid capacity")
}

fn subscribe(bus: &MessageBus, worker: &str) {
    bus.subscribe(worker, &[]).expect("subscribe");
}

fn send(bus: &MessageBus, from: &str, to: &str, priority: MessagePriority, payload: &str) -> u64 {
    bus.send(
        OutgoingMessage::new(from, to)
            .priority(priority)
            .payload(payload),
    )
    .expect("send succeeds")
}

fn recv(bus: &MessageBus, worker: &str) -> String {
    bus.try_recv(worker)
        .expect("recv ok")
        .expect("message present")
        .message
        .payload
}

// ---------------------------------------------------------------------
// Strict priority ordering
// ---------------------------------------------------------------------

#[test]
fn urgent_sent_last_is_dequeued_first() {
    let bus = bus(16);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Low, "low-1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "normal-1");
    send(&bus, "queen", "w1", MessagePriority::High, "high-1");
    send(&bus, "queen", "w1", MessagePriority::Urgent, "urgent-1");

    assert_eq!(recv(&bus, "w1"), "urgent-1");
    assert_eq!(recv(&bus, "w1"), "high-1");
    assert_eq!(recv(&bus, "w1"), "normal-1");
    assert_eq!(recv(&bus, "w1"), "low-1");
    assert!(bus.try_recv("w1").expect("ok").is_none());
}

#[test]
fn same_tier_is_fifo() {
    let bus = bus(16);
    subscribe(&bus, "w1");
    for index in 0..5 {
        send(
            &bus,
            "queen",
            "w1",
            MessagePriority::Normal,
            &format!("n{index}"),
        );
    }
    for index in 0..5 {
        assert_eq!(recv(&bus, "w1"), format!("n{index}"));
    }
}

#[test]
fn priorities_interleave_correctly_across_inboxes() {
    let bus = bus(32);
    subscribe(&bus, "a");
    subscribe(&bus, "b");
    send(&bus, "queen", "a", MessagePriority::Low, "a-low");
    send(&bus, "queen", "b", MessagePriority::Urgent, "b-urgent");
    send(&bus, "queen", "a", MessagePriority::High, "a-high");
    send(&bus, "queen", "b", MessagePriority::Normal, "b-normal");

    assert_eq!(recv(&bus, "b"), "b-urgent");
    assert_eq!(recv(&bus, "a"), "a-high");
    assert_eq!(recv(&bus, "b"), "b-normal");
    assert_eq!(recv(&bus, "a"), "a-low");
}

// ---------------------------------------------------------------------
// Bounded eviction
// ---------------------------------------------------------------------

#[test]
fn full_bus_evicts_oldest_low_priority_message() {
    let bus = bus(2);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Low, "low-old");
    send(&bus, "queen", "w1", MessagePriority::Normal, "normal");
    // Bus is full (2). This High arrival must evict the oldest Low.
    send(&bus, "queen", "w1", MessagePriority::High, "high");

    assert_eq!(recv(&bus, "w1"), "high");
    assert_eq!(recv(&bus, "w1"), "normal");
    assert!(bus.try_recv("w1").expect("ok").is_none());
    // The eviction is observable.
    assert!(bus.events().iter().any(|event| matches!(
        event,
        vesper_swarm::bus::BusEvent::Evicted(_, MessagePriority::Low)
    )));
}

#[test]
fn urgent_messages_are_never_evicted() {
    let bus = bus(2);
    subscribe(&bus, "w1");
    let urgent_id = send(&bus, "queen", "w1", MessagePriority::Urgent, "urgent");
    send(&bus, "queen", "w1", MessagePriority::Low, "low");
    // Full. A Normal arrival evicts the Low, never the Urgent.
    send(&bus, "queen", "w1", MessagePriority::Normal, "normal");

    assert_eq!(recv(&bus, "w1"), "urgent");
    assert_eq!(recv(&bus, "w1"), "normal");
    assert_eq!(bus.events().len(), 1);
    let _ = urgent_id;
}

#[test]
fn lowest_priority_arrival_is_refused_when_nothing_lower_exists() {
    let bus = bus(2);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "n1");
    send(&bus, "queen", "w1", MessagePriority::High, "h1");
    // Full with Normal+High. A Low arrival has nothing below it to evict:
    // it must be refused loudly, not displace a higher tier.
    let refused = bus.send(
        OutgoingMessage::new("queen", "w1")
            .priority(MessagePriority::Low)
            .payload("low"),
    );
    assert_eq!(
        refused.unwrap_err(),
        SwarmError::BusFull {
            capacity: 2,
            priority: MessagePriority::Low,
        }
    );
    // And the existing traffic is intact.
    assert_eq!(recv(&bus, "w1"), "h1");
    assert_eq!(recv(&bus, "w1"), "n1");
    assert!(bus.events().iter().any(|event| matches!(
        event,
        vesper_swarm::bus::BusEvent::Refused(_, MessagePriority::Low)
    )));
}

#[test]
fn eviction_scans_tiers_bottom_up_preferring_oldest() {
    let bus = bus(3);
    subscribe(&bus, "w1");
    subscribe(&bus, "w2");
    send(
        &bus,
        "queen",
        "w1",
        MessagePriority::Normal,
        "w1-normal-old",
    );
    send(&bus, "queen", "w2", MessagePriority::Low, "w2-low-oldest");
    send(&bus, "queen", "w1", MessagePriority::Low, "w1-low-newer");
    // Full. Urgent arrival evicts the globally oldest Low (w2's).
    send(&bus, "queen", "w1", MessagePriority::Urgent, "urgent");

    // w2's low was the oldest admission, so it is gone (the only eviction).
    assert!(bus.try_recv("w2").expect("ok").is_none());
    assert_eq!(bus.events().len(), 1);
    // w1 keeps its remaining messages, strict-priority ordered: Urgent,
    // then Normal (higher tier than Low), then the surviving Low.
    assert_eq!(recv(&bus, "w1"), "urgent");
    assert_eq!(recv(&bus, "w1"), "w1-normal-old");
    assert_eq!(recv(&bus, "w1"), "w1-low-newer");
}

#[test]
fn normal_tier_is_evicted_before_high_when_no_low_exists() {
    let bus = bus(2);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "normal");
    send(&bus, "queen", "w1", MessagePriority::High, "high");
    // Full. Urgent arrival: no Low exists, so the oldest Normal goes.
    send(&bus, "queen", "w1", MessagePriority::Urgent, "urgent");
    assert_eq!(recv(&bus, "w1"), "urgent");
    assert_eq!(recv(&bus, "w1"), "high");
    assert!(bus.try_recv("w1").expect("ok").is_none());
}

// ---------------------------------------------------------------------
// Backpressure: never blocks, always loud
// ---------------------------------------------------------------------

#[test]
fn sends_never_block_and_report_bus_full_loudly() {
    let bus = bus(1);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "first");
    // Second send: evicts nothing (Normal has nothing below), refuses.
    let refused = bus.send(OutgoingMessage::new("queen", "w1").payload("second"));
    assert!(matches!(refused, Err(SwarmError::BusFull { .. })));
    // The bus is still coherent.
    assert_eq!(bus.queued(), 1);
    assert_eq!(recv(&bus, "w1"), "first");
}

#[test]
fn urgent_backpressure_still_succeeds_by_evicting_low() {
    let bus = bus(1);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Low, "low");
    // Urgent must always find room.
    send(&bus, "queen", "w1", MessagePriority::Urgent, "urgent");
    assert_eq!(recv(&bus, "w1"), "urgent");
    assert!(bus.try_recv("w1").expect("ok").is_none());
}

// ---------------------------------------------------------------------
// TTL expiry
// ---------------------------------------------------------------------

#[test]
fn expired_messages_are_discarded_at_dequeue() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    bus.send(
        OutgoingMessage::new("queen", "w1")
            .payload("stale")
            .ttl(Duration::from_millis(1)),
    )
    .expect("send");
    // Let it expire.
    std::thread::sleep(Duration::from_millis(20));
    assert!(bus.try_recv("w1").expect("ok").is_none());
    assert!(
        bus.events()
            .iter()
            .any(|event| matches!(event, vesper_swarm::bus::BusEvent::Expired(_)))
    );
}

#[test]
fn live_messages_within_ttl_are_delivered() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    bus.send(
        OutgoingMessage::new("queen", "w1")
            .payload("fresh")
            .ttl(Duration::from_secs(60)),
    )
    .expect("send");
    assert_eq!(recv(&bus, "w1"), "fresh");
}

#[test]
fn expiry_drains_until_a_live_message_or_empty() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    bus.send(
        OutgoingMessage::new("queen", "w1")
            .priority(MessagePriority::High)
            .payload("stale-high")
            .ttl(Duration::from_millis(1)),
    )
    .expect("send");
    bus.send(OutgoingMessage::new("queen", "w1").payload("live-low"))
        .expect("send");
    std::thread::sleep(Duration::from_millis(20));
    // The stale High is discarded; the live Low is delivered.
    assert_eq!(recv(&bus, "w1"), "live-low");
    assert!(bus.try_recv("w1").expect("ok").is_none());
}

// ---------------------------------------------------------------------
// Filters and broadcast
// ---------------------------------------------------------------------

#[test]
fn inbox_filters_drop_unwanted_kinds() {
    let bus = bus(8);
    bus.subscribe("w1", &[MessageKind::TaskAssign, MessageKind::Heartbeat])
        .expect("subscribe");
    bus.send(
        OutgoingMessage::new("queen", "w1")
            .kind(MessageKind::TaskAssign)
            .payload("task"),
    )
    .expect("send");
    // Data is filtered out: not an error, consumes no capacity.
    let filtered = bus
        .send(OutgoingMessage::new("queen", "w1").payload("data"))
        .expect("filtered delivery is not an error");
    assert_eq!(filtered, 0);
    assert_eq!(recv(&bus, "w1"), "task");
    assert!(bus.try_recv("w1").expect("ok").is_none());
}

#[test]
fn broadcast_fans_out_to_every_accepting_subscriber() {
    let bus = bus(16);
    subscribe(&bus, "a");
    subscribe(&bus, "b");
    bus.subscribe("c", &[MessageKind::Control])
        .expect("subscribe");
    let ids = bus
        .broadcast(
            OutgoingBroadcast::new("queen")
                .kind(MessageKind::Data)
                .payload("to-all"),
        )
        .expect("broadcast");
    // a and b accept Data; c filters it out (id 0, no capacity consumed).
    assert_eq!(ids.len(), 3);
    assert_eq!(recv(&bus, "a"), "to-all");
    assert_eq!(recv(&bus, "b"), "to-all");
    assert!(bus.try_recv("c").expect("ok").is_none());
}

#[test]
fn broadcast_respects_global_capacity_loudly() {
    let bus = bus(2);
    subscribe(&bus, "a");
    subscribe(&bus, "b");
    // Fanout of 2 into capacity 2 is fine.
    bus.broadcast(OutgoingBroadcast::new("queen").payload("wave"))
        .expect("fits");
    // A second fanout of 2 must fail loudly (would evict nothing below
    // Normal across the two inboxes... it can evict the Normal wave).
    let result = bus.broadcast(OutgoingBroadcast::new("queen").payload("wave-2"));
    match result {
        Ok(_) => {
            // Eviction succeeded (old wave evicted) — also legal. Both
            // subscribers then see only the new wave.
            assert_eq!(recv(&bus, "a"), "wave-2");
            assert_eq!(recv(&bus, "b"), "wave-2");
        }
        Err(SwarmError::BusFull { .. }) => {}
        other => panic!("unexpected broadcast outcome: {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Acknowledgments
// ---------------------------------------------------------------------

#[test]
fn ack_tracking_requires_acknowledgment_for_marked_messages() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    bus.send(
        OutgoingMessage::new("queen", "w1")
            .payload("must-ack")
            .requires_ack(),
    )
    .expect("send");
    assert_eq!(bus.pending_ack_count(), 0, "ack owed only after delivery");
    let received = bus.try_recv("w1").expect("ok").expect("present");
    assert!(received.message.requires_ack);
    assert_eq!(bus.pending_ack_count(), 1);
    bus.ack("w1", received.message.id).expect("ack");
    assert_eq!(bus.pending_ack_count(), 0);
    // Double ack is rejected.
    assert_eq!(
        bus.ack("w1", received.message.id).unwrap_err(),
        SwarmError::UnknownAck(received.message.id)
    );
}

#[test]
fn unmarked_messages_require_no_ack() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "plain");
    let _ = bus.try_recv("w1").expect("ok").expect("present");
    assert_eq!(bus.pending_ack_count(), 0);
}

// ---------------------------------------------------------------------
// Subscriber lifecycle
// ---------------------------------------------------------------------

#[test]
fn unsubscribe_drops_queued_traffic_and_rejects_further_use() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "lost");
    bus.unsubscribe("w1").expect("unsubscribe");
    assert_eq!(bus.queued(), 0);
    assert!(matches!(
        bus.try_recv("w1"),
        Err(SwarmError::UnknownSubscriber(_))
    ));
    assert!(matches!(
        bus.send(OutgoingMessage::new("queen", "w1")),
        Err(SwarmError::UnknownSubscriber(_))
    ));
    // Resubscription is legal.
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "again");
    assert_eq!(recv(&bus, "w1"), "again");
}

#[test]
fn closed_bus_refuses_everything() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    bus.close();
    assert!(matches!(
        bus.send(OutgoingMessage::new("queen", "w1")),
        Err(SwarmError::BusClosed)
    ));
    assert!(matches!(
        bus.broadcast(OutgoingBroadcast::new("queen")),
        Err(SwarmError::BusClosed)
    ));
    assert!(matches!(
        bus.subscribe("w2", &[]),
        Err(SwarmError::BusClosed)
    ));
}

#[test]
fn sends_to_unknown_subscribers_fail_loudly() {
    let bus = bus(8);
    assert!(matches!(
        bus.send(OutgoingMessage::new("queen", "ghost")),
        Err(SwarmError::UnknownSubscriber(_))
    ));
    subscribe(&bus, "w1");
    assert!(matches!(
        bus.try_recv("ghost"),
        Err(SwarmError::UnknownSubscriber(_))
    ));
}

// ---------------------------------------------------------------------
// Async recv: parks the task, wakes on admission
// ---------------------------------------------------------------------

#[tokio::test]
async fn recv_parks_until_a_message_arrives_then_wakes() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    let waiter = tokio::spawn({
        let bus = bus.clone();
        async move {
            let received = bus.recv("w1").await.expect("recv");
            received.message.payload
        }
    });
    // Give the waiter a moment to park.
    tokio::time::sleep(Duration::from_millis(20)).await;
    send(&bus, "queen", "w1", MessagePriority::High, "wake-up");
    let payload = waiter.await.expect("join");
    assert_eq!(payload, "wake-up");
}

#[tokio::test]
async fn recv_delivers_already_queued_traffic_immediately() {
    let bus = bus(8);
    subscribe(&bus, "w1");
    send(&bus, "queen", "w1", MessagePriority::Normal, "pre-queued");
    let received = bus.recv("w1").await.expect("recv");
    assert_eq!(received.message.payload, "pre-queued");
}

#[tokio::test]
async fn broadcast_marker_constant_is_not_a_valid_subscriber() {
    let bus = bus(8);
    assert!(matches!(
        bus.subscribe(BROADCAST, &[]),
        Err(SwarmError::DuplicateSubscriber(_)) | Ok(())
    ));
}

// ---------------------------------------------------------------------
// O(1) dequeue sanity: 100k messages across tiers stay fast and ordered
// ---------------------------------------------------------------------

#[test]
fn large_volume_preserves_order_and_never_loses_urgent() {
    let bus = bus(100_000);
    subscribe(&bus, "w1");
    for index in 0..50_000 {
        bus.send(
            OutgoingMessage::new("queen", "w1")
                .priority(MessagePriority::Low)
                .payload(index.to_string()),
        )
        .expect("send");
    }
    send(
        &bus,
        "queen",
        "w1",
        MessagePriority::Urgent,
        "last-but-first",
    );
    let first = recv(&bus, "w1");
    assert_eq!(first, "last-but-first");
    let second = recv(&bus, "w1");
    assert_eq!(second, "0");
    assert_eq!(bus.queued(), 49_999);
}

#[tokio::test]
async fn close_and_unsubscribe_release_parked_receivers() {
    for close in [true, false] {
        let bus = bus(2);
        subscribe(&bus, "w");
        let mut receive = Box::pin(bus.recv("w"));
        std::future::poll_fn(|cx| {
            use std::future::Future;
            assert!(receive.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        if close {
            bus.close();
        } else {
            bus.unsubscribe("w").unwrap();
        }
        let error = tokio::time::timeout(Duration::from_secs(1), receive)
            .await
            .expect("receiver wakes")
            .unwrap_err();
        assert_eq!(
            error,
            if close {
                SwarmError::BusClosed
            } else {
                SwarmError::UnknownSubscriber("w".into())
            }
        );
        if close {
            assert_eq!(bus.try_recv("w"), Err(SwarmError::BusClosed));
        }
    }
}

#[test]
fn refused_broadcast_has_no_partial_deliveries() {
    let bus = bus(1);
    subscribe(&bus, "a");
    subscribe(&bus, "b");
    assert!(
        bus.broadcast(OutgoingBroadcast::new("q").priority(MessagePriority::Urgent))
            .is_err()
    );
    assert_eq!(bus.queued(), 0);
}

#[test]
fn expired_and_unsubscribed_traffic_leaves_no_ack_debt() {
    let bus = bus(2);
    subscribe(&bus, "w");
    bus.send(
        OutgoingMessage::new("q", "w")
            .ttl(Duration::ZERO)
            .requires_ack(),
    )
    .unwrap();
    assert!(bus.try_recv("w").unwrap().is_none());
    assert_eq!(bus.pending_ack_count(), 0);
    bus.send(OutgoingMessage::new("q", "w").requires_ack())
        .unwrap();
    bus.try_recv("w").unwrap().unwrap();
    assert_eq!(bus.pending_ack_count(), 1);
    bus.unsubscribe("w").unwrap();
    assert_eq!(bus.pending_ack_count(), 0);
}
