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

/// Hard per-message payload byte ceiling.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Aggregate queued payload and identity byte ceiling.
pub const MAX_QUEUED_BYTES: usize = 64 * 1024 * 1024;
/// Hard subscriber ceiling, independent of message capacity.
pub const MAX_SUBSCRIBERS: usize = 4096;
const MAX_ID_BYTES: usize = 256;
const MAX_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn validate_message(
    from: &str,
    to: &str,
    payload: &str,
    ttl: Option<Duration>,
) -> Result<(), SwarmError> {
    if from.len() > MAX_ID_BYTES || to.len() > MAX_ID_BYTES {
        return Err(SwarmError::ResourceLimit("identity bytes"));
    }
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(SwarmError::ResourceLimit("payload bytes"));
    }
    if ttl.is_some_and(|ttl| ttl > MAX_TTL) {
        return Err(SwarmError::ResourceLimit("TTL"));
    }
    Ok(())
}

fn message_bytes(message: &Message) -> usize {
    message.from.len() + message.to.len() + message.payload.len()
}

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
    /// A governance gate: a task suspended for host resolution (VRO-16).
    /// Rides the Urgent tier so it is never evicted and always dequeued
    /// before ordinary work; the suspended task itself is *not* dispatched.
    Governance,
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
    pending_acks: HashMap<u64, std::time::Instant>,
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
    queued_bytes: usize,
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
}

/// Monotonic caller-time seam for queue and acknowledgement TTL accounting.
/// Implementations must be cheap, nonblocking and never re-enter the bus.
pub trait BusClock: Send + Sync {
    /// Current monotonic time. Implementations must not move backwards.
    fn now(&self) -> std::time::Instant;
}
struct MonotonicClock;
impl BusClock for MonotonicClock {
    fn now(&self) -> std::time::Instant {
        std::time::Instant::now()
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
    clock: Arc<dyn BusClock>,
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
            clock: self.clock.clone(),
            shared: Arc::clone(&self.shared),
            closed_flag: Arc::clone(&self.closed_flag),
        }
    }
}

impl MessageBus {
    /// Creates a bus with the given global queued-message capacity.
    pub fn new(capacity: usize) -> Result<Self, SwarmError> {
        Self::with_clock(capacity, Arc::new(MonotonicClock))
    }

    /// Creates a bus driven by a caller-owned monotonic clock.
    pub fn with_clock(capacity: usize, clock: Arc<dyn BusClock>) -> Result<Self, SwarmError> {
        if capacity > 1_000_000 {
            return Err(SwarmError::ResourceLimit("message capacity"));
        }
        if capacity == 0 {
            return Err(SwarmError::InvalidCapacity(0));
        }
        Ok(Self {
            capacity,
            clock,
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
        if worker_id.len() > MAX_ID_BYTES || worker_id.is_empty() {
            return Err(SwarmError::ResourceLimit("subscriber identity"));
        }
        if state.inboxes.len() >= MAX_SUBSCRIBERS {
            return Err(SwarmError::ResourceLimit("subscribers"));
        }
        if filters.len() > 32 {
            return Err(SwarmError::ResourceLimit("subscriber filters"));
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
        state.queued_bytes -= inbox
            .tiers
            .iter()
            .flatten()
            .map(|item| message_bytes(&item.message))
            .sum::<usize>();
        self.shared
            .live_acks
            .fetch_sub(inbox.pending_acks.len() as u64, Ordering::AcqRel);
        inbox.notify.notify_waiters();
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
        let mut state = self.shared.lock();
        if state.closed {
            return Err(SwarmError::BusClosed);
        }
        validate_message(&message.from, "", &message.payload, message.ttl)?;
        let mut recipients: Vec<_> = state.inboxes.keys().cloned().collect();
        recipients.sort();
        let required = state
            .inboxes
            .values()
            .filter(|inbox| inbox.accepts(message.kind))
            .count();
        let tier = Inbox::tier_index(message.priority);
        let evictable: usize = state
            .inboxes
            .values()
            .map(|inbox| inbox.tiers[..tier].iter().map(VecDeque::len).sum::<usize>())
            .sum();
        let required_bytes: usize = state
            .inboxes
            .iter()
            .filter(|(_, inbox)| inbox.accepts(message.kind))
            .map(|(to, _)| message.from.len() + to.len() + message.payload.len())
            .sum();
        let evictable_bytes: usize = state
            .inboxes
            .values()
            .flat_map(|inbox| inbox.tiers[..tier].iter().flatten())
            .map(|envelope| message_bytes(&envelope.message))
            .sum();
        if required_bytes > MAX_QUEUED_BYTES - state.queued_bytes + evictable_bytes {
            return Err(SwarmError::ResourceLimit("queued bytes"));
        }
        if state.next_message_id.checked_add(required as u64).is_none()
            || state.next_seq.checked_add(required as u64).is_none()
        {
            return Err(SwarmError::ResourceLimit("message identities"));
        }
        if required > self.capacity.saturating_sub(state.queued) + evictable {
            return Err(SwarmError::BusFull {
                capacity: self.capacity,
                priority: message.priority,
            });
        }
        let mut ids = Vec::with_capacity(recipients.len());
        for recipient in recipients {
            let mut outgoing = OutgoingMessage::from(message.clone());
            outgoing.to = recipient.clone();
            ids.push(self.admit_locked(&mut state, outgoing, Some(recipient))?);
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
                if state.closed {
                    return Err(SwarmError::BusClosed);
                }
                self.expire_acks(&mut state, self.clock.now());
                let Some(inbox) = state.inboxes.get_mut(worker_id) else {
                    return Err(SwarmError::UnknownSubscriber(worker_id.to_string()));
                };
                if inbox
                    .tiers
                    .iter()
                    .rev()
                    .find_map(|tier| tier.front())
                    .is_some_and(|envelope| {
                        envelope.message.requires_ack
                            && envelope
                                .expires_at
                                .is_some_and(|expires| expires > self.clock.now())
                    })
                    && self.shared.live_acks.load(Ordering::Acquire) >= self.capacity as u64
                {
                    return Err(SwarmError::ResourceLimit("pending acknowledgments"));
                }
                let Some(envelope) = inbox.pop_highest() else {
                    return Ok(None);
                };
                state.queued = state.queued.saturating_sub(1);
                state.queued_bytes -= message_bytes(&envelope.message);
                if envelope
                    .expires_at
                    .is_some_and(|expires| self.clock.now() >= expires)
                {
                    self.shared.record(BusEvent::Expired(envelope.message.id));
                    continue;
                }
                if envelope.message.requires_ack {
                    if let Some(inbox) = state.inboxes.get_mut(worker_id) {
                        inbox.pending_acks.insert(
                            envelope.message.id,
                            envelope.expires_at.expect("admission validates TTL"),
                        );
                    }
                    self.shared.live_acks.fetch_add(1, Ordering::AcqRel);
                }
                envelope
            };
            return Ok(Some(Received {
                message: popped.message,
            }));
        }
    }

    /// Async receive: parks the calling task (never a thread) until a live
    /// message exists for the subscriber.
    pub async fn recv(&self, worker_id: &str) -> Result<Received, SwarmError> {
        loop {
            // Register before checking state so send/close/unsubscribe cannot
            // slip between an empty check and notification registration.
            let notify = self.waiter_notify(worker_id)?;
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(received) = self.try_recv(worker_id)? {
                return Ok(received);
            }
            notified.await;
        }
    }

    /// Clones the subscriber's wakeup primitive out of the lock so a
    /// waiter can await it without holding the state lock.
    fn waiter_notify(
        &self,
        worker_id: &str,
    ) -> Result<std::sync::Arc<tokio::sync::Notify>, SwarmError> {
        let state = self.shared.lock();
        if state.closed {
            return Err(SwarmError::BusClosed);
        }
        let Some(inbox) = state.inboxes.get(worker_id) else {
            return Err(SwarmError::UnknownSubscriber(worker_id.to_string()));
        };
        Ok(std::sync::Arc::clone(&inbox.notify))
    }

    /// Acknowledges a previously delivered message that required one.
    pub fn ack(&self, worker_id: &str, message_id: u64) -> Result<(), SwarmError> {
        let mut state = self.shared.lock();
        self.expire_acks(&mut state, self.clock.now());
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
        self.expire_acks(&mut self.shared.lock(), self.clock.now());
        self.shared.live_acks.load(Ordering::Acquire)
    }

    fn expire_acks(&self, state: &mut BusState, now: std::time::Instant) {
        let mut expired = 0;
        for inbox in state.inboxes.values_mut() {
            let before = inbox.pending_acks.len();
            inbox.pending_acks.retain(|_, expires| *expires > now);
            expired += before - inbox.pending_acks.len();
        }
        self.shared
            .live_acks
            .fetch_sub(expired as u64, Ordering::AcqRel);
    }

    /// Closes the bus: no further admissions; parked receivers wake and
    /// observe [`SwarmError::BusClosed`] on their next operation.
    pub fn close(&self) {
        let mut state = self.shared.lock();
        state.closed = true;
        self.closed_flag.store(true, Ordering::Release);
        state.queued = 0;
        state.queued_bytes = 0;
        for inbox in state.inboxes.values_mut() {
            inbox.tiers.iter_mut().for_each(VecDeque::clear);
            inbox.pending_acks.clear();
            inbox.notify.notify_waiters();
        }
        self.shared.live_acks.store(0, Ordering::Release);
    }

    /// Admits one message into one inbox under the capacity + eviction
    /// policy. `explicit_target` is `Some` for directed sends and `None`
    /// reserved for broadcast internals (already resolved per recipient).
    fn admit(
        &self,
        message: OutgoingMessage,
        explicit_target: Option<String>,
    ) -> Result<u64, SwarmError> {
        self.admit_locked(&mut self.shared.lock(), message, explicit_target)
    }

    fn admit_locked(
        &self,
        state: &mut BusState,
        message: OutgoingMessage,
        explicit_target: Option<String>,
    ) -> Result<u64, SwarmError> {
        let priority = message.priority;
        if state.closed {
            return Err(SwarmError::BusClosed);
        }
        let target = explicit_target.unwrap_or_else(|| message.to.clone());
        validate_message(&message.from, &target, &message.payload, message.ttl)?;
        let expires_at = self
            .clock
            .now()
            .checked_add(message.ttl.unwrap_or(DEFAULT_TTL))
            .ok_or(SwarmError::ResourceLimit("TTL overflow"))?;
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
        state.next_message_id = state
            .next_message_id
            .checked_add(1)
            .ok_or(SwarmError::ResourceLimit("message identities"))?;
        let id = state.next_message_id;
        state.next_seq = state
            .next_seq
            .checked_add(1)
            .ok_or(SwarmError::ResourceLimit("message sequence"))?;
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
            expires_at: Some(expires_at),
        };
        // Capacity: evict deterministically when full.
        let bytes = message_bytes(&envelope.message);
        while state.queued >= self.capacity || state.queued_bytes + bytes > MAX_QUEUED_BYTES {
            self.evict_one(state, priority)?;
        }
        state.queued_bytes += bytes;
        if let Some(inbox) = state.inboxes.get_mut(&target) {
            inbox.push(envelope);
            inbox.notify.notify_one();
        }
        state.queued += 1;
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
            state.queued_bytes -= message_bytes(&removed.message);
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
