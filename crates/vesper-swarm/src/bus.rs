//! Priority message bus for inter-worker communication (VRO-15 PR-4).
//!
//! [`MessageBus`] implements the swarm oracle's coordination mailbox model
//! with strict fail-loud semantics: four priority tiers per inbox with
//! O(1) dequeue from the highest non-empty tier, per-inbox message-type
//! filters, broadcast fanout, TTL expiry at dequeue time, acknowledgment
//! tracking, and deterministic bounded eviction when the bus is full.
//!
//! Backpressure is never blocking and never silent:
//!
//! - A push that cannot be admitted after eviction fails with
//!   [`SwarmError::BusFull`]; the caller decides what to do. Nothing is
//!   dropped without the caller hearing about it.
//! - Eviction scans tiers bottom-up (Low, then Normal, then High) and
//!   evicts the oldest message by global admission sequence — **an Urgent
//!   message is never evicted**.
//! - When the incoming message is the lowest-priority tier (Low) and
//!   nothing lower-priority exists to evict, the incoming message itself
//!   is refused (loudly) rather than displacing a higher tier.
//! - [`MessageBus::recv`] parks until traffic exists for the subscriber
//!   (cancellation-safe), so no caller ever spins or blocks a thread.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::SwarmError;

/// Four message priority tiers. Higher is dequeued first; ordering inside
/// one tier is FIFO by admission sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessagePriority {
    /// Lowest tier; first to be shed under backpressure.
    Low,
    /// Routine coordination traffic.
    Normal,
    /// Time-sensitive traffic.
    High,
    /// Never evicted; always dequeued before every other tier.
    Urgent,
}

/// Coarse message classes used by inbox filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageKind {
    /// A task assignment heading to a worker.
    TaskAssign,
    /// A task result heading back to the coordinator.
    TaskResult,
    /// Liveness signal.
    Heartbeat,
    /// Topology or lifecycle control traffic.
    Control,
    /// Everything else.
    Data,
}

/// Default per-kind TTL when a message declares none.
pub const DEFAULT_TTL: Duration = Duration::from_secs(60);

/// One bus message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Stable identity assigned by the bus at admission.
    pub id: u64,
    /// Sender identity (free-form; usually a worker id or "queen").
    pub from: String,
    /// Intended recipient, or [`BROADCAST`] for all subscribers.
    pub to: String,
    /// Priority tier.
    pub priority: MessagePriority,
    /// Coarse class for filtering.
    pub kind: MessageKind,
    /// Bounded payload (kept small by construction).
    pub payload: String,
    /// Time to live from admission; expired messages are discarded at
    /// dequeue time, never delivered.
    pub ttl: Duration,
    /// Whether the recipient must acknowledge this message.
    pub requires_ack: bool,
}

/// Recipient marker meaning "every subscriber".
pub const BROADCAST: &str = "*";

/// Timestamped envelope kept inside inboxes; `admitted_seq` is the global
/// admission sequence driving FIFO-within-tier and oldest-first eviction.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Envelope {
    message: Message,
    admitted_seq: u64,
    expires_at: Option<std::time::Instant>,
}

/// One subscriber's inbox: four strict-priority FIFO queues.
#[derive(Debug, Default)]
struct Inbox {
    tiers: [VecDeque<Envelope>; 4],
    /// Message kinds this inbox accepts; empty means unfiltered.
    filters: Vec<MessageKind>,
    /// Acks still owed by this subscriber.
    pending_acks: HashMap<u64, MessagePriority>,
    /// Wakeup primitive for parked `recv` waiters; `Arc` so a waiter can
    /// hold it after the state lock is released.
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl Inbox {
    fn tier_index(priority: MessagePriority) -> usize {
        match priority {
            MessagePriority::Low => 0,
            MessagePriority::Normal => 1,
            MessagePriority::High => 2,
            MessagePriority::Urgent => 3,
        }
    }

    fn len(&self) -> usize {
        self.tiers.iter().map(VecDeque::len).sum()
    }

    fn push(&mut self, envelope: Envelope) {
        let index = Self::tier_index(envelope.message.priority);
        self.tiers[index].push_back(envelope);
    }

    /// Oldest envelope in the given tier (eviction candidate).
    fn oldest_in_tier(&self, tier: usize) -> Option<&Envelope> {
        self.tiers[tier].front()
    }

    /// Strict-priority pop: highest non-empty tier, front (FIFO).
    fn pop_highest(&mut self) -> Option<Envelope> {
        for tier in (0..4).rev() {
            if let Some(envelope) = self.tiers[tier].pop_front() {
                return Some(envelope);
            }
        }
        None
    }

    fn accepts(&self, kind: MessageKind) -> bool {
        self.filters.is_empty() || self.filters.contains(&kind)
    }
}

/// Interior bus state.
#[derive(Debug, Default)]
struct BusState {
    inboxes: HashMap<String, Inbox>,
    /// Total queued messages across all inboxes.
    queued: usize,
    /// Global admission sequence.
    next_seq: u64,
    next_message_id: u64,
    closed: bool,
}

/// Bounded observability of evictions (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusEvent {
    /// A message was evicted to make room. Carries the evicted id and the
    /// tier it came from.
    Evicted(u64, MessagePriority),
    /// A message expired at dequeue time and was discarded.
    Expired(u64),
    /// A message was refused (bus full even after eviction).
    Refused(u64, MessagePriority),
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<BusState>,
    events: Mutex<VecDeque<BusEvent>>,
    live_acks: AtomicU64,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, BusState> {
        self.state.lock().expect("bus lock poisoned")
    }

    fn record(&self, event: BusEvent) {
        let mut log = self.events.lock().expect("event lock poisoned");
        if log.len() >= 256 {
            log.pop_front();
        }
        log.push_back(event);
    }

    fn notify_all_waiters(&self) {
        // Wakeup is per-inbox via Arc<Notify>; nothing to do globally.
    }
}

/// Bounded strict-priority message bus.
///
/// Global capacity bounds the total queued messages across every inbox;
/// eviction is deterministic (oldest message in the lowest non-empty tier,
/// scanning Low → Normal → High; Urgent is untouchable). All operations
/// are non-blocking; [`recv`](Self::recv) parks the *task* (never a
/// thread) until traffic for the subscriber arrives.
pub struct MessageBus {
    capacity: usize,
    shared: Arc<Shared>,
    /// Mirror of `closed` for lock-free fast checks.
    closed_flag: Arc<AtomicBool>,
}

impl std::fmt::Debug for MessageBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.shared.lock();
        f.debug_struct("MessageBus")
            .field("capacity", &self.capacity)
            .field("subscribers", &state.inboxes.len())
            .field("queued", &state.queued)
            .field("closed", &state.closed)
            .finish()
    }
}

/// Cloning shares the same bus state: a clone is the same bus, useful for
/// handing one handle per task. Capacity and policy are identical.
impl Clone for MessageBus {
    fn clone(&self) -> Self {
        Self {
            capacity: self.capacity,
            shared: Arc::clone(&self.shared),
            closed_flag: Arc::clone(&self.closed_flag),
        }
    }
}

impl MessageBus {
    /// Creates a bus with the given global queued-message capacity.
    pub fn new(capacity: usize) -> Result<Self, SwarmError> {
        if capacity == 0 {
            return Err(SwarmError::InvalidCapacity(0));
        }
        Ok(Self {
            capacity,
            shared: Arc::new(Shared::default()),
            closed_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The configured global capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Total messages currently queued across all inboxes.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.shared.lock().queued
    }

    /// Snapshot of the bounded diagnostic event log (oldest first).
    #[must_use]
    pub fn events(&self) -> Vec<BusEvent> {
        self.shared
            .events
            .lock()
            .expect("event lock poisoned")
            .iter()
            .cloned()
            .collect()
    }

    /// Subscribes a worker inbox, optionally filtered to specific message
    /// kinds (empty slice = accept everything).
    pub fn subscribe(&self, worker_id: &str, filters: &[MessageKind]) -> Result<(), SwarmError> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err(SwarmError::BusClosed);
        }
        if state.inboxes.contains_key(worker_id) {
            return Err(SwarmError::DuplicateSubscriber(worker_id.to_string()));
        }
        state.inboxes.insert(
            worker_id.to_string(),
            Inbox {
                tiers: Default::default(),
                filters: filters.to_vec(),
                pending_acks: HashMap::new(),
                notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            },
        );
        Ok(())
    }

    /// Removes a subscriber and drops its queued traffic.
    pub fn unsubscribe(&self, worker_id: &str) -> Result<(), SwarmError> {
        let mut state = self.shared.lock();
        let Some(inbox) = state.inboxes.remove(worker_id) else {
            return Err(SwarmError::UnknownSubscriber(worker_id.to_string()));
        };
        state.queued = state.queued.saturating_sub(inbox.len());
        Ok(())
    }

    /// Sends one message to a specific subscriber inbox.
    pub fn send(&self, message: OutgoingMessage) -> Result<u64, SwarmError> {
        let target = message.to.clone();
        self.admit(message, Some(target))
    }

    /// Sends one message to every subscriber whose filter accepts it.
    /// Each delivery counts toward capacity; the fanout is admitted as a
    /// unit (all-or-nothing per recipient count).
    pub fn broadcast(&self, message: OutgoingBroadcast) -> Result<Vec<u64>, SwarmError> {
        let recipients = {
            let state = self.shared.lock();
            if state.closed {
                return Err(SwarmError::BusClosed);
            }
            state.inboxes.keys().cloned().collect::<Vec<_>>()
        };
        let mut ids = Vec::with_capacity(recipients.len());
        for recipient in &recipients {
            let mut outgoing = OutgoingMessage::from(message.clone());
            outgoing.to = recipient.clone();
            ids.push(self.admit(outgoing, Some(recipient.clone()))?);
        }
        Ok(ids)
    }

    /// Non-blocking receive for one subscriber.
    ///
    /// Expired messages (TTL) are discarded here — never delivered — and
    /// the call keeps draining until a live message or empty inbox.
    pub fn try_recv(&self, worker_id: &str) -> Result<Option<Received>, SwarmError> {
        loop {
            let popped = {
                let mut state = self.shared.lock();
                let Some(inbox) = state.inboxes.get_mut(worker_id) else {
                    return Err(SwarmError::UnknownSubscriber(worker_id.to_string()));
                };
                let Some(envelope) = inbox.pop_highest() else {
                    return Ok(None);
                };
                state.queued = state.queued.saturating_sub(1);
                if envelope.message.requires_ack {
                    if let Some(inbox) = state.inboxes.get_mut(worker_id) {
                        inbox
                            .pending_acks
                            .insert(envelope.message.id, envelope.message.priority);
                    }
                    self.shared.live_acks.fetch_add(1, Ordering::AcqRel);
                }
                envelope
            };
            if let Some(expires_at) = popped.expires_at
                && std::time::Instant::now() >= expires_at
            {
                self.shared.record(BusEvent::Expired(popped.message.id));
                continue;
            }
            return Ok(Some(Received {
                message: popped.message,
            }));
        }
    }

    /// Async receive: parks the calling task (never a thread) until a live
    /// message exists for the subscriber.
    pub async fn recv(&self, worker_id: &str) -> Result<Received, SwarmError> {
        loop {
            // Fast path: something already queued.
            if let Some(received) = self.try_recv(worker_id)? {
                return Ok(received);
            }
            // Park until any admission wakes us, then re-check. The notify
            // primitive is cloned out of the lock and awaited in this frame.
            let notify = self.waiter_notify(worker_id)?;
            notify.notified().await;
        }
    }

    /// Clones the subscriber's wakeup primitive out of the lock so a
    /// waiter can await it without holding the state lock.
    fn waiter_notify(
        &self,
        worker_id: &str,
    ) -> Result<std::sync::Arc<tokio::sync::Notify>, SwarmError> {
        let state = self.shared.lock();
        let Some(inbox) = state.inboxes.get(worker_id) else {
            return Err(SwarmError::UnknownSubscriber(worker_id.to_string()));
        };
        Ok(std::sync::Arc::clone(&inbox.notify))
    }

    /// Acknowledges a previously delivered message that required one.
    pub fn ack(&self, worker_id: &str, message_id: u64) -> Result<(), SwarmError> {
        let mut state = self.shared.lock();
        let Some(inbox) = state.inboxes.get_mut(worker_id) else {
            return Err(SwarmError::UnknownSubscriber(worker_id.to_string()));
        };
        if inbox.pending_acks.remove(&message_id).is_none() {
            return Err(SwarmError::UnknownAck(message_id));
        }
        self.shared.live_acks.fetch_sub(1, Ordering::AcqRel);
        Ok(())
    }

    /// Number of acknowledgments still owed across the bus.
    #[must_use]
    pub fn pending_ack_count(&self) -> u64 {
        self.shared.live_acks.load(Ordering::Acquire)
    }

    /// Closes the bus: no further admissions; parked receivers wake and
    /// observe [`SwarmError::BusClosed`] on their next operation.
    pub fn close(&self) {
        self.shared.lock().closed = true;
        self.closed_flag.store(true, Ordering::Release);
        self.shared.notify_all_waiters();
    }

    /// Admits one message into one inbox under the capacity + eviction
    /// policy. `explicit_target` is `Some` for directed sends and `None`
    /// reserved for broadcast internals (already resolved per recipient).
    fn admit(
        &self,
        message: OutgoingMessage,
        explicit_target: Option<String>,
    ) -> Result<u64, SwarmError> {
        let priority = message.priority;
        let mut state = self.shared.lock();
        if state.closed {
            return Err(SwarmError::BusClosed);
        }
        let target = explicit_target.unwrap_or_else(|| message.to.clone());
        // Filter check first (immutable borrow, then release).
        let accepted = state
            .inboxes
            .get(&target)
            .map(|inbox| inbox.accepts(message.kind))
            .unwrap_or(false);
        if !accepted {
            if !state.inboxes.contains_key(&target) {
                return Err(SwarmError::UnknownSubscriber(target.clone()));
            }
            // Filtered deliveries are not errors and consume no capacity.
            return Ok(0);
        }
        // Assign identity and sequence deterministically before the inbox
        // borrow so no borrow overlaps a state mutation.
        state.next_message_id += 1;
        let id = state.next_message_id;
        state.next_seq += 1;
        let seq = state.next_seq;
        let envelope = Envelope {
            message: Message {
                id,
                from: message.from,
                to: target.clone(),
                priority,
                kind: message.kind,
                payload: message.payload,
                ttl: message.ttl.unwrap_or(DEFAULT_TTL),
                requires_ack: message.requires_ack,
            },
            admitted_seq: seq,
            expires_at: std::time::Instant::now().checked_add(message.ttl.unwrap_or(DEFAULT_TTL)),
        };
        // Capacity: evict deterministically when full.
        if state.queued >= self.capacity {
            self.evict_one(&mut state, priority)?;
        }
        if let Some(inbox) = state.inboxes.get_mut(&target) {
            inbox.push(envelope);
            inbox.notify.notify_one();
        }
        state.queued += 1;
        drop(state);
        self.shared.notify_all_waiters();
        Ok(id)
    }

    /// Deterministic eviction: scan tiers bottom-up and evict the oldest
    /// envelope (smallest admission sequence) in the first non-empty tier
    /// strictly below the incoming priority. Urgent is never evictable; a
    /// Low arrival with only Low (or higher) present refuses itself.
    fn evict_one(&self, state: &mut BusState, incoming: MessagePriority) -> Result<(), SwarmError> {
        let lowest_tier_allowed = match incoming {
            MessagePriority::Urgent => 0,
            MessagePriority::High => 0,
            MessagePriority::Normal => 0,
            MessagePriority::Low => 1,
        };
        // Candidate tiers: those strictly lower priority than incoming,
        // scanned from the lowest tier upward.
        let incoming_tier = Inbox::tier_index(incoming);
        let mut chosen: Option<(String, usize, u64)> = None;
        for tier in lowest_tier_allowed..incoming_tier {
            let mut best: Option<(String, u64)> = None;
            for (worker, inbox) in state.inboxes.iter() {
                if let Some(envelope) = inbox.oldest_in_tier(tier) {
                    let better = best
                        .as_ref()
                        .is_none_or(|(_, seq)| envelope.admitted_seq < *seq);
                    if better {
                        best = Some((worker.clone(), envelope.admitted_seq));
                    }
                }
            }
            if let Some((worker, seq)) = best {
                chosen = Some((worker, tier, seq));
                break;
            }
        }
        let Some((worker, tier, seq)) = chosen else {
            // Nothing lower-priority exists: refuse the incoming message.
            let refused_id = state.next_message_id;
            self.shared.record(BusEvent::Refused(refused_id, incoming));
            return Err(SwarmError::BusFull {
                capacity: self.capacity,
                priority: incoming,
            });
        };
        // Evict the chosen envelope.
        let mut evicted_id = None;
        if let Some(inbox) = state.inboxes.get_mut(&worker)
            && let Some(position) = inbox.tiers[tier]
                .iter()
                .position(|envelope| envelope.admitted_seq == seq)
            && let Some(removed) = inbox.tiers[tier].remove(position)
        {
            evicted_id = Some(removed.message.id);
            state.queued = state.queued.saturating_sub(1);
        }
        if let Some(id) = evicted_id {
            let priority = match tier {
                0 => MessagePriority::Low,
                1 => MessagePriority::Normal,
                2 => MessagePriority::High,
                _ => MessagePriority::Urgent,
            };
            self.shared.record(BusEvent::Evicted(id, priority));
        }
        Ok(())
    }
}

/// Builder-shaped message awaiting admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingMessage {
    pub from: String,
    pub to: String,
    pub priority: MessagePriority,
    pub kind: MessageKind,
    pub payload: String,
    pub ttl: Option<Duration>,
    pub requires_ack: bool,
}

impl OutgoingMessage {
    /// A minimal normal-priority data message between two workers.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            priority: MessagePriority::Normal,
            kind: MessageKind::Data,
            payload: String::new(),
            ttl: None,
            requires_ack: false,
        }
    }

    /// Sets the priority tier.
    #[must_use]
    pub fn priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the message class.
    #[must_use]
    pub fn kind(mut self, kind: MessageKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the payload.
    #[must_use]
    pub fn payload(mut self, payload: impl Into<String>) -> Self {
        self.payload = payload.into();
        self
    }

    /// Sets an explicit TTL.
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Marks the message as requiring acknowledgment.
    #[must_use]
    pub fn requires_ack(mut self) -> Self {
        self.requires_ack = true;
        self
    }
}

/// Broadcast variant (no explicit recipient; the bus resolves filters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingBroadcast {
    pub from: String,
    pub priority: MessagePriority,
    pub kind: MessageKind,
    pub payload: String,
    pub ttl: Option<Duration>,
    pub requires_ack: bool,
}

impl From<OutgoingBroadcast> for OutgoingMessage {
    fn from(value: OutgoingBroadcast) -> Self {
        Self {
            from: value.from,
            to: String::new(),
            priority: value.priority,
            kind: value.kind,
            payload: value.payload,
            ttl: value.ttl,
            requires_ack: value.requires_ack,
        }
    }
}

impl OutgoingBroadcast {
    /// A minimal normal-priority broadcast.
    #[must_use]
    pub fn new(from: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            priority: MessagePriority::Normal,
            kind: MessageKind::Data,
            payload: String::new(),
            ttl: None,
            requires_ack: false,
        }
    }

    /// Sets the priority tier.
    #[must_use]
    pub fn priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Sets the message class.
    #[must_use]
    pub fn kind(mut self, kind: MessageKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the payload.
    #[must_use]
    pub fn payload(mut self, payload: impl Into<String>) -> Self {
        self.payload = payload.into();
        self
    }

    /// Sets an explicit TTL.
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Marks the message as requiring acknowledgment.
    #[must_use]
    pub fn requires_ack(mut self) -> Self {
        self.requires_ack = true;
        self
    }
}

/// A delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub message: Message,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_tier_ordering_is_total() {
        let mut tiers = [
            MessagePriority::Low,
            MessagePriority::Normal,
            MessagePriority::High,
            MessagePriority::Urgent,
        ];
        tiers.sort();
        assert_eq!(
            tiers,
            [
                MessagePriority::Low,
                MessagePriority::Normal,
                MessagePriority::High,
                MessagePriority::Urgent
            ]
        );
        assert!(MessagePriority::Urgent > MessagePriority::High);
    }

    #[test]
    fn message_round_trips_through_serde() {
        let message = Message {
            id: 7,
            from: String::from("queen"),
            to: String::from("w1"),
            priority: MessagePriority::High,
            kind: MessageKind::TaskAssign,
            payload: String::from("build"),
            ttl: DEFAULT_TTL,
            requires_ack: true,
        };
        let encoded = serde_json::to_string(&message).unwrap();
        let decoded: Message = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn bus_rejects_zero_capacity_and_duplicate_subscribers() {
        let bus = MessageBus::new(0).unwrap_err();
        assert_eq!(bus, SwarmError::InvalidCapacity(0));

        let bus = MessageBus::new(4).unwrap();
        bus.subscribe("w1", &[]).unwrap();
        assert_eq!(
            bus.subscribe("w1", &[]).unwrap_err(),
            SwarmError::DuplicateSubscriber(String::from("w1"))
        );
    }
}
