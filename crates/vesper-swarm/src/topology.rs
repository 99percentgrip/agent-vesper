//! Pure swarm-topology data model (VRO-15 PR-1).
//!
//! These types describe *how worker agents are arranged* — nothing here
//! executes, performs I/O, or names a provider. The model is extracted from
//! the swarm oracle's topology layer (mesh, hierarchical, centralized,
//! hybrid) with deliberate divergences documented on [`TopologyConfig`]:
//! potentially destructive behaviors are opt-in (`auto_rebalance` and
//! `failover_enabled` default to `false`), and state carries no wall-clock
//! timestamps — ordering is structural, not temporal.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Stable identity of one worker node inside a single hive run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Creates a node identity from its stable string form.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The stable string form of this identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How a hive arranges its worker nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TopologyKind {
    /// Every node may exchange messages with every other node.
    Mesh,
    /// A parent/child tree; coordination authority flows downward.
    Hierarchical,
    /// One hub node; all other nodes connect only to the hub.
    Centralized,
    /// Hierarchical backbone with mesh links inside each partition.
    Hybrid,
}

/// How nodes are assigned to topology partitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PartitionStrategy {
    /// Deterministic placement derived from the node identity.
    Hash,
    /// Contiguous identity-ordered placement.
    Range,
    /// Round-robin placement in join order.
    RoundRobin,
}

/// The coordination role one node plays inside its topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TopologyRole {
    /// Strategic decision-maker coordinating the hive.
    Queen,
    /// Execution unit running bounded tasks.
    Worker,
    /// Mid-level authority relaying between queen and workers.
    Coordinator,
    /// Equal participant with no special authority.
    Peer,
}

/// Lifecycle status of one node inside its topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeStatus {
    /// Created but not yet admitted to the topology.
    Initializing,
    /// Admitted and participating.
    Active,
    /// Admitted but currently reconciling state.
    Syncing,
    /// Deliberately not participating (e.g. paused).
    Inactive,
    /// Failed or evicted; scheduled for replacement.
    Failed,
}

/// Errors rejected by [`TopologyConfig::validate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TopologyConfigError {
    /// `max_agents` must be at least one.
    #[error("max_agents must be at least one")]
    ZeroMaxAgents,
    /// `replication_factor` must be in `1..=max_agents`.
    #[error("replication_factor {replication_factor} must be in 1..={max_agents}")]
    InvalidReplicationFactor {
        /// The rejected replication factor.
        replication_factor: u32,
        /// The configured maximum agent count.
        max_agents: u32,
    },
    /// Structural shape parameters (`nodes_per_partition`, `mesh_degree`,
    /// `hierarchical_fanout`) must all be at least one.
    #[error("{parameter} must be at least one")]
    ZeroShapeParameter {
        /// The rejected parameter name.
        parameter: &'static str,
    },
}

/// Declarative description of one hive's topology.
///
/// Two deliberate divergences from the swarm oracle's defaults: both
/// `failover_enabled` and `auto_rebalance` default to **`false`**. Anything
/// that can move leadership or rewire edges is an explicit operator opt-in;
/// rebalancing without supervision is never implicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyConfig {
    /// Maximum number of worker nodes this topology admits.
    pub max_agents: u32,
    /// Replica count per partition (leader plus standbys).
    pub replication_factor: u32,
    /// How nodes are assigned to partitions.
    pub partition_strategy: PartitionStrategy,
    /// Whether partition leadership may fail over when a leader disappears.
    pub failover_enabled: bool,
    /// Whether structural changes trigger an automatic rebalance.
    /// Always opt-in: defaults to `false`.
    pub auto_rebalance: bool,
    /// Maximum nodes assigned to one partition before a new partition is
    /// opened. Must be at least one; `1` means one node per partition.
    pub nodes_per_partition: u32,
    /// Maximum distinct neighbors one node gains under a mesh (or hybrid
    /// intra-partition) rebalance. Bounded-degree meshes keep edge growth
    /// linear in node count. Must be at least one.
    pub mesh_degree: u32,
    /// Children per parent in a hierarchical (or hybrid backbone) rebalance.
    /// Must be at least one.
    pub hierarchical_fanout: u32,
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            max_agents: 8,
            replication_factor: 1,
            partition_strategy: PartitionStrategy::Hash,
            failover_enabled: false,
            auto_rebalance: false,
            nodes_per_partition: 4,
            mesh_degree: 4,
            hierarchical_fanout: 4,
        }
    }
}

impl TopologyConfig {
    /// Creates a config with the given agent cap and otherwise default
    /// values (`replication_factor` 1, hash partitioning, all behaviors
    /// opt-in/off).
    #[must_use]
    pub fn new(max_agents: u32) -> Self {
        Self {
            max_agents,
            ..Self::default()
        }
    }

    /// Fails closed when the declared shape cannot describe a valid hive.
    pub fn validate(&self) -> Result<(), TopologyConfigError> {
        if self.max_agents == 0 {
            return Err(TopologyConfigError::ZeroMaxAgents);
        }
        if self.replication_factor == 0 || self.replication_factor > self.max_agents {
            return Err(TopologyConfigError::InvalidReplicationFactor {
                replication_factor: self.replication_factor,
                max_agents: self.max_agents,
            });
        }
        for parameter in ["nodes_per_partition", "mesh_degree", "hierarchical_fanout"] {
            let value = match parameter {
                "nodes_per_partition" => self.nodes_per_partition,
                "mesh_degree" => self.mesh_degree,
                _ => self.hierarchical_fanout,
            };
            if value == 0 {
                return Err(TopologyConfigError::ZeroShapeParameter { parameter });
            }
        }
        Ok(())
    }
}

/// One node's structural record inside the topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyNode {
    /// Stable identity of this node.
    pub id: NodeId,
    /// Coordination role this node plays.
    pub role: TopologyRole,
    /// Current lifecycle status.
    pub status: NodeStatus,
    /// Identities this node holds edges to, in edge-creation order.
    pub connections: Vec<NodeId>,
    /// Small, bounded, string-only annotation surface (no clock, no secrets).
    pub metadata: BTreeMap<String, String>,
}

impl TopologyNode {
    /// Creates an initializing node with no connections and no metadata.
    #[must_use]
    pub fn new(id: NodeId, role: TopologyRole) -> Self {
        Self {
            id,
            role,
            status: NodeStatus::Initializing,
            connections: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }
}

/// One directed (or declared bidirectional) edge between nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopologyEdge {
    /// Edge origin.
    pub from: NodeId,
    /// Edge destination.
    pub to: NodeId,
    /// Non-negative routing weight; equal weights mean unweighted.
    pub weight: f64,
    /// Whether the reverse edge is implied.
    pub bidirectional: bool,
}

/// One partition of the topology: a node group with a leader and standbys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyPartition {
    /// Stable partition identifier.
    pub id: String,
    /// Member node identities, in join order.
    pub nodes: Vec<NodeId>,
    /// Current leader of this partition.
    pub leader: NodeId,
    /// Declared replica count for this partition.
    pub replica_count: u32,
}

/// The complete structural snapshot of one hive's topology.
///
/// Pure data: construction and mutation rules (edge creation per kind,
/// leader election, partition assignment) belong to the manager in PR-2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopologyState {
    /// The kind this state instantiates.
    pub kind: TopologyKind,
    /// All admitted nodes, keyed by identity.
    pub nodes: BTreeMap<NodeId, TopologyNode>,
    /// Admission order of every current member, oldest first. This is the
    /// single source of structural ordering: elections, round-robin
    /// partitioning, and tree construction all read it. Maintained by the
    /// manager; manual construction should keep it consistent with `nodes`.
    #[serde(default)]
    pub join_order: Vec<NodeId>,
    /// All declared edges, in creation order.
    pub edges: Vec<TopologyEdge>,
    /// The topology-wide leader, once elected.
    pub leader: Option<NodeId>,
    /// All partitions, in creation order.
    pub partitions: Vec<TopologyPartition>,
}

impl TopologyState {
    /// Creates an empty state of the given kind.
    #[must_use]
    pub fn new(kind: TopologyKind) -> Self {
        Self {
            kind,
            nodes: BTreeMap::new(),
            join_order: Vec::new(),
            edges: Vec::new(),
            leader: None,
            partitions: Vec::new(),
        }
    }

    /// Number of admitted nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the topology admits no nodes yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
        let encoded = serde_json::to_string(value).expect("serialization succeeds");
        serde_json::from_str(&encoded).expect("deserialization succeeds")
    }

    #[test]
    fn topology_kinds_round_trip_with_kebab_case_names() {
        for (kind, name) in [
            (TopologyKind::Mesh, "mesh"),
            (TopologyKind::Hierarchical, "hierarchical"),
            (TopologyKind::Centralized, "centralized"),
            (TopologyKind::Hybrid, "hybrid"),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{name}\""));
            assert_eq!(round_trip(&kind), kind);
        }
    }

    #[test]
    fn partition_strategies_round_trip_with_kebab_case_names() {
        for (strategy, name) in [
            (PartitionStrategy::Hash, "hash"),
            (PartitionStrategy::Range, "range"),
            (PartitionStrategy::RoundRobin, "round-robin"),
        ] {
            assert_eq!(
                serde_json::to_string(&strategy).unwrap(),
                format!("\"{name}\"")
            );
            assert_eq!(round_trip(&strategy), strategy);
        }
    }

    #[test]
    fn roles_and_statuses_round_trip_with_kebab_case_names() {
        for (role, name) in [
            (TopologyRole::Queen, "queen"),
            (TopologyRole::Worker, "worker"),
            (TopologyRole::Coordinator, "coordinator"),
            (TopologyRole::Peer, "peer"),
        ] {
            assert_eq!(serde_json::to_string(&role).unwrap(), format!("\"{name}\""));
        }
        for (status, name) in [
            (NodeStatus::Initializing, "initializing"),
            (NodeStatus::Active, "active"),
            (NodeStatus::Syncing, "syncing"),
            (NodeStatus::Inactive, "inactive"),
            (NodeStatus::Failed, "failed"),
        ] {
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                format!("\"{name}\"")
            );
            assert_eq!(round_trip(&status), status);
        }
    }

    #[test]
    fn node_id_is_transparent_and_round_trips() {
        let id = NodeId::new("driver-3");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"driver-3\"");
        assert_eq!(round_trip(&id), id);
        assert_eq!(id.to_string(), "driver-3");
        assert_eq!(id.as_str(), "driver-3");
    }

    #[test]
    fn config_defaults_are_fail_closed() {
        let config = TopologyConfig::default();
        assert_eq!(config.max_agents, 8);
        assert_eq!(config.replication_factor, 1);
        assert_eq!(config.partition_strategy, PartitionStrategy::Hash);
        // VRO-15 divergence from the swarm oracle: both behaviors opt-in.
        assert!(!config.failover_enabled);
        assert!(!config.auto_rebalance);
    }

    #[test]
    fn config_round_trips_with_every_field_non_default() {
        let config = TopologyConfig {
            max_agents: 32,
            replication_factor: 3,
            partition_strategy: PartitionStrategy::RoundRobin,
            failover_enabled: true,
            auto_rebalance: true,
            nodes_per_partition: 2,
            mesh_degree: 6,
            hierarchical_fanout: 3,
        };
        assert_eq!(round_trip(&config), config);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_defaults_cover_shape_parameters() {
        let config = TopologyConfig::default();
        assert_eq!(config.nodes_per_partition, 4);
        assert_eq!(config.mesh_degree, 4);
        assert_eq!(config.hierarchical_fanout, 4);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_rejects_zero_shape_parameters() {
        for (config, parameter) in [
            (
                TopologyConfig {
                    nodes_per_partition: 0,
                    ..TopologyConfig::default()
                },
                "nodes_per_partition",
            ),
            (
                TopologyConfig {
                    mesh_degree: 0,
                    ..TopologyConfig::default()
                },
                "mesh_degree",
            ),
            (
                TopologyConfig {
                    hierarchical_fanout: 0,
                    ..TopologyConfig::default()
                },
                "hierarchical_fanout",
            ),
        ] {
            assert_eq!(
                config.validate().unwrap_err(),
                TopologyConfigError::ZeroShapeParameter { parameter }
            );
            assert_eq!(
                TopologyConfigError::ZeroShapeParameter { parameter }.to_string(),
                format!("{parameter} must be at least one")
            );
        }
    }

    #[test]
    fn config_rejects_zero_max_agents() {
        let error = TopologyConfig::new(0).validate().unwrap_err();
        assert_eq!(error, TopologyConfigError::ZeroMaxAgents);
    }

    #[test]
    fn config_rejects_out_of_range_replication_factor() {
        for factor in [0, 5] {
            let config = TopologyConfig {
                max_agents: 4,
                replication_factor: factor,
                ..TopologyConfig::default()
            };
            assert_eq!(
                config.validate().unwrap_err(),
                TopologyConfigError::InvalidReplicationFactor {
                    replication_factor: factor,
                    max_agents: 4,
                }
            );
        }
    }

    #[test]
    fn config_error_messages_are_stable() {
        assert_eq!(
            TopologyConfigError::ZeroMaxAgents.to_string(),
            "max_agents must be at least one"
        );
        assert_eq!(
            TopologyConfigError::InvalidReplicationFactor {
                replication_factor: 7,
                max_agents: 4
            }
            .to_string(),
            "replication_factor 7 must be in 1..=4"
        );
    }

    #[test]
    fn node_new_starts_initializing_and_round_trips() {
        let node = TopologyNode::new(NodeId::new("n1"), TopologyRole::Worker);
        assert_eq!(node.status, NodeStatus::Initializing);
        assert!(node.connections.is_empty());
        assert!(node.metadata.is_empty());
        let decoded = round_trip(&node);
        assert_eq!(decoded, node);
    }

    #[test]
    fn node_round_trips_with_connections_and_metadata() {
        let node = TopologyNode {
            id: NodeId::new("queen"),
            role: TopologyRole::Queen,
            status: NodeStatus::Active,
            connections: vec![NodeId::new("w1"), NodeId::new("w2")],
            metadata: BTreeMap::from([
                (String::from("class"), String::from("navigator")),
                (String::from("domain"), String::from("core")),
            ]),
        };
        assert_eq!(round_trip(&node), node);
    }

    #[test]
    fn edge_round_trips() {
        let edge = TopologyEdge {
            from: NodeId::new("w1"),
            to: NodeId::new("w2"),
            weight: 0.5,
            bidirectional: true,
        };
        assert_eq!(round_trip(&edge), edge);
    }

    #[test]
    fn partition_round_trips() {
        let partition = TopologyPartition {
            id: String::from("partition_0"),
            nodes: vec![NodeId::new("w1"), NodeId::new("w2")],
            leader: NodeId::new("w1"),
            replica_count: 2,
        };
        assert_eq!(round_trip(&partition), partition);
    }

    #[test]
    fn state_round_trips_a_populated_topology() {
        let mut state = TopologyState::new(TopologyKind::Hybrid);
        for (name, role) in [
            ("queen", TopologyRole::Queen),
            ("w1", TopologyRole::Worker),
            ("w2", TopologyRole::Worker),
        ] {
            state.nodes.insert(NodeId::new(name), {
                let mut node = TopologyNode::new(NodeId::new(name), role);
                node.status = NodeStatus::Active;
                node
            });
        }
        state.edges.push(TopologyEdge {
            from: NodeId::new("queen"),
            to: NodeId::new("w1"),
            weight: 1.0,
            bidirectional: false,
        });
        state.leader = Some(NodeId::new("queen"));
        state.partitions.push(TopologyPartition {
            id: String::from("partition_0"),
            nodes: vec![NodeId::new("queen"), NodeId::new("w1"), NodeId::new("w2")],
            leader: NodeId::new("queen"),
            replica_count: 1,
        });

        assert_eq!(state.node_count(), 3);
        assert!(!state.is_empty());
        assert_eq!(round_trip(&state), state);
    }

    #[test]
    fn empty_state_round_trips_for_every_kind() {
        for kind in [
            TopologyKind::Mesh,
            TopologyKind::Hierarchical,
            TopologyKind::Centralized,
            TopologyKind::Hybrid,
        ] {
            let state = TopologyState::new(kind);
            assert!(state.is_empty());
            assert_eq!(state.node_count(), 0);
            assert_eq!(state.leader, None);
            assert_eq!(round_trip(&state), state);
        }
    }
}
