//! Phase 2: longest-path layer assignment and long-edge dummy slots (elkrb
//! `Layered::LayerAssigner`, `DummySlot`). Layer constraints
//! (`layerConstraint`, `constraints.layer`) are not part of the supported
//! option set, so `LayerConstraints#apply` is a no-op here.
use crate::elk::model::{Graph, NodeIndex};

/// Internal layer position reserved for one segment of a long edge.
#[derive(Debug, Clone)]
pub struct DummySlot {
    pub edge: usize,
    pub layer_index: usize,
    pub edge_order: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DummySlot {
    pub fn id(&self) -> String {
        format!("__elkrb_dummy_{}_{}", self.edge_order, self.layer_index)
    }
}

/// A layer entry. Layers hold indices into `Layering::items`; a node's item
/// index equals its index in `Graph::children`, dummies follow.
#[derive(Debug, Clone)]
pub enum Item {
    Node(usize),
    Dummy(DummySlot),
}

#[derive(Debug, Clone)]
pub struct Layering {
    pub items: Vec<Item>,
    pub layers: Vec<Vec<usize>>,
    /// Layer of each child.
    pub node_layer: Vec<usize>,
}

/// The (source, target) child indices of an edge, read in the orientation
/// the cycle breaker chose.
fn oriented(graph: &Graph, index: &NodeIndex, edge: usize, reversed: &[bool]) -> Option<(usize, usize)> {
    let e = &graph.edges[edge];
    let s = index.owner(&e.sources[0])?;
    let t = index.owner(&e.targets[0])?;
    Some(if reversed[edge] { (t, s) } else { (s, t) })
}

pub fn assign_layers(graph: &Graph, index: &NodeIndex, reversed: &[bool]) -> Layering {
    let n = graph.children.len();
    let mut predecessors: Vec<Vec<usize>> = vec![vec![]; n];
    for edge in 0..graph.edges.len() {
        if let Some((s, t)) = oriented(graph, index, edge, reversed) {
            if s != t {
                predecessors[t].push(s);
            }
        }
    }

    // Iterative post-order walk of each node's predecessors; the order
    // nodes get their layer in is the order they enter their layer.
    let mut layer_of: Vec<Option<usize>> = vec![None; n];
    let mut assigned: Vec<usize> = vec![];
    let mut active = vec![false; n];
    for root in 0..n {
        if layer_of[root].is_some() {
            continue;
        }
        active[root] = true;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(current, next)) = stack.last() {
            let incoming = &predecessors[current];
            if next >= incoming.len() {
                let layer = incoming.iter().filter_map(|&p| layer_of[p]).max().map_or(0, |m| m + 1);
                layer_of[current] = Some(layer);
                assigned.push(current);
                stack.pop();
                continue;
            }
            let p = incoming[next];
            stack.last_mut().unwrap().1 = next + 1;
            if layer_of[p].is_some() || active[p] {
                // assigned already, or (defensive) a cycle left unbroken
                continue;
            }
            active[p] = true;
            stack.push((p, 0));
        }
    }

    let node_layer: Vec<usize> = layer_of.iter().map(|l| l.unwrap()).collect();
    let max_layer = node_layer.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for &node in &assigned {
        layers[node_layer[node]].push(node);
    }
    let mut items: Vec<Item> = (0..n).map(Item::Node).collect();

    for edge in 0..graph.edges.len() {
        let Some((s, t)) = oriented(graph, index, edge, reversed) else { continue };
        let (sl, tl) = (node_layer[s], node_layer[t]);
        if sl.abs_diff(tl) <= 1 {
            continue;
        }
        let (low, high) = (sl.min(tl), sl.max(tl));
        for layer in (low + 1)..high {
            items.push(Item::Dummy(DummySlot {
                edge,
                layer_index: layer,
                edge_order: edge,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }));
            layers[layer].push(items.len() - 1);
        }
    }
    Layering { items, layers, node_layer }
}
