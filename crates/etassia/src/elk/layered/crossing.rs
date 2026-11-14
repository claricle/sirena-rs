//! Phase 3: crossing minimization (elkrb `Layered::CrossingMinimizer`,
//! `DiscoveryOrder`, `PortOrder`). etassia always runs the model-order sweep
//! (`considerModelOrder.strategy = NODES_AND_EDGES`) or no sweep at all
//! (`NONE`); the seeded random `LayerSweep` is `Unported`.
use super::layering::{Item, Layering};
use crate::elk::java_random::JavaRandom;
use crate::elk::model::{Graph, NodeIndex};
use crate::Error;
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};

/// ELK only runs its greedy switch on graphs smaller than this.
const GREEDY_SWITCH_NODE_LIMIT: usize = 40;
/// ELK's default for elk.randomSeed.
const RANDOM_SEED: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    East,
    West,
}

/// One slot on the EAST or WEST side of a layer item (an edge endpoint;
/// etassia declares no ports).
#[derive(Debug, Clone)]
pub struct Port {
    pub item: usize,
    pub side: Side,
    pub sequence: (u8, usize),
    pub segments: Vec<usize>,
}

/// One edge crossing one layer gap, `from` in the earlier layer.
#[derive(Debug, Clone)]
pub struct Segment {
    pub edge: usize,
    pub from: usize,
    pub to: usize,
}

/// The port slots every layer item exposes toward its neighbouring layers.
/// EAST lists run top to bottom, WEST lists bottom to top; `visual` always
/// answers top to bottom.
#[derive(Debug, Clone)]
pub struct PortOrder {
    pub ports: Vec<Port>,
    pub segments: Vec<Segment>,
    lists: HashMap<(usize, Side), Vec<usize>>,
}

impl PortOrder {
    fn new(graph: &Graph, index: &NodeIndex, lay: &Layering) -> PortOrder {
        let mut layer_of: Vec<Option<usize>> = vec![None; lay.items.len()];
        for (number, layer) in lay.layers.iter().enumerate() {
            for &item in layer {
                layer_of[item] = Some(number);
            }
        }
        // dummy slots of each edge, by layer
        let mut slots: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (i, item) in lay.items.iter().enumerate() {
            if let Item::Dummy(d) = item {
                slots.entry(d.edge).or_default().push((d.layer_index, i));
            }
        }
        let mut po = PortOrder { ports: vec![], segments: vec![], lists: HashMap::new() };
        let mut keyed: HashMap<(usize, Side, usize), usize> = HashMap::new();
        for (order, edge) in graph.edges.iter().enumerate() {
            let (Some(s), Some(t)) = (index.owner(&edge.sources[0]), index.owner(&edge.targets[0])) else {
                continue;
            };
            let (Some(sl), Some(tl)) = (layer_of[s], layer_of[t]) else { continue };
            let (first, last) = if sl < tl { (s, t) } else { (t, s) };
            let mut chain = vec![first];
            if let Some(v) = slots.get_mut(&order) {
                v.sort_by_key(|&(layer, _)| layer);
                chain.extend(v.iter().map(|&(_, i)| i));
            }
            chain.push(last);
            for pair in chain.windows(2) {
                let (from, to) = (pair[0], pair[1]);
                if layer_of[to].unwrap() as isize - layer_of[from].unwrap() as isize != 1 {
                    continue;
                }
                let mut port = |item: usize, side: Side| {
                    *keyed.entry((item, side, order)).or_insert_with(|| {
                        po.ports.push(Port { item, side, sequence: (1, order), segments: vec![] });
                        po.ports.len() - 1
                    })
                };
                let (fp, tp) = (port(from, Side::East), port(to, Side::West));
                let seg = po.segments.len();
                po.segments.push(Segment { edge: order, from: fp, to: tp });
                po.ports[fp].segments.push(seg);
                po.ports[tp].segments.push(seg);
            }
        }
        let mut lists: HashMap<(usize, Side), Vec<usize>> = HashMap::new();
        for (i, p) in po.ports.iter().enumerate() {
            lists.entry((p.item, p.side)).or_default().push(i);
        }
        for list in lists.values_mut() {
            list.sort_by_key(|&i| po.ports[i].sequence);
        }
        po.lists = lists;
        po
    }

    /// Ports of one side, top to bottom.
    pub fn visual(&self, item: usize, side: Side) -> Vec<usize> {
        let mut list = self.lists.get(&(item, side)).cloned().unwrap_or_default();
        if side == Side::West {
            list.reverse();
        }
        list
    }

    /// The ports on the far end of each segment of `port`.
    pub fn others(&self, port: usize) -> Vec<usize> {
        self.ports[port]
            .segments
            .iter()
            .map(|&s| {
                let seg = &self.segments[s];
                if seg.from == port { seg.to } else { seg.from }
            })
            .collect()
    }

    /// Stable ascending reorder of a list by `key`.
    fn sort_by_key(&mut self, item: usize, side: Side, key: impl Fn(&PortOrder, usize) -> f64) {
        let Some(list) = self.lists.get(&(item, side)).cloned() else { return };
        let mut keyed: Vec<(f64, usize, usize)> =
            list.iter().enumerate().map(|(pos, &p)| (key(self, p), pos, p)).collect();
        keyed.sort_by(|a, b| cmp_f(a.0, b.0).then(a.1.cmp(&b.1)));
        self.lists.insert((item, side), keyed.into_iter().map(|k| k.2).collect());
    }
}

fn cmp_f(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Input-order keys of every layer item: nodes by position, dummies after.
fn input_order(n: usize, lay: &Layering) -> Vec<usize> {
    let mut order = vec![0usize; lay.items.len()];
    for (i, o) in order.iter_mut().enumerate().take(n) {
        *o = i;
    }
    let mut known = n; // Hash#length
    for layer in &lay.layers {
        for &item in layer {
            if let Item::Dummy(d) = &lay.items[item] {
                order[item] = known + d.edge_order;
                known += 1;
            }
        }
    }
    order
}

/// The order ELK lists a connected graph's nodes in: a depth-first walk
/// from the first node along each node's edges. Reorders each layer in
/// place; dummy slots keep their relative place after the nodes.
fn discovery_sort(graph: &Graph, index: &NodeIndex, lay: &mut Layering) {
    let n = graph.children.len();
    let mut table: Vec<Vec<usize>> = vec![vec![]; n];
    for edge in &graph.edges {
        if let (Some(s), Some(t)) = (index.owner(&edge.sources[0]), index.owner(&edge.targets[0])) {
            table[s].push(t);
            table[t].push(s);
        }
    }
    let mut ranks: Vec<Option<usize>> = vec![None; n];
    let mut count = 0usize;
    for root in 0..n {
        if ranks[root].is_some() {
            continue;
        }
        let mut pending: Vec<VecDeque<usize>> = vec![VecDeque::from([root])];
        while let Some(top) = pending.last_mut() {
            let Some(found) = top.pop_front() else {
                pending.pop();
                continue;
            };
            if ranks[found].is_some() {
                continue;
            }
            ranks[found] = Some(count);
            count += 1;
            pending.push(table[found].iter().copied().collect());
        }
    }
    for layer in lay.layers.iter_mut() {
        let mut keyed: Vec<(usize, usize, usize)> = layer
            .iter()
            .enumerate()
            .map(|(pos, &item)| (ranks.get(item).copied().flatten().unwrap_or(count), pos, item))
            .collect();
        keyed.sort();
        *layer = keyed.into_iter().map(|k| k.2).collect();
    }
}

struct Minimizer<'a> {
    layers: &'a mut Vec<Vec<usize>>,
    po: PortOrder,
    input_order: Vec<usize>,
    random: JavaRandom,
    node_count: usize,
}

type Ranks = HashMap<usize, usize>;

impl<'a> Minimizer<'a> {
    fn trivial(&self) -> bool {
        self.layers.iter().all(|l| l.is_empty()) || (self.layers.len() == 1 && self.layers[0].len() == 1)
    }

    /// Rank of every port on one side of a layer, top to bottom.
    fn port_ranks(&self, layer: &[usize], side: Side) -> Ranks {
        let mut ranks = Ranks::new();
        for &item in layer {
            for p in self.po.visual(item, side) {
                let r = ranks.len();
                ranks.insert(p, r);
            }
        }
        ranks
    }

    fn sweep_until_stable(&mut self) {
        let mut forward = true;
        let mut crossings = self.sweep_and_count(forward);
        loop {
            let previous = crossings;
            forward = !forward;
            crossings = self.sweep_and_count(forward);
            if !(previous > crossings && crossings > 0) {
                break;
            }
        }
    }

    fn sweep_and_count(&mut self, forward: bool) -> usize {
        let len = self.layers.len() as isize;
        if forward {
            for i in 1..len {
                self.reorder(i as usize, (i - 1) as usize);
            }
        } else {
            for i in (0..=len - 2).rev() {
                self.reorder(i as usize, (i + 1) as usize);
            }
        }
        self.count_crossings()
    }

    fn reorder(&mut self, layer_index: usize, fixed_index: usize) {
        let forward = fixed_index < layer_index;
        let free_side = if forward { Side::West } else { Side::East };
        let ranks = self.port_ranks(&self.layers[fixed_index].clone(), if forward { Side::East } else { Side::West });
        let layer = self.layers[layer_index].clone();
        // Model order keeps the WEST ports of a node in the order of the
        // nodes they come from; the stored WEST list runs bottom to top.
        let input_order = &self.input_order;
        for &item in &layer {
            self.po.sort_by_key(item, free_side, |po, port| {
                let others = po.others(port);
                if free_side == Side::West {
                    -(others.iter().map(|&o| input_order[po.ports[o].item]).min().unwrap() as f64)
                } else {
                    mean(others.iter().map(|o| ranks[o]))
                }
            });
        }
        let sorted = self.sorted(&layer, free_side, &ranks);
        self.layers[layer_index] = sorted;
    }

    fn sorted(&self, layer: &[usize], side: Side, ranks: &Ranks) -> Vec<usize> {
        let bary: Vec<Option<f64>> = layer
            .iter()
            .map(|&item| {
                let others: Vec<usize> =
                    self.po.visual(item, side).into_iter().flat_map(|p| self.po.others(p)).collect();
                (!others.is_empty()).then(|| mean(others.iter().map(|o| ranks[o])))
            })
            .collect();
        // A node with no edge toward the fixed layer takes the barycenter
        // of the node before it, so it keeps its place.
        let lead = bary.iter().flatten().next().copied().unwrap_or(0.0);
        let mut last: Option<f64> = None;
        let keys: Vec<(bool, f64)> = bary
            .iter()
            .map(|&v| {
                if v.is_some() {
                    last = v;
                }
                (v.is_some(), v.or(last).unwrap_or(lead))
            })
            .collect();
        let mut order: Vec<usize> = (0..layer.len()).collect();
        order.sort_by(|&a, &b| {
            let (ka, va) = keys[a];
            let (kb, vb) = keys[b];
            let tie = |known: bool, pos: usize, item: usize| if known { self.input_order[item] } else { pos };
            cmp_f(va, vb)
                .then((!ka).cmp(&!kb))
                .then(tie(ka, a, layer[a]).cmp(&tie(kb, b, layer[b])))
        });
        order.into_iter().map(|i| layer[i]).collect()
    }

    fn count_crossings(&self) -> usize {
        let mut positions: HashMap<usize, (usize, usize)> = HashMap::new();
        for (number, layer) in self.layers.iter().enumerate() {
            for (pos, &item) in layer.iter().enumerate() {
                positions.insert(item, (number, pos));
            }
        }
        let mut visual: HashMap<usize, usize> = HashMap::new();
        for layer in self.layers.iter() {
            for &item in layer {
                for side in [Side::East, Side::West] {
                    for (i, p) in self.po.visual(item, side).into_iter().enumerate() {
                        visual.insert(p, i);
                    }
                }
            }
        }
        let mut gaps: HashMap<usize, Vec<((usize, usize), (usize, usize))>> = HashMap::new();
        for seg in &self.po.segments {
            let from = self.po.ports[seg.from].item;
            let to = self.po.ports[seg.to].item;
            let slot = |item: usize, port: usize| (positions[&item].1, visual[&port]);
            gaps.entry(positions[&from].0).or_default().push((slot(from, seg.from), slot(to, seg.to)));
        }
        gaps.values()
            .map(|pairs| {
                let mut n = 0;
                for (i, (a1, b1)) in pairs.iter().enumerate() {
                    for (a2, b2) in &pairs[i + 1..] {
                        if (a1.cmp(a2) as i32) * (b1.cmp(b2) as i32) == -1 {
                            n += 1;
                        }
                    }
                }
                n
            })
            .sum()
    }

    /// ELK runs its two-sided greedy switch after whichever minimizer ran,
    /// `NONE` included. Its first direction is a draw from the seeded
    /// generator.
    fn greedy_switch(&mut self) {
        if self.node_count >= GREEDY_SWITCH_NODE_LIMIT {
            return;
        }
        self.random.next_long();
        let mut forward = self.random.next_bits(1) != 0;
        loop {
            let mut indexes: Vec<usize> = (0..self.layers.len()).collect();
            if !forward {
                indexes.reverse();
            }
            let (first, free) = indexes.split_first().unwrap();
            let mut improved = self.switch_once(*first);
            for &index in free {
                let mut any = false;
                while self.switch_once(index) {
                    any = true;
                }
                improved |= any;
            }
            forward = !forward;
            if !improved {
                break;
            }
        }
    }

    fn switch_once(&mut self, index: usize) -> bool {
        let sides = self.switch_sides(index);
        let len = self.layers[index].len();
        let mut any = false;
        for upper in 0..len.saturating_sub(1) {
            let (a, b) = (self.layers[index][upper], self.layers[index][upper + 1]);
            if self.crossings_below(a, b, &sides) > self.crossings_below(b, a, &sides) {
                self.layers[index].swap(upper, upper + 1);
                any = true;
            }
        }
        any
    }

    fn switch_sides(&self, index: usize) -> [(Side, Ranks); 2] {
        let neighbour = |i: isize, side: Side| {
            if i < 0 || i as usize >= self.layers.len() {
                Ranks::new()
            } else {
                self.port_ranks(&self.layers[i as usize], side)
            }
        };
        [
            (Side::West, neighbour(index as isize - 1, Side::East)),
            (Side::East, neighbour(index as isize + 1, Side::West)),
        ]
    }

    /// Crossings between the edges of `above` and `below` when `above` sits
    /// on top.
    fn crossings_below(&self, above: usize, below: usize, sides: &[(Side, Ranks); 2]) -> usize {
        let far = |item: usize, side: Side, ranks: &Ranks| -> Vec<usize> {
            self.po
                .visual(item, side)
                .into_iter()
                .flat_map(|p| self.po.others(p))
                .map(|o| ranks[&o])
                .collect()
        };
        sides
            .iter()
            .map(|(side, ranks)| {
                let (hs, ls) = (far(above, *side, ranks), far(below, *side, ranks));
                hs.iter().map(|h| ls.iter().filter(|l| h > l).count()).sum::<usize>()
            })
            .sum()
    }
}

fn mean(values: impl Iterator<Item = usize>) -> f64 {
    let v: Vec<usize> = values.collect();
    v.iter().sum::<usize>() as f64 / v.len() as f64
}

/// Reorders the nodes inside the assigned layers. Returns the final port
/// lists for the node placer.
pub fn minimize(graph: &Graph, index: &NodeIndex, lay: &mut Layering) -> Result<PortOrder, Error> {
    let strategy_none = graph.options.crossing_strategy == "NONE";
    let preserve = graph.options.model_order == "NODES_AND_EDGES";
    let n = graph.children.len();
    let input_order = input_order(n, lay);
    discovery_sort(graph, index, lay);
    let po = PortOrder::new(graph, index, lay);
    let mut m = Minimizer {
        layers: &mut lay.layers,
        po,
        input_order,
        random: JavaRandom::new(RANDOM_SEED),
        node_count: n,
    };
    if !m.trivial() {
        if !strategy_none {
            if !preserve {
                return Err(Error::Unported("elk layer sweep without model order".into()));
            }
            m.sweep_until_stable();
        }
        m.greedy_switch();
    }
    Ok(m.po)
}
