//! Nested placement: Ruby `Layout::Flowchart::DirectedPlacement` over
//! `LevelEdges`, `LevelFrame` and `ClusterReach`, then the collision-node
//! compaction and the flattening to canvas coordinates.
use super::place::{self, Edge as PEdge};
use super::render::PAGE_MARGIN;
use super::*;
use std::collections::HashSet;

const CLUSTER_PADDING: f64 = 20.0;
const CLUSTER_SIDE: f64 = 8.0;
const EXTERNAL_CLUSTER_SIDE: f64 = 35.0;
const NESTED_CLUSTER_SIDE: f64 = 37.5;
const CLUSTER_VERTICAL: f64 = 25.0;
const INDEPENDENT_CLUSTER_PADDING: f64 = 37.5;
const DEFAULT_NODE_SPACING: f64 = 50.0;
const RANK_STEP: f64 = 25.0;
const REACH: f64 = 46.0;
const COLLISION_PADDING: (f64, f64) = (32.0, 16.0);

/// A box in the nest; children sit in the cluster's own coordinates.
struct T {
    n: usize,
    kids: Vec<T>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

type Entry = (usize, Option<usize>, Option<usize>);

struct Plan<'a> {
    at: HashMap<Option<usize>, Vec<Entry>>,
    external: HashSet<usize>,
    edges: &'a [PEdge],
    /// Per model edge: does either end name a subgraph.
    to_cluster: Vec<bool>,
    nodes: &'a [Node],
    subs: &'a [Sub],
}

/// Where the cluster layout leaves the boxes: drawing order and, per edge
/// id, the boxes the edge is laid out between at its own level.
pub(super) struct Placed {
    pub(super) order: Vec<usize>,
    pub(super) level: HashMap<String, Option<(usize, usize)>>,
    /// Per node: its y inside the box that holds it.
    pub(super) rel_y: Vec<f64>,
}

fn build(model: &Model) -> Vec<T> {
    fn sub(model: &Model, j: usize) -> T {
        let s = &model.subs[j];
        let mut kids: Vec<T> = (0..model.subs.len()).filter(|&k| model.subs[k].parent == Some(j)).map(|k| sub(model, k)).collect();
        kids.extend(s.members.iter().map(|&n| leaf(model, n)));
        let n = &model.nodes[s.node];
        T { n: s.node, kids, x: 0.0, y: 0.0, w: n.w, h: n.h }
    }
    fn leaf(model: &Model, n: usize) -> T {
        T { n, kids: vec![], x: 0.0, y: 0.0, w: model.nodes[n].w, h: model.nodes[n].h }
    }
    let held: HashSet<usize> = model.subs.iter().flat_map(|s| s.members.iter().copied()).collect();
    let mut roots: Vec<T> = (0..model.nodes.len()).filter(|n| !model.nodes[*n].cluster && !held.contains(n)).map(|n| leaf(model, n)).collect();
    roots.extend((0..model.subs.len()).filter(|&j| model.subs[j].parent.is_none()).map(|j| sub(model, j)));
    roots
}

fn index_paths(boxes: &[T], ancestors: &[usize], paths: &mut HashMap<usize, Vec<usize>>) {
    for b in boxes {
        let mut path = ancestors.to_vec();
        path.push(b.n);
        paths.insert(b.n, path.clone());
        index_paths(&b.kids, &path, paths);
    }
}

fn level_edges(boxes: &[T], ends: &[(usize, usize)]) -> (HashMap<Option<usize>, Vec<Entry>>, HashSet<usize>) {
    let mut paths = HashMap::new();
    index_paths(boxes, &[], &mut paths);
    let mut at: HashMap<Option<usize>, Vec<Entry>> = HashMap::new();
    let mut external = HashSet::new();
    for (k, &(s, t)) in ends.iter().enumerate() {
        let (Some(sp), Some(tp)) = (paths.get(&s), paths.get(&t)) else { continue };
        let shared = sp.iter().zip(tp.iter()).take_while(|(a, b)| a == b).count();
        let container = if shared == 0 { None } else { Some(sp[shared - 1]) };
        at.entry(container).or_default().push((k, sp.get(shared).copied(), tp.get(shared).copied()));
        let (hs, ht) = (&sp[..sp.len() - 1], &tp[..tp.len() - 1]);
        external.extend(hs.iter().filter(|u| !ht.contains(u)).copied());
        external.extend(ht.iter().filter(|u| !hs.contains(u)).copied());
    }
    (at, external)
}

fn turned(dir: &'static str) -> &'static str {
    if dir == "DOWN" { "RIGHT" } else { "DOWN" }
}

fn first_index(boxes: &[T]) -> HashMap<usize, usize> {
    let mut first = HashMap::new();
    for (i, b) in boxes.iter().enumerate() {
        first.entry(b.n).or_insert(i);
    }
    first
}

fn place_level(boxes: &mut [T], container: Option<usize>, dir: &'static str, rank: f64, plan: &Plan) -> Result<(), Error> {
    for b in boxes.iter_mut() {
        if plan.nodes[b.n].cluster {
            place_cluster(b, dir, rank, container.is_none(), plan)?;
        }
    }
    if boxes.is_empty() {
        return Ok(());
    }
    let empty = vec![];
    let level = plan.at.get(&container).unwrap_or(&empty);
    let first = first_index(boxes);
    // dagre re-adds an edge that ends on a subgraph after the others
    let mut ordered: Vec<&Entry> = level.iter().collect();
    ordered.sort_by_key(|e| plan.to_cluster[e.0]);
    let flat: Vec<PEdge> = ordered
        .iter()
        .filter_map(|&&(k, from, to)| {
            let (src, tgt) = (*first.get(&from?)?, *first.get(&to?)?);
            Some(PEdge { src, tgt, span: plan.edges[k].span, labels: plan.edges[k].labels.clone() })
        })
        .collect();
    let sizes: Vec<(f64, f64)> = boxes.iter().map(|b| (b.w, b.h)).collect();
    let clusters: Vec<bool> = boxes.iter().map(|b| plan.nodes[b.n].cluster).collect();
    let leading: Vec<bool> = if container.is_some() { boxes.iter().map(|b| split_linked(b, plan)).collect() } else { vec![] };
    let sideways = matches!(dir, "LEFT" | "RIGHT");
    let pos = place::place_flat(&sizes, &flat, dir, rank, &clusters, &leading)?;
    for (b, (x, y)) in boxes.iter_mut().zip(pos) {
        b.x = x;
        b.y = y;
    }
    cluster_reach(boxes, level, sideways, plan);
    Ok(())
}

/// `ClusterReach`: a subgraph stretches along the rank to cover the boxes
/// with an edge into it.
fn cluster_reach(boxes: &mut [T], level: &[Entry], sideways: bool, plan: &Plan) {
    let first = first_index(boxes);
    let centre = |b: &T| if sideways { b.y + b.h / 2.0 } else { b.x + b.w / 2.0 };
    let mut wanted: Vec<(usize, Vec<f64>)> = vec![];
    let mut reach = |cluster: usize, other: usize, boxes: &[T]| {
        if cluster == other || !plan.nodes[boxes[cluster].n].cluster {
            return;
        }
        let c = centre(&boxes[other]);
        match wanted.iter_mut().find(|w| w.0 == cluster) {
            Some(w) => w.1.push(c),
            None => wanted.push((cluster, vec![c])),
        }
    };
    for &(_, from, to) in level {
        if let (Some(&f), Some(&t)) = (from.and_then(|x| first.get(&x)), to.and_then(|x| first.get(&x))) {
            reach(f, t, boxes);
            reach(t, f, boxes);
        }
    }
    for (i, centres) in wanted {
        let b = &mut boxes[i];
        let (pos, size) = if sideways { (b.y, b.h) } else { (b.x, b.w) };
        let low = pos.min(centres.iter().cloned().fold(f64::INFINITY, f64::min) - REACH);
        let high = (pos + size).max(centres.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + REACH);
        if low == pos && high == pos + size {
            continue;
        }
        let grown = pos - low;
        if sideways {
            b.y = low;
            b.h = high - low;
        } else {
            b.x = low;
            b.w = high - low;
        }
        for k in &mut b.kids {
            if sideways {
                k.y += grown;
            } else {
                k.x += grown;
            }
        }
    }
}

/// `split_linked_cluster?`: four or more boxes, all linked, in more than one component.
fn split_linked(c: &T, plan: &Plan) -> bool {
    if !plan.nodes[c.n].cluster {
        return false;
    }
    let empty = vec![];
    let level = plan.at.get(&Some(c.n)).unwrap_or(&empty);
    let linked: HashSet<usize> = level.iter().flat_map(|&(_, f, t)| [f, t]).flatten().collect();
    let ids: Vec<usize> = c.kids.iter().map(|k| k.n).collect();
    if ids.len() < 4 || ids.iter().any(|i| !linked.contains(i)) {
        return false;
    }
    let mut links: HashMap<usize, Vec<usize>> = ids.iter().map(|&i| (i, vec![])).collect();
    for &(_, f, t) in level {
        if let (Some(f), Some(t)) = (f, t) {
            if links.contains_key(&f) && links.contains_key(&t) {
                links.get_mut(&f).unwrap().push(t);
                links.get_mut(&t).unwrap().push(f);
            }
        }
    }
    let mut seen: HashSet<usize> = HashSet::new();
    let mut comps = 0;
    for &i in &ids {
        if seen.contains(&i) {
            continue;
        }
        comps += 1;
        let mut pending = vec![i];
        while let Some(n) = pending.pop() {
            if seen.insert(n) {
                pending.extend(links[&n].iter().copied());
            }
        }
    }
    comps > 1
}

fn sub_of<'a>(plan: &'a Plan, c: &T) -> &'a Sub {
    plan.subs.iter().find(|s| s.node == c.n).expect("cluster has a sub")
}

fn place_cluster(c: &mut T, dir: &'static str, rank: f64, top: bool, plan: &Plan) -> Result<(), Error> {
    let sub = sub_of(plan, c);
    let external = plan.external.contains(&c.n);
    let (inner_dir, inner_rank) = match sub.dir {
        Some(d) => (d, rank + RANK_STEP),
        None if external => (dir, rank),
        None => (turned(dir), rank + RANK_STEP),
    };
    place_level(&mut c.kids, Some(c.n), inner_dir, inner_rank, plan)?;
    let (side, vertical) = if split_linked(c, plan) && !top {
        (EXTERNAL_CLUSTER_SIDE, DEFAULT_NODE_SPACING)
    } else if external {
        (if top { EXTERNAL_CLUSTER_SIDE } else { NESTED_CLUSTER_SIDE }, CLUSTER_VERTICAL)
    } else if !top {
        (CLUSTER_SIDE, CLUSTER_VERTICAL)
    } else {
        let nested: Vec<&T> = c.kids.iter().filter(|k| plan.nodes[k.n].cluster).collect();
        if nested.is_empty() {
            (INDEPENDENT_CLUSTER_PADDING, INDEPENDENT_CLUSTER_PADDING)
        } else if nested.iter().any(|k| split_linked(k, plan)) {
            (INDEPENDENT_CLUSTER_PADDING, EXTERNAL_CLUSTER_SIDE)
        } else if nested.iter().all(|k| k.kids.len() == 1) {
            (INDEPENDENT_CLUSTER_PADDING, CLUSTER_VERTICAL)
        } else {
            (CLUSTER_SIDE, CLUSTER_VERTICAL)
        }
    };
    let left = c.kids.iter().map(|k| k.x).fold(f64::INFINITY, f64::min);
    let right = c.kids.iter().map(|k| k.x + k.w).fold(f64::NEG_INFINITY, f64::max);
    let inner = right - left;
    let titled = sub.label.0.trunc() + CLUSTER_PADDING * 2.0;
    c.w = (inner + side * 2.0).max(titled).ceil();
    let lh = sub.label.1.trunc();
    let header = if lh < vertical { vertical } else { lh + PAGE_MARGIN };
    let top_of = c.kids.iter().map(|k| k.y).fold(f64::INFINITY, f64::min);
    let (dx, dy) = ((c.w - inner) / 2.0 - left, header - top_of);
    for k in &mut c.kids {
        k.x += dx;
        k.y += dy;
    }
    let bottom = c.kids.iter().map(|k| k.y + k.h).fold(f64::NEG_INFINITY, f64::max);
    c.h = bottom + vertical;
    Ok(())
}

/// `compact_collision_nodes`: a node that shares its id with an empty
/// subgraph keeps the label box of the placeholder, centred where it was.
fn compact(boxes: &mut [T], nodes: &[Node]) {
    for b in boxes.iter_mut() {
        compact(&mut b.kids, nodes);
        let n = &nodes[b.n];
        if n.collision {
            let (lw, lh) = layout::label_dims(&n.lines, &n.icons);
            let (w, h) = (lw + COLLISION_PADDING.0, lh + COLLISION_PADDING.1);
            b.x += (b.w - w) / 2.0;
            b.y += (b.h - h) / 2.0;
            b.w = w;
            b.h = h;
        }
    }
}

fn flatten(boxes: &[T], origin: (f64, f64), nodes: &mut [Node], order: &mut Vec<usize>, rel: &mut [f64]) {
    for b in boxes {
        let (x, y) = (b.x + origin.0, b.y + origin.1);
        let n = &mut nodes[b.n];
        n.x = x;
        n.y = y;
        n.w = b.w;
        n.h = b.h;
        order.push(b.n);
        rel[b.n] = b.y;
        flatten(&b.kids, (x, y), nodes, order, rel);
    }
}

/// Places every node and subgraph box; page margin included.
pub(super) fn place_all(model: &mut Model) -> Result<Placed, Error> {
    let mut roots = build(model);
    let ends: Vec<(usize, usize)> = model.edges.iter().map(|e| (e.src, e.tgt)).collect();
    let (at, external) = level_edges(&roots, &ends);
    let pedges: Vec<PEdge> = model.edges.iter().map(|e| PEdge { src: e.src, tgt: e.tgt, span: e.span, labels: e.label.as_ref().map(|_| vec![e.label_dim]).unwrap_or_default() }).collect();
    let to_cluster: Vec<bool> = model.edges.iter().map(|e| model.nodes[e.src].cluster || model.nodes[e.tgt].cluster).collect();
    let mut level = HashMap::new();
    let mut containers: Vec<Option<usize>> = vec![None];
    containers.extend(model.subs.iter().map(|s| Some(s.node)));
    for c in containers {
        for &(k, f, t) in at.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            let e = &model.edges[k];
            let id = format!("{}_to_{}", model.nodes[e.src].id, model.nodes[e.tgt].id);
            level.insert(id, f.zip(t));
        }
    }
    {
        let plan = Plan { at, external, edges: &pedges, to_cluster, nodes: &model.nodes, subs: &model.subs };
        let dir: &'static str = model.dir;
        place_level(&mut roots, None, dir, 50.0, &plan)?;
    }
    let left = roots.iter().map(|b| b.x).fold(f64::INFINITY, f64::min);
    let top = roots.iter().map(|b| b.y).fold(f64::INFINITY, f64::min);
    for b in roots.iter_mut() {
        b.x += PAGE_MARGIN - left;
        b.y += PAGE_MARGIN - top;
    }
    compact(&mut roots, &model.nodes);
    let mut order = vec![];
    let mut rel_y = vec![0.0; model.nodes.len()];
    flatten(&roots, (0.0, 0.0), &mut model.nodes, &mut order, &mut rel_y);
    Ok(Placed { order, level, rel_y })
}
