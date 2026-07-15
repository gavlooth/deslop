use std::collections::{BTreeMap, VecDeque};

use deslop_parse::{
    GraphEvidenceLayer, ProgramDependenceGraphKey, ProgramDependenceNodeKey,
    ProgramDependenceProjection,
};

use crate::{GraphEntityRef, ImpactCone, ImpactConeQuery, ImpactDirection};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImpactQueryError {
    #[error("impact query maximum depth must be positive")]
    ZeroDepth,
    #[error("impact query references missing program-dependence graph {0}")]
    MissingGraph(String),
    #[error("impact query references missing program-dependence node {0}")]
    MissingNode(String),
}

pub fn program_dependence_impact_cone(
    projection: &ProgramDependenceProjection,
    graph_key: &ProgramDependenceGraphKey,
    root: &ProgramDependenceNodeKey,
    direction: ImpactDirection,
    maximum_depth: u32,
) -> Result<ImpactCone, ImpactQueryError> {
    if maximum_depth == 0 {
        return Err(ImpactQueryError::ZeroDepth);
    }
    let graph = projection
        .document()
        .graphs()
        .iter()
        .find(|graph| graph.key() == graph_key)
        .ok_or_else(|| ImpactQueryError::MissingGraph(graph_key.as_str().into()))?;
    if !graph.nodes().iter().any(|node| node.key() == root) {
        return Err(ImpactQueryError::MissingNode(root.as_str().into()));
    }

    let mut distances = BTreeMap::from([(root.clone(), 0_u32)]);
    let mut pending = VecDeque::from([root.clone()]);
    while let Some(current) = pending.pop_front() {
        let distance = distances[&current];
        if distance == maximum_depth {
            continue;
        }
        for adjacent in graph.edges().iter().filter_map(|edge| match direction {
            ImpactDirection::Incoming if edge.to() == &current => Some(edge.from()),
            ImpactDirection::Outgoing if edge.from() == &current => Some(edge.to()),
            ImpactDirection::Bidirectional if edge.to() == &current => Some(edge.from()),
            ImpactDirection::Bidirectional if edge.from() == &current => Some(edge.to()),
            _ => None,
        }) {
            if !distances.contains_key(adjacent) {
                distances.insert(adjacent.clone(), distance + 1);
                pending.push_back(adjacent.clone());
            }
        }
    }

    let entity = |node: &ProgramDependenceNodeKey| GraphEntityRef {
        layer: GraphEvidenceLayer::ProgramDependence,
        graph: graph.key().as_str().into(),
        entity: node.as_str().into(),
    };
    let root = entity(root);
    let entities = distances.keys().map(entity).collect::<Vec<_>>();
    let truncated = distances.iter().any(|(node, distance)| {
        *distance == maximum_depth
            && graph.edges().iter().any(|edge| match direction {
                ImpactDirection::Incoming => {
                    edge.to() == node && !distances.contains_key(edge.from())
                }
                ImpactDirection::Outgoing => {
                    edge.from() == node && !distances.contains_key(edge.to())
                }
                ImpactDirection::Bidirectional => {
                    (edge.to() == node && !distances.contains_key(edge.from()))
                        || (edge.from() == node && !distances.contains_key(edge.to()))
                }
            })
    });
    Ok(ImpactCone {
        query: ImpactConeQuery {
            roots: vec![root],
            direction,
            layers: vec![GraphEvidenceLayer::ProgramDependence],
            maximum_depth,
        },
        entities,
        truncated,
    })
}
