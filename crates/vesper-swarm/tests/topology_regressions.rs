//! Desired-behavior failover regressions.
use vesper_swarm::manager::{NodeUpdate, TopologyManager};
use vesper_swarm::topology::{NodeId, NodeStatus, TopologyConfig, TopologyKind, TopologyRole};

#[test]
fn disabled_failover_stays_vacant_across_rebalances() {
    let manager = TopologyManager::new(
        TopologyKind::Mesh,
        TopologyConfig {
            auto_rebalance: true,
            failover_enabled: false,
            ..TopologyConfig::default()
        },
    )
    .unwrap();
    let mut state = manager.initial_state();
    for name in ["a", "b"] {
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
    manager.remove_node(&mut state, &NodeId::new("a")).unwrap();
    for _ in 0..3 {
        manager.rebalance(&mut state).unwrap();
        assert_eq!(state.leader, None);
    }
    assert_eq!(manager.elect_leader(&mut state).unwrap(), NodeId::new("b"));
}

#[test]
fn failover_skips_failed_workers() {
    let manager = TopologyManager::new(
        TopologyKind::Mesh,
        TopologyConfig {
            failover_enabled: true,
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
                    status: Some(if name == "b" {
                        NodeStatus::Failed
                    } else {
                        NodeStatus::Active
                    }),
                    ..NodeUpdate::none()
                },
            )
            .unwrap();
    }
    manager.elect_leader(&mut state).unwrap();
    manager.remove_node(&mut state, &NodeId::new("a")).unwrap();
    assert_eq!(state.leader, Some(NodeId::new("c")));
}

#[test]
fn inconsistent_join_ledger_refusal_does_not_mutate_node() {
    let manager = TopologyManager::new(TopologyKind::Mesh, TopologyConfig::default()).unwrap();
    let mut state = manager.initial_state();
    let id = NodeId::new("queen");
    manager
        .add_node(&mut state, id.clone(), TopologyRole::Queen)
        .unwrap();
    state.join_order.clear();
    let before = state.clone();
    assert!(manager.remove_node(&mut state, &id).is_err());
    assert_eq!(before, state);
}
