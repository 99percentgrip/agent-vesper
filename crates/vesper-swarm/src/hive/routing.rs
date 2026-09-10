//! Directed topology eligibility for dispatch; transport does not grant a route.
use crate::topology::{NodeId, NodeStatus, TopologyState};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub(super) fn reachable_from(state: &TopologyState, source: &NodeId) -> BTreeSet<NodeId> {
    let eligible = |id: &NodeId| {
        state
            .nodes
            .get(id)
            .is_some_and(|node| matches!(node.status, NodeStatus::Active | NodeStatus::Syncing))
    };
    let mut visited = BTreeSet::new();
    if !eligible(source) {
        return visited;
    }
    let mut adjacency: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for edge in &state.edges {
        if !edge.weight.is_finite()
            || edge.weight < 0.0
            || !eligible(&edge.from)
            || !eligible(&edge.to)
        {
            continue;
        }
        adjacency.entry(&edge.from).or_default().push(&edge.to);
        if edge.bidirectional {
            adjacency.entry(&edge.to).or_default().push(&edge.from);
        }
    }
    let mut queue = VecDeque::from([source.clone()]);
    visited.insert(source.clone());
    while let Some(node) = queue.pop_front() {
        if let Some(neighbors) = adjacency.get(&node) {
            for next in neighbors {
                if visited.insert((*next).clone()) {
                    queue.push_back((*next).clone());
                }
            }
        }
    }
    visited
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        manager::{NodeUpdate, TopologyManager},
        topology::{TopologyConfig, TopologyEdge, TopologyKind, TopologyRole},
    };
    #[test]
    fn directed_paths_require_live_intermediates_and_valid_edges() {
        let manager =
            TopologyManager::new(TopologyKind::Hierarchical, TopologyConfig::new(4)).unwrap();
        let mut state = manager.initial_state();
        let ids: Vec<_> = ["source", "relay", "target"]
            .into_iter()
            .map(NodeId::new)
            .collect();
        for id in &ids {
            manager
                .add_node(&mut state, id.clone(), TopologyRole::Worker)
                .unwrap();
            manager
                .update_node(
                    &mut state,
                    id,
                    NodeUpdate {
                        status: Some(NodeStatus::Active),
                        ..NodeUpdate::none()
                    },
                )
                .unwrap();
        }
        state.edges = vec![
            TopologyEdge {
                from: ids[0].clone(),
                to: ids[1].clone(),
                weight: 1.0,
                bidirectional: false,
            },
            TopologyEdge {
                from: ids[1].clone(),
                to: ids[2].clone(),
                weight: 1.0,
                bidirectional: false,
            },
        ];
        assert!(reachable_from(&state, &ids[0]).contains(&ids[2]));
        assert!(!reachable_from(&state, &ids[2]).contains(&ids[0]));
        state.nodes.get_mut(&ids[1]).unwrap().status = NodeStatus::Failed;
        assert!(!reachable_from(&state, &ids[0]).contains(&ids[2]));
        state.nodes.get_mut(&ids[1]).unwrap().status = NodeStatus::Active;
        state.edges[1].weight = f64::NAN;
        assert!(!reachable_from(&state, &ids[0]).contains(&ids[2]));
        state.edges[1].weight = 1.0;
        for edge in &mut state.edges {
            edge.bidirectional = true;
        }
        assert!(reachable_from(&state, &ids[2]).contains(&ids[0]));
        state.edges.clear();
        assert_eq!(reachable_from(&state, &ids[0]).len(), 1);
        assert!(reachable_from(&state, &NodeId::new("absent")).is_empty());
    }
}
