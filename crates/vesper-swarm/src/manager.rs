//! Deterministic topology mutations (VRO-15 PR-2).
//!
//! [`TopologyManager`] owns every way a [`TopologyState`] can change.
//! All rules are deterministic by construction: admission order is an
//! explicit ledger, elections pick the oldest eligible member by join
//! order, partition placement follows the configured strategy, and edge
//! construction walks nodes in admission order. No clock, no randomness,
//! no map-iteration-order dependence — the same operation sequence on the
//! same config always produces the same topology.
//!
//! Semantics diverge from the swarm oracle in exactly two places, both
//! fail-closed: leadership relocation requires `failover_enabled`, and
//! automatic rewiring requires `auto_rebalance` (or an explicit
//! [`TopologyManager::rebalance`] call). Without them, `add_node` is pure
//! admission and `remove_node` leaves leadership vacant rather than
//! promoting implicitly.

use std::collections::BTreeMap;

use crate::topology::{
    NodeId, NodeStatus, PartitionStrategy, TopologyConfig, TopologyConfigError, TopologyEdge,
    TopologyKind, TopologyNode, TopologyPartition, TopologyRole, TopologyState,
};

/// Failures reported by topology mutations. Every variant is a structural
/// rejection, never an I/O error: this manager owns pure state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TopologyError {
    /// A node with this identity already exists.
    #[error("node {0} already exists")]
    DuplicateNode(NodeId),
    /// The referenced node does not exist.
    #[error("node {0} does not exist")]
    UnknownNode(NodeId),
    /// Admission was refused because [`TopologyConfig::max_agents`] is
    /// already reached.
    #[error("max_agents ({0}) already reached")]
    CapacityReached(u32),
    /// No admitted node is currently eligible to lead.
    #[error("no eligible leader among {0} admitted node(s)")]
    NoEligibleLeader(usize),
    /// The config passed to the manager is not valid.
    #[error("invalid topology config: {0}")]
    InvalidConfig(TopologyConfigError),
}

/// One node's mutable fields, patched by [`TopologyManager::update_node`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeUpdate {
    /// Replacement coordination role.
    pub role: Option<TopologyRole>,
    /// Replacement lifecycle status.
    pub status: Option<NodeStatus>,
    /// Metadata patch applied last.
    pub metadata: Option<MetadataPatch>,
}

impl NodeUpdate {
    /// An update that changes nothing.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// Targeted patch for one node's metadata map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataPatch {
    /// Sets or replaces one key.
    Insert(String, String),
    /// Deletes one key if present.
    Remove(String),
    /// Replaces the whole map.
    Replace(BTreeMap<String, String>),
}

/// Deterministic topology mutator.
///
/// Constructed from a validated [`TopologyConfig`] and the
/// [`TopologyKind`] the state instantiates. The manager is a pure state
/// machine: methods mutate a borrowed [`TopologyState`] without performing
/// I/O, so every operation sequence is trivially replayable and
/// snapshot-testable.
#[derive(Debug)]
pub struct TopologyManager {
    config: TopologyConfig,
    kind: TopologyKind,
}

impl TopologyManager {
    /// Creates a manager enforcing `config`, which is validated up front.
    pub fn new(kind: TopologyKind, config: TopologyConfig) -> Result<Self, TopologyError> {
        config.validate().map_err(TopologyError::InvalidConfig)?;
        Ok(Self { config, kind })
    }

    /// The enforced configuration.
    #[must_use]
    pub fn config(&self) -> &TopologyConfig {
        &self.config
    }

    /// The kind this manager instantiates.
    #[must_use]
    pub fn kind(&self) -> TopologyKind {
        self.kind
    }

    /// Creates the empty managed state.
    #[must_use]
    pub fn initial_state(&self) -> TopologyState {
        TopologyState::new(self.kind)
    }

    /// Admits a node without wiring it into the topology.
    ///
    /// Admission appends to `join_order` and inserts an `Initializing`
    /// node record. With `auto_rebalance` enabled a full rebalance runs
    /// immediately, so edges and partitions track membership; with it
    /// disabled (the default) wiring is deferred to an explicit
    /// [`rebalance`](Self::rebalance) call — the node exists but holds no
    /// edges and no partition membership yet.
    pub fn add_node(
        &self,
        state: &mut TopologyState,
        id: NodeId,
        role: TopologyRole,
    ) -> Result<(), TopologyError> {
        if state.nodes.contains_key(&id) {
            return Err(TopologyError::DuplicateNode(id));
        }
        if state.nodes.len() >= self.config.max_agents as usize {
            return Err(TopologyError::CapacityReached(self.config.max_agents));
        }
        state
            .nodes
            .insert(id.clone(), TopologyNode::new(id.clone(), role));
        state.join_order.push(id);
        if self.config.auto_rebalance {
            self.rebalance(state)?;
        }
        Ok(())
    }

    /// Applies a partial update to one admitted node.
    ///
    /// Identity is immutable; role, status, and metadata are patchable.
    /// With `auto_rebalance` enabled, a status change re-reconciles
    /// leadership (eligibility depends on status) but never rewires edges.
    pub fn update_node(
        &self,
        state: &mut TopologyState,
        id: &NodeId,
        update: NodeUpdate,
    ) -> Result<(), TopologyError> {
        let Some(node) = state.nodes.get_mut(id) else {
            return Err(TopologyError::UnknownNode(id.clone()));
        };
        if let Some(role) = update.role {
            node.role = role;
        }
        if let Some(status) = update.status {
            node.status = status;
        }
        match update.metadata {
            Some(MetadataPatch::Insert(key, value)) => {
                node.metadata.insert(key, value);
            }
            Some(MetadataPatch::Remove(key)) => {
                node.metadata.remove(&key);
            }
            Some(MetadataPatch::Replace(map)) => {
                node.metadata = map;
            }
            None => {}
        }
        if self.config.auto_rebalance {
            self.reconcile_leadership(state)?;
        }
        Ok(())
    }

    /// Removes a node and every structural trace of it: its record, its
    /// `join_order` entry, every edge touching it (either endpoint), its
    /// entries in other nodes' connection lists, and its partition
    /// membership.
    ///
    /// If the removed node was the topology leader, or led a partition:
    ///
    /// - with `failover_enabled`, the **oldest remaining member by join
    ///   order** takes over each affected leadership (topology-wide and
    ///   per-partition);
    /// - without it (the default), topology leadership becomes vacant and
    ///   any partition it led is **dissolved** rather than left holding a
    ///   dangling leader reference. No implicit promotion ever happens.
    pub fn remove_node(&self, state: &mut TopologyState, id: &NodeId) -> Result<(), TopologyError> {
        if state.nodes.remove(id).is_none() {
            return Err(TopologyError::UnknownNode(id.clone()));
        }
        let Some(removal_index) = state.join_order.iter().position(|member| member == id) else {
            // Consistency guard: the node map and the join ledger disagree.
            // Refuse the mutation rather than corrupting the ledger.
            state.nodes.insert(
                id.clone(),
                TopologyNode::new(id.clone(), TopologyRole::Worker),
            );
            return Err(TopologyError::UnknownNode(id.clone()));
        };
        state.join_order.remove(removal_index);
        state
            .edges
            .retain(|edge| edge.from != *id && edge.to != *id);
        for node in state.nodes.values_mut() {
            node.connections.retain(|connection| connection != id);
        }
        let was_topology_leader = state.leader.as_ref() == Some(id);
        if was_topology_leader {
            state.leader = None;
        }
        state.partitions.retain_mut(|partition| {
            let was_leader = partition.leader == *id;
            partition.nodes.retain(|member| member != id);
            if partition.nodes.is_empty() {
                return false;
            }
            if was_leader && self.config.failover_enabled {
                partition.leader = partition.nodes[0].clone();
            } else if was_leader {
                return false;
            }
            true
        });
        if was_topology_leader && self.config.failover_enabled && !state.join_order.is_empty() {
            state.leader = Some(state.join_order[0].clone());
        }
        if self.config.auto_rebalance {
            self.rebalance(state)?;
        }
        Ok(())
    }

    /// Elects the topology-wide leader deterministically and records it.
    ///
    /// Policy: the oldest eligible member by admission order that holds a
    /// `Queen` role; if no queen is eligible, the oldest eligible member of
    /// any role. Eligible statuses are `Active`, `Syncing`, and `Inactive`;
    /// `Failed` and `Initializing` nodes never lead.
    pub fn elect_leader(&self, state: &mut TopologyState) -> Result<NodeId, TopologyError> {
        let Some(leader) = self.leader_candidate(state) else {
            return Err(TopologyError::NoEligibleLeader(state.nodes.len()));
        };
        state.leader = Some(leader.clone());
        Ok(leader)
    }

    /// Recomputes membership-driven structure for the whole topology.
    ///
    /// Partitions are rebuilt first (from current membership), then edges
    /// are reconstructed per kind:
    ///
    /// - **Mesh** — each node, in admission order, connects to up to
    ///   `mesh_degree` next neighbors in the same ordering (ring layout);
    ///   the reverse edge is also declared, so meshes are symmetric with
    ///   bounded degree.
    /// - **Hierarchical** — a breadth-first tree over admission order with
    ///   `hierarchical_fanout` children per parent; the oldest node roots
    ///   the tree. Edges flow parent → child.
    /// - **Centralized** — the current leader (or the oldest member before
    ///   any election) is the hub; every other node receives one hub →
    ///   spoke edge.
    /// - **Hybrid** — the hierarchical backbone plus a bounded mesh inside
    ///   each partition (the mesh rule at intra-partition scope).
    ///
    /// Leadership is reconciled after wiring: a leader that vanished or
    /// became ineligible is re-elected; a membered topology with no leader
    /// elects its first eligible candidate.
    ///
    /// Rebalancing is idempotent: rebalancing an already-balanced state
    /// changes nothing.
    pub fn rebalance(&self, state: &mut TopologyState) -> Result<(), TopologyError> {
        self.rebuild_partitions(state);
        state.edges.clear();
        let members = state.join_order.clone();
        match self.kind {
            TopologyKind::Mesh => self.wire_mesh(state, &members),
            TopologyKind::Hierarchical => self.wire_hierarchical(state, &members),
            TopologyKind::Centralized => self.wire_centralized(state, &members),
            TopologyKind::Hybrid => {
                self.wire_hierarchical(state, &members);
                let partition_members: Vec<Vec<NodeId>> = state
                    .partitions
                    .iter()
                    .map(|partition| partition.nodes.clone())
                    .collect();
                for group in &partition_members {
                    self.wire_mesh(state, group);
                }
            }
        }
        self.sync_connections(state);
        self.reconcile_leadership(state)
    }

    fn leader_candidate(&self, state: &TopologyState) -> Option<NodeId> {
        state
            .join_order
            .iter()
            .find(|id| {
                self.is_eligible_leader(state, id) && state.nodes[id].role == TopologyRole::Queen
            })
            .or_else(|| {
                state
                    .join_order
                    .iter()
                    .find(|id| self.is_eligible_leader(state, id))
            })
            .cloned()
    }

    fn is_eligible_leader(&self, state: &TopologyState, id: &NodeId) -> bool {
        state.nodes.get(id).is_some_and(|node| {
            matches!(
                node.status,
                NodeStatus::Active | NodeStatus::Syncing | NodeStatus::Inactive
            )
        })
    }

    fn reconcile_leadership(&self, state: &mut TopologyState) -> Result<(), TopologyError> {
        match state.leader.as_ref() {
            Some(current) if self.is_eligible_leader(state, current) => Ok(()),
            _ => {
                if self.leader_candidate(state).is_some() {
                    self.elect_leader(state)?;
                } else {
                    state.leader = None;
                }
                Ok(())
            }
        }
    }

    fn rebuild_partitions(&self, state: &mut TopologyState) {
        state.partitions.clear();
        if state.join_order.is_empty() {
            return;
        }
        let mut ordered = state.join_order.clone();
        match self.config.partition_strategy {
            PartitionStrategy::RoundRobin => {}
            PartitionStrategy::Range => ordered.sort(),
            PartitionStrategy::Hash => ordered.sort_by_key(|id| fnv1a(id.as_str().as_bytes())),
        }
        let capacity = self.config.nodes_per_partition.max(1) as usize;
        for (partition_index, chunk) in ordered.chunks(capacity).enumerate() {
            let nodes = chunk.to_vec();
            let leader = nodes[0].clone();
            let replica_count = self.config.replication_factor.min(nodes.len() as u32);
            state.partitions.push(TopologyPartition {
                id: format!("partition_{partition_index}"),
                nodes,
                leader,
                replica_count,
            });
        }
    }

    fn wire_mesh(&self, state: &mut TopologyState, members: &[NodeId]) {
        let degree = self.config.mesh_degree as usize;
        for (index, node) in members.iter().enumerate() {
            let mut connected = 0;
            for offset in 1..members.len() {
                if connected >= degree {
                    break;
                }
                let other = &members[(index + offset) % members.len()];
                if other == node {
                    continue;
                }
                self.push_edge(state, node, other);
                self.push_edge(state, other, node);
                connected += 1;
            }
        }
    }

    fn wire_hierarchical(&self, state: &mut TopologyState, members: &[NodeId]) {
        let fanout = self.config.hierarchical_fanout as usize;
        let mut child_index = 1;
        let mut parent_index = 0;
        while child_index < members.len() && parent_index < child_index {
            for _ in 0..fanout {
                if child_index >= members.len() {
                    break;
                }
                self.push_edge(state, &members[parent_index], &members[child_index]);
                child_index += 1;
            }
            parent_index += 1;
        }
    }

    fn wire_centralized(&self, state: &mut TopologyState, members: &[NodeId]) {
        if members.len() < 2 {
            return;
        }
        let hub = state
            .leader
            .clone()
            .filter(|leader| members.contains(leader))
            .unwrap_or_else(|| members[0].clone());
        for spoke in members {
            if *spoke != hub {
                self.push_edge(state, &hub, spoke);
            }
        }
    }

    fn push_edge(&self, state: &mut TopologyState, from: &NodeId, to: &NodeId) {
        let exists = state
            .edges
            .iter()
            .any(|edge| edge.from == *from && edge.to == *to);
        if exists {
            return;
        }
        state.edges.push(TopologyEdge {
            from: from.clone(),
            to: to.clone(),
            weight: 1.0,
            bidirectional: false,
        });
    }

    fn sync_connections(&self, state: &mut TopologyState) {
        for node in state.nodes.values_mut() {
            node.connections.clear();
        }
        for edge in &state.edges {
            if let Some(node) = state.nodes.get_mut(&edge.from) {
                node.connections.push(edge.to.clone());
            }
        }
    }
}

/// FNV-1a over node-id bytes. Inline and stable forever: hash-strategy
/// partition placement must never depend on std hasher internals or crate
/// versions.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(name: &str) -> NodeId {
        NodeId::new(name)
    }

    fn admit(manager: &TopologyManager, state: &mut TopologyState, names: &[&str]) {
        for name in names {
            manager
                .add_node(state, worker(name), TopologyRole::Worker)
                .expect("admission succeeds");
        }
    }

    fn ids(names: &[&str]) -> Vec<NodeId> {
        names.iter().map(|name| NodeId::new(*name)).collect()
    }

    fn activate_all(manager: &TopologyManager, state: &mut TopologyState) {
        let members = state.join_order.clone();
        for member in members {
            manager
                .update_node(
                    state,
                    &member,
                    NodeUpdate {
                        status: Some(NodeStatus::Active),
                        ..NodeUpdate::none()
                    },
                )
                .expect("activation succeeds");
        }
    }

    /// Deterministic xorshift64* PRNG for seeded property tests. No `rand`
    /// dependency: the seed sequence is fully reproducible forever.
    struct Lcg(u64);

    impl Lcg {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn below(&mut self, bound: usize) -> usize {
            if bound == 0 {
                0
            } else {
                (self.next_u64() % bound as u64) as usize
            }
        }
    }

    fn assert_invariants(state: &TopologyState) {
        assert_eq!(
            state.nodes.len(),
            state.join_order.len(),
            "node map and join ledger disagree"
        );
        for id in &state.join_order {
            assert!(state.nodes.contains_key(id), "ledger member {id} missing");
        }
        for edge in &state.edges {
            assert!(state.nodes.contains_key(&edge.from), "dangling edge from");
            assert!(state.nodes.contains_key(&edge.to), "dangling edge to");
        }
        let mut seen = std::collections::BTreeSet::new();
        for edge in &state.edges {
            assert!(
                seen.insert((&edge.from, &edge.to)),
                "duplicate directed edge {edge:?}"
            );
        }
        for partition in &state.partitions {
            assert!(!partition.nodes.is_empty(), "empty partition");
            for member in &partition.nodes {
                assert!(state.nodes.contains_key(member), "partition ghost member");
            }
            assert!(
                partition.nodes.contains(&partition.leader),
                "partition leader not a member"
            );
        }
        if let Some(leader) = &state.leader {
            assert!(state.nodes.contains_key(leader), "topology leader vanished");
        }
        for node in state.nodes.values() {
            for connection in &node.connections {
                assert!(
                    state.nodes.contains_key(connection),
                    "connection to unknown node"
                );
            }
        }
    }

    #[test]
    fn manager_rejects_invalid_config() {
        let error = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                replication_factor: 0,
                ..TopologyConfig::default()
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            TopologyError::InvalidConfig(TopologyConfigError::InvalidReplicationFactor {
                replication_factor: 0,
                max_agents: 8,
            })
        );
    }

    #[test]
    fn add_node_admits_in_join_order_without_wiring_by_default() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c"]);
        assert_eq!(state.join_order, ids(&["a", "b", "c"]));
        assert!(state.edges.is_empty());
        assert!(state.partitions.is_empty());
        assert_eq!(state.nodes[&worker("a")].status, NodeStatus::Initializing);
    }

    #[test]
    fn add_node_rejects_duplicates_and_capacity() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::new(2)).unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a"]);
        assert_eq!(
            manager.add_node(&mut state, worker("a"), TopologyRole::Worker),
            Err(TopologyError::DuplicateNode(worker("a")))
        );
        admit(&manager, &mut state, &["b"]);
        assert_eq!(
            manager.add_node(&mut state, worker("c"), TopologyRole::Worker),
            Err(TopologyError::CapacityReached(2))
        );
        assert_eq!(state.node_count(), 2);
    }

    #[test]
    fn update_node_patches_role_status_and_metadata() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a"]);
        manager
            .update_node(
                &mut state,
                &worker("a"),
                NodeUpdate {
                    status: Some(NodeStatus::Active),
                    metadata: Some(MetadataPatch::Insert("class".into(), "driver".into())),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        let node = &state.nodes[&worker("a")];
        assert_eq!(node.status, NodeStatus::Active);
        assert_eq!(node.metadata["class"], "driver");
        manager
            .update_node(
                &mut state,
                &worker("a"),
                NodeUpdate {
                    role: Some(TopologyRole::Coordinator),
                    metadata: Some(MetadataPatch::Remove("class".into())),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        let node = &state.nodes[&worker("a")];
        assert_eq!(node.role, TopologyRole::Coordinator);
        assert!(node.metadata.is_empty());
        manager
            .update_node(
                &mut state,
                &worker("a"),
                NodeUpdate {
                    metadata: Some(MetadataPatch::Replace(BTreeMap::from([(
                        "k".into(),
                        "v".into(),
                    )]))),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        assert_eq!(state.nodes[&worker("a")].metadata["k"], "v");
        assert_eq!(
            manager.update_node(&mut state, &worker("zz"), NodeUpdate::none()),
            Err(TopologyError::UnknownNode(worker("zz")))
        );
    }

    #[test]
    fn remove_node_strips_every_trace() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                auto_rebalance: true,
                nodes_per_partition: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c", "d"]);
        activate_all(&manager, &mut state);
        assert_invariants(&state);
        manager.remove_node(&mut state, &worker("b")).unwrap();
        assert!(!state.nodes.contains_key(&worker("b")));
        assert_eq!(state.join_order, ids(&["a", "c", "d"]));
        for edge in &state.edges {
            assert_ne!(edge.from, worker("b"));
            assert_ne!(edge.to, worker("b"));
        }
        for node in state.nodes.values() {
            assert!(!node.connections.contains(&worker("b")));
        }
        for partition in &state.partitions {
            assert!(!partition.nodes.contains(&worker("b")));
        }
        assert_invariants(&state);
    }

    #[test]
    fn remove_node_rejects_unknown_ids() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        assert_eq!(
            manager.remove_node(&mut state, &worker("ghost")),
            Err(TopologyError::UnknownNode(worker("ghost")))
        );
    }

    #[test]
    fn leader_failover_enabled_promotes_oldest_remaining_member() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                failover_enabled: true,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["first", "second", "third"]);
        activate_all(&manager, &mut state);
        let elected = manager.elect_leader(&mut state).unwrap();
        assert_eq!(elected, worker("first"));
        assert_eq!(state.leader, Some(worker("first")));
        manager.remove_node(&mut state, &worker("first")).unwrap();
        assert_eq!(state.leader, Some(worker("second")));
        manager.remove_node(&mut state, &worker("second")).unwrap();
        assert_eq!(state.leader, Some(worker("third")));
        manager.remove_node(&mut state, &worker("third")).unwrap();
        assert_eq!(state.leader, None);
    }

    #[test]
    fn leader_failover_disabled_leaves_leadership_vacant() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                failover_enabled: false,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b"]);
        activate_all(&manager, &mut state);
        manager.elect_leader(&mut state).unwrap();
        manager.remove_node(&mut state, &worker("a")).unwrap();
        assert_eq!(state.leader, None);
        // No implicit promotion: a later explicit election restores it.
        assert_eq!(manager.elect_leader(&mut state).unwrap(), worker("b"));
    }

    #[test]
    fn partition_leadership_fails_over_or_dissolves() {
        // Range strategy keeps placement deterministic: [a,b] then [c,d],
        // leaders a and c.
        let mut manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                partition_strategy: PartitionStrategy::Range,
                nodes_per_partition: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c", "d"]);
        activate_all(&manager, &mut state);
        manager.rebalance(&mut state).unwrap();
        let led = state
            .partitions
            .iter()
            .find(|partition| partition.leader == worker("a"))
            .cloned()
            .expect("a leads a partition");
        assert_eq!(led.nodes, ids(&["a", "b"]));
        // Failover disabled: removing the leader dissolves the partition.
        manager.remove_node(&mut state, &worker("a")).unwrap();
        assert!(!state.partitions.iter().any(|p| p.id == led.id));
        assert_invariants(&state);

        // Failover enabled: the remaining member takes over in-partition.
        manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                partition_strategy: PartitionStrategy::Range,
                nodes_per_partition: 2,
                failover_enabled: true,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c", "d"]);
        activate_all(&manager, &mut state);
        manager.rebalance(&mut state).unwrap();
        let led = state
            .partitions
            .iter()
            .find(|partition| partition.leader == worker("c"))
            .cloned()
            .expect("c leads a partition");
        manager.remove_node(&mut state, &worker("c")).unwrap();
        let after = state
            .partitions
            .iter()
            .find(|p| p.id == led.id)
            .expect("partition survives failover");
        assert_eq!(after.nodes, ids(&["d"]));
        assert_eq!(after.leader, worker("d"));
    }

    #[test]
    fn elect_leader_prefers_oldest_eligible_queen_then_oldest_member() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        manager
            .add_node(&mut state, worker("old-worker"), TopologyRole::Worker)
            .unwrap();
        manager
            .add_node(&mut state, worker("mid-queen"), TopologyRole::Queen)
            .unwrap();
        manager
            .add_node(&mut state, worker("young-queen"), TopologyRole::Queen)
            .unwrap();
        activate_all(&manager, &mut state);
        assert_eq!(
            manager.elect_leader(&mut state).unwrap(),
            worker("mid-queen")
        );
        // Demote that queen: the next queen still outranks the older worker.
        manager
            .update_node(
                &mut state,
                &worker("mid-queen"),
                NodeUpdate {
                    role: Some(TopologyRole::Worker),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        assert_eq!(
            manager.elect_leader(&mut state).unwrap(),
            worker("young-queen")
        );
        // Demote the last queen too: only now does the oldest member lead.
        manager
            .update_node(
                &mut state,
                &worker("young-queen"),
                NodeUpdate {
                    role: Some(TopologyRole::Worker),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        assert_eq!(
            manager.elect_leader(&mut state).unwrap(),
            worker("old-worker")
        );
        // Failed and initializing nodes are never eligible.
        manager
            .update_node(
                &mut state,
                &worker("old-worker"),
                NodeUpdate {
                    status: Some(NodeStatus::Failed),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        assert_eq!(
            manager.elect_leader(&mut state).unwrap(),
            worker("mid-queen")
        );
        manager
            .update_node(
                &mut state,
                &worker("mid-queen"),
                NodeUpdate {
                    status: Some(NodeStatus::Initializing),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        assert_eq!(
            manager.elect_leader(&mut state).unwrap(),
            worker("young-queen")
        );
    }

    #[test]
    fn elect_leader_fails_closed_when_nobody_is_eligible() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a"]);
        assert_eq!(
            manager.elect_leader(&mut state),
            Err(TopologyError::NoEligibleLeader(1))
        );
        let mut empty = manager.initial_state();
        assert_eq!(
            manager.elect_leader(&mut empty),
            Err(TopologyError::NoEligibleLeader(0))
        );
    }

    #[test]
    fn partitions_open_only_when_nodes_exceed_nodes_per_partition() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                nodes_per_partition: 3,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b"]);
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.partitions.len(), 1);
        admit(&manager, &mut state, &["c"]);
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.partitions.len(), 1, "three nodes still fit");
        admit(&manager, &mut state, &["d"]);
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.partitions.len(), 2, "fourth node opens partition 1");
        assert_eq!(state.partitions[0].nodes.len(), 3);
        assert_eq!(state.partitions[1].nodes.len(), 1);
        assert_invariants(&state);
    }

    #[test]
    fn round_robin_partitions_follow_join_order() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                partition_strategy: PartitionStrategy::RoundRobin,
                nodes_per_partition: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c", "d", "e"]);
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.partitions[0].nodes, ids(&["a", "b"]));
        assert_eq!(state.partitions[1].nodes, ids(&["c", "d"]));
        assert_eq!(state.partitions[2].nodes, ids(&["e"]));
    }

    #[test]
    fn range_partitions_are_identity_sorted() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                partition_strategy: PartitionStrategy::Range,
                nodes_per_partition: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(
            &manager,
            &mut state,
            &["delta", "alpha", "charlie", "bravo"],
        );
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.partitions[0].nodes, ids(&["alpha", "bravo"]));
        assert_eq!(state.partitions[1].nodes, ids(&["charlie", "delta"]));
    }

    #[test]
    fn hash_partitions_follow_stable_fnv_ordering() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                partition_strategy: PartitionStrategy::Hash,
                nodes_per_partition: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["n1", "n2", "n3", "n4"]);
        manager.rebalance(&mut state).unwrap();
        let mut expected = ids(&["n1", "n2", "n3", "n4"]);
        expected.sort_by_key(|id| fnv1a(id.as_str().as_bytes()));
        assert_eq!(state.partitions[0].nodes, expected[..2].to_vec());
        assert_eq!(state.partitions[1].nodes, expected[2..].to_vec());
        assert_eq!(state.partitions[0].leader, expected[0]);
    }

    #[test]
    fn hash_partitions_are_independent_of_admission_order() {
        let config = TopologyConfig {
            partition_strategy: PartitionStrategy::Hash,
            nodes_per_partition: 2,
            ..TopologyConfig::default()
        };
        let manager = TopologyManager::new(TopologyKind::Mesh, config).unwrap();
        let mut forward = manager.initial_state();
        admit(&manager, &mut forward, &["a", "b", "c", "d", "e", "f"]);
        manager.rebalance(&mut forward).unwrap();
        let mut reverse = manager.initial_state();
        admit(&manager, &mut reverse, &["f", "e", "d", "c", "b", "a"]);
        manager.rebalance(&mut reverse).unwrap();
        let membership = |state: &TopologyState| {
            state
                .partitions
                .iter()
                .map(|partition| {
                    let mut members = partition.nodes.clone();
                    members.sort();
                    members
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(membership(&forward), membership(&reverse));
    }

    #[test]
    fn mesh_wiring_is_bounded_degree_and_symmetric() {
        let manager = TopologyManager::new(
            TopologyKind::Mesh,
            TopologyConfig {
                mesh_degree: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c", "d", "e"]);
        manager.rebalance(&mut state).unwrap();
        // Each node initiates at most mesh_degree edges and receives at
        // most mesh_degree, so its total connection count stays bounded at
        // 2 * mesh_degree — edge growth stays linear in node count.
        for node in state.nodes.values() {
            assert!(
                node.connections.len() <= 4,
                "node {} has degree {}",
                node.id,
                node.connections.len()
            );
            assert!(!node.connections.is_empty());
        }
        for edge in &state.edges {
            let reverse = state
                .edges
                .iter()
                .any(|other| other.from == edge.to && other.to == edge.from);
            assert!(reverse, "mesh edge {edge:?} has no reverse");
        }
        assert_invariants(&state);
    }

    #[test]
    fn hierarchical_wiring_builds_a_fanout_tree() {
        let manager = TopologyManager::new(
            TopologyKind::Hierarchical,
            TopologyConfig {
                hierarchical_fanout: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(
            &manager,
            &mut state,
            &["root", "c1", "c2", "g1", "g2", "g3"],
        );
        manager.rebalance(&mut state).unwrap();
        let children_of = |id: &str| {
            state.nodes[&worker(id)]
                .connections
                .iter()
                .map(|c| c.as_str().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(children_of("root"), vec!["c1", "c2"]);
        assert_eq!(children_of("c1"), vec!["g1", "g2"]);
        assert_eq!(children_of("c2"), vec!["g3"]);
        assert!(children_of("g1").is_empty());
        // All edges flow parent -> child; no reverse edges in a tree.
        assert_eq!(state.edges.len(), 5);
    }

    #[test]
    fn centralized_wiring_uses_the_leader_as_hub() {
        let manager =
            TopologyManager::new(TopologyKind::Centralized, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["hub", "s1", "s2", "s3"]);
        activate_all(&manager, &mut state);
        manager.elect_leader(&mut state).unwrap();
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.edges.len(), 3);
        for edge in &state.edges {
            assert_eq!(edge.from, worker("hub"));
        }
        let spokes = state.nodes[&worker("hub")]
            .connections
            .iter()
            .map(|c| c.as_str().to_string())
            .collect::<Vec<_>>();
        assert_eq!(spokes, vec!["s1", "s2", "s3"]);
        // Spokes connect only to the hub.
        assert!(state.nodes[&worker("s1")].connections.is_empty());
    }

    #[test]
    fn hybrid_wiring_combines_backbone_and_intra_partition_mesh() {
        // Range strategy: partitions [a,b] and [c,d], deterministic.
        let manager = TopologyManager::new(
            TopologyKind::Hybrid,
            TopologyConfig {
                partition_strategy: PartitionStrategy::Range,
                hierarchical_fanout: 4,
                mesh_degree: 1,
                nodes_per_partition: 2,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c", "d"]);
        manager.rebalance(&mut state).unwrap();
        // Backbone: a roots the tree and reaches every node (fanout 4).
        assert!(
            state
                .edges
                .iter()
                .any(|edge| edge.from == worker("a") && edge.to == worker("b"))
        );
        assert!(
            state
                .edges
                .iter()
                .any(|edge| edge.from == worker("a") && edge.to == worker("d"))
        );
        // Intra-partition mesh: degree 1 inside [a,b] and [c,d] adds the
        // reverse links the backbone alone would not declare.
        assert!(
            state
                .edges
                .iter()
                .any(|edge| edge.from == worker("b") && edge.to == worker("a"))
        );
        assert!(
            state
                .edges
                .iter()
                .any(|edge| edge.from == worker("c") && edge.to == worker("d"))
                || state
                    .edges
                    .iter()
                    .any(|edge| edge.from == worker("d") && edge.to == worker("c"))
        );
        assert_invariants(&state);
    }

    #[test]
    fn auto_rebalance_off_keeps_add_pure_and_rebalance_idempotent() {
        let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
        let mut state = manager.initial_state();
        admit(&manager, &mut state, &["a", "b", "c"]);
        assert!(state.edges.is_empty());
        manager.rebalance(&mut state).unwrap();
        let once = state.clone();
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state, once, "rebalance is idempotent");
    }

    #[test]
    fn auto_rebalance_on_tracks_membership_automatically() {
        let manager = TopologyManager::new(
            TopologyKind::Centralized,
            TopologyConfig {
                auto_rebalance: true,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        manager
            .add_node(&mut state, worker("h"), TopologyRole::Queen)
            .unwrap();
        manager
            .update_node(
                &mut state,
                &worker("h"),
                NodeUpdate {
                    status: Some(NodeStatus::Active),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        assert!(state.edges.is_empty(), "single node has no edges");
        manager
            .add_node(&mut state, worker("s1"), TopologyRole::Worker)
            .unwrap();
        assert_eq!(state.edges.len(), 1);
        assert_eq!(state.edges[0].from, worker("h"));
        manager.remove_node(&mut state, &worker("s1")).unwrap();
        assert!(state.edges.is_empty());
    }

    #[test]
    fn seeded_property_sequences_are_deterministic_and_consistent() {
        let mut rng = Lcg(0x5eed_1234_abcd_ef01);
        for case in 0..24 {
            let kind = [
                TopologyKind::Mesh,
                TopologyKind::Hierarchical,
                TopologyKind::Centralized,
                TopologyKind::Hybrid,
            ][rng.below(4)];
            let config = TopologyConfig {
                max_agents: 12,
                replication_factor: 1 + rng.below(3) as u32,
                partition_strategy: [
                    PartitionStrategy::Hash,
                    PartitionStrategy::Range,
                    PartitionStrategy::RoundRobin,
                ][rng.below(3)],
                failover_enabled: rng.below(2) == 0,
                auto_rebalance: rng.below(2) == 0,
                nodes_per_partition: 1 + rng.below(4) as u32,
                mesh_degree: 1 + rng.below(4) as u32,
                hierarchical_fanout: 1 + rng.below(3) as u32,
            };
            let run = |seed_case: usize| -> TopologyState {
                let mut local = Lcg(0x5eed_1234_abcd_ef01);
                for _ in 0..seed_case {
                    local.next_u64();
                }
                let manager = TopologyManager::new(kind, config.clone()).expect("valid config");
                let mut state = manager.initial_state();
                let mut admitted: Vec<String> = Vec::new();
                for step in 0..40 {
                    let choice = local.below(4);
                    if choice == 0 || admitted.len() < 2 {
                        let name = format!("node-{step}");
                        if manager
                            .add_node(&mut state, NodeId::new(name.clone()), TopologyRole::Worker)
                            .is_ok()
                        {
                            admitted.push(name);
                        }
                    } else if choice == 1 {
                        let victim = local.below(admitted.len());
                        let _ =
                            manager.remove_node(&mut state, &NodeId::new(admitted[victim].clone()));
                        admitted.remove(victim);
                    } else if choice == 2 {
                        let target = local.below(admitted.len());
                        let statuses = [
                            NodeStatus::Active,
                            NodeStatus::Syncing,
                            NodeStatus::Inactive,
                            NodeStatus::Failed,
                        ];
                        let _ = manager.update_node(
                            &mut state,
                            &NodeId::new(admitted[target].clone()),
                            NodeUpdate {
                                status: Some(statuses[local.below(4)]),
                                ..NodeUpdate::none()
                            },
                        );
                    } else {
                        let _ = manager.rebalance(&mut state);
                    }
                }
                let _ = manager.rebalance(&mut state);
                assert_invariants(&state);
                state
            };
            let first = run(case);
            let second = run(case);
            assert_eq!(first, second, "case {case} diverged across runs");
        }
    }

    #[test]
    fn seeded_property_same_membership_same_wiring() {
        // Whatever the operation order, a final rebalance makes wiring a
        // pure function of (kind, config, member set, join order).
        let mut rng = Lcg(0xa11c_e517_f00d_ba5e);
        for _ in 0..12 {
            let kind = [
                TopologyKind::Mesh,
                TopologyKind::Hierarchical,
                TopologyKind::Centralized,
                TopologyKind::Hybrid,
            ][rng.below(4)];
            let config = TopologyConfig {
                max_agents: 16,
                nodes_per_partition: 1 + rng.below(3) as u32,
                mesh_degree: 1 + rng.below(3) as u32,
                hierarchical_fanout: 1 + rng.below(3) as u32,
                ..TopologyConfig::default()
            };
            let manager = TopologyManager::new(kind, config).unwrap();
            let names: Vec<String> = (0..8).map(|index| format!("m{index}")).collect();

            let build = |order: &[String]| -> TopologyState {
                let mut state = manager.initial_state();
                for name in order {
                    manager
                        .add_node(&mut state, NodeId::new(name.clone()), TopologyRole::Worker)
                        .unwrap();
                }
                manager.rebalance(&mut state).unwrap();
                state
            };
            let forward = build(&names);
            let reversed: Vec<String> = names.iter().rev().cloned().collect();
            let backward = build(&reversed);

            // Wiring may depend on join order, but structural invariants
            // hold in both directions and both are deterministic.
            assert_invariants(&forward);
            assert_invariants(&backward);
            let forward_again = build(&names);
            assert_eq!(forward, forward_again);
            let backward_again = build(&reversed);
            assert_eq!(backward, backward_again);
        }
    }
}
