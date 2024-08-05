use std::{collections::BTreeMap, fmt::Debug};

use crate::{GraphView, RelRc};

use petgraph::algo::toposort;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
struct SerializeNodeId(usize);

#[derive(Serialize, Deserialize)]
struct SerializeNodeData<N, E> {
    value: N,
    incoming: Vec<SerializeEdgeData<E>>,
}

#[derive(Serialize, Deserialize)]
struct SerializeEdgeData<E> {
    source: SerializeNodeId,
    value: E,
}

#[derive(Serialize, Deserialize)]
struct GraphViewSerializer<N, E> {
    sources: Vec<SerializeNodeId>,
    sinks: Vec<SerializeNodeId>,
    all_nodes: Vec<SerializeNodeData<N, E>>,
}

impl<N: Clone, E: Clone> From<&GraphView<N, E>> for GraphViewSerializer<N, E> {
    fn from(graph: &GraphView<N, E>) -> Self {
        let mut node_id_map = BTreeMap::new();

        let mut all_nodes = Vec::new();

        // Fill all_nodes in topological order
        for node_id in toposort(&graph, None).unwrap() {
            if node_id_map.contains_key(&node_id) {
                continue;
            }
            let node = graph.get_node(node_id);
            // Start with empty `incoming`, add once all nodes have been added
            let data = SerializeNodeData {
                value: node.value().clone(),
                incoming: Vec::new(),
            };
            let ser_id = SerializeNodeId(all_nodes.len());
            all_nodes.push(data);
            node_id_map.insert(node_id, ser_id);
        }

        // Add incoming edges
        for (&node_id, &ser_id) in &node_id_map {
            let node_mut = &mut all_nodes[ser_id.0];
            let node = graph.get_node(node_id);
            node_mut.incoming = node
                .all_incoming()
                .iter()
                .map(|e| {
                    let source = node_id_map[&e.source().into()];
                    SerializeEdgeData {
                        source,
                        value: e.value().clone(),
                    }
                })
                .collect();
        }

        // Add sources and sinks
        let sources = graph.sources().iter().map(|n| node_id_map[n]).collect();
        let sinks = graph
            .sinks()
            .iter()
            .map(|n| node_id_map[&n.into()])
            .collect();

        Self {
            sources,
            sinks,
            all_nodes,
        }
    }
}

#[derive(Error, Debug)]
pub enum GraphDeserializationError {
    #[error("Invalid graph: unknown parent (nodes must be in topological order)")]
    InvalidTopologicalOrder,
}

impl<N, E> TryFrom<GraphViewSerializer<N, E>> for GraphView<N, E> {
    type Error = GraphDeserializationError;

    fn try_from(ser_graph: GraphViewSerializer<N, E>) -> Result<Self, Self::Error> {
        let mut nodes: Vec<RelRc<N, E>> = Vec::new();
        for ser_node in ser_graph.all_nodes {
            let SerializeNodeData { value, incoming } = ser_node;
            if incoming.iter().any(|e| e.source.0 >= nodes.len()) {
                return Err(GraphDeserializationError::InvalidTopologicalOrder);
            }
            let parents = incoming
                .into_iter()
                .map(|ser_edge| (nodes[ser_edge.source.0].clone(), ser_edge.value));
            let node = RelRc::with_parents(value, parents);
            nodes.push(node);
        }
        let sinks = ser_graph
            .sinks
            .into_iter()
            .map(|id| nodes[id.0].clone())
            .collect();
        let sources = ser_graph
            .sources
            .into_iter()
            .map(|id| (&nodes[id.0]).into())
            .collect();
        let all_nodes = nodes.into_iter().map(|n| (&n).into()).collect();

        Ok(GraphView {
            sources,
            sinks,
            all_nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::graph_view::RelRc;

    use insta::assert_snapshot;
    use itertools::Itertools;
    use rstest::{fixture, rstest};

    #[fixture]
    fn sample_graph() -> Vec<RelRc<String, u32>> {
        let root = RelRc::new("Root".to_string());
        let child1 = RelRc::with_parents("Child 1".to_string(), vec![(root.clone(), 1)]);
        let child2 = RelRc::with_parents("Child 2".to_string(), vec![(root.clone(), 2)]);
        let grandchild = RelRc::with_parents(
            "Grandchild".to_string(),
            vec![(child1.clone(), 3), (child2.clone(), 4)],
        );

        vec![grandchild]
    }

    #[rstest]
    fn test_serialization(sample_graph: Vec<RelRc<String, u32>>) {
        let graph = GraphView::from_sinks(sample_graph);
        let serialized = GraphViewSerializer::from(&graph);

        let json = serde_json::to_string_pretty(&serialized).unwrap();
        assert_snapshot!(json);
    }

    #[rstest]
    fn test_roundtrip(sample_graph: Vec<RelRc<String, u32>>) {
        let original_graph = GraphView::from_sinks(sample_graph);
        let serialized = GraphViewSerializer::from(&original_graph);
        let deserialized_graph = GraphView::try_from(serialized).unwrap();

        let (root, child1, child2, grandchild) = toposort(&deserialized_graph, None)
            .unwrap()
            .into_iter()
            .collect_tuple()
            .unwrap();
        assert_eq!(deserialized_graph.get_node(root).value(), "Root");
        let children_values = BTreeSet::from([
            deserialized_graph.get_node(child1).value().as_str(),
            deserialized_graph.get_node(child2).value().as_str(),
        ]);
        assert_eq!(children_values, BTreeSet::from(["Child 1", "Child 2"]));
        assert_eq!(
            deserialized_graph.get_node(grandchild).value(),
            original_graph.get_node(grandchild).value()
        );
    }
}