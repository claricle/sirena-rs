//! Phase 1: find the back edges without changing the graph (elkrb
//! `Layered::CycleBreaker`). The result only tells layering which edges to
//! read reversed.
use crate::elk::model::{Graph, NodeIndex};

#[derive(Clone, Copy, PartialEq)]
enum Color {
    Active,
    Complete,
}

/// `result[i]` is true when `graph.edges[i]` is a back edge.
pub fn break_cycles(graph: &Graph, index: &NodeIndex) -> Vec<bool> {
    let n = graph.children.len();
    let mut reversed = vec![false; graph.edges.len()];
    // adjacency[source] = [(target, edge)], in edge order; self-loops skipped.
    let mut adjacency: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for (i, edge) in graph.edges.iter().enumerate() {
        let s = index.owner(&edge.sources[0]);
        let t = index.owner(&edge.targets[0]);
        if let (Some(s), Some(t)) = (s, t) {
            if s != t {
                adjacency[s].push((t, i));
            }
        }
    }
    let mut colors: Vec<Option<Color>> = vec![None; n];
    for root in 0..n {
        if colors[root].is_some() {
            continue;
        }
        colors[root] = Some(Color::Active);
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(current, next)) = stack.last() {
            if next >= adjacency[current].len() {
                colors[current] = Some(Color::Complete);
                stack.pop();
                continue;
            }
            let (target, edge) = adjacency[current][next];
            stack.last_mut().unwrap().1 = next + 1;
            match colors[target] {
                Some(Color::Active) => reversed[edge] = true,
                None => {
                    colors[target] = Some(Color::Active);
                    stack.push((target, 0));
                }
                Some(Color::Complete) => {}
            }
        }
    }
    reversed
}
