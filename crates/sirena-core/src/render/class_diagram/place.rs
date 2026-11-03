//! Directed placement of class boxes: the cluster-free subset of Ruby
//! `Layout::Flowchart::DirectedPlacement` (+ `EdgeOrder`, `RowCentering`,
//! `BrandesKoepf`, `EdgeChains`, `LayerSpacing`) over `ElkPlacement`.
//! `Elkrb.layout` is `crate::elk::layout`.
use crate::Error;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const PAGE_MARGIN: f64 = 8.0;
const NODE_SEP: f64 = 50.0;
const EDGE_SEP: f64 = 20.0;
const LINE_HEIGHT: f64 = 1.5;

/// A class-diagram edge between two boxes; `labels` are (width, height).
pub struct Edge {
    pub src: usize,
    pub tgt: usize,
    pub labels: Vec<(f64, f64)>,
}

#[derive(Clone, Copy)]
struct Bx {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Nid {
    Box(usize),
    Edge(usize),
    Chain(usize, usize),
}

fn fmax(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Positions of `sizes` (width, height) joined by `edges`, for layout direction
/// `dir` (DOWN, UP, LEFT or RIGHT) at 50 spacing.
pub fn place(sizes: &[(f64, f64)], edges: &[Edge], dir: &str) -> Result<Vec<(f64, f64)>, Error> {
    let sideways = dir == "LEFT" || dir == "RIGHT";
    let mut bs: Vec<Bx> = sizes.iter().map(|&(w, h)| Bx { x: f64::NAN, y: f64::NAN, w, h }).collect();
    if sideways {
        transpose(&mut bs);
    }
    // EdgeOrder
    let mut rank: HashMap<usize, usize> = HashMap::new();
    for e in edges {
        for id in [e.src, e.tgt] {
            let n = rank.len();
            rank.entry(id).or_insert(n);
        }
    }
    let mut linked: Vec<usize> = (0..bs.len()).filter(|i| rank.contains_key(i)).collect();
    linked.sort_by_key(|i| rank[i]);
    let mut it = linked.into_iter();
    let order: Vec<usize> = (0..bs.len()).map(|i| if rank.contains_key(&i) { it.next().unwrap() } else { i }).collect();

    // ElkPlacement
    let placed = elk_place(&bs, &order, edges)?;
    for (i, (x, y)) in placed {
        bs[i].x = x;
        bs[i].y = y;
    }
    // apply_label_geometry
    row_centering(&mut bs, &order, edges);
    layer_spacing(&mut bs, edges, if sideways { Axis::W } else { Axis::H });
    if sideways {
        transpose(&mut bs);
    }
    match dir {
        "LEFT" => flip(&mut bs, true),
        "UP" => flip(&mut bs, false),
        _ => {}
    }
    // margin_page
    let left = bs.iter().map(|b| b.x).fold(f64::INFINITY, f64::min);
    let top = bs.iter().map(|b| b.y).fold(f64::INFINITY, f64::min);
    let (dx, dy) = (PAGE_MARGIN - left, PAGE_MARGIN - top);
    Ok(bs.iter().map(|b| (b.x + dx, b.y + dy)).collect())
}

fn transpose(bs: &mut [Bx]) {
    for b in bs {
        if !b.x.is_nan() {
            std::mem::swap(&mut b.x, &mut b.y);
        }
        std::mem::swap(&mut b.w, &mut b.h);
    }
}

fn flip(bs: &mut [Bx], horizontal: bool) {
    let ext = bs.iter().map(|b| if horizontal { b.x + b.w } else { b.y + b.h }).fold(f64::NEG_INFINITY, f64::max);
    for b in bs {
        if horizontal {
            b.x = ext - b.x - b.w;
        } else {
            b.y = ext - b.y - b.h;
        }
    }
}

fn elk_place(bs: &[Bx], order: &[usize], edges: &[Edge]) -> Result<Vec<(usize, (f64, f64))>, Error> {
    let child_opts = json!({
        "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
        "elk.direction": "DOWN",
        "elk.alignment": "TOP",
        "elk.separateConnectedComponents": false,
        "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
        "elk.spacing.nodeNode": 50,
        "elk.layered.spacing.nodeNodeBetweenLayers": 50,
    });
    let children: Vec<Value> = order
        .iter()
        .map(|&i| json!({"id": format!("box-{i}"), "width": bs[i].w, "height": bs[i].h, "layoutOptions": child_opts}))
        .collect();
    // ElkPlacement drops the edges that close a cycle (self-loops included).
    let mut links: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut kept = vec![];
    for (k, e) in edges.iter().enumerate() {
        if e.src == e.tgt || reachable(e.tgt, e.src, &links) {
            continue;
        }
        links.entry(e.src).or_default().push(e.tgt);
        kept.push(json!({"id": format!("edge-{k}"), "sources": [format!("box-{}", e.src)],
                         "targets": [format!("box-{}", e.tgt)], "labels": [], "metadata": {}}));
    }
    let mut opts = Map::new();
    for (k, v) in [
        ("elk.layered.considerModelOrder.strategy", json!("NODES_AND_EDGES")),
        ("elk.algorithm", json!("layered")),
        ("elk.direction", json!("DOWN")),
        ("elk.spacing.nodeNode", json!(50)),
        ("elk.layered.spacing.nodeNodeBetweenLayers", json!(50)),
        ("elk.hierarchyHandling", json!("INCLUDE_CHILDREN")),
        ("elk.layered.crossingMinimization.strategy", json!("LAYER_SWEEP")),
        ("elk.alignment", json!("TOP")),
        ("elk.separateConnectedComponents", json!(false)),
    ] {
        opts.insert(k.into(), v);
    }
    let input = json!({"id": "class_diagram", "children": children, "edges": kept, "layoutOptions": opts});
    let out = crate::elk::layout(&input)?;
    let mut res = vec![];
    for c in out["children"].as_array().ok_or_else(|| Error::Unported("elk result".into()))? {
        let id = c["id"].as_str().and_then(|s| s.strip_prefix("box-")).and_then(|s| s.parse::<usize>().ok());
        match (id, c["x"].as_f64(), c["y"].as_f64()) {
            (Some(i), Some(x), Some(y)) => res.push((i, (x, y))),
            _ => return Err(Error::Unported("elk result".into())),
        }
    }
    Ok(res)
}

fn reachable(start: usize, target: usize, links: &HashMap<usize, Vec<usize>>) -> bool {
    let mut pending = vec![start];
    let mut seen = std::collections::HashSet::new();
    while let Some(n) = pending.pop() {
        if n == target {
            return true;
        }
        if !seen.insert(n) {
            continue;
        }
        if let Some(l) = links.get(&n) {
            pending.extend(l.iter().copied());
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    W,
    H,
}

fn sorted_tops(bs: &[Bx]) -> Vec<f64> {
    let mut tops: Vec<f64> = vec![];
    for b in bs {
        if !tops.contains(&b.y) {
            tops.push(b.y);
        }
    }
    tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
    tops
}

fn layer_spacing(bs: &mut [Bx], edges: &[Edge], axis: Axis) {
    let tops = sorted_tops(bs);
    let layer = |i: usize| tops.iter().position(|&t| t == bs[i].y).unwrap();
    let mut rooms = vec![0.0f64; tops.len()];
    for e in edges {
        let (a, b) = (layer(e.src), layer(e.tgt));
        let (from, to) = if a <= b { (a, b) } else { (b, a) };
        if to == 0 || from == to {
            continue;
        }
        let gap = (from + to - 1) / 2;
        let size = e.labels.iter().map(|l| if axis == Axis::W { l.0 } else { l.1 }).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| fmax(m, v))));
        let room = match size {
            None => 0.0,
            Some(s) if axis == Axis::H => s * LINE_HEIGHT,
            Some(s) => s,
        };
        rooms[gap] = fmax(rooms[gap], room);
    }
    let mut shifts = vec![];
    let mut total = 0.0;
    for i in 0..tops.len() {
        if i > 0 {
            total += rooms[i - 1];
        }
        shifts.push(total);
    }
    if shifts.iter().all(|&s| s == 0.0) {
        return;
    }
    let tops_of: Vec<usize> = bs.iter().map(|b| tops.iter().position(|&t| t == b.y).unwrap()).collect();
    for (b, t) in bs.iter_mut().zip(tops_of) {
        b.y += shifts[t];
    }
}

// ---- RowCentering ----

fn group_rows(bs: &[Bx], order: &[usize]) -> Vec<(f64, Vec<usize>)> {
    let mut rows: Vec<(f64, Vec<usize>)> = vec![];
    for &i in order {
        match rows.iter_mut().find(|(y, _)| *y == bs[i].y) {
            Some(r) => r.1.push(i),
            None => rows.push((bs[i].y, vec![i])),
        }
    }
    rows
}

fn row_centering(bs: &mut [Bx], order: &[usize], edges: &[Edge]) {
    let rows = group_rows(bs, order);
    let mut ys: Vec<f64> = rows.iter().map(|r| r.0).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut ranks: HashMap<usize, usize> = HashMap::new();
    for (rank, y) in ys.iter().enumerate() {
        for (ry, ids) in &rows {
            if ry == y {
                for &i in ids {
                    ranks.insert(i, rank);
                }
            }
        }
    }
    // joins: every edge unlabeled and between adjacent rows
    let mut pairs: Vec<(usize, usize)> = vec![];
    let mut all = true;
    for e in edges {
        if !e.labels.is_empty() {
            all = false;
            break;
        }
        let (mut a, mut b) = (e.src, e.tgt);
        if ranks[&a] > ranks[&b] {
            std::mem::swap(&mut a, &mut b);
        }
        if ranks[&b] != ranks[&a] + 1 {
            all = false;
            break;
        }
        pairs.push((a, b));
    }
    if all && !pairs.is_empty() {
        return aligned(bs, &rows, &pairs);
    }
    // chains
    if !edges.is_empty() && edges.iter().all(|e| e.labels.is_empty() && ranks[&e.src] != ranks[&e.tgt]) {
        let ends: Vec<(usize, usize, bool)> = edges
            .iter()
            .map(|e| if ranks[&e.src] > ranks[&e.tgt] { (e.tgt, e.src, true) } else { (e.src, e.tgt, false) })
            .collect();
        let mut by_pos: Vec<usize> = order.to_vec();
        by_pos.sort_by(|&a, &b| (bs[a].x, bs[a].y).partial_cmp(&(bs[b].x, bs[b].y)).unwrap());
        let chains = Chains::new(&ranks, &ends, &by_pos);
        let mut widths: HashMap<Nid, f64> = order.iter().map(|&i| (Nid::Box(i), bs[i].w)).collect();
        for d in &chains.dummies {
            widths.insert(d.clone(), 0.0);
        }
        let centres = Bk::new(&chains.layers, &widths, &chains.links, &chains.dummies).positions();
        let tops = ys;
        for &i in order {
            bs[i].y = tops[chains.ranks[&i]];
        }
        let all_ids: Vec<usize> = order.to_vec();
        place_boxes(bs, &all_ids, &centres);
        return;
    }
    // centre_rows
    let span = |bs: &[Bx], ids: &[usize]| {
        let l = ids.iter().map(|&i| bs[i].x).fold(f64::INFINITY, f64::min);
        let r = ids.iter().map(|&i| bs[i].x + bs[i].w).fold(f64::NEG_INFINITY, f64::max);
        r - l
    };
    let widest = rows.iter().map(|r| span(bs, &r.1)).fold(f64::NEG_INFINITY, f64::max);
    let left = rows.iter().flat_map(|r| r.1.iter()).map(|&i| bs[i].x).fold(f64::INFINITY, f64::min);
    for (_, ids) in &rows {
        let start = ids.iter().map(|&i| bs[i].x).fold(f64::INFINITY, f64::min);
        let shift = left + ((widest - span(bs, ids)) / 2.0) - start;
        for &i in ids {
            bs[i].x += shift;
        }
    }
}

fn place_boxes(bs: &mut [Bx], ids: &[usize], centres: &HashMap<Nid, f64>) {
    let edge = |bs: &[Bx], i: usize| centres[&Nid::Box(i)] - (bs[i].w / 2.0);
    let left = ids.iter().map(|&i| edge(bs, i)).fold(f64::INFINITY, f64::min);
    for &i in ids {
        bs[i].x = edge(bs, i) - left;
    }
}

fn aligned(bs: &mut [Bx], rows: &[(f64, Vec<usize>)], joins: &[(usize, usize)]) {
    let mut sorted: Vec<&(f64, Vec<usize>)> = rows.iter().collect();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let ordered: Vec<Vec<usize>> = sorted
        .iter()
        .map(|r| {
            let mut ids = r.1.clone();
            ids.sort_by(|&a, &b| bs[a].x.partial_cmp(&bs[b].x).unwrap());
            ids
        })
        .collect();
    let mut slot: HashMap<usize, usize> = HashMap::new();
    for row in &ordered {
        for (i, &b) in row.iter().enumerate() {
            slot.insert(b, i);
        }
    }
    let mut stand: Vec<((usize, usize), usize)> = joins.iter().copied().zip(0..).collect();
    stand.sort_by_key(|&((u, l), _)| (slot[&l], slot[&u]));
    let mut layers: Vec<Vec<Nid>> = vec![];
    for (rank, row) in ordered.iter().enumerate() {
        layers.push(row.iter().map(|&i| Nid::Box(i)).collect());
        if rank != ordered.len() - 1 {
            layers.push(stand.iter().filter(|((u, _), _)| row.contains(u)).map(|&(_, k)| Nid::Edge(k)).collect());
        }
    }
    let mut links = vec![];
    for &((up, low), k) in &stand {
        links.push((Nid::Box(up), Nid::Edge(k)));
        links.push((Nid::Edge(k), Nid::Box(low)));
    }
    let dummies: Vec<Nid> = stand.iter().map(|&(_, k)| Nid::Edge(k)).collect();
    // unlabeled: stand-in width is 0
    let mut widths: HashMap<Nid, f64> = ordered.iter().flatten().map(|&i| (Nid::Box(i), bs[i].w)).collect();
    for d in &dummies {
        widths.insert(d.clone(), 0.0);
    }
    let centres = Bk::new(&layers, &widths, &links, &dummies).positions();
    let flat: Vec<usize> = ordered.iter().flatten().copied().collect();
    place_boxes(bs, &flat, &centres);
}

// ---- EdgeChains ----

struct Chains {
    ranks: HashMap<usize, usize>,
    layers: Vec<Vec<Nid>>,
    links: Vec<(Nid, Nid)>,
    dummies: Vec<Nid>,
}

impl Chains {
    fn new(ranks: &HashMap<usize, usize>, ends: &[(usize, usize, bool)], order: &[usize]) -> Chains {
        let mut c = Chains { ranks: ranks.clone(), layers: vec![], links: vec![], dummies: vec![] };
        c.sink(ends);
        let stacked = c.stacked(ends, order);
        c.layers = c.sweep(stacked);
        c
    }

    fn sink(&mut self, ends: &[(usize, usize, bool)]) {
        let mut ids: Vec<usize> = self.ranks.keys().copied().collect();
        ids.sort_by_key(|id| std::cmp::Reverse(self.ranks[id]));
        for id in ids {
            let below: Vec<usize> = ends.iter().filter(|e| e.0 == id).map(|e| self.ranks[&e.1]).collect();
            let sinkable = match below.iter().min() {
                None => false,
                Some(&m) => {
                    m as i64 - 1 > self.ranks[&id] as i64 && ends.iter().filter(|e| e.1 == id).count() == below.len()
                }
            };
            if sinkable {
                let m = *below.iter().min().unwrap();
                self.ranks.insert(id, m - 1);
            }
        }
    }

    fn stacked(&mut self, ends: &[(usize, usize, bool)], order: &[usize]) -> Vec<Vec<Nid>> {
        let max = *self.ranks.values().max().unwrap();
        let mut layers: Vec<Vec<Nid>> = vec![vec![]; max * 2 + 1];
        for &id in order {
            layers[self.ranks[&id] * 2].push(Nid::Box(id));
        }
        let mut idx: Vec<usize> = (0..ends.len()).collect();
        idx.sort_by_key(|&k| (usize::from(ends[k].2), k));
        for k in idx {
            let (upper, lower, _) = ends[k];
            let mut above = Nid::Box(upper);
            for layer in (self.ranks[&upper] * 2 + 1)..=(self.ranks[&lower] * 2 - 1) {
                let dummy = Nid::Chain(k, layer);
                layers[layer].push(dummy.clone());
                self.dummies.push(dummy.clone());
                self.links.push((above, dummy.clone()));
                above = dummy;
            }
            self.links.push((above, Nid::Box(lower)));
        }
        layers
    }

    fn sweep(&self, layers: Vec<Vec<Nid>>) -> Vec<Vec<Nid>> {
        let mut above: HashMap<Nid, Vec<Nid>> = HashMap::new();
        for (u, l) in &self.links {
            above.entry(l.clone()).or_default().push(u.clone());
        }
        let mut done: Vec<Vec<Nid>> = vec![layers[0].clone()];
        for layer in layers.iter().skip(1) {
            let prev = done.last().unwrap();
            let index: HashMap<&Nid, usize> = prev.iter().enumerate().map(|(i, n)| (n, i)).collect();
            let mut fixed = vec![];
            let mut free = vec![];
            for (place, id) in layer.iter().enumerate() {
                match above.get(id) {
                    Some(u) if !u.is_empty() => free.push((id, place, u)),
                    _ => fixed.push((id, place)),
                }
            }
            let mut keyed: Vec<(f64, usize, &Nid)> = free
                .iter()
                .map(|(id, place, u)| (u.iter().map(|x| index[x]).sum::<usize>() as f64 / u.len() as f64, *place, *id))
                .collect();
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
            let mut sorted: Vec<Nid> = keyed.into_iter().map(|k| k.2.clone()).collect();
            for (id, place) in fixed {
                let at = place.min(sorted.len());
                sorted.insert(at, id.clone());
            }
            done.push(sorted);
        }
        done
    }
}

// ---- BrandesKoepf ----

struct Bk<'a> {
    layers: &'a [Vec<Nid>],
    widths: &'a HashMap<Nid, f64>,
    dummies: &'a [Nid],
    up: HashMap<Nid, Vec<Nid>>,
    down: HashMap<Nid, Vec<Nid>>,
}

type Pos = HashMap<Nid, f64>;

impl<'a> Bk<'a> {
    fn new(layers: &'a [Vec<Nid>], widths: &'a HashMap<Nid, f64>, links: &[(Nid, Nid)], dummies: &'a [Nid]) -> Self {
        let mut up: HashMap<Nid, Vec<Nid>> = HashMap::new();
        let mut down: HashMap<Nid, Vec<Nid>> = HashMap::new();
        for (u, l) in links {
            up.entry(l.clone()).or_default().push(u.clone());
            down.entry(u.clone()).or_default().push(l.clone());
        }
        Bk { layers, widths, dummies, up, down }
    }

    fn positions(&self) -> Pos {
        let mut layouts: Vec<(bool, Pos)> = vec![];
        for vert_up in [true, false] {
            for left in [true, false] {
                layouts.push((left, self.layout(vert_up, left)));
            }
        }
        self.balance(self.align_to_narrowest(layouts))
    }

    fn layout(&self, vert_up: bool, left: bool) -> Pos {
        let mut layering: Vec<Vec<Nid>> = self.layers.to_vec();
        if !vert_up {
            layering.reverse();
        }
        if !left {
            for l in &mut layering {
                l.reverse();
            }
        }
        let roots = self.align_blocks(&layering, if vert_up { &self.up } else { &self.down });
        let pos = self.compact(&layering, &roots);
        if left { pos } else { pos.into_iter().map(|(k, v)| (k, -v)).collect() }
    }

    fn align_blocks(&self, layering: &[Vec<Nid>], nb: &HashMap<Nid, Vec<Nid>>) -> HashMap<Nid, Nid> {
        let mut root: HashMap<Nid, Nid> = HashMap::new();
        let mut pos: HashMap<Nid, usize> = HashMap::new();
        for layer in layering {
            for (i, n) in layer.iter().enumerate() {
                root.insert(n.clone(), n.clone());
                pos.insert(n.clone(), i);
            }
        }
        let mut align = root.clone();
        for layer in layering {
            let mut taken: i64 = -1;
            for node in layer {
                let mut above: Vec<&Nid> = nb.get(node).map(|v| v.iter().collect()).unwrap_or_default();
                above.sort_by_key(|o| pos[*o]);
                let meds: Vec<&Nid> = if above.is_empty() {
                    vec![]
                } else {
                    let (lo, hi) = ((above.len() - 1) / 2, above.len() / 2);
                    if lo == hi { vec![above[lo]] } else { vec![above[lo], above[hi]] }
                };
                for other in meds {
                    if align[node] == *node && taken < pos[other] as i64 {
                        align.insert(other.clone(), node.clone());
                        let r = root[other].clone();
                        root.insert(node.clone(), r.clone());
                        align.insert(node.clone(), r);
                        taken = pos[other] as i64;
                    }
                }
            }
        }
        root
    }

    fn separation(&self, near: &Nid, far: &Nid) -> f64 {
        let half = |n: &Nid| self.widths[n] / 2.0;
        let reach = |n: &Nid| if self.dummies.contains(n) { EDGE_SEP } else { NODE_SEP };
        half(near) + half(far) + ((reach(near) + reach(far)) / 2.0)
    }

    fn compact(&self, layering: &[Vec<Nid>], root: &HashMap<Nid, Nid>) -> Pos {
        let mut lefts: HashMap<Nid, Vec<(Nid, f64)>> = HashMap::new();
        let mut rights: HashMap<Nid, Vec<(Nid, f64)>> = HashMap::new();
        let upsert = |m: &mut HashMap<Nid, Vec<(Nid, f64)>>, a: &Nid, b: &Nid, g: f64| {
            let v = m.entry(a.clone()).or_default();
            match v.iter_mut().find(|(k, _)| k == b) {
                Some(s) => s.1 = g,
                None => v.push((b.clone(), g)),
            }
        };
        for layer in layering {
            for w in layer.windows(2) {
                let (near, far) = (&w[0], &w[1]);
                let (nb, fb) = (&root[near], &root[far]);
                let kept = lefts.get(fb).and_then(|v| v.iter().find(|(k, _)| k == nb)).map_or(0.0, |x| x.1);
                let gap = fmax(self.separation(near, far), kept);
                upsert(&mut lefts, fb, nb, gap);
                upsert(&mut rights, nb, fb, gap);
            }
        }
        let mut blocks: Vec<Nid> = vec![];
        for v in root.values() {
            if !blocks.contains(v) {
                blocks.push(v.clone());
            }
        }
        let mut packed: HashMap<Nid, f64> = HashMap::new();
        for b in &blocks {
            visit_left(b, &lefts, &mut packed);
        }
        let mut settled: HashMap<Nid, f64> = HashMap::new();
        for b in &blocks {
            visit_right(b, &rights, &packed, &mut settled);
        }
        root.iter().map(|(n, b)| (n.clone(), settled[b])).collect()
    }

    fn extent(&self, p: &Pos) -> f64 {
        let hi = p.iter().map(|(n, x)| x + self.widths[n] / 2.0).fold(f64::NEG_INFINITY, f64::max);
        let lo = p.iter().map(|(n, x)| x - self.widths[n] / 2.0).fold(f64::INFINITY, f64::min);
        hi - lo
    }

    fn align_to_narrowest(&self, layouts: Vec<(bool, Pos)>) -> Vec<Pos> {
        let mut best = 0;
        for i in 1..layouts.len() {
            if self.extent(&layouts[i].1) < self.extent(&layouts[best].1) {
                best = i;
            }
        }
        let vals = layouts[best].1.values();
        let low = vals.clone().fold(f64::INFINITY, |a, &b| a.min(b));
        let high = vals.fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        layouts
            .into_iter()
            .map(|(left, p)| {
                let edge = if left {
                    p.values().fold(f64::INFINITY, |a, &b| a.min(b))
                } else {
                    p.values().fold(f64::NEG_INFINITY, |a, &b| a.max(b))
                };
                let delta = (if left { low } else { high }) - edge;
                p.into_iter().map(|(k, v)| (k, v + delta)).collect()
            })
            .collect()
    }

    fn balance(&self, cands: Vec<Pos>) -> Pos {
        cands[0]
            .keys()
            .map(|n| {
                let mut v: Vec<f64> = cands.iter().map(|p| p[n]).collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                (n.clone(), (v[1] + v[2]) / 2.0)
            })
            .collect()
    }
}

fn visit_left(b: &Nid, lefts: &HashMap<Nid, Vec<(Nid, f64)>>, memo: &mut HashMap<Nid, f64>) -> f64 {
    if let Some(&v) = memo.get(b) {
        return v;
    }
    let mut best = 0.0f64;
    if let Some(v) = lefts.get(b) {
        for (near, gap) in v {
            best = fmax(best, visit_left(near, lefts, memo) + gap);
        }
    }
    memo.insert(b.clone(), best);
    best
}

fn visit_right(b: &Nid, rights: &HashMap<Nid, Vec<(Nid, f64)>>, packed: &HashMap<Nid, f64>, memo: &mut HashMap<Nid, f64>) -> f64 {
    if let Some(&v) = memo.get(b) {
        return v;
    }
    let mut limit: Option<f64> = None;
    if let Some(v) = rights.get(b) {
        for (far, gap) in v {
            let c = visit_right(far, rights, packed, memo) - gap;
            limit = Some(limit.map_or(c, |l: f64| if c < l { c } else { l }));
        }
    }
    let r = match limit {
        Some(l) => fmax(packed[b], l),
        None => packed[b],
    };
    memo.insert(b.clone(), r);
    r
}
