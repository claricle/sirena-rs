//! Phase 4: node placement (elkrb `Layered::NodePlacer`, `BkNodePlacer`,
//! `Bk::{View,Aligner,Compactor}`, `PortSpread`, and the padding step of
//! `LayeredAlgorithm#apply_padding_with_dummies`). Float arithmetic and
//! operation order follow the Ruby source.
//!
//! The orthogonal router widens a layer gap when it needs many routing
//! slots (`NodePlacer#gap_widths`). Slot counts belong to the router; here
//! the gap is `layer_spacing` whenever the number of edges crossing it is
//! small enough that no slot count could exceed it, otherwise `Unported`.
use super::crossing::{PortOrder, Side};
use super::routing;
use super::layering::Item;
use super::LayeredState;
use crate::elk::model::Graph;
use crate::Error;

const EDGE_THICKNESS: f64 = 1.0;
const EDGE_SPACING: f64 = 10.0;
/// `OrthogonalRouter::EDGE_SPACING` and `EDGE_NODE_SPACING`.
const ROUTER_EDGE_SPACING: f64 = 10.0;
const ROUTER_EDGE_NODE_SPACING: f64 = 10.0;

#[derive(Clone, Copy, PartialEq)]
enum Sweep {
    Left,
    Right,
}

/// Positions `graph.children` (x, y) and the dummy slots in
/// `state.layering.items`, then sets `graph.width`/`graph.height`. Runs after
/// crossing minimization: `state.layering.layers` holds each layer's final
/// order and `state.port_order` the port lists the last sweep left.
pub fn run(graph: &mut Graph, state: &mut LayeredState) -> Result<(), Error> {
    place_nodes(graph, state)?;
    apply_padding_with_dummies(graph, state);
    Ok(())
}

fn direction(graph: &Graph) -> &str {
    match graph.options.direction.as_str() {
        "UNDEFINED" => "RIGHT",
        d => d,
    }
}

fn is_dummy(state: &LayeredState, item: usize) -> bool {
    matches!(state.layering.items[item], Item::Dummy(_))
}

/// The `size` of a node or slot along `horizontal`/`width`...
fn dim(graph: &Graph, state: &LayeredState, item: usize, width: bool) -> f64 {
    match &state.layering.items[item] {
        Item::Node(i) => {
            let n = &graph.children[*i];
            (if width { n.width } else { n.height }).unwrap_or(0.0)
        }
        Item::Dummy(d) => {
            if width {
                d.width
            } else {
                d.height
            }
        }
    }
}

fn place_nodes(graph: &mut Graph, state: &mut LayeredState) -> Result<(), Error> {
    if state.layering.layers.is_empty() {
        return Ok(());
    }
    let dir = direction(graph).to_string();
    let horizontal = dir == "RIGHT" || dir == "LEFT";
    let mirrored = dir == "LEFT" || dir == "UP";
    let layer_spacing = graph.options.layer_spacing;
    let node_spacing = graph.options.node_node;
    let layers = state.layering.layers.clone();

    // size_dummy_slots
    for layer in &layers {
        let nodes: Vec<usize> = layer.iter().copied().filter(|&i| !is_dummy(state, i)).collect();
        let max = |width: bool| {
            let mut m: Option<f64> = None;
            for &i in &nodes {
                let v = dim(graph, state, i, width);
                m = Some(match m {
                    Some(c) if c >= v => c,
                    _ => v,
                });
            }
            m.unwrap_or(0.0)
        };
        let (w, h) = (max(true), max(false));
        let (layer_dim, cross_dim) = if horizontal { (w, h) } else { (h, w) };
        for &i in layer {
            if let Item::Dummy(d) = &mut state.layering.items[i] {
                if horizontal {
                    d.width = layer_dim;
                    d.height = cross_dim;
                } else {
                    d.height = layer_dim;
                    d.width = cross_dim;
                }
            }
        }
    }

    let cross_size = |graph: &Graph, state: &LayeredState, i: usize| dim(graph, state, i, !horizontal);
    let layer_size = |graph: &Graph, state: &LayeredState, i: usize| dim(graph, state, i, horizontal);

    // cross coordinates (Brandes-Köpf)
    let n_items = state.layering.items.len();
    let sizes: Vec<f64> = (0..n_items)
        .map(|i| if is_dummy(state, i) { EDGE_THICKNESS } else { cross_size(graph, state, i) })
        .collect();
    let dummy: Vec<bool> = (0..n_items).map(|i| is_dummy(state, i)).collect();
    let spacing = |a: usize, b: usize| {
        if !dummy[a] && !dummy[b] {
            node_spacing
        } else {
            EDGE_SPACING
        }
    };
    let cross = bk_place(&layers, &state.port_order, &sizes, &dummy, &spacing);

    // gap widths (orthogonal routing is the default style): one routing slot
    // per parallel run, with a clear margin at both ends.
    let measure = routing::Measure { start: cross.clone(), extent: sizes.clone() };
    let gaps: Vec<f64> = routing::slot_counts(&layers, &state.port_order, &dummy, &measure)
        .into_iter()
        .map(|slots| {
            if slots == 0 {
                return layer_spacing;
            }
            let runs = (slots as f64 - 1.0) * ROUTER_EDGE_SPACING;
            let margins = 2.0 * ROUTER_EDGE_NODE_SPACING;
            let width = runs + margins;
            if layer_spacing > width { layer_spacing } else { width }
        })
        .collect();

    // layer extents and positions
    let extents: Vec<f64> = layers
        .iter()
        .map(|l| l.iter().map(|&i| layer_size(graph, state, i)).fold(None, |m: Option<f64>, v| match m {
            Some(c) if c >= v => Some(c),
            _ => Some(v),
        }).unwrap_or(0.0))
        .collect();
    let mut positions: Vec<f64> = vec![];
    let mut position = 0.0f64;
    for (i, extent) in extents.iter().enumerate() {
        positions.push(position);
        position += extent + gaps.get(i).copied().unwrap_or(layer_spacing);
    }

    // place_all_layers
    for (li, layer) in layers.iter().enumerate() {
        let extent = extents[li];
        for &i in layer {
            let offset = if dummy[i] {
                0.0
            } else {
                let room = extent - 0.0 - 0.0;
                let Item::Node(c) = state.layering.items[i] else { unreachable!() };
                let ratio = alignment_ratio(graph, state, c, i, horizontal, mirrored);
                0.0 + ((room - layer_size(graph, state, i)) * ratio)
            };
            let lead = if dummy[i] { (cross_size(graph, state, i) - EDGE_THICKNESS) / 2.0 } else { 0.0 };
            let (lp, cp) = (positions[li] + offset, cross[i] - lead);
            let (x, y) = if horizontal { (lp, cp) } else { (cp, lp) };
            match &mut state.layering.items[i] {
                Item::Node(c) => {
                    let n = &mut graph.children[*c];
                    n.x = Some(x);
                    n.y = Some(y);
                }
                Item::Dummy(d) => {
                    d.x = x;
                    d.y = y;
                }
            }
        }
    }

    // mirror_layers
    if mirrored {
        let bound = positions
            .iter()
            .zip(&extents)
            .map(|(p, e)| p + e)
            .fold(f64::NEG_INFINITY, f64::max);
        let x_axis = dir == "LEFT";
        for layer in &layers {
            for &i in layer {
                let size = dim(graph, state, i, x_axis);
                match &mut state.layering.items[i] {
                    Item::Node(c) => {
                        let n = &mut graph.children[*c];
                        let cur = if x_axis { n.x } else { n.y }.unwrap();
                        let v = Some(bound - cur - size);
                        if x_axis { n.x = v } else { n.y = v }
                    }
                    Item::Dummy(d) => {
                        let cur = if x_axis { d.x } else { d.y };
                        let v = bound - cur - size;
                        if x_axis { d.x = v } else { d.y = v }
                    }
                }
            }
        }
    }
    Ok(())
}

fn alignment_ratio(graph: &Graph, state: &LayeredState, c: usize, item: usize, horizontal: bool, mirrored: bool) -> f64 {
    let sides: [(&str, f64); 3] = if horizontal {
        [("LEFT", 0.0), ("RIGHT", 1.0), ("CENTER", 0.5)]
    } else {
        [("TOP", 0.0), ("BOTTOM", 1.0), ("CENTER", 0.5)]
    };
    let alignment = graph.children[c].options.alignment.as_str();
    if let Some((_, ratio)) = sides.iter().find(|(name, _)| *name == alignment) {
        return if mirrored { 1.0 - ratio } else { *ratio };
    }
    let outgoing = state.port_order.visual(item, Side::East).len();
    let incoming = state.port_order.visual(item, Side::West).len();
    let total = outgoing + incoming;
    if total == 0 {
        0.5
    } else {
        outgoing as f64 / total as f64
    }
}

// ---------------------------------------------------------------- BK

/// `BkNodePlacer#place` (alignment :smallest): cross-axis start of every
/// layer item, shifted so the smallest is 0.
fn bk_place(
    layers: &[Vec<usize>],
    po: &PortOrder,
    sizes: &[f64],
    dummy: &[bool],
    spacing: &dyn Fn(usize, usize) -> f64,
) -> Vec<f64> {
    let n = sizes.len();
    let offsets = port_offsets(layers, po, sizes);
    let marked = type_one_conflicts(layers, po, dummy);
    let passes = [(Sweep::Left, false), (Sweep::Left, true), (Sweep::Right, false), (Sweep::Right, true)];
    let mut results: Vec<Vec<f64>> = vec![];
    for (sweep, up) in passes {
        let view = View::new(layers, po, sizes, &offsets, sweep, up);
        let aligner = Aligner::new(&view, &marked, n);
        let mut starts = Compactor::new(&view, &aligner, spacing, n).place();
        if up {
            for (id, s) in starts.iter_mut().enumerate() {
                if view.has(id) {
                    *s = -(*s + sizes[id]);
                }
            }
        }
        results.push(starts);
    }
    // narrowest: first pass with the least span width
    let present: Vec<usize> = layers.iter().flatten().copied().collect();
    let mut best: Option<(f64, usize)> = None;
    for (pi, tops) in results.iter().enumerate() {
        let lo = present.iter().map(|&i| tops[i]).fold(f64::INFINITY, f64::min);
        let hi = present.iter().map(|&i| tops[i] + sizes[i]).fold(f64::NEG_INFINITY, f64::max);
        let w = hi - lo;
        if best.map_or(true, |(bw, _)| w < bw) {
            best = Some((w, pi));
        }
    }
    let tops = &results[best.map_or(0, |b| b.1)];
    let low = present.iter().map(|&i| tops[i]).fold(f64::INFINITY, f64::min);
    let mut out = vec![0.0; n];
    for &i in &present {
        out[i] = tops[i] - low;
    }
    out
}

/// `PortSpread.offsets`: no declared ports, so every port has width 0 and
/// they spread evenly along the item's cross extent.
pub(super) fn port_offsets(layers: &[Vec<usize>], po: &PortOrder, sizes: &[f64]) -> Vec<f64> {
    let mut offsets = vec![0.0; po.ports.len()];
    for &item in layers.iter().flatten() {
        for side in [Side::East, Side::West] {
            let ports = po.visual(item, side);
            let gap = (sizes[item] - 0.0) / (ports.len() as f64 + 1.0);
            for (i, &p) in ports.iter().enumerate() {
                offsets[p] = 0.0 + gap * (i as f64 + 1.0) + 0.0 / 2.0;
            }
        }
    }
    offsets
}

fn segments_toward_west(po: &PortOrder, item: usize) -> Vec<usize> {
    po.visual(item, Side::West).into_iter().flat_map(|p| po.ports[p].segments.clone()).collect()
}

fn inner(po: &PortOrder, dummy: &[bool], seg: usize) -> bool {
    let s = &po.segments[seg];
    dummy[po.ports[s.from].item] && dummy[po.ports[s.to].item]
}

fn type_one_conflicts(layers: &[Vec<usize>], po: &PortOrder, dummy: &[bool]) -> Vec<bool> {
    let mut marked = vec![false; po.segments.len()];
    for pair in layers.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        let pos = |item: usize| left.iter().position(|&x| x == item).unwrap();
        let mut low: isize = 0;
        let mut scanned = 0usize;
        for (index, &item) in right.iter().enumerate() {
            let last = index == right.len() - 1;
            let inner_seg = if dummy[item] {
                segments_toward_west(po, item).into_iter().find(|&s| inner(po, dummy, s))
            } else {
                None
            };
            let high: Option<isize> = match inner_seg {
                Some(s) => Some(pos(po.ports[po.segments[s].from].item) as isize),
                None if last => Some(left.len() as isize - 1),
                None => None,
            };
            let Some(high) = high else { continue };
            for &run_item in &right[scanned..=index] {
                for s in segments_toward_west(po, run_item) {
                    let above = pos(po.ports[po.segments[s].from].item) as isize;
                    if !(above >= low && above <= high) && !inner(po, dummy, s) {
                        marked[s] = true;
                    }
                }
            }
            scanned = index + 1;
            low = high;
        }
    }
    marked
}

struct Link {
    neighbor: usize,
    own_offset: f64,
    neighbor_offset: f64,
    segment: usize,
}

struct View<'a> {
    layers: Vec<Vec<usize>>,
    po: &'a PortOrder,
    sizes: &'a [f64],
    offsets: &'a [f64],
    facing: Side,
    flip: bool,
    position: Vec<usize>,
    layer_of: Vec<usize>,
    present: Vec<bool>,
    links: std::cell::RefCell<Vec<Option<std::rc::Rc<Vec<Link>>>>>,
}

impl<'a> View<'a> {
    fn new(layers: &[Vec<usize>], po: &'a PortOrder, sizes: &'a [f64], offsets: &'a [f64], sweep: Sweep, up: bool) -> View<'a> {
        let mut ordered: Vec<Vec<usize>> = layers.to_vec();
        if sweep == Sweep::Right {
            ordered.reverse();
        }
        if up {
            for l in ordered.iter_mut() {
                l.reverse();
            }
        }
        let n = sizes.len();
        let (mut position, mut layer_of, mut present) = (vec![0; n], vec![0; n], vec![false; n]);
        for (number, layer) in ordered.iter().enumerate() {
            for (index, &id) in layer.iter().enumerate() {
                position[id] = index;
                layer_of[id] = number;
                present[id] = true;
            }
        }
        View {
            layers: ordered,
            po,
            sizes,
            offsets,
            facing: if sweep == Sweep::Left { Side::West } else { Side::East },
            flip: up,
            position,
            layer_of,
            present,
            links: std::cell::RefCell::new(vec![None; n]),
        }
    }

    fn has(&self, id: usize) -> bool {
        self.present[id]
    }

    fn predecessor(&self, id: usize) -> Option<usize> {
        let index = self.position[id];
        if index > 0 {
            Some(self.layers[self.layer_of[id]][index - 1])
        } else {
            None
        }
    }

    fn offset(&self, id: usize, port: usize) -> f64 {
        let v = self.offsets[port];
        if self.flip {
            self.sizes[id] - v
        } else {
            v
        }
    }

    fn links(&self, id: usize) -> std::rc::Rc<Vec<Link>> {
        if let Some(l) = &self.links.borrow()[id] {
            return l.clone();
        }
        let mut all: Vec<Link> = vec![];
        for port in self.po.visual(id, self.facing) {
            for &seg in &self.po.ports[port].segments {
                let s = &self.po.segments[seg];
                let other = if s.from == port { s.to } else { s.from };
                let neighbor = self.po.ports[other].item;
                all.push(Link {
                    neighbor,
                    own_offset: self.offset(id, port),
                    neighbor_offset: self.offset(neighbor, other),
                    segment: seg,
                });
            }
        }
        let mut keyed: Vec<(usize, usize, Link)> =
            all.into_iter().enumerate().map(|(i, l)| (self.position[l.neighbor], i, l)).collect();
        keyed.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let rc = std::rc::Rc::new(keyed.into_iter().map(|k| k.2).collect::<Vec<_>>());
        self.links.borrow_mut()[id] = Some(rc.clone());
        rc
    }
}

struct Aligner {
    root: Vec<usize>,
    align: Vec<usize>,
    inner: Vec<f64>,
}

impl Aligner {
    fn new(view: &View, marked: &[bool], n: usize) -> Aligner {
        let mut a = Aligner { root: (0..n).collect(), align: (0..n).collect(), inner: vec![0.0; n] };
        let mut via: Vec<Option<(f64, f64)>> = vec![None; n]; // (own_offset, neighbor_offset)
        // align_layers
        for layer in view.layers.iter().skip(1) {
            let mut previous: isize = -1;
            for &id in layer {
                let links = view.links(id);
                if links.is_empty() {
                    continue;
                }
                let lo = (links.len() - 1) / 2;
                let hi = links.len() / 2;
                let found = links[lo..=hi]
                    .iter()
                    .find(|l| !marked[l.segment] && previous < view.position[l.neighbor] as isize);
                let Some(link) = found else { continue };
                let upper = link.neighbor;
                a.align[upper] = id;
                a.root[id] = a.root[upper];
                a.align[id] = a.root[id];
                via[id] = Some((link.own_offset, link.neighbor_offset));
                previous = view.position[upper] as isize;
            }
        }
        // shift_blocks
        let order: Vec<usize> = view.layers.iter().flatten().copied().collect();
        for &id in &order {
            if a.root[id] == id {
                let chain = a.block(id);
                for w in chain.windows(2) {
                    let (upper, lower) = (w[0], w[1]);
                    let (own, nb) = via[lower].unwrap();
                    a.inner[lower] = a.inner[upper] + (nb - own);
                }
                let top = chain.iter().map(|&m| a.inner[m]).fold(f64::INFINITY, f64::min);
                for &m in &chain {
                    a.inner[m] -= top;
                }
            }
        }
        a
    }

    fn block(&self, root: usize) -> Vec<usize> {
        let mut chain = vec![root];
        let mut node = self.align[root];
        while node != root {
            chain.push(node);
            node = self.align[node];
        }
        chain
    }
}

struct Compactor<'a> {
    view: &'a View<'a>,
    aligner: &'a Aligner,
    spacing: &'a dyn Fn(usize, usize) -> f64,
    coordinate: Vec<Option<f64>>,
    sink: Vec<usize>,
    shift: Vec<f64>,
}

impl<'a> Compactor<'a> {
    fn new(view: &'a View<'a>, aligner: &'a Aligner, spacing: &'a dyn Fn(usize, usize) -> f64, n: usize) -> Self {
        Compactor { view, aligner, spacing, coordinate: vec![None; n], sink: (0..n).collect(), shift: vec![f64::INFINITY; n] }
    }

    fn place(mut self) -> Vec<f64> {
        let order: Vec<usize> = self.view.layers.iter().flatten().copied().collect();
        for &id in &order {
            if self.aligner.root[id] == id {
                self.place_block(id);
            }
        }
        let mut out = vec![0.0; self.coordinate.len()];
        for &id in &order {
            let root = self.aligner.root[id];
            let shift = self.shift[self.sink[root]];
            let mut base = self.coordinate[root].unwrap();
            if shift.is_finite() {
                base += shift;
            }
            out[id] = base + self.aligner.inner[id];
        }
        out
    }

    fn place_block(&mut self, root: usize) {
        if self.coordinate[root].is_some() {
            return;
        }
        self.coordinate[root] = Some(0.0);
        for member in self.aligner.block(root) {
            if let Some(neighbour) = self.view.predecessor(member) {
                self.place_against(root, member, neighbour);
            }
        }
    }

    fn place_against(&mut self, root: usize, member: usize, neighbour: usize) {
        let other = self.aligner.root[neighbour];
        self.place_block(other);
        if self.sink[root] == root {
            self.sink[root] = self.sink[other];
        }
        let required = self.aligner.inner[neighbour] + self.view.sizes[neighbour] + (self.spacing)(neighbour, member)
            - self.aligner.inner[member];
        if self.sink[root] == self.sink[other] {
            let c = self.coordinate[root].unwrap();
            let d = self.coordinate[other].unwrap() + required;
            self.coordinate[root] = Some(if d > c { d } else { c });
        } else {
            let sink = self.sink[other];
            let slack = self.coordinate[root].unwrap() - self.coordinate[other].unwrap() - required;
            if slack < self.shift[sink] {
                self.shift[sink] = slack;
            }
        }
    }
}

// ----------------------------------------------------------- padding

/// `LayeredAlgorithm#apply_padding_with_dummies`.
fn apply_padding_with_dummies(graph: &mut Graph, state: &mut LayeredState) {
    // padding_shift / apply_padding
    let first_before = graph.children.first().map(|c| (c.x.unwrap_or(f64::NAN), c.y.unwrap_or(f64::NAN)));
    let pad = graph.options.padding;
    if !graph.children.is_empty() {
        let xs = |c: &crate::elk::model::Node| c.x.unwrap_or(0.0);
        let ys = |c: &crate::elk::model::Node| c.y.unwrap_or(0.0);
        let min_x = graph.children.iter().map(xs).fold(f64::INFINITY, f64::min);
        let min_y = graph.children.iter().map(ys).fold(f64::INFINITY, f64::min);
        let max_x = graph.children.iter().map(|c| xs(c) + c.width.unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
        let max_y = graph.children.iter().map(|c| ys(c) + c.height.unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
        let (bw, bh) = (max_x - min_x, max_y - min_y);
        for c in graph.children.iter_mut() {
            c.x = Some(c.x.unwrap_or(0.0) - min_x + pad.left);
            c.y = Some(c.y.unwrap_or(0.0) - min_y + pad.top);
        }
        graph.width = Some(bw + pad.left + pad.right);
        graph.height = Some(bh + pad.top + pad.bottom);
    }
    let (shift_x, shift_y) = match first_before {
        Some((bx, by)) => {
            let f = &graph.children[0];
            (f.x.unwrap() - bx, f.y.unwrap() - by)
        }
        None => (0.0, 0.0),
    };
    let slots: Vec<usize> =
        (0..state.layering.items.len()).filter(|&i| matches!(state.layering.items[i], Item::Dummy(_))).collect();
    for &i in &slots {
        if let Item::Dummy(d) = &mut state.layering.items[i] {
            d.x += shift_x;
            d.y += shift_y;
        }
    }
    if slots.is_empty() {
        return;
    }
    // include_long_edges
    let vertical = matches!(graph.options.direction.as_str(), "DOWN" | "UP");
    let centre = |d: &super::layering::DummySlot| if vertical { d.x + d.width / 2.0 } else { d.y + d.height / 2.0 };
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for &i in &slots {
        if let Item::Dummy(d) = &state.layering.items[i] {
            let c = centre(d);
            if c < low {
                low = c;
            }
            if c > high {
                high = c;
            }
        }
    }
    let reach = EDGE_THICKNESS / 2.0;
    let (lead, trail) = if vertical { (pad.left, pad.right) } else { (pad.top, pad.bottom) };
    let extent = if vertical { graph.width.unwrap() } else { graph.height.unwrap() };
    let (start, stop) = (lead, extent - trail);
    let before = (start - (low - reach)).max(0.0);
    let after = (high + reach - stop).max(0.0);
    for c in graph.children.iter_mut() {
        if vertical {
            c.x = Some(c.x.unwrap() + before);
        } else {
            c.y = Some(c.y.unwrap() + before);
        }
    }
    for &i in &slots {
        if let Item::Dummy(d) = &mut state.layering.items[i] {
            if vertical {
                d.x += before;
            } else {
                d.y += before;
            }
        }
    }
    let amount = before + after;
    if amount > 0.0 {
        if vertical {
            graph.width = Some(graph.width.unwrap() + amount);
        } else {
            graph.height = Some(graph.height.unwrap() + amount);
        }
    }
}
