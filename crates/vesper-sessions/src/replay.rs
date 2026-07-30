use std::{fmt, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use vesper_domain::{BoundedString, MessageId, MessageRole};

/// Boxed acknowledgement future for one bounded replay update.
pub type ReplayFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ReplayError>> + Send + 'a>>;

/// Replay delivery failed before the lifecycle response may be completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayError {
    /// Safe implementation-owned diagnostic.
    pub message: BoundedString<4096>,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for ReplayError {}

/// One user-visible historical message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayMessage {
    pub message_id: MessageId,
    pub role: MessageRole,
    pub text: BoundedString<1_048_576>,
}

/// Persisted plan status mapped without executing the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplayPlanStatus {
    Pending,
    InProgress,
    Completed,
}

/// Persisted plan priority mapped for ACP display only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplayPlanPriority {
    Low,
    Medium,
    High,
}

/// One bounded plan item for display-only replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlanEntry {
    pub content: BoundedString<4096>,
    pub status: ReplayPlanStatus,
    pub priority: ReplayPlanPriority,
}

/// Safe metadata/config state shown after history and plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMetadata {
    pub title: Option<BoundedString<1024>>,
    pub updated_at: Option<BoundedString<128>>,
    pub operating_mode: vesper_domain::SessionOperatingMode,
    pub configuration_required: bool,
}

/// One slash command that the current runtime can actually execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableCommandDescriptor {
    pub name: BoundedString<128>,
    pub description: BoundedString<1024>,
}

/// Ordered, ACP-neutral replay update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayUpdate {
    VisibleMessage(ReplayMessage),
    Plan(Vec<ReplayPlanEntry>),
    Metadata(ReplayMetadata),
    AvailableCommands(Vec<AvailableCommandDescriptor>),
}

/// A sink must resolve only after the update has been accepted by its writer.
pub trait ReplaySink: Send {
    fn accept<'a>(&'a mut self, update: &'a ReplayUpdate) -> ReplayFuture<'a>;
}

/// Bounded ordered replay plan. The lifecycle completion is deliberately not
/// stored here: callers may send it only after `deliver` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayPlan {
    updates: Vec<ReplayUpdate>,
}

impl ReplayPlan {
    /// Builds the only permitted replay order.
    #[must_use]
    pub fn new(
        messages: Vec<ReplayMessage>,
        plan: Vec<ReplayPlanEntry>,
        metadata: ReplayMetadata,
        available_commands: Vec<AvailableCommandDescriptor>,
    ) -> Self {
        let mut updates = Vec::with_capacity(messages.len() + 3);
        updates.extend(messages.into_iter().map(ReplayUpdate::VisibleMessage));
        if !plan.is_empty() {
            updates.push(ReplayUpdate::Plan(plan));
        }
        updates.push(ReplayUpdate::Metadata(metadata));
        updates.push(ReplayUpdate::AvailableCommands(available_commands));
        Self { updates }
    }

    /// Returns ordered updates without encoding them.
    pub fn updates(&self) -> impl ExactSizeIterator<Item = &ReplayUpdate> {
        self.updates.iter()
    }

    /// Delivers each update sequentially and awaits writer acceptance.
    pub async fn deliver(&self, sink: &mut dyn ReplaySink) -> Result<(), ReplayError> {
        for update in &self.updates {
            sink.accept(update).await?;
        }
        Ok(())
    }
}
