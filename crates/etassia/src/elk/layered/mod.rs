//! The layered (Sugiyama) pipeline (elkrb `Algorithms::LayeredAlgorithm`):
//! 1. cycle breaking   -> `cycle`      (back edges, graph untouched)
//! 2. layer assignment -> `layering`   (layers and long-edge dummy slots)
//! 3. crossing min.    -> `crossing`   (layer order and port lists)
//! 4. node placement   -> `placement`  (hook, elk-place card)
//! 5. edge routing     -> `routing`    (hook, elk-route card)
use super::model::{Graph, NodeIndex};
use crate::Error;

pub mod crossing;
pub mod cycle;
pub mod layering;
pub mod placement;
pub mod routing;

/// What the front half hands to placement and routing.
#[derive(Debug, Clone)]
pub struct LayeredState {
    pub index: NodeIndex,
    /// `reversed[i]` is true when `graph.edges[i]` is a back edge.
    pub reversed: Vec<bool>,
    /// Layer items (nodes, then long-edge dummies) and the layers holding
    /// them in their final, minimized order.
    pub layering: layering::Layering,
    pub port_order: crossing::PortOrder,
}

/// Lays out one flat level: `graph.children` get x/y, `graph` its size,
/// `graph.edges` their sections.
pub fn run(graph: &mut Graph) -> Result<LayeredState, Error> {
    let state = run_placement(graph)?;
    routing::run(graph, &state)?;
    Ok(state)
}

/// Phases 1-4: everything but edge routing (node x/y and graph size set).
pub fn run_placement(graph: &mut Graph) -> Result<LayeredState, Error> {
    let index = NodeIndex::build(graph)?;
    if graph.children.is_empty() {
        return Err(Error::Unported("elk empty graph".into()));
    }
    if graph.options.separate_components {
        return Err(Error::Unported("elk separateConnectedComponents".into()));
    }
    let reversed = cycle::break_cycles(graph, &index);
    let mut layering = layering::assign_layers(graph, &index, &reversed);
    let port_order = crossing::minimize(graph, &index, &mut layering)?;
    let mut state = LayeredState { index, reversed, layering, port_order };
    placement::run(graph, &mut state)?;
    Ok(state)
}
