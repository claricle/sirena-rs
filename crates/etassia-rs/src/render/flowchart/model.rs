//! Flowchart model types.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Shape {
    Rect,
    Rounded,
    Stadium,
    Circle,
    Rhombus,
    Hexagon,
    DoubleCircle,
    Asymmetric,
    Skewed,
}

pub(super) struct Node {
    pub(super) id: String,
    pub(super) lines: Vec<String>,
    pub(super) icons: Vec<usize>,
    pub(super) shape: Shape,
    pub(super) fill: Option<String>,
    pub(super) stroke: Option<String>,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) w: f64,
    pub(super) h: f64,
    /// A subgraph box (drawn behind the edges, titled `lines[0]`).
    pub(super) cluster: bool,
    /// A node whose id a subgraph with nothing in it also names: shrunk to a label box.
    pub(super) collision: bool,
}

pub(super) struct Edge {
    pub(super) src: usize,
    pub(super) tgt: usize,
    pub(super) kind: String,
    pub(super) span: usize,
    pub(super) label: Option<String>,
    pub(super) label_dim: (f64, f64),
}

/// A drawn subgraph: `node` is its box in `Model::nodes`.
pub(super) struct Sub {
    pub(super) node: usize,
    pub(super) parent: Option<usize>,
    pub(super) dir: Option<&'static str>,
    pub(super) members: Vec<usize>,
    pub(super) label: (f64, f64),
}

pub(super) struct Model {
    pub(super) subs: Vec<Sub>,
    pub(super) dir: &'static str,
    pub(super) nodes: Vec<Node>,
    pub(super) edges: Vec<Edge>,
}
