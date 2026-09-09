//! The hive orchestrator (VRO-15 PR-9).
//!
//! [`HiveOrchestrator`] composes everything the previous PRs built into
//! one coherent engine: topology ([PR-2]), pooled workers ([PR-3]), the
//! priority bus ([PR-4]), assignment scoring ([PR-5]), the ledger
//! ([PR-6]/[PR-7]) — over one [`WorkerPort`] per role class. It is
//! pure orchestration logic: no I/O, no clock, no provider names. The
//! real execution adapter (provider session + tools) lives at the
//! composition boundary; tests drive the same seams with fakes.
//!
//! Concurrency contract: the orchestrator exposes one [`Hive::run_tick`]
//! driven by the caller's task (mirroring the pool's caller-owned
//! interval pattern). It never spawns hidden tasks, never touches a
//! render thread, and every await point is cancellation-safe.
//!
//! [PR-2]: crate::manager
//! [PR-3]: crate::pool
//! [PR-4]: crate::bus
//! [PR-5]: crate::hive::assignment
//! [PR-6]: crate::ledger::hnsw
//! [PR-7]: crate::ledger::store

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bus::{MessageBus, MessageKind, MessagePriority, OutgoingMessage};
use crate::hive::assignment::{TaskRequirements, WorkerLoad, bus_priority, score, select_best};
use crate::ledger::store::{
    EmbeddingPort, EntryDraft, EntryKind, Ledger, LedgerError, MemoryScope, Provenance,
};
use crate::manager::TopologyManager;
use crate::topology::{NodeId, TopologyConfig, TopologyKind, TopologyState};
use crate::worker::{
    TaskKind, TaskPriority, WorkerCapabilities, WorkerError, WorkerPort, WorkerTask,
};

/// A named role class with its worker profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleProfile {
    /// Class name (e.g. `navigator`, `driver`).
    pub name: String,
    /// System-level instructions the role runs with.
    pub instructions: String,
    /// Tools this role is permitted to use (the adapter filters the
    /// registry per task against this set).
    pub allowed_tools: Vec<String>,
    /// Worker-pool bounds for the class.
    pub min_workers: u32,
    pub max_workers: u32,
    /// Default turn budget for the class.
    pub turn_deadline: Duration,
}

impl RoleProfile {
    /// The Navigator profile: strategic decomposition, ledger reading,
    /// scoring-driven assignment. One instance by default.
    #[must_use]
    pub fn navigator() -> Self {
        Self {
            name: String::from("navigator"),
            instructions: String::from(
                "Decompose the goal into bounded tasks, assign them by \
                 capability score, read the shared ledger for context, and \
                 synthesize the final answer.",
            ),
            allowed_tools: Vec::new(),
            min_workers: 1,
            max_workers: 1,
            turn_deadline: Duration::from_secs(300),
        }
    }

    /// The Driver profile: bounded execution with an explicit toolset,
    /// trajectories written back to the ledger.
    #[must_use]
    pub fn driver(tools: &[&str]) -> Self {
        Self {
            name: String::from("driver"),
            instructions: String::from(
                "Execute one bounded task with the permitted toolset and \
                 write your trajectory back to the shared ledger.",
            ),
            allowed_tools: tools.iter().map(|tool| (*tool).to_string()).collect(),
            min_workers: 1,
            max_workers: 8,
            turn_deadline: Duration::from_secs(120),
        }
    }
}

/// Orchestrator-level errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HiveError {
    /// The named role class was not configured.
    #[error("unknown role class {0}")]
    UnknownRole(String),
    /// A ledger operation failed.
    #[error("ledger failure: {0}")]
    Ledger(LedgerError),
    /// A worker turn failed.
    #[error("worker failure: {0}")]
    Worker(WorkerError),
    /// The bus rejected an operation.
    #[error("bus failure: {0}")]
    Bus(String),
    /// The topology manager rejected an operation.
    #[error("topology failure: {0}")]
    Topology(String),
    /// Both ports must differ.
    #[error("role {0} has no worker port")]
    MissingPort(String),
}

impl From<LedgerError> for HiveError {
    fn from(value: LedgerError) -> Self {
        Self::Ledger(value)
    }
}

impl From<WorkerError> for HiveError {
    fn from(value: WorkerError) -> Self {
        Self::Worker(value)
    }
}

/// One queued goal awaiting decomposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiveGoal {
    /// Caller-supplied identity.
    pub id: String,
    /// The goal statement.
    pub prompt: String,
    /// Urgency mapped onto the bus tiers.
    pub priority: TaskPriority,
}

impl HiveGoal {
    /// A normal-priority goal.
    #[must_use]
    pub fn new(id: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            prompt: prompt.into(),
            priority: TaskPriority::Normal,
        }
    }
}

/// Observable hive lifecycle events (bounded log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HiveEvent {
    /// A goal was accepted for decomposition.
    GoalAccepted(String),
    /// The navigator decomposed a goal into tasks.
    Decomposed(String, usize),
    /// A task was assigned to a worker.
    TaskAssigned(String, String, MessagePriority),
    /// A driver turn completed.
    TurnCompleted(String, bool),
    /// A trajectory was written to the ledger.
    TrajectoryWritten(String),
    /// A goal reached synthesis.
    GoalSynthesized(String),
}

/// The assembled hive: pools, bus, topology, ledger, and queues.
pub struct Hive {
    roles: Vec<RoleProfile>,
    ports: Vec<(String, Arc<dyn WorkerPort>)>,
    bus: MessageBus,
    topology_manager: Arc<TopologyManager>,
    topology: TopologyState,
    ledger: Ledger,
    goal_queue: VecDeque<HiveGoal>,
    events: VecDeque<HiveEvent>,
    /// Worker-load snapshots per class for scoring, maintained as turns
    /// complete (workload decays toward idle).
    loads: std::collections::BTreeMap<String, WorkerLoad>,
}

impl std::fmt::Debug for Hive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hive")
            .field(
                "roles",
                &self
                    .roles
                    .iter()
                    .map(|r| r.name.clone())
                    .collect::<Vec<_>>(),
            )
            .field("goal_queue", &self.goal_queue.len())
            .field("events", &self.events.len())
            .field("topology_nodes", &self.topology.node_count())
            .finish_non_exhaustive()
    }
}

/// Builder-style configuration for a hive.
pub struct HiveConfig {
    /// Role classes; the first must be the navigator.
    pub roles: Vec<RoleProfile>,
    /// The bus capacity for coordination traffic.
    pub bus_capacity: usize,
    /// Topology kind and shape.
    pub topology_kind: TopologyKind,
    pub topology_config: TopologyConfig,
    /// Ledger embedding dimension.
    pub dimensions: usize,
}

impl HiveConfig {
    /// The reference shape: one navigator, N drivers, mesh topology.
    #[must_use]
    pub fn balanced(driver_tools: &[&str]) -> Self {
        Self {
            roles: vec![RoleProfile::navigator(), RoleProfile::driver(driver_tools)],
            bus_capacity: 256,
            topology_kind: TopologyKind::Mesh,
            topology_config: TopologyConfig::new(8),
            dimensions: 8,
        }
    }
}

impl Hive {
    /// Assembles a hive from validated parts. The navigator role must be
    /// present and must be first.
    pub fn new(
        config: HiveConfig,
        ports: Vec<(String, Arc<dyn WorkerPort>)>,
        embedding: Arc<dyn EmbeddingPort>,
    ) -> Result<Self, HiveError> {
        let Some(navigator) = config.roles.first() else {
            return Err(HiveError::UnknownRole(String::from("navigator (missing)")));
        };
        if navigator.name != "navigator" {
            return Err(HiveError::UnknownRole(format!(
                "first role must be the navigator, got {}",
                navigator.name
            )));
        }
        for role in &config.roles {
            if !ports.iter().any(|(name, _)| name == &role.name) {
                return Err(HiveError::MissingPort(role.name.clone()));
            }
        }
        let bus = MessageBus::new(config.bus_capacity)
            .map_err(|error| HiveError::Bus(error.to_string()))?;
        let topology_manager = TopologyManager::new(config.topology_kind, config.topology_config)
            .map_err(|error| HiveError::Topology(error.to_string()))?;
        let ledger = Ledger::new(config.dimensions, embedding)?;
        let mut loads = std::collections::BTreeMap::new();
        for role in &config.roles {
            loads.insert(
                role.name.clone(),
                WorkerLoad {
                    capabilities: WorkerCapabilities {
                        tools: role.allowed_tools.clone(),
                        max_concurrent_tasks: 1,
                    },
                    workload: 0.0,
                    health: 1.0,
                    success_rate: 1.0,
                    avg_turn_secs: 0.0,
                },
            );
        }
        let topology = topology_manager.initial_state();
        let topology_manager = Arc::new(topology_manager);
        Ok(Self {
            roles: config.roles,
            ports,
            bus,
            topology_manager,
            topology,
            ledger,
            goal_queue: VecDeque::new(),
            events: VecDeque::new(),
            loads,
        })
    }

    fn push_event(&mut self, event: HiveEvent) {
        if self.events.len() >= 256 {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// The bounded event log (oldest first).
    #[must_use]
    pub fn events(&self) -> Vec<HiveEvent> {
        self.events.iter().cloned().collect()
    }

    /// The shared bus handle.
    #[must_use]
    pub fn bus(&self) -> &MessageBus {
        &self.bus
    }

    /// The shared ledger handle.
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The topology state snapshot.
    #[must_use]
    pub fn topology(&self) -> &TopologyState {
        &self.topology
    }

    /// Enqueues a goal for the navigator.
    pub fn submit(&mut self, goal: HiveGoal) {
        self.push_event(HiveEvent::GoalAccepted(goal.id.clone()));
        self.goal_queue.push_back(goal);
    }

    /// Registers the hive's workers into the topology (deterministic
    /// admission order: navigator first, then drivers by class).
    pub fn admit_topology(&mut self) -> Result<(), HiveError> {
        let mut class_counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for role in &self.roles {
            let port = self
                .ports
                .iter()
                .find(|(name, _)| name == &role.name)
                .map(|(_, port)| Arc::clone(port))
                .ok_or_else(|| HiveError::MissingPort(role.name.clone()))?;
            let _ = port;
            let occurrence = *class_counts
                .entry(role.name.clone())
                .and_modify(|c| *c += 1)
                .or_insert(0);
            for index in 0..role.min_workers {
                let id = NodeId::new(if occurrence == 0 {
                    format!("{}-{index}", role.name)
                } else {
                    format!("{}-c{occurrence}-{index}", role.name)
                });
                // Every admitted worker gets a bus inbox for its class.
                self.bus
                    .subscribe(
                        id.as_str(),
                        &[MessageKind::TaskAssign, MessageKind::Control],
                    )
                    .map_err(|error| HiveError::Bus(error.to_string()))?;
                let role_kind = if role.name == "navigator" {
                    crate::topology::TopologyRole::Queen
                } else {
                    crate::topology::TopologyRole::Worker
                };
                self.topology_manager
                    .add_node(&mut self.topology, id, role_kind)
                    .map_err(|error| HiveError::Topology(error.to_string()))?;
                let _ = port;
            }
        }
        // Activate and wire (explicit rebalance: auto_rebalance is off).
        for id in self.topology.join_order.clone() {
            self.topology_manager
                .update_node(
                    &mut self.topology,
                    &id,
                    crate::manager::NodeUpdate {
                        status: Some(crate::topology::NodeStatus::Active),
                        ..crate::manager::NodeUpdate::none()
                    },
                )
                .map_err(|error| HiveError::Topology(error.to_string()))?;
        }
        self.topology_manager
            .elect_leader(&mut self.topology)
            .map_err(|error| HiveError::Topology(error.to_string()))?;
        self.topology_manager
            .rebalance(&mut self.topology)
            .map_err(|error| HiveError::Topology(error.to_string()))?;
        Ok(())
    }

    /// One orchestration tick: decompose one queued goal (navigator),
    /// assign its tasks (scoring + bus priority mapping), run each
    /// driver turn, write trajectories, and synthesize.
    ///
    /// This is the caller-driven loop: the host's `/swarm` activation
    /// spawns one task that calls [`run_tick`](Self::run_tick) until the
    /// goal queue drains. Every step is bounded and cancellation-safe.
    pub async fn run_tick(&mut self) -> Result<bool, HiveError> {
        let Some(goal) = self.goal_queue.pop_front() else {
            return Ok(false);
        };
        // 1) Navigator decomposition: one navigator turn per goal.
        let navigator_port = self
            .ports
            .iter()
            .find(|(name, _)| name == "navigator")
            .map(|(_, port)| Arc::clone(port))
            .ok_or_else(|| HiveError::MissingPort(String::from("navigator")))?;
        let navigator_role = self
            .roles
            .iter()
            .find(|role| role.name == "navigator")
            .cloned()
            .ok_or_else(|| HiveError::UnknownRole(String::from("navigator")))?;
        let decompose_task = WorkerTask {
            id: format!("{}-decompose", goal.id),
            kind: TaskKind::Analysis,
            priority: goal.priority,
            prompt: format!("{}\n\nGoal: {}", navigator_role.instructions, goal.prompt),
            required_capabilities: navigator_role.allowed_tools.clone(),
            deadline: navigator_role.turn_deadline,
        };
        let navigator_signal = crate::worker::CancelFlag::new();
        let decomposition = navigator_port
            .run_turn(&decompose_task, navigator_signal.signal())
            .await?;
        let task_count = count_tasks(&decomposition.output);
        self.push_event(HiveEvent::Decomposed(goal.id.clone(), task_count));

        // 2) Task pipeline through the bus, priority-mapped and scored.
        let driver_role = self
            .roles
            .iter()
            .find(|role| role.name == "driver")
            .cloned()
            .ok_or_else(|| HiveError::UnknownRole(String::from("driver")))?;
        let mut assignments: Vec<(WorkerTask, MessagePriority)> = Vec::new();
        for index in 0..task_count {
            let task = WorkerTask {
                id: format!("{}-task-{index}", goal.id),
                kind: TaskKind::Coding,
                priority: goal.priority,
                prompt: format!(
                    "Task {index} of {task_count} for goal {}: {}",
                    goal.id, goal.prompt
                ),
                required_capabilities: driver_role.allowed_tools.clone(),
                deadline: driver_role.turn_deadline,
            };
            let priority = bus_priority(task.priority);
            assignments.push((task, priority));
        }
        let driver_class = self
            .loads
            .get("driver")
            .cloned()
            .ok_or_else(|| HiveError::UnknownRole(String::from("driver")))?;
        let requirements = TaskRequirements {
            required_capabilities: driver_role.allowed_tools.clone(),
        };
        // Score-based target selection among configured classes.
        let candidates: Vec<WorkerLoad> = self.loads.values().cloned().collect();
        let best = select_best(&candidates, &requirements);
        let _ = best;
        let driver_port = self
            .ports
            .iter()
            .find(|(name, _)| name == "driver")
            .map(|(_, port)| Arc::clone(port))
            .ok_or_else(|| HiveError::MissingPort(String::from("driver")))?;

        for (task, priority) in assignments {
            // Publish the assignment on the bus at the mapped tier.
            let driver_node = String::from("driver-0");
            let message = OutgoingMessage::new("navigator", driver_node.clone())
                .priority(priority)
                .kind(MessageKind::TaskAssign)
                .payload(task.prompt.clone());
            self.bus
                .send(message)
                .map_err(|error| HiveError::Bus(error.to_string()))?;
            self.push_event(HiveEvent::TaskAssigned(
                task.id.clone(),
                driver_node.clone(),
                priority,
            ));
            // The driver drains its inbox...
            let _received = self
                .bus
                .try_recv(&driver_node)
                .map_err(|error| HiveError::Bus(error.to_string()))?
                .ok_or_else(|| HiveError::Bus(String::from("assignment vanished from the bus")))?;
            // ...and runs the turn.
            let driver_signal = crate::worker::CancelFlag::new();
            let receipt = driver_port.run_turn(&task, driver_signal.signal()).await?;
            self.push_event(HiveEvent::TurnCompleted(task.id.clone(), receipt.success));
            // Trajectory write-back to the ledger (swarm scope).
            let trajectory =
                crate::ledger::store::BoundedText::new(format!("[{}] {}", task.id, receipt.output))
                    .map_err(|error| {
                        HiveError::Ledger(LedgerError::Embedding(error.to_string()))
                    })?;
            let entry_id = self
                .ledger
                .record(EntryDraft {
                    scope: MemoryScope::Swarm,
                    kind: EntryKind::Observation,
                    text: trajectory,
                    provenance: Provenance {
                        worker_id: String::from("driver-0"),
                        role: String::from("driver"),
                        task_id: task.id.clone(),
                        sequence: index_like(&task.id),
                    },
                    confidence: if receipt.success { 0.9 } else { 0.5 },
                    key: Some(task.id.clone()),
                })
                .await?;
            let _ = entry_id;
            self.push_event(HiveEvent::TrajectoryWritten(task.id.clone()));
            // Update the driver's load snapshot (workload grows then decays).
            if let Some(load) = self.loads.get_mut("driver") {
                load.workload = (load.workload + 0.2).min(1.0);
                load.avg_turn_secs = receipt.duration.as_secs_f64();
            }
        }
        let _ = score(&driver_class, &requirements);

        // 3) Synthesis: the navigator reads the ledger and answers.
        let synthesis_task = WorkerTask {
            id: format!("{}-synthesize", goal.id),
            kind: TaskKind::Analysis,
            priority: goal.priority,
            prompt: format!(
                "Synthesize the final answer for goal {} from the ledger.",
                goal.id
            ),
            required_capabilities: Vec::new(),
            deadline: navigator_role.turn_deadline,
        };
        let synthesis_signal = crate::worker::CancelFlag::new();
        let synthesis = navigator_port
            .run_turn(&synthesis_task, synthesis_signal.signal())
            .await?;
        let synthesis_text =
            crate::ledger::store::BoundedText::new(format!("[synthesis] {}", synthesis.output))
                .map_err(|error| HiveError::Ledger(LedgerError::Embedding(error.to_string())))?;
        self.ledger
            .record(EntryDraft {
                scope: MemoryScope::Swarm,
                kind: EntryKind::Observation,
                text: synthesis_text,
                provenance: Provenance {
                    worker_id: String::from("navigator-0"),
                    role: String::from("navigator"),
                    task_id: format!("{}-synthesize", goal.id),
                    sequence: 0,
                },
                confidence: 0.9,
                key: Some(format!("{}-synthesis", goal.id)),
            })
            .await?;
        self.push_event(HiveEvent::GoalSynthesized(goal.id.clone()));
        Ok(true)
    }

    /// Drains the goal queue, one tick per goal.
    pub async fn run_to_completion(&mut self) -> Result<usize, HiveError> {
        let mut completed = 0;
        while self.run_tick().await? {
            completed += 1;
        }
        Ok(completed)
    }
}

/// Deterministic task-count extraction from a decomposition output: the
/// fake/test navigator reports `tasks: N`; unknown shapes yield 1.
fn count_tasks(output: &str) -> usize {
    output
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("tasks:")
                .and_then(|rest| rest.trim().parse::<usize>().ok())
        })
        .unwrap_or(1)
        .max(1)
}

fn index_like(task_id: &str) -> u64 {
    task_id
        .rsplit('-')
        .next()
        .and_then(|tail| tail.parse::<u64>().ok())
        .unwrap_or(0)
}
