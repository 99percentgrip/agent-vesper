//! Partition leadership is eligibility- and failover-policy constrained.
use vesper_swarm::manager::{NodeUpdate, TopologyManager};
use vesper_swarm::topology::{
    NodeId, NodeStatus, PartitionStrategy, TopologyConfig, TopologyKind, TopologyRole,
    TopologyState,
};

fn status(manager: &TopologyManager, state: &mut TopologyState, name: &str, value: NodeStatus) {
    manager
        .update_node(
            state,
            &NodeId::new(name),
            NodeUpdate {
                status: Some(value),
                ..NodeUpdate::none()
            },
        )
        .unwrap();
}

#[test]
fn partitions_never_elect_initializing_or_failed_members() {
    let manager = TopologyManager::new(TopologyKind::Hybrid, TopologyConfig::default()).unwrap();
    let mut state = manager.initial_state();
    for name in ["a", "b"] {
        manager
            .add_node(&mut state, NodeId::new(name), TopologyRole::Worker)
            .unwrap();
    }
    manager.rebalance(&mut state).unwrap();
    assert!(state.partitions.is_empty());
    status(&manager, &mut state, "a", NodeStatus::Failed);
    status(&manager, &mut state, "b", NodeStatus::Syncing);
    manager.rebalance(&mut state).unwrap();
    assert_eq!(state.partitions[0].leader, NodeId::new("b"));
}

#[test]
fn partition_loss_respects_policy_across_rebuild_and_serialization() {
    for automatic in [false, true] {
        for failover in [false, true] {
            for remove in [false, true] {
                let manager = TopologyManager::new(
                    TopologyKind::Hybrid,
                    TopologyConfig {
                        auto_rebalance: automatic,
                        failover_enabled: failover,
                        partition_strategy: PartitionStrategy::Range,
                        nodes_per_partition: 3,
                        replication_factor: 3,
                        ..TopologyConfig::default()
                    },
                )
                .unwrap();
                let mut state = manager.initial_state();
                // Admission order differs from range ordering. Oldest eligible wins.
                for name in ["c", "b", "a"] {
                    manager
                        .add_node(&mut state, NodeId::new(name), TopologyRole::Worker)
                        .unwrap();
                    status(&manager, &mut state, name, NodeStatus::Active);
                }
                manager.rebalance(&mut state).unwrap();
                assert_eq!(state.partitions[0].leader, NodeId::new("c"));
                if remove {
                    manager.remove_node(&mut state, &NodeId::new("c")).unwrap();
                } else {
                    status(&manager, &mut state, "c", NodeStatus::Failed);
                }
                if !failover {
                    assert!(state.partitions.is_empty());
                }
                for _ in 0..3 {
                    state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
                    manager.rebalance(&mut state).unwrap();
                    if failover {
                        assert_eq!(state.partitions[0].leader, NodeId::new("b"));
                        assert!(
                            state.partitions[0].replica_count as usize
                                <= state.partitions[0].nodes.len()
                        );
                    } else {
                        assert!(
                            state.partitions.is_empty(),
                            "rebalance cannot undo disabled failover"
                        );
                    }
                }
                manager.elect_leader(&mut state).unwrap();
                manager.rebalance(&mut state).unwrap();
                assert_eq!(state.partitions[0].leader, NodeId::new("b"));
            }
        }
    }
}
