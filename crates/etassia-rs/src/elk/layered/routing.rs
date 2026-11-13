//! Phase 5: edge routing (elkrb `Layered::OrthogonalRouter`,
//! `Orthogonal::{GapSolver, HyperEdgeSegment, SegmentOrder}` and the
//! `LayeredAlgorithm#route_orthogonal_edges` glue). The router also sizes the
//! layer gaps during node placement (`slot_counts`).
//!
//! etassia declares no ports and no edge routing option, so the generic
//! `EdgeRouter` only ever matters for self-loops and edges that cross no
//! layer gap; both stay `Unported` (the golden corpus has neither). Junction
//! points and edge labels are not part of the projection and are not
//! computed.
use super::crossing::{PortOrder, Side};
use super::layering::Item;
use super::placement::port_offsets;
use super::LayeredState;
use crate::elk::geometry::{Point, Section};
use crate::elk::model::Graph;
use crate::Error;
use std::collections::HashMap;

/// Distance between neighbouring routing slots.
pub const EDGE_SPACING: f64 = 10.0;
/// Distance between a layer and its first routing slot.
pub const EDGE_NODE_SPACING: f64 = 10.0;
const TOLERANCE: f64 = 1e-3;
const CONFLICT_THRESHOLD_FACTOR: f64 = 0.5;
const CRITICAL_CONFLICT_THRESHOLD_FACTOR: f64 = 0.2;
const CONFLICT_PENALTY: i64 = 1;
const CROSSING_PENALTY: i64 = 16;
const EDGE_THICKNESS: f64 = 1.0;

/// Cross-axis geometry of every layer item: where it starts and how far it
/// extends (`OrthogonalRouter::Measure`, ports are all width 0).
pub struct Measure {
    pub start: Vec<f64>,
    pub extent: Vec<f64>,
}

// ------------------------------------------------------------ gap solver

#[derive(Debug, Default)]
struct Seg {
    ports: Vec<usize>,
    incoming: Vec<f64>,
    outgoing: Vec<f64>,
    out_deps: Vec<usize>,
    in_deps: Vec<usize>,
    slot: usize,
    mark: i64,
    in_weight: i64,
    out_weight: i64,
    critical_in: i64,
    critical_out: i64,
}

fn sorted_insert(list: &mut Vec<f64>, v: f64) {
    if !list.contains(&v) {
        list.push(v);
    }
    list.sort_by(|a, b| a.partial_cmp(b).unwrap());
}

impl Seg {
    fn start(&self) -> f64 {
        match (self.incoming.first(), self.outgoing.first()) {
            (Some(&a), Some(&b)) => if b < a { b } else { a },
            (Some(&a), None) => a,
            (None, Some(&b)) => b,
            (None, None) => 0.0,
        }
    }
    fn end(&self) -> f64 {
        match (self.incoming.last(), self.outgoing.last()) {
            (Some(&a), Some(&b)) => if b > a { b } else { a },
            (Some(&a), None) => a,
            (None, Some(&b)) => b,
            (None, None) => 0.0,
        }
    }
    /// A straight run takes no slot and needs no bend points.
    fn straight(&self) -> bool {
        (self.start() - self.end()).abs() < TOLERANCE
    }
}

struct Dep {
    source: usize,
    target: usize,
    weight: i64,
    critical: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Conflict {
    Critical,
    Count(i64),
}

struct Solver {
    segs: Vec<Seg>,
    deps: Vec<Dep>,
    conflict_threshold: f64,
    critical_threshold: f64,
}

fn near(first: f64, second: f64, threshold: f64) -> bool {
    first > second - threshold && first < second + threshold
}

fn crossings(positions: &[f64], from: f64, to: f64) -> i64 {
    positions.iter().filter(|&&p| p >= from && p <= to).count() as i64
}

fn minimum_difference(values: &[f64]) -> f64 {
    let mut distinct: Vec<f64> = vec![];
    for &v in values {
        if !distinct.contains(&v) {
            distinct.push(v);
        }
    }
    distinct.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if distinct.len() < 2 {
        return f64::MAX;
    }
    distinct.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, |m, d| if d < m { d } else { m })
}

impl Solver {
    /// `GapSolver#solve`: segments of one gap with `slot` set.
    fn solve(source_ports: &[usize], po: &PortOrder, cross: &dyn Fn(usize) -> f64) -> Solver {
        let mut s = Solver {
            segs: vec![],
            deps: vec![],
            conflict_threshold: CONFLICT_THRESHOLD_FACTOR * EDGE_SPACING,
            critical_threshold: 0.0,
        };
        let mut owner: Vec<Option<usize>> = vec![None; po.ports.len()];
        for &port in source_ports {
            if owner[port].is_some() {
                continue;
            }
            let id = s.segs.len();
            s.segs.push(Seg::default());
            s.collect(port, id, &mut owner, po, cross);
        }
        let incoming: Vec<f64> = s.segs.iter().flat_map(|g| g.incoming.clone()).collect();
        let outgoing: Vec<f64> = s.segs.iter().flat_map(|g| g.outgoing.clone()).collect();
        let a = minimum_difference(&incoming);
        let b = minimum_difference(&outgoing);
        let minimum = if b < a { b } else { a };
        s.critical_threshold = CRITICAL_CONFLICT_THRESHOLD_FACTOR * minimum;
        s.link();
        s.break_cycles();
        s.number();
        s
    }

    fn collect(&mut self, port: usize, seg: usize, owner: &mut [Option<usize>], po: &PortOrder, cross: &dyn Fn(usize) -> f64) {
        if owner[port].is_some() {
            return;
        }
        owner[port] = Some(seg);
        let c = cross(port);
        let g = &mut self.segs[seg];
        g.ports.push(port);
        if po.ports[port].side == Side::East {
            sorted_insert(&mut g.incoming, c);
        } else {
            sorted_insert(&mut g.outgoing, c);
        }
        for other in po.others(port) {
            self.collect(other, seg, owner, po, cross);
        }
    }

    fn add_dep(&mut self, source: usize, target: usize, weight: i64, critical: bool) {
        let id = self.deps.len();
        self.deps.push(Dep { source, target, weight, critical });
        self.segs[source].out_deps.push(id);
        self.segs[target].in_deps.push(id);
    }

    fn detach(&mut self, id: usize) {
        let (s, t) = (self.deps[id].source, self.deps[id].target);
        self.segs[s].out_deps.retain(|&d| d != id);
        self.segs[t].in_deps.retain(|&d| d != id);
    }

    fn reverse(&mut self, id: usize) {
        let (s, t) = (self.deps[id].source, self.deps[id].target);
        self.detach(id);
        self.deps[id].source = t;
        self.deps[id].target = s;
        self.segs[t].out_deps.push(id);
        self.segs[s].in_deps.push(id);
    }

    fn link(&mut self) {
        let n = self.segs.len();
        for i in 0..n {
            for j in (i + 1)..n {
                self.create_dependency(i, j);
            }
        }
    }

    fn create_dependency(&mut self, first: usize, second: usize) {
        if self.segs[first].straight() || self.segs[second].straight() {
            return;
        }
        let first_first = self.conflicts(&self.segs[first].outgoing, &self.segs[second].incoming);
        let second_first = self.conflicts(&self.segs[second].outgoing, &self.segs[first].incoming);
        if first_first == Conflict::Critical || second_first == Conflict::Critical {
            if first_first == Conflict::Critical {
                self.add_dep(second, first, 1, true);
            }
            if second_first == Conflict::Critical {
                self.add_dep(first, second, 1, true);
            }
        } else {
            let (Conflict::Count(ff), Conflict::Count(sf)) = (first_first, second_first) else { unreachable!() };
            let left = self.penalty(first, second, ff);
            let right = self.penalty(second, first, sf);
            if left < right {
                self.add_dep(first, second, right - left, false);
            } else if left > right {
                self.add_dep(second, first, left - right, false);
            } else if left > 0 {
                self.add_dep(first, second, 0, false);
                self.add_dep(second, first, 0, false);
            }
        }
    }

    /// Cost of putting `before` in a lower slot than `after`.
    fn penalty(&self, before: usize, after: usize, conflicts: i64) -> i64 {
        let (b, a) = (&self.segs[before], &self.segs[after]);
        let cross = crossings(&b.outgoing, a.start(), a.end()) + crossings(&a.incoming, b.start(), b.end());
        CONFLICT_PENALTY * conflicts + CROSSING_PENALTY * cross
    }

    /// Conflicts between two sorted coordinate lists, or Critical as soon as
    /// two coordinates are almost equal.
    fn conflicts(&self, first: &[f64], second: &[f64]) -> Conflict {
        if first.is_empty() || second.is_empty() {
            return Conflict::Count(0);
        }
        let mut count = 0;
        let (mut l, mut r) = (0usize, 0usize);
        loop {
            let (one, two) = (first[l], second[r]);
            if near(one, two, self.critical_threshold) {
                return Conflict::Critical;
            }
            if near(one, two, self.conflict_threshold) {
                count += 1;
            }
            if first[l] <= second[r] && l + 1 < first.len() {
                l += 1;
            } else if second[r] <= first[l] && r + 1 < second.len() {
                r += 1;
            } else {
                break;
            }
        }
        Conflict::Count(count)
    }

    fn break_cycles(&mut self) {
        self.assign_marks();
        let mut cyclic: Vec<usize> = vec![];
        for s in 0..self.segs.len() {
            for &d in &self.segs[s].out_deps {
                if self.segs[s].mark > self.segs[self.deps[d].target].mark {
                    cyclic.push(d);
                }
            }
        }
        for d in cyclic {
            if self.deps[d].weight == 0 {
                self.detach(d);
            } else {
                self.reverse(d);
            }
        }
    }

    // ---- SegmentOrder (greedy feedback arc set)

    fn assign_marks(&mut self) {
        let n = self.segs.len();
        for i in 0..n {
            let (inw, outw, cin, cout) = {
                let g = &self.segs[i];
                let sum = |ids: &[usize]| ids.iter().map(|&d| self.deps[d].weight).sum::<i64>();
                let crit = |ids: &[usize]| ids.iter().filter(|&&d| self.deps[d].critical).map(|&d| self.deps[d].weight).sum::<i64>();
                (sum(&g.in_deps), sum(&g.out_deps), crit(&g.in_deps), crit(&g.out_deps))
            };
            let g = &mut self.segs[i];
            g.mark = -(i as i64 + 1);
            g.in_weight = inw;
            g.out_weight = outw;
            g.critical_in = cin;
            g.critical_out = cout;
        }
        let mut sinks: Vec<usize> = (0..n).filter(|&i| self.segs[i].out_weight == 0).collect();
        let mut sources: Vec<usize> =
            (0..n).filter(|&i| self.segs[i].out_weight > 0 && self.segs[i].in_weight == 0).collect();
        sources.retain(|s| !sinks.contains(s));
        let mut unprocessed: Vec<usize> = (0..n).collect();
        let mut next_sink = n as i64 - 1;
        let mut next_source = n as i64 + 1;
        while !unprocessed.is_empty() {
            while !sinks.is_empty() {
                let s = sinks.remove(0);
                self.settle(s, next_sink, &mut unprocessed, &mut sources, &mut sinks);
                next_sink -= 1;
            }
            while !sources.is_empty() {
                let s = sources.remove(0);
                self.settle(s, next_source, &mut unprocessed, &mut sources, &mut sinks);
                next_source += 1;
            }
            if unprocessed.is_empty() {
                break;
            }
            let best = self.best_candidate(&unprocessed);
            self.settle(best, next_source, &mut unprocessed, &mut sources, &mut sinks);
            next_source += 1;
        }
        let base = n as i64;
        for g in self.segs.iter_mut() {
            if g.mark < base {
                g.mark += base + 1;
            }
        }
    }

    fn settle(&mut self, s: usize, mark: i64, unprocessed: &mut Vec<usize>, sources: &mut Vec<usize>, sinks: &mut Vec<usize>) {
        unprocessed.retain(|&u| u != s);
        self.segs[s].mark = mark;
        let open = |me: &Solver, ids: &[usize], far_target: bool| -> Vec<usize> {
            ids.iter()
                .copied()
                .filter(|&d| {
                    let far = if far_target { me.deps[d].target } else { me.deps[d].source };
                    me.deps[d].weight > 0 && me.segs[far].mark < 0
                })
                .collect()
        };
        for d in open(self, &self.segs[s].out_deps.clone(), true) {
            let t = self.deps[d].target;
            let w = self.deps[d].weight;
            let crit = self.deps[d].critical;
            let g = &mut self.segs[t];
            g.in_weight -= w;
            if crit {
                g.critical_in -= w;
            }
            if g.in_weight <= 0 && g.out_weight > 0 {
                sources.push(t);
            }
        }
        for d in open(self, &self.segs[s].in_deps.clone(), false) {
            let o = self.deps[d].source;
            let w = self.deps[d].weight;
            let crit = self.deps[d].critical;
            let g = &mut self.segs[o];
            g.out_weight -= w;
            if crit {
                g.critical_out -= w;
            }
            if g.out_weight <= 0 && g.in_weight > 0 {
                sinks.push(o);
            }
        }
    }

    /// Unprocessed segments are visited by ascending mark, i.e. the last one
    /// created first.
    fn best_candidate(&self, unprocessed: &[usize]) -> usize {
        let mut ordered = unprocessed.to_vec();
        ordered.sort_by_key(|&s| self.segs[s].mark);
        if let Some(&s) = ordered.iter().find(|&&s| self.segs[s].critical_out > 0 && self.segs[s].critical_in <= 0) {
            return s;
        }
        let balance = |s: usize| self.segs[s].out_weight - self.segs[s].in_weight;
        let top = ordered.iter().map(|&s| balance(s)).max().unwrap();
        *ordered.iter().find(|&&s| balance(s) == top).unwrap()
    }

    fn number(&mut self) {
        let mut queue: Vec<usize> = (0..self.segs.len()).filter(|&s| self.segs[s].in_deps.is_empty()).collect();
        let mut remaining: Vec<i64> = self.segs.iter().map(|g| g.in_deps.len() as i64).collect();
        let mut at = 0;
        while at < queue.len() {
            let s = queue[at];
            at += 1;
            let slot = self.segs[s].slot;
            for d in self.segs[s].out_deps.clone() {
                let t = self.deps[d].target;
                let g = &mut self.segs[t];
                g.slot = g.slot.max(slot + 1);
                remaining[t] -= 1;
                if remaining[t] == 0 {
                    queue.push(t);
                }
            }
        }
    }

    /// Number of slots the non-straight segments occupy.
    fn slot_count(&self) -> usize {
        self.segs.iter().filter(|g| !g.straight()).map(|g| g.slot + 1).max().unwrap_or(0)
    }
}

// ---------------------------------------------------------------- router

fn solve_gaps(layers: &[Vec<usize>], po: &PortOrder, cross: &dyn Fn(usize) -> f64) -> Vec<Solver> {
    layers
        .windows(2)
        .map(|pair| {
            let ports: Vec<usize> = pair[0].iter().flat_map(|&i| po.visual(i, Side::East)).collect();
            Solver::solve(&ports, po, cross)
        })
        .collect()
}

fn cross_fn<'a>(po: &'a PortOrder, dummy: &'a [bool], measure: &'a Measure, offsets: &'a [f64]) -> impl Fn(usize) -> f64 + 'a {
    move |port| {
        let item = po.ports[port].item;
        let offset = if dummy[item] { 0.0 } else { offsets[port] };
        measure.start[item] + offset
    }
}

/// `OrthogonalRouter#slot_counts`: routing slots in each gap between two
/// layers, from the cross coordinates placement just computed.
pub fn slot_counts(layers: &[Vec<usize>], po: &PortOrder, dummy: &[bool], measure: &Measure) -> Vec<usize> {
    let offsets = port_offsets(layers, po, &measure.extent);
    let cross = cross_fn(po, dummy, measure, &offsets);
    solve_gaps(layers, po, &cross).iter().map(Solver::slot_count).collect()
}

fn unported(what: &str) -> Error {
    Error::Unported(format!("elk routing {what}"))
}

/// Writes `Edge::sections` for every edge of `graph`, from the node and
/// dummy-slot positions placement set. Runs after `placement::run`.
pub fn run(graph: &mut Graph, state: &LayeredState) -> Result<(), Error> {
    let dir = match graph.options.direction.as_str() {
        "UNDEFINED" => "RIGHT",
        d => d,
    };
    let vertical = dir == "DOWN" || dir == "UP";
    let sign: f64 = if dir == "LEFT" || dir == "UP" { -1.0 } else { 1.0 };
    let lay = &state.layering;
    let po = &state.port_order;
    let n_items = lay.items.len();
    let dummy: Vec<bool> = lay.items.iter().map(|i| matches!(i, Item::Dummy(_))).collect();

    // item geometry: (x, y, width, height) with the Ruby nil defaults
    let geometry: Vec<(f64, f64, f64, f64)> = lay
        .items
        .iter()
        .map(|item| match item {
            Item::Node(c) => {
                let n = &graph.children[*c];
                (n.x.unwrap_or(0.0), n.y.unwrap_or(0.0), n.width.unwrap_or(0.0), n.height.unwrap_or(0.0))
            }
            Item::Dummy(d) => (d.x, d.y, d.width, d.height),
        })
        .collect();
    let extent: Vec<f64> = (0..n_items)
        .map(|i| {
            if dummy[i] {
                EDGE_THICKNESS
            } else if vertical {
                geometry[i].2
            } else {
                geometry[i].3
            }
        })
        .collect();
    let start: Vec<f64> = (0..n_items)
        .map(|i| {
            let (x, y, w, h) = geometry[i];
            let s = if vertical { x } else { y };
            if !dummy[i] {
                return s;
            }
            let spare = (if vertical { w } else { h }) - EDGE_THICKNESS;
            s + (spare / 2.0)
        })
        .collect();
    let measure = Measure { start, extent };
    let offsets = port_offsets(&lay.layers, po, &measure.extent);
    let cross = cross_fn(po, &dummy, &measure, &offsets);

    // abstract along range of an item (mirrored directions are negated)
    let along_range = |i: usize| -> (f64, f64) {
        let (x, y, w, h) = geometry[i];
        let (s, size) = if vertical { (y, h) } else { (x, w) };
        let (low, high) = (s, s + size);
        if sign > 0.0 { (low, high) } else { (-high, -low) }
    };
    let layer_index: HashMap<usize, usize> =
        lay.layers.iter().enumerate().flat_map(|(l, items)| items.iter().map(move |&i| (i, l))).collect();

    // bend points per (edge, gap)
    let gaps = solve_gaps(&lay.layers, po, &cross);
    let mut bends: HashMap<(usize, usize), [(f64, f64); 2]> = HashMap::new();
    for (gap, solver) in gaps.iter().enumerate() {
        let layer = &lay.layers[gap];
        let high = if layer.is_empty() {
            0.0
        } else {
            layer.iter().map(|&i| along_range(i).1 + 0.0).fold(f64::NEG_INFINITY, |m, v| if v > m { v } else { m })
        };
        let begin = high + EDGE_NODE_SPACING;
        for seg in &solver.segs {
            if seg.straight() {
                continue;
            }
            let along = begin + (seg.slot as f64 * EDGE_SPACING);
            for &p in seg.ports.iter().filter(|&&p| po.ports[p].side == Side::East) {
                for &link in &po.ports[p].segments {
                    let l = &po.segments[link];
                    let (from, to) = (cross(l.from), cross(l.to));
                    if (from - to).abs() <= TOLERANCE {
                        continue;
                    }
                    bends.insert((l.edge, gap), [(along, from), (along, to)]);
                }
            }
        }
    }

    let anchor = |port: usize, high: bool| -> (f64, f64) {
        let (low, hi) = along_range(po.ports[port].item);
        (if high { hi + 0.0 } else { low - 0.0 }, cross(port))
    };
    let real = |(along, c): (f64, f64)| -> Point {
        let r = sign * along;
        if vertical { Point { x: c, y: r } } else { Point { x: r, y: c } }
    };

    let mut grouped: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, seg) in po.segments.iter().enumerate() {
        grouped.entry(seg.edge).or_default().push(i);
    }
    let mut sections: Vec<Option<Section>> = vec![];
    for (e, edge) in graph.edges.iter().enumerate() {
        let (s, t) = (state.index.owner(&edge.sources[0]), state.index.owner(&edge.targets[0]));
        let (Some(s), Some(t)) = (s, t) else {
            sections.push(None);
            continue;
        };
        if s == t {
            return Err(unported("self-loop"));
        }
        let Some(links) = grouped.get(&e) else {
            return Err(unported("edge outside a layer gap"));
        };
        let mut links = links.clone();
        links.sort_by_key(|&l| layer_index[&po.ports[po.segments[l].from].item]);
        let first = po.segments[links[0]].from;
        let last = po.segments[*links.last().unwrap()].to;
        let mut points: Vec<(f64, f64)> = vec![anchor(first, true)];
        for &l in &links {
            let gap = layer_index[&po.ports[po.segments[l].from].item];
            if let Some(b) = bends.get(&(e, gap)) {
                points.extend_from_slice(b);
            }
        }
        points.push(anchor(last, false));
        let forward = s == po.ports[first].item;
        let mut pts: Vec<Point> = points.into_iter().map(real).collect();
        if !forward {
            pts.reverse();
        }
        let start_point = pts[0];
        let end_point = *pts.last().unwrap();
        let bend_points = pts[1..pts.len() - 1].to_vec();
        sections.push(Some(Section { start_point, end_point, bend_points }));
    }
    for (edge, section) in graph.edges.iter_mut().zip(sections) {
        edge.sections = section.into_iter().collect();
    }
    Ok(())
}
