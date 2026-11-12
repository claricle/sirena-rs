//! Entry point (elkrb `Layout::LayoutEngine.layout`): selects the algorithm
//! from the graph's options and runs it.
use super::layered;
use super::model::Graph;
use crate::Error;

pub fn layout(graph: &mut Graph) -> Result<(), Error> {
    match graph.options.algorithm.as_str() {
        "layered" => layered::run(graph).map(|_| ()),
        other => Err(Error::Unported(format!("elk algorithm {other}"))),
    }
}

/// Placement only (no edge routing): node positions and graph size.
pub fn layout_placement(graph: &mut Graph) -> Result<(), Error> {
    match graph.options.algorithm.as_str() {
        "layered" => layered::run_placement(graph).map(|_| ()),
        other => Err(Error::Unported(format!("elk algorithm {other}"))),
    }
}
