//! Centralized wiring must follow reconciled leadership, never a failed hub.
use vesper_swarm::manager::{NodeUpdate, TopologyManager};
use vesper_swarm::topology::{NodeId, NodeStatus, TopologyConfig, TopologyKind, TopologyRole};

#[test]
fn centralized_rebalance_uses_successor_or_leaves_vacant_hub() {
    for (failover, automatic) in [(false, false), (false, true), (true, false), (true, true)] {
        let manager = TopologyManager::new(
            TopologyKind::Centralized,
            TopologyConfig {
                failover_enabled: failover,
                auto_rebalance: automatic,
                ..TopologyConfig::default()
            },
        )
        .unwrap();
        let mut state = manager.initial_state();
        for name in ["a", "b", "c"] {
            let id = NodeId::new(name);
            manager
                .add_node(&mut state, id.clone(), TopologyRole::Worker)
                .unwrap();
            manager
                .update_node(
                    &mut state,
                    &id,
                    NodeUpdate {
                        status: Some(NodeStatus::Active),
                        ..NodeUpdate::none()
                    },
                )
                .unwrap();
        }
        manager.elect_leader(&mut state).unwrap();
        manager.rebalance(&mut state).unwrap();
        manager
            .update_node(
                &mut state,
                &NodeId::new("a"),
                NodeUpdate {
                    status: Some(NodeStatus::Failed),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
        if !automatic {
            manager.rebalance(&mut state).unwrap();
        }
        if failover {
            assert_eq!(state.leader, Some(NodeId::new("b")));
            assert_eq!(state.edges.len(), 2);
            assert!(state.edges.iter().all(|edge| edge.from == NodeId::new("b")));
        } else {
            assert_eq!(state.leader, None);
            assert!(
                state.edges.is_empty(),
                "vacancy must not fall back to a failed hub"
            );
        }
        let once = state.clone();
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state, once);
    }
}
