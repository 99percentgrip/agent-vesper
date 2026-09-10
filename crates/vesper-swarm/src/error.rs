//! Crate-wide error surface (VRO-15).
//!
//! One error enum shared by the swarm coordination layers. Every variant
//! is a structural refusal — this crate performs no I/O, so there are no
//! I/O errors here.

use crate::bus::MessagePriority;

/// Errors surfaced by the swarm coordination layers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SwarmError {
    /// A bounded coordination resource refused growth.
    #[error("bus resource limit exceeded: {0}")]
    ResourceLimit(&'static str),
    /// The bus was constructed with a capacity below one.
    #[error("bus capacity must be at least one, got {0}")]
    InvalidCapacity(usize),
    /// The bus is at capacity and could not admit the message even after
    /// eviction. The caller learns this loudly — nothing is dropped
    /// silently, and no urgent message is ever silently discarded.
    #[error("bus full at capacity {capacity}; {priority:?} message refused")]
    BusFull {
        /// The configured global capacity.
        capacity: usize,
        /// The priority of the refused message.
        priority: MessagePriority,
    },
    /// The bus (or inbox) is closed; no further traffic is accepted.
    #[error("bus is closed")]
    BusClosed,
    /// A worker id was subscribed twice.
    #[error("subscriber {0} already exists")]
    DuplicateSubscriber(String),
    /// The referenced worker has no inbox.
    #[error("subscriber {0} does not exist")]
    UnknownSubscriber(String),
    /// An acknowledgment referenced a message that is not awaiting one.
    #[error("message {0} is not awaiting acknowledgment")]
    UnknownAck(u64),
}
