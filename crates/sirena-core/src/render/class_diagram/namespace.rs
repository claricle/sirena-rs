//! Namespace boxes of a class diagram: port of Ruby `ClassNamespaces.collect`
//! (claiming), `ClassNamespaceNodes` (nest / flatten) and
//! `ClassNamespacePlacement` over `Flowchart::DirectedPlacement`
//! (+ `LevelEdges`, `LevelFrame`, `ClusterReach`).
//!
//! Declined (`Error::Unported`): a subgraph whose linked components Mermaid
//! keeps in declaration order (`split_linked_cluster?`).
use super::place::{self, Edge};
use crate::Error;
use std::collections::{HashMap, HashSet};

const PAGE_MARGIN: f64 = 8.0;
const CLUSTER_PADDING: f64 = 20.0;
const CLUSTER_SIDE: f64 = 8.0;
const EXTERNAL_CLUSTER_SIDE: f64 = 35.0;
const NESTED_CLUSTER_SIDE: f64 = 37.5;
const CLUSTER_VERTICAL: f64 = 25.0;
const INDEPENDENT_CLUSTER_PADDING: f64 = 37.5;
const RANK_STEP: f64 = 25.0;
const REACH: f64 = 46.0;

/// A box in the nested layout: a class, a namespace cluster or an empty
/// namespace leaf. Children sit in the cluster's own coordinates.
pub struct Nd {
    pub uid: usize,
    /// Class index for a class box.
    pub ent: Option<usize>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub cluster: bool,
    /// Ruby holds a fitted cluster width as an Integer (`ceil`), which
    /// makes the title's centre an integer division.
    pub w_int: bool,
    /// The namespace id (`namespace:Name`) and drawn title.
    pub name: String,
    pub title: Option<String>,
    pub label: (f64, f64),
    pub kids: Vec<Nd>,
}

/// A positioned namespace box in canvas coordinates.
pub struct NsBox {
    pub id: String,
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub title_x: f64,
}

/// `ClassNamespaces.claim_last`: a class belongs to the last namespace
/// that lists it. `blocks` are the source blocks (name, ids named).
pub fn collect(blocks: &[(String, Vec<String>)], known: &[String]) -> Vec<(String, Vec<String>)> {
    let mut order: Vec<String> = vec![];
    let mut ids: HashMap<String, Vec<String>> = HashMap::new();
    for (name, body) in blocks {
        if !ids.contains_key(name) {
            order.push(name.clone());
        }
        let e = ids.entry(name.clone()).or_default();
        for id in body {
            if known.contains(id) && !e.contains(id) {
                e.push(id.clone());
            }
        }
    }
    let mut claimed: HashSet<String> = HashSet::new();
    let mut out: Vec<(String, Vec<String>)> = order.iter().map(|n| (n.clone(), ids[n].clone())).collect();
    for item in out.iter_mut().rev() {
        item.1.retain(|id| !claimed.contains(id));
        claimed.extend(item.1.iter().cloned());
    }
    out
}

/// `hierarchical_namespaces?`: a relationship joins two prefix-related
/// namespaces.
pub fn hierarchical(namespaces: &[(String, Vec<String>)], rels: &[(&str, &str)]) -> bool {
    let mut owners: HashMap<&str, &str> = HashMap::new();
    for (name, ids) in namespaces {
        for id in ids {
            owners.insert(id.as_str(), name.as_str());
        }
    }
    rels.iter().any(|(a, b)| match (owners.get(a), owners.get(b)) {
        (Some(f), Some(t)) => f != t && (f.starts_with(&format!("{t}.")) || t.starts_with(&format!("{f}."))),
        _ => false,
    })
}

struct Builder<'a> {
    next: usize,
    size: &'a dyn Fn(&str) -> (f64, f64),
}

impl Builder<'_> {
    fn uid(&mut self) -> usize {
        self.next += 1;
        self.next
    }

    /// An empty namespace: mmdc pads the title 32px each side in a 56px box.
    fn leaf(&mut self, name: &str, title: &str) -> Nd {
        let label = (self.size)(title);
        Nd {
            uid: self.uid(),
            ent: None,
            x: 0.0,
            y: 0.0,
            w: label.0 + 64.0,
            h: 56.0,
            cluster: false,
            w_int: false,
            name: format!("namespace:{name}"),
            title: Some(title.to_string()),
            label,
            kids: vec![],
        }
    }

    fn cluster(&mut self, name: &str, title: &str, kids: Vec<Nd>) -> Nd {
        let label = (self.size)(title);
        Nd {
            uid: self.uid(),
            ent: None,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            cluster: true,
            w_int: false,
            name: format!("namespace:{name}"),
            title: Some(title.to_string()),
            label,
            kids,
        }
    }
}

fn title_for(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |p| p.1)
}

fn parent_name(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |p| p.0)
}

/// `ClassNamespaceNodes#nest`. `classes` are (entity index, id, w, h) in
/// entity order; the result is the top-level boxes.
pub fn nest(
    classes: &[(usize, String, f64, f64)],
    namespaces: &[(String, Vec<String>)],
    hierarchical: bool,
    size: &dyn Fn(&str) -> (f64, f64),
) -> Vec<Nd> {
    let mut b = Builder { next: 0, size };
    let mut leaves: Vec<Option<Nd>> = classes
        .iter()
        .map(|(ent, _, w, h)| {
            let uid = b.uid();
            Some(Nd { uid, ent: Some(*ent), x: 0.0, y: 0.0, w: *w, h: *h, cluster: false, w_int: false, name: String::new(), title: None, label: (0.0, 0.0), kids: vec![] })
        })
        .collect();
    let ids: Vec<&str> = classes.iter().map(|c| c.1.as_str()).collect();
    let unclaimed = |class_ids: &[String], owners: &HashMap<usize, usize>| -> Vec<usize> {
        (0..ids.len()).filter(|&i| class_ids.iter().any(|c| c == ids[i]) && !owners.contains_key(&i)).collect()
    };
    let mut out: Vec<Nd> = vec![];
    if !hierarchical {
        // flat_nest: one cluster per namespace, at its first member
        let mut owners: HashMap<usize, usize> = HashMap::new();
        let mut made: Vec<(usize, Vec<usize>)> = vec![];
        for (k, (_, class_ids)) in namespaces.iter().enumerate() {
            let members = unclaimed(class_ids, &owners);
            if members.is_empty() {
                continue;
            }
            for &m in &members {
                owners.insert(m, k);
            }
            made.push((k, members));
        }
        let mut clusters: HashMap<usize, Nd> = HashMap::new();
        for (k, members) in made {
            let kids: Vec<Nd> = members.iter().map(|&m| leaves[m].take().unwrap()).collect();
            clusters.insert(k, b.cluster(&namespaces[k].0, &namespaces[k].0, kids));
        }
        let mut seen: HashSet<usize> = HashSet::new();
        // members were moved into clusters: walk entity order
        for i in 0..ids.len() {
            match owners.get(&i) {
                Some(&k) => {
                    if seen.insert(k) {
                        out.push(clusters.remove(&k).unwrap());
                    }
                }
                None => out.push(leaves[i].take().unwrap()),
            }
        }
        for (k, (name, _)) in namespaces.iter().enumerate() {
            if !owners.values().any(|&o| o == k) {
                out.push(b.leaf(name, name));
            }
        }
        return out;
    }
    // hierarchical: nested by dotted prefix, titled by the last part
    let mut names: Vec<String> = vec![];
    let mut specs: HashMap<String, Vec<String>> = HashMap::new();
    for (name, class_ids) in namespaces {
        let parts: Vec<&str> = name.split('.').collect();
        for i in 0..parts.len() {
            let prefix = parts[..=i].join(".");
            if !specs.contains_key(&prefix) {
                names.push(prefix.clone());
                specs.insert(prefix, vec![]);
            }
        }
        specs.get_mut(name).unwrap().extend(class_ids.iter().cloned());
    }
    let mut owners: HashMap<usize, usize> = HashMap::new(); // class -> index in names
    let mut members: Vec<Vec<usize>> = vec![];
    for (k, name) in names.iter().enumerate() {
        let m = unclaimed(&specs[name], &owners);
        for &i in &m {
            owners.insert(i, k);
        }
        members.push(m);
    }
    let index: HashMap<&str, usize> = names.iter().enumerate().map(|(k, n)| (n.as_str(), k)).collect();
    let mut child_names: Vec<Vec<usize>> = vec![vec![]; names.len()];
    for (k, name) in names.iter().enumerate() {
        if let Some(&p) = index.get(parent_name(name)) {
            child_names[p].push(k);
        }
    }
    let root_of = |mut k: usize| {
        while let Some(&p) = index.get(parent_name(&names[k])) {
            k = p;
        }
        k
    };
    fn build(k: usize, names: &[String], members: &[Vec<usize>], child_names: &[Vec<usize>], leaves: &mut [Option<Nd>], b: &mut Builder) -> Nd {
        let title = title_for(&names[k]).to_string();
        if members[k].is_empty() && child_names[k].is_empty() {
            return b.leaf(&names[k], &title);
        }
        let mut kids: Vec<Nd> = members[k].iter().map(|&m| leaves[m].take().unwrap()).collect();
        for &c in &child_names[k] {
            kids.push(build(c, names, members, child_names, leaves, b));
        }
        b.cluster(&names[k], &title, kids)
    }
    let roots: Vec<usize> = (0..names.len()).filter(|&k| !index.contains_key(parent_name(&names[k]))).collect();
    let mut built: HashMap<usize, Nd> = HashMap::new();
    for &r in &roots {
        let nd = build(r, &names, &members, &child_names, &mut leaves, &mut b);
        built.insert(r, nd);
    }
    let mut seen: HashSet<usize> = HashSet::new();
    for i in 0..ids.len() {
        match owners.get(&i) {
            Some(&k) => {
                let r = root_of(k);
                if seen.insert(r) {
                    out.push(built.remove(&r).unwrap());
                }
            }
            None => out.push(leaves[i].take().unwrap()),
        }
    }
    for r in roots {
        if let Some(nd) = built.remove(&r) {
            out.push(nd);
        }
    }
    out
}

/// `LevelEdges`: each edge laid out at the innermost container holding both
/// ends; the containers an edge crosses.
struct Levels {
    at: HashMap<Option<usize>, Vec<(usize, usize, usize)>>,
    external: HashSet<usize>,
}

fn index_paths(boxes: &[Nd], ancestors: &[usize], ent_paths: &mut HashMap<usize, Vec<usize>>) {
    for b in boxes {
        let mut path = ancestors.to_vec();
        path.push(b.uid);
        if let Some(e) = b.ent {
            ent_paths.insert(e, path.clone());
        }
        index_paths(&b.kids, &path, ent_paths);
    }
}

/// Everything `place_level` needs besides the boxes.
struct Plan<'a> {
    levels: Levels,
    edges: &'a [Edge],
    dir: &'a str,
}

fn level_edges(boxes: &[Nd], ends: &[(usize, usize)]) -> Levels {
    let mut paths = HashMap::new();
    index_paths(boxes, &[], &mut paths);
    let mut at: HashMap<Option<usize>, Vec<(usize, usize, usize)>> = HashMap::new();
    let mut external: HashSet<usize> = HashSet::new();
    for (k, &(s, t)) in ends.iter().enumerate() {
        let (Some(sp), Some(tp)) = (paths.get(&s), paths.get(&t)) else { continue };
        let shared = sp.iter().zip(tp.iter()).take_while(|(a, b)| a == b).count();
        if let (Some(&from), Some(&to)) = (sp.get(shared), tp.get(shared)) {
            let container = if shared == 0 { None } else { Some(sp[shared - 1]) };
            at.entry(container).or_default().push((k, from, to));
        }
        let (hs, ht) = (&sp[..sp.len() - 1], &tp[..tp.len() - 1]);
        external.extend(hs.iter().filter(|u| !ht.contains(u)).copied());
        external.extend(ht.iter().filter(|u| !hs.contains(u)).copied());
    }
    Levels { at, external }
}

/// Positions the nested boxes in place; page margin included.
pub fn place_all(boxes: &mut [Nd], edges: &[Edge], dir: &str) -> Result<(), Error> {
    let ends: Vec<(usize, usize)> = edges.iter().map(|e| (e.src, e.tgt)).collect();
    let plan = Plan { levels: level_edges(boxes, &ends), edges, dir };
    place_level(boxes, None, plan.dir, 80.0, &plan)?;
    if boxes.is_empty() {
        return Ok(());
    }
    let left = boxes.iter().map(|b| b.x).fold(f64::INFINITY, f64::min);
    let top = boxes.iter().map(|b| b.y).fold(f64::INFINITY, f64::min);
    for b in boxes.iter_mut() {
        b.x += PAGE_MARGIN - left;
        b.y += PAGE_MARGIN - top;
    }
    center_vertical_siblings(boxes);
    Ok(())
}

/// `ClassNamespacePlacement.center_vertical_siblings`: boxes sharing a left
/// edge but not a row are centred on the widest of them.
fn center_vertical_siblings(boxes: &mut [Nd]) {
    for b in boxes.iter_mut() {
        center_vertical_siblings(&mut b.kids);
    }
    let mut columns: Vec<(f64, Vec<usize>)> = vec![];
    for (i, b) in boxes.iter().enumerate() {
        match columns.iter_mut().find(|c| c.0 == b.x) {
            Some(c) => c.1.push(i),
            None => columns.push((b.x, vec![i])),
        }
    }
    for (_, column) in columns {
        let first = boxes[column[0]].y;
        if column.iter().all(|&i| boxes[i].y == first) {
            continue;
        }
        let width = column.iter().map(|&i| boxes[i].w).fold(f64::NEG_INFINITY, f64::max);
        for i in column {
            boxes[i].x += (width - boxes[i].w) / 2.0;
        }
    }
}

fn place_level(boxes: &mut [Nd], container: Option<usize>, dir: &str, rank: f64, plan: &Plan) -> Result<(), Error> {
    for b in boxes.iter_mut() {
        if b.cluster {
            place_cluster(b, dir, rank, container.is_none(), plan)?;
        }
    }
    if boxes.is_empty() {
        return Ok(());
    }
    let empty = vec![];
    let level = plan.levels.at.get(&container).unwrap_or(&empty);
    let mut first: HashMap<usize, usize> = HashMap::new();
    for (i, b) in boxes.iter().enumerate() {
        first.entry(b.uid).or_insert(i);
    }
    let flat: Vec<Edge> = level
        .iter()
        .filter_map(|&(k, from, to)| {
            let (src, tgt) = (*first.get(&from)?, *first.get(&to)?);
            Some(Edge { src, tgt, labels: plan.edges[k].labels.clone() })
        })
        .collect();
    let sizes: Vec<(f64, f64)> = boxes.iter().map(|b| (b.w, b.h)).collect();
    let clusters: Vec<bool> = boxes.iter().map(|b| b.cluster).collect();
    let pos = place::place_flat(&sizes, &flat, dir, rank, &clusters)?;
    for (b, (x, y)) in boxes.iter_mut().zip(pos) {
        b.x = x;
        b.y = y;
    }
    cluster_reach(boxes, level, matches!(dir, "LEFT" | "RIGHT"));
    Ok(())
}

/// `ClusterReach`: a subgraph stretches along the rank to cover the boxes
/// with an edge into it.
fn cluster_reach(boxes: &mut [Nd], level: &[(usize, usize, usize)], sideways: bool) {
    let mut first: HashMap<usize, usize> = HashMap::new();
    for (i, b) in boxes.iter().enumerate() {
        first.entry(b.uid).or_insert(i);
    }
    let centre = |b: &Nd| if sideways { b.y + b.h / 2.0 } else { b.x + b.w / 2.0 };
    let mut wanted: Vec<(usize, Vec<f64>)> = vec![];
    let mut reach = |cluster: usize, other: usize, boxes: &[Nd]| {
        if cluster == other || !boxes[cluster].cluster {
            return;
        }
        let c = centre(&boxes[other]);
        match wanted.iter_mut().find(|w| w.0 == cluster) {
            Some(w) => w.1.push(c),
            None => wanted.push((cluster, vec![c])),
        }
    };
    for &(_, from, to) in level {
        if let (Some(&f), Some(&t)) = (first.get(&from), first.get(&to)) {
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
            b.w_int = false;
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

fn external_padding(top: bool) -> (f64, f64) {
    (if top { EXTERNAL_CLUSTER_SIDE } else { NESTED_CLUSTER_SIDE }, CLUSTER_VERTICAL)
}

/// `split_linked_cluster?`: four or more boxes, all linked, in more than
/// one component.
fn split_linked(c: &Nd, plan: &Plan) -> bool {
    if !c.cluster {
        return false;
    }
    let empty = vec![];
    let level = plan.levels.at.get(&Some(c.uid)).unwrap_or(&empty);
    let linked: HashSet<usize> = level.iter().flat_map(|&(_, f, t)| [f, t]).collect();
    let ids: Vec<usize> = c.kids.iter().map(|k| k.uid).collect();
    if ids.len() < 4 || ids.iter().any(|i| !linked.contains(i)) {
        return false;
    }
    let mut links: HashMap<usize, Vec<usize>> = ids.iter().map(|&i| (i, vec![])).collect();
    for &(_, f, t) in level {
        if links.contains_key(&f) && links.contains_key(&t) {
            links.get_mut(&f).unwrap().push(t);
            links.get_mut(&t).unwrap().push(f);
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

fn place_cluster(c: &mut Nd, dir: &str, rank: f64, top: bool, plan: &Plan) -> Result<(), Error> {
    if split_linked(c, plan) {
        return Err(Error::Unported("class_diagram".into()));
    }
    // `inherit_nested_direction` gives the root direction only to a
    // subgraph that holds another subgraph or namespace leaf; any other
    // follows its level when an edge crosses it, else turns sideways.
    let holds_nested = c.kids.iter().any(|k| k.title.is_some());
    let (inner_dir, inner_rank) = if holds_nested {
        (plan.dir, rank + RANK_STEP)
    } else if plan.levels.external.contains(&c.uid) {
        (dir, rank)
    } else {
        (if dir == "DOWN" { "RIGHT" } else { "DOWN" }, rank + RANK_STEP)
    };
    place_level(&mut c.kids, Some(c.uid), inner_dir, inner_rank, plan)?;
    let padding = if plan.levels.external.contains(&c.uid) {
        external_padding(top)
    } else if !top {
        (CLUSTER_SIDE, CLUSTER_VERTICAL)
    } else {
        let nested: Vec<&Nd> = c.kids.iter().filter(|k| k.cluster).collect();
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
    let (side, vertical) = padding;
    let left = c.kids.iter().map(|k| k.x).fold(f64::INFINITY, f64::min);
    let right = c.kids.iter().map(|k| k.x + k.w).fold(f64::NEG_INFINITY, f64::max);
    let inner = right - left;
    let titled = c.label.0.trunc() + CLUSTER_PADDING * 2.0;
    c.w = (inner + side * 2.0).max(titled).ceil();
    c.w_int = true;
    let lh = c.label.1.trunc();
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

/// `ClassNamespaceNodes#flatten`: plain class boxes (entity, x, y) and the
/// namespace boxes, both in canvas coordinates, in tree order.
pub fn flatten(boxes: &[Nd], origin: (f64, f64), plain: &mut Vec<(usize, f64, f64)>, out: &mut Vec<NsBox>) {
    for b in boxes {
        let (x, y) = (origin.0 + b.x, origin.1 + b.y);
        match &b.title {
            None => plain.push((b.ent.unwrap(), x, y)),
            Some(t) => {
                let half = if b.w_int { (b.w / 2.0).floor() } else { b.w / 2.0 };
                out.push(NsBox { id: b.name.clone(), title: t.clone(), x, y, w: b.w, h: b.h, title_x: x + half });
                flatten(&b.kids, (x, y), plain, out);
            }
        }
    }
}
