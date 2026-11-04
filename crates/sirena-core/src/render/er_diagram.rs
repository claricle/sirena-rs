//! ER diagram renderer: port of Ruby `Parser::Builders::ErDiagram` (model
//! rules), `Layout::ErDiagram*` (layering, ordering, coordinates, markers)
//! and `Renderer::ErDiagram` (class styles included).
//!
//! Declined to the Ruby path (`Error::Unported`): an entity whose id contains
//! `:` (it could collide with a layout placeholder id).
use super::pie::{fsum, items, strip, text_of};
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text;
use crate::Error;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::sync::OnceLock;

const FONT_SIZE: f64 = 16.0;
const TEXT_HEIGHT: i64 = 24;
const PADDING: f64 = 25.0;
const TEXT_PADDING: f64 = 18.75;
const ROW_HEIGHT: f64 = TEXT_HEIGHT as f64 + TEXT_PADDING;
const LABEL_PADDING_X: f64 = 20.0;
const LABEL_PADDING_Y: i64 = 30;
const MIN_ENTITY_WIDTH: f64 = 100.0;
const ENTITY_PADDING: f64 = 10.0;
const CARDINALITY_SIZE: i64 = 15;
const CANVAS_MARGIN: f64 = 8.0;
const EMPTY_DIAGRAM_PADDING: f64 = 8.0;

// placement
const NODE_SPACING: f64 = 140.0;
const RANK_SPACING: f64 = 80.0;
const EDGE_SPACING: f64 = 20.0;
const MARGIN: f64 = 8.0;
const LABEL_FONT_SIZE: f64 = 14.0;
const LABEL_HEIGHT: f64 = 21.0;
const MIN_LENGTH: i64 = 2;
const MAX_PASSES: usize = 100;
const STALL_LIMIT: usize = 4;

/// A frontmatter/directive layout choice (e.g. `layout: elk`) selects another
/// placement engine, which is not ported.
fn source_layout(source: &str) -> bool {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    preamble.contains("layout")
}

fn unported() -> Error {
    Error::Unported("er_diagram".into())
}

// ---- model (builder) ----

#[derive(Default, Clone)]
struct Attr {
    name: String,
    ty: Option<String>,
    key: Option<String>,
    note: Option<String>,
}

struct Ent {
    id: String,
    attrs: Vec<Attr>,
    classes: Vec<String>,
}

struct Rel {
    from: String,
    to: String,
    identifying: bool,
    card_from: Option<&'static str>,
    card_to: Option<&'static str>,
    label: Option<String>,
}

#[derive(Default)]
struct Model {
    ents: Vec<Ent>,
    rels: Vec<Rel>,
    class_defs: Vec<(String, String)>,
}

fn cardinality(sym: &str) -> Option<&'static str> {
    Some(match sym {
        "||" => "one",
        "o{" | "{o" => "zero_or_more",
        "|{" | "{|" | "}{" | "{}" | "}|" => "one_or_more",
        "}o" | "o|" | "|o" => "zero_or_one",
        _ => return None,
    })
}

/// Ruby `String#split(sep)`: trailing empty fields dropped.
fn ruby_split<'a>(s: &'a str, sep: char) -> Vec<&'a str> {
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

fn class_names(v: &Value) -> Vec<String> {
    ruby_split(&text_of(v), ',').into_iter().map(|s| strip(s).to_string()).collect()
}

fn unquote(v: &Value) -> String {
    let s = text_of(v);
    let s = s.strip_prefix('"').unwrap_or(&s);
    s.strip_suffix('"').unwrap_or(s).to_string()
}

fn extract_text(v: &Value) -> String {
    match v {
        Value::Object(o) if !(o.contains_key("offset") && o.contains_key("value")) => {
            if let Some(s) = o.get("string") {
                return to_s(s);
            }
            if let Some(k) = o.get("key_type") {
                if !k.is_null() {
                    return to_s(k);
                }
            }
            o.values().next().map(to_s).unwrap_or_default()
        }
        v => to_s(v),
    }
}

fn to_s(v: &Value) -> String {
    match v {
        Value::Array(a) if a.is_empty() => "[]".to_string(),
        Value::Null => String::new(),
        v => text_of(v),
    }
}

fn list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v) => vec![v],
    }
}

fn parse_generic_types(input: &str) -> String {
    // input.split(/(,)/): fields and the captured commas; trailing empties dropped
    let mut sets: Vec<String> = vec![];
    for (i, part) in input.split(',').enumerate() {
        if i > 0 {
            sets.push(",".into());
        }
        sets.push(part.to_string());
    }
    while sets.last().is_some_and(|s| s.is_empty()) {
        sets.pop();
    }
    let tildes = |s: &str| s.matches('~').count();
    let mut output: Vec<String> = vec![];
    let mut index = 0;
    while index < sets.len() {
        let combinable = sets[index] == "," && index > 0 && index + 1 < sets.len() && tildes(&sets[index - 1]) == 1 && tildes(&sets[index + 1]) == 1;
        let current = if combinable {
            output.pop();
            let c = format!("{},{}", sets[index - 1], sets[index + 1]);
            index += 2;
            c
        } else {
            index += 1;
            sets[index - 1].clone()
        };
        output.push(process_tilde_set(&current));
    }
    output.concat()
}

fn process_tilde_set(input: &str) -> String {
    let starting = input.matches('~').count() % 2 == 1 && input.starts_with('~');
    let body = if starting { &input[1..] } else { input };
    let mut chars: Vec<char> = body.chars().collect();
    let indices: Vec<usize> = chars.iter().enumerate().filter(|(_, c)| **c == '~').map(|(i, _)| i).collect();
    for pair in 0..indices.len() / 2 {
        chars[indices[pair]] = '<';
        chars[indices[indices.len() - 1 - pair]] = '>';
    }
    let mut out: String = chars.into_iter().collect();
    if starting {
        out.insert(0, '~');
    }
    out
}

fn find_or_create(m: &mut Model, id: &str) -> usize {
    if let Some(i) = m.ents.iter().position(|e| e.id == id) {
        return i;
    }
    m.ents.push(Ent { id: id.to_string(), attrs: vec![], classes: vec![] });
    m.ents.len() - 1
}

fn build(tree: &Value) -> Result<Model, Error> {
    let mut m = Model::default();
    for stmt in items(tree.get("statements").unwrap_or(&Value::Null)) {
        let has = |k: &str| stmt.get(k).is_some_and(|v| !v.is_null());
        if has("classdef_names") {
            let styles = to_s(&stmt["classdef_styles"]);
            for name in class_names(&stmt["classdef_names"]) {
                match m.class_defs.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, d)) => *d = format!("{d},{styles}"),
                    None => m.class_defs.push((name, styles.clone())),
                }
            }
        } else if has("entity_id") {
            let id = unquote(&stmt["entity_id"]);
            let i = find_or_create(&mut m, &id);
            if has("entity_classes") {
                m.ents[i].classes.extend(class_names(&stmt["entity_classes"]));
            }
            for a in list(stmt.get("attributes")).into_iter().filter(|a| a.is_object()) {
                let mut attr = Attr { name: to_s(&a["name"]), ..Default::default() };
                if has_v(a, "type") {
                    attr.ty = Some(parse_generic_types(&extract_text(&a["type"])));
                }
                if has_v(a, "key") {
                    let keys: Vec<String> = list(a.get("key"))
                        .into_iter()
                        .map(extract_text)
                        .filter(|k| matches!(k.as_str(), "PK" | "FK" | "UK"))
                        .collect();
                    if !keys.is_empty() {
                        attr.key = Some(keys.join(","));
                    }
                }
                if has_v(a, "note") {
                    attr.note = Some(extract_text(&a["note"]));
                }
                m.ents[i].attrs.push(attr);
            }
        } else if has("from_id") && has("to_id") && has("pattern") {
            let (from, to) = (unquote(&stmt["from_id"]), unquote(&stmt["to_id"]));
            let a = find_or_create(&mut m, &from);
            let b = find_or_create(&mut m, &to);
            if has("from_classes") {
                m.ents[a].classes.extend(class_names(&stmt["from_classes"]));
            }
            if has("to_classes") {
                m.ents[b].classes.extend(class_names(&stmt["to_classes"]));
            }
            let pattern = &stmt["pattern"];
            let label = stmt
                .get("label")
                .and_then(|l| l.get("label_text"))
                .filter(|v| !v.is_null())
                .map(|l| strip(&extract_text(l)).to_string())
                .filter(|t| !t.is_empty());
            m.rels.push(Rel {
                from,
                to,
                identifying: extract_text(&pattern["operator"]) == "==",
                card_from: cardinality(&extract_text(&pattern["card_from"])),
                card_to: cardinality(&extract_text(&pattern["card_to"])),
                label,
            });
        }
    }
    if m.ents.iter().any(|e| e.id.contains(':')) {
        return Err(unported());
    }
    Ok(m)
}

fn has_v(v: &Value, k: &str) -> bool {
    v.get(k).is_some_and(|x| !x.is_null())
}

// ---- entity size ----

fn text_width(t: &str) -> f64 {
    let sum: i64 = t.chars().map(|c| text::trebuchet_advance_of(c as u32).unwrap_or(1000)).sum();
    (sum * 16) as f64 / 1000.0
}

fn key_cell(key: &Option<String>) -> String {
    let k = key.clone().unwrap_or_default();
    let mut parts: Vec<&str> = k.split(',').map(|p| p.trim()).collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    if k.is_empty() {
        parts.clear();
    }
    parts.join(",")
}

/// (width, height) of an entity box.
fn entity_size(name: &str, attrs: &[Attr]) -> (f64, f64) {
    if attrs.is_empty() {
        let width = text_width(name) + LABEL_PADDING_X * 2.0;
        return (width.max(MIN_ENTITY_WIDTH), (TEXT_HEIGHT + LABEL_PADDING_Y * 2) as f64);
    }
    let cells: Vec<[String; 4]> = attrs
        .iter()
        .map(|a| {
            [
                a.ty.clone().unwrap_or_default(),
                a.name.clone(),
                key_cell(&a.key),
                a.note.clone().unwrap_or_default(),
            ]
        })
        .collect();
    let columns: Vec<f64> = (0..4)
        .map(|c| {
            let widest = cells.iter().map(|row| text_width(&row[c])).fold(f64::NEG_INFINITY, f64::max) + PADDING;
            if c >= 2 && widest <= PADDING { 0.0 } else { widest }
        })
        .collect();
    let widest_cell = cells.iter().flat_map(|r| r.iter()).map(|c| text_width(c)).fold(f64::NEG_INFINITY, f64::max);
    let width = fsum(columns).max(text_width(name) + PADDING * 2.0).max(widest_cell + PADDING * 2.0);
    (width, ROW_HEIGHT * (attrs.len() as f64 + 1.0))
}

// ---- placement ----

struct Adj {
    succ: Vec<Vec<usize>>,
    pred: Vec<Vec<usize>>,
}

fn adjacency(n: usize, edges: &[(usize, usize)]) -> Adj {
    let mut a = Adj { succ: vec![vec![]; n], pred: vec![vec![]; n] };
    for &(from, to) in edges {
        a.succ[from].push(to);
        a.pred[to].push(from);
    }
    a
}

fn visit_cycles(id: usize, edges: &[(usize, usize)], on_stack: &mut [bool], seen: &mut [bool], reversed: &mut [bool]) {
    if seen[id] {
        return;
    }
    seen[id] = true;
    on_stack[id] = true;
    for (index, &(from, to)) in edges.iter().enumerate() {
        if from != id || to == id {
            continue;
        }
        if on_stack[to] {
            reversed[index] = true;
        }
        visit_cycles(to, edges, on_stack, seen, reversed);
    }
    on_stack[id] = false;
}

/// `ErDiagramLayering.call`: ranks per id and the oriented edges.
fn layering(n: usize, edges: &[(usize, usize)]) -> (Vec<i64>, Vec<Option<(usize, usize)>>) {
    let (mut on_stack, mut seen, mut reversed) = (vec![false; n], vec![false; n], vec![false; edges.len()]);
    for id in 0..n {
        visit_cycles(id, edges, &mut on_stack, &mut seen, &mut reversed);
    }
    let oriented: Vec<Option<(usize, usize)>> = edges
        .iter()
        .enumerate()
        .map(|(i, &(from, to))| if from == to { None } else if reversed[i] { Some((to, from)) } else { Some((from, to)) })
        .collect();
    let compact: Vec<(usize, usize)> = oriented.iter().flatten().copied().collect();
    let mut ranks = vec![0i64; n];
    for _ in 0..n + 1 {
        for &(from, to) in &compact {
            ranks[to] = ranks[to].max(ranks[from] + MIN_LENGTH);
        }
    }
    for _ in 0..MAX_PASSES {
        let moved: Vec<bool> = (0..n)
            .map(|id| {
                let incoming: Vec<usize> = compact.iter().filter(|e| e.1 == id).map(|e| e.0).collect();
                let outgoing: Vec<usize> = compact.iter().filter(|e| e.0 == id).map(|e| e.1).collect();
                let target = match incoming.len().cmp(&outgoing.len()) {
                    std::cmp::Ordering::Greater => incoming.iter().map(|&f| ranks[f] + MIN_LENGTH).max(),
                    std::cmp::Ordering::Less => outgoing.iter().map(|&t| ranks[t] - MIN_LENGTH).min(),
                    std::cmp::Ordering::Equal => None,
                };
                match target {
                    Some(t) if t != ranks[id] => {
                        ranks[id] = t;
                        true
                    }
                    _ => false,
                }
            })
            .collect();
        if !moved.iter().any(|m| *m) {
            break;
        }
    }
    let floor = ranks.iter().copied().min().unwrap_or(0);
    (ranks.into_iter().map(|r| r - floor).collect(), oriented)
}

fn descend(id: usize, ranks: &[i64], adj: &Adj, layers: &mut [Vec<usize>], seen: &mut [bool]) {
    if seen[id] {
        return;
    }
    seen[id] = true;
    layers[ranks[id] as usize].push(id);
    for &to in &adj.succ[id] {
        descend(to, ranks, adj, layers, seen);
    }
}

fn barycenter(neighbours: &[usize], position: &HashMap<usize, usize>) -> Option<f64> {
    let near: Vec<usize> = neighbours.iter().filter_map(|o| position.get(o).copied()).collect();
    if near.is_empty() {
        None
    } else {
        Some(near.iter().sum::<usize>() as f64 / near.len() as f64)
    }
}

fn reorder(layer: &[usize], fixed: &[usize], neighbours: &[Vec<usize>], bias_right: bool) -> Vec<usize> {
    let position: HashMap<usize, usize> = fixed.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut movable: Vec<(usize, usize, f64)> = vec![];
    let mut keepers: Vec<(usize, usize)> = vec![];
    for (index, &id) in layer.iter().enumerate() {
        match barycenter(&neighbours[id], &position) {
            Some(c) => movable.push((id, index, c)),
            None => keepers.push((id, index)),
        }
    }
    movable.sort_by(|a, b| {
        let key = |m: &(usize, usize, f64)| if bias_right { -(m.1 as i64) } else { m.1 as i64 };
        a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal).then(key(a).cmp(&key(b)))
    });
    let mut ordered: Vec<usize> = vec![];
    let mut keepers: std::collections::VecDeque<(usize, usize)> = keepers.into();
    let take = |ordered: &mut Vec<usize>, keepers: &mut std::collections::VecDeque<(usize, usize)>| {
        while keepers.front().is_some_and(|k| k.1 <= ordered.len()) {
            ordered.push(keepers.pop_front().unwrap().0);
        }
    };
    take(&mut ordered, &mut keepers);
    for (id, _, _) in movable {
        ordered.push(id);
        take(&mut ordered, &mut keepers);
    }
    ordered
}

fn crossings_between(upper: &[usize], lower: &[usize], adj: &Adj) -> usize {
    let position: HashMap<usize, usize> = lower.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut pairs: Vec<usize> = vec![];
    for &id in upper {
        let mut ps: Vec<usize> = adj.succ[id].iter().filter_map(|t| position.get(t).copied()).collect();
        ps.sort();
        pairs.extend(ps);
    }
    (0..pairs.len()).map(|i| pairs[i + 1..].iter().filter(|&&l| l < pairs[i]).count()).sum()
}

fn crossing_count(layers: &[Vec<usize>], adj: &Adj) -> usize {
    layers.windows(2).map(|w| crossings_between(&w[0], &w[1], adj)).sum()
}

/// `ErDiagramOrdering.call`.
fn ordering(ranks: &[i64], adj: &Adj) -> Vec<Vec<usize>> {
    let count = ranks.iter().copied().max().map_or(0, |m| m + 1) as usize;
    let mut layers: Vec<Vec<usize>> = vec![vec![]; count];
    let mut seen = vec![false; ranks.len()];
    let mut order: Vec<usize> = (0..ranks.len()).collect();
    order.sort_by_key(|&i| (ranks[i], i));
    for id in order {
        descend(id, ranks, adj, &mut layers, &mut seen);
    }
    let mut best: Option<(Vec<Vec<usize>>, usize)> = None;
    let mut stalled = 0;
    let mut sweep = 0usize;
    loop {
        let mut next = layers.clone();
        let downwards = sweep % 2 == 1;
        let bias_right = sweep % 4 >= 2;
        let plan: Vec<usize> = if downwards { (1..next.len()).collect() } else { (0..next.len().saturating_sub(1)).rev().collect() };
        for rank in plan {
            let (fixed, neighbours) = if downwards { (&next[rank - 1], &adj.pred) } else { (&next[rank + 1], &adj.succ) };
            let reordered = reorder(&next[rank], fixed, neighbours, bias_right);
            next[rank] = reordered;
        }
        layers = next;
        let crossings = crossing_count(&layers, adj);
        match &best {
            Some((_, c)) if *c <= crossings => stalled += 1,
            _ => {
                best = Some((layers.clone(), crossings));
                stalled = 1;
            }
        }
        if stalled >= STALL_LIMIT {
            return best.unwrap().0;
        }
        sweep += 1;
    }
}

fn conflicts(layers: &[Vec<usize>], adj: &Adj, dummies: &[bool]) -> HashSet<(usize, usize)> {
    let mut order = vec![0usize; dummies.len()];
    for layer in layers {
        for (i, &id) in layer.iter().enumerate() {
            order[id] = i;
        }
    }
    let inner = |id: usize| -> Option<usize> {
        if !dummies[id] {
            return None;
        }
        adj.pred[id].iter().copied().find(|&u| dummies[u])
    };
    let mut found = HashSet::new();
    for w in layers.windows(2) {
        let (previous, layer) = (&w[0], &w[1]);
        let (mut left, mut scanned) = (0usize, 0usize);
        for (index, &id) in layer.iter().enumerate() {
            let other = inner(id);
            let right = other.map_or(previous.len(), |o| order[o]);
            if !(other.is_some() || Some(&id) == layer.last()) {
                continue;
            }
            for &node in &layer[scanned..=index] {
                for &upper in &adj.pred[node] {
                    let position = order[upper];
                    if (position < left || right < position) && !(dummies[upper] && dummies[node]) {
                        found.insert((upper, node));
                        found.insert((node, upper));
                    }
                }
            }
            scanned = index + 1;
            left = right;
        }
    }
    found
}

fn alignment(neighbours: &[Vec<usize>], conflicts: &HashSet<(usize, usize)>, layers: &[Vec<usize>], n: usize) -> Vec<usize> {
    let mut root: Vec<usize> = (0..n).collect();
    let mut align: Vec<usize> = (0..n).collect();
    let mut position = vec![0usize; n];
    for layer in layers {
        for (i, &id) in layer.iter().enumerate() {
            root[id] = id;
            align[id] = id;
            position[id] = i;
        }
        let mut previous: i64 = -1;
        for &id in layer {
            let mut candidates = neighbours[id].clone();
            candidates.sort_by_key(|&o| position[o]);
            if candidates.is_empty() {
                continue;
            }
            let middle = (candidates.len() as f64 - 1.0) / 2.0;
            for index in (middle.floor() as usize)..=(middle.ceil() as usize) {
                let other = candidates[index];
                if align[id] == id && previous < position[other] as i64 && !conflicts.contains(&(id, other)) {
                    align[other] = id;
                    root[id] = root[other];
                    align[id] = root[id];
                    previous = position[other] as i64;
                }
            }
        }
    }
    root
}

#[derive(Default)]
struct OMap {
    keys: Vec<usize>,
    inner: HashMap<usize, Vec<(usize, f64)>>,
}

impl OMap {
    fn touch(&mut self, k: usize) {
        if !self.inner.contains_key(&k) {
            self.keys.push(k);
            self.inner.insert(k, vec![]);
        }
    }
    fn get(&mut self, k: usize, from: usize) -> Option<f64> {
        self.touch(k);
        self.inner[&k].iter().find(|(f, _)| *f == from).map(|(_, g)| *g)
    }
    fn set(&mut self, k: usize, from: usize, gap: f64) {
        self.touch(k);
        let v = self.inner.get_mut(&k).unwrap();
        match v.iter_mut().find(|(f, _)| *f == from) {
            Some(slot) => slot.1 = gap,
            None => v.push((from, gap)),
        }
    }
}

fn visit(graph: &OMap, f: &mut dyn FnMut(usize)) {
    let mut done: HashSet<usize> = HashSet::new();
    for &start in &graph.keys {
        let mut stack: Vec<(usize, bool)> = vec![(start, false)];
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                f(id);
                continue;
            }
            if !done.insert(id) {
                continue;
            }
            stack.push((id, true));
            for (o, _) in &graph.inner[&id] {
                stack.push((*o, false));
            }
        }
    }
}

fn compaction(layers: &[Vec<usize>], root: &[usize], sep: &dyn Fn(usize, usize) -> f64, n: usize) -> Vec<f64> {
    let (mut incoming, mut outgoing) = (OMap::default(), OMap::default());
    for layer in layers {
        for w in layer.windows(2) {
            let (left, right) = (w[0], w[1]);
            let (from, to) = (root[left], root[right]);
            let gap = sep(left, right).max(incoming.get(to, from).unwrap_or(0.0));
            incoming.set(to, from, gap);
            outgoing.set(from, to, gap);
        }
        for &id in layer {
            incoming.touch(root[id]);
            outgoing.touch(root[id]);
        }
    }
    let mut positions: HashMap<usize, f64> = HashMap::new();
    visit(&incoming, &mut |block| {
        let farthest = incoming.inner[&block].iter().map(|(from, gap)| positions[from] + gap).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
        positions.insert(block, farthest.unwrap_or(0.0));
    });
    visit(&outgoing, &mut |block| {
        let least = outgoing.inner[&block].iter().map(|(to, gap)| positions[to] - gap).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v))));
        if let Some(least) = least {
            let cur = positions[&block];
            positions.insert(block, cur.max(least));
        }
    });
    (0..n).map(|id| positions[&root[id]]).collect()
}

/// `ErDiagramCoordinates.call`: x centre per id.
fn coordinates(layers: &[Vec<usize>], edges: &[(usize, usize)], widths: &[f64], dummies: &[bool]) -> Vec<f64> {
    let n = widths.len();
    let adj = adjacency(n, edges);
    let found = conflicts(layers, &adj, dummies);
    let margin = |id: usize| (if dummies[id] { EDGE_SPACING } else { NODE_SPACING }) / 2.0;
    let sep = |l: usize, r: usize| (widths[l] / 2.0) + margin(l) + margin(r) + (widths[r] / 2.0);
    let mut by: Vec<(bool, Vec<f64>)> = vec![];
    for (up, left) in [(true, true), (true, false), (false, true), (false, false)] {
        let mut order: Vec<Vec<usize>> = if up { layers.to_vec() } else { layers.iter().rev().cloned().collect() };
        if !left {
            order = order.into_iter().map(|l| l.into_iter().rev().collect()).collect();
        }
        let neighbours = if up { &adj.pred } else { &adj.succ };
        let root = alignment(neighbours, &found, &order, n);
        let mut xs = compaction(&order, &root, &sep, n);
        if !left {
            xs = xs.into_iter().map(|x| -x).collect();
        }
        by.push((left, xs));
    }
    let extent = |xs: &[f64]| {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for (id, x) in xs.iter().enumerate() {
            lo = lo.min(x - widths[id] / 2.0);
            hi = hi.max(x + widths[id] / 2.0);
        }
        hi - lo
    };
    let mut narrowest = 0;
    for i in 1..by.len() {
        if extent(&by[i].1) < extent(&by[narrowest].1) {
            narrowest = i;
        }
    }
    let low = by[narrowest].1.iter().copied().fold(f64::INFINITY, f64::min);
    let high = by[narrowest].1.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    for i in 0..by.len() {
        if i == narrowest {
            continue;
        }
        let delta = if by[i].0 {
            low - by[i].1.iter().copied().fold(f64::INFINITY, f64::min)
        } else {
            high - by[i].1.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        };
        by[i].1 = by[i].1.iter().map(|x| x + delta).collect();
    }
    (0..n)
        .map(|id| {
            let mut middle: Vec<f64> = by.iter().map(|b| b.1[id]).collect();
            middle.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (middle[1] + middle[2]) / 2.0
        })
        .collect()
}

struct Placed {
    x: f64,
    y: f64,
}

/// `ErDiagramPlacement#apply`: entity positions and the canvas extent.
fn place(sizes: &[(f64, f64)], edges: &[(usize, usize)], labels: &[Option<String>]) -> (Vec<Placed>, (f64, f64)) {
    let n = sizes.len();
    let (mut ranks, oriented) = layering(n, edges);
    let mut widths: Vec<f64> = sizes.iter().map(|s| s.0).collect();
    let mut heights: Vec<f64> = sizes.iter().map(|s| s.1).collect();
    let mut dummies = vec![false; n];
    let mut chain: Vec<(usize, usize)> = vec![];
    for (index, pair) in oriented.iter().enumerate() {
        let Some((from, to)) = pair else { continue };
        let middle = (ranks[*from] + ranks[*to]).div_euclid(2);
        let mut path = vec![*from];
        for rank in (ranks[*from] + 1)..ranks[*to] {
            let size = if rank == middle {
                labels[index].as_deref().filter(|t| !t.is_empty()).map(|t| (text_width(t) * LABEL_FONT_SIZE / FONT_SIZE, LABEL_HEIGHT))
            } else {
                None
            };
            let (w, h) = size.unwrap_or((0.0, 0.0));
            widths.push(w);
            heights.push(h);
            dummies.push(true);
            ranks.push(rank);
            path.push(widths.len() - 1);
        }
        path.push(*to);
        chain.extend(path.windows(2).map(|w| (w[0], w[1])));
    }
    let adj = adjacency(widths.len(), &chain);
    let layers = ordering(&ranks, &adj);
    let centres = coordinates(&layers, &chain, &widths, &dummies);
    let left = layers.iter().flatten().map(|&id| centres[id] - widths[id] / 2.0).fold(f64::INFINITY, f64::min);
    let mut placed: Vec<Placed> = (0..n).map(|_| Placed { x: 0.0, y: 0.0 }).collect();
    let mut top = MARGIN;
    for layer in &layers {
        let tallest = layer.iter().map(|&id| heights[id]).fold(0.0f64, f64::max);
        let centre_y = top + tallest / 2.0;
        for &id in layer.iter().filter(|&&id| !dummies[id]) {
            let center_x = centres[id] - left + MARGIN;
            placed[id] = Placed { x: center_x - widths[id] / 2.0, y: centre_y - heights[id] / 2.0 };
        }
        top += tallest + RANK_SPACING / 2.0;
    }
    let right = layers.iter().flatten().map(|&id| centres[id] - left + MARGIN + widths[id] / 2.0).fold(f64::NEG_INFINITY, f64::max);
    (placed, (right, top - RANK_SPACING / 2.0))
}

// ---- geometry ----

#[derive(Clone, Copy)]
struct Pt {
    x: f64,
    y: f64,
}

struct Ln {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

struct Marker {
    lines: Vec<Ln>,
    circles: Vec<(f64, f64, f64)>,
    circle_first: bool,
}

fn direction(p: Pt, o: Pt) -> f64 {
    crate::js_math::atan2(o.y - p.y, o.x - p.x)
}

fn offset_point(p: Pt, o: Pt, distance: f64) -> Pt {
    let angle = direction(p, o);
    Pt { x: p.x + distance * crate::js_math::cos(angle), y: p.y + distance * crate::js_math::sin(angle) }
}

fn one_lines(p: Pt, o: Pt) -> Vec<Ln> {
    let perpendicular = direction(p, o) + PI / 2.0;
    let half = (CARDINALITY_SIZE / 2) as f64;
    let (h, v) = (half * crate::js_math::cos(perpendicular), half * crate::js_math::sin(perpendicular));
    vec![Ln { x1: p.x + h, y1: p.y + v, x2: p.x - h, y2: p.y - v }]
}

fn circle(p: Pt, o: Pt) -> (f64, f64, f64) {
    let c = offset_point(p, o, (CARDINALITY_SIZE / 2) as f64);
    (c.x, c.y, (CARDINALITY_SIZE / 3) as f64)
}

fn crows_foot(p: Pt, o: Pt) -> Vec<Ln> {
    let angle = direction(p, o);
    let base = offset_point(p, o, CARDINALITY_SIZE as f64);
    let size = CARDINALITY_SIZE as f64;
    [-PI / 4.0, 0.0, PI / 4.0]
        .iter()
        .map(|offset| {
            let a = angle + PI + offset;
            Ln { x1: base.x, y1: base.y, x2: base.x + size * crate::js_math::cos(a), y2: base.y + size * crate::js_math::sin(a) }
        })
        .collect()
}

fn marker(p: Pt, o: Pt, card: Option<&str>) -> Marker {
    let mut lines = vec![];
    if matches!(card, Some("one" | "one_or_more" | "zero_or_one")) {
        lines = one_lines(p, o);
    }
    if matches!(card, Some("zero_or_more" | "one_or_more")) {
        lines.extend(crows_foot(p, o));
    }
    let circles = if matches!(card, Some("zero_or_more" | "zero_or_one")) { vec![circle(p, o)] } else { vec![] };
    Marker { lines, circles, circle_first: card.is_some_and(|c| c.starts_with("zero_")) }
}

struct Bounds {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn connection_point(from: &Bounds, to: &Bounds) -> Pt {
    let fc = Pt { x: from.x + from.w / 2.0, y: from.y + from.h / 2.0 };
    let tc = Pt { x: to.x + to.w / 2.0, y: to.y + to.h / 2.0 };
    let (dx, dy) = (tc.x - fc.x, tc.y - fc.y);
    if dx.abs() < 0.001 && dy.abs() < 0.001 {
        return fc;
    }
    if dx.abs() > dy.abs() {
        let x = if dx > 0.0 { from.x + from.w } else { from.x };
        let y = if dy.abs() < 0.001 { fc.y } else { fc.y + ((dy / dx) * (x - fc.x)) };
        Pt { x, y }
    } else {
        let y = if dy > 0.0 { from.y + from.h } else { from.y };
        let x = if dx.abs() < 0.001 { fc.x } else { fc.x + ((dx / dy) * (y - fc.y)) };
        Pt { x, y }
    }
}

// ---- styles ----

fn present(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.is_empty())
}

const CSS_COLOR_KEYWORDS: &[&str] = &[
    "aliceblue", "antiquewhite", "aqua", "aquamarine", "azure", "beige", "bisque", "black", "blanchedalmond", "blue", "blueviolet", "brown", "burlywood",
    "cadetblue", "chartreuse", "chocolate", "coral", "cornflowerblue", "cornsilk", "crimson", "cyan", "currentcolor", "darkblue", "darkcyan",
    "darkgoldenrod", "darkgray", "darkgreen", "darkgrey", "darkkhaki", "darkmagenta", "darkolivegreen", "darkorange", "darkorchid", "darkred",
    "darksalmon", "darkseagreen", "darkslateblue", "darkslategray", "darkslategrey", "darkturquoise", "darkviolet", "deeppink", "deepskyblue",
    "dimgray", "dimgrey", "dodgerblue", "firebrick", "floralwhite", "forestgreen", "fuchsia", "gainsboro", "ghostwhite", "gold", "goldenrod", "gray",
    "green", "greenyellow", "grey", "honeydew", "hotpink", "indianred", "indigo", "ivory", "khaki", "lavender", "lavenderblush", "lawngreen",
    "lemonchiffon", "lightblue", "lightcoral", "lightcyan", "lightgoldenrodyellow", "lightgray", "lightgreen", "lightgrey", "lightpink",
    "lightsalmon", "lightseagreen", "lightskyblue", "lightslategray", "lightslategrey", "lightsteelblue", "lightyellow", "lime", "limegreen", "linen",
    "magenta", "maroon", "mediumaquamarine", "mediumblue", "mediumorchid", "mediumpurple", "mediumseagreen", "mediumslateblue", "mediumspringgreen",
    "mediumturquoise", "mediumvioletred", "midnightblue", "mintcream", "mistyrose", "moccasin", "navajowhite", "navy", "none", "oldlace", "olive",
    "olivedrab", "orange", "orangered", "orchid", "palegoldenrod", "palegreen", "paleturquoise", "palevioletred", "papayawhip", "peachpuff", "peru",
    "pink", "plum", "powderblue", "purple", "rebeccapurple", "red", "rosybrown", "royalblue", "saddlebrown", "salmon", "sandybrown", "seagreen",
    "seashell", "sienna", "silver", "skyblue", "slateblue", "slategray", "slategrey", "snow", "springgreen", "steelblue", "tan", "teal", "thistle",
    "tomato", "transparent", "turquoise", "violet", "wheat", "white", "whitesmoke", "yellow", "yellowgreen",
];

fn css_color(value: &str) -> bool {
    static HEX: OnceLock<Regex> = OnceLock::new();
    static FUNC: OnceLock<Regex> = OnceLock::new();
    let hex = HEX.get_or_init(|| Regex::new(r"^#(?i:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$").unwrap());
    let func = FUNC.get_or_init(|| Regex::new(r"(?i)^(?:rgb|rgba|hsl|hsla)\([^\n]+\)$").unwrap());
    CSS_COLOR_KEYWORDS.contains(&value.to_lowercase().as_str()) || hex.is_match(value) || func.is_match(value)
}

fn css_length(value: &str) -> bool {
    static LEN: OnceLock<Regex> = OnceLock::new();
    LEN.get_or_init(|| Regex::new(r"(?i)^[0-9]+(?:\.[0-9]+)?(?:px|em|rem|%|pt|cm|mm|in|pc|ex|ch|vw|vh)?$").unwrap()).is_match(value)
}

fn css_valid(property: &str, value: &str) -> bool {
    if property == "stroke-width" { css_length(value) } else { css_color(value) }
}

type Styles = Vec<(String, String)>;

fn mermaid_split(chunk: &str) -> (Option<&str>, Option<&str>) {
    if chunk.is_empty() {
        return (None, None);
    }
    let mut parts = chunk.split(':');
    (parts.next(), parts.next())
}

fn class_chunks(declaration: &str) -> Vec<String> {
    let chunks = ruby_split(declaration, ',');
    let mut out: Vec<String> = chunks.iter().map(|s| s.to_string()).collect();
    for c in &chunks {
        if c.contains("color") {
            let (k, v) = mermaid_split(c);
            if v.is_some() && strip(k.unwrap_or("")) != "fill" {
                out.push(c.to_string());
            }
        }
    }
    out
}

fn entity_styles(classes: &[String], class_defs: &[(String, String)]) -> Styles {
    let mut styles: Styles = vec![];
    let all = std::iter::once("default".to_string()).chain(classes.iter().cloned());
    for name in all {
        let Some((_, decl)) = class_defs.iter().find(|(n, _)| *n == name) else { continue };
        for chunk in class_chunks(decl) {
            let (k, v) = mermaid_split(&chunk);
            let Some(v) = v else { continue };
            let (k, v) = (strip(k.unwrap_or("")).to_string(), strip(v).to_string());
            match styles.iter_mut().find(|(n, _)| *n == k) {
                Some(slot) => slot.1 = v,
                None => styles.push((k, v)),
            }
        }
    }
    styles
}

fn last_valid_or_raw(values: Vec<&str>, valid: impl Fn(&str) -> bool) -> Option<String> {
    let present: Vec<&str> = values.into_iter().filter(|v| !v.is_empty()).collect();
    if present.is_empty() {
        return None;
    }
    present.iter().rev().find(|v| valid(v)).or(present.last()).map(|s| s.to_string())
}

fn box_style(styles: &Styles, property: &str) -> Option<String> {
    let values = styles.iter().filter(|(k, _)| k.to_lowercase() == property).map(|(_, v)| v.as_str()).collect();
    last_valid_or_raw(values, |v| css_valid(property, v))
}

fn ambient_color(styles: &Styles) -> Option<String> {
    let values: Vec<&str> = styles.iter().filter(|(k, _)| k != "color" && k.to_lowercase() == "color").map(|(_, v)| v.as_str()).collect();
    values.into_iter().filter(|v| !v.is_empty()).rev().find(|v| css_color(v)).map(|s| s.to_string())
}

fn box_property(styles: &Styles, property: &str, attributed: bool) -> Option<String> {
    if attributed {
        return present(styles.iter().find(|(k, _)| k == property).map(|(_, v)| v.as_str())).map(|s| s.to_string());
    }
    let value = box_style(styles, property);
    if property == "fill" || property == "stroke" {
        if value.as_deref().map(|v| v.to_lowercase()).as_deref() == Some("currentcolor") {
            return ambient_color(styles).or(value);
        }
    }
    value
}

// ---- svg ----

fn font_size_value(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { ruby_float(v) }
}

fn line_elem(l: &Ln, stroke: &str, width: &str) -> Elem {
    Elem::new("line").attr("stroke", stroke).attr("stroke-width", width).num("x1", l.x1).num("y1", l.y1).num("x2", l.x2).num("y2", l.y2)
}

fn emit_marker(m: &Marker, stroke: &str, group: &mut Elem) {
    let circles: Vec<Elem> = m
        .circles
        .iter()
        .map(|(cx, cy, r)| Elem::new("circle").attr("fill", "none").attr("stroke", stroke).attr("stroke-width", "2").num("cx", *cx).num("cy", *cy).num("r", *r))
        .collect();
    let lines: Vec<Elem> = m.lines.iter().map(|l| line_elem(l, stroke, "2")).collect();
    let all = if m.circle_first { circles.into_iter().chain(lines).collect::<Vec<_>>() } else { lines.into_iter().chain(circles).collect() };
    for e in all {
        group.push(e);
    }
}

pub fn render(source: &str) -> Result<String, Error> {
    if source_layout(source) {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "er_diagram")?;
    let model = build(&tree)?;
    let theme = Theme::default_theme();
    let col = |k: &str| theme.color(k).unwrap_or("").to_string();
    let large = theme.number("typography", "font_size_large").filter(|v| v.is_finite() && *v > 0.0).unwrap_or(16.0);
    let small = theme.number("typography", "font_size_small").filter(|v| v.is_finite() && *v > 0.0).unwrap_or(12.0);

    if model.ents.is_empty() && model.rels.is_empty() {
        let size = EMPTY_DIAGRAM_PADDING * 2.0;
        let mut doc = Document::new(size, size);
        doc.view_box = Some(format!("0 0 {} {}", size as i64, size as i64));
        return Ok(doc.to_xml());
    }

    let ids: Vec<&str> = model.ents.iter().map(|e| e.id.as_str()).collect();
    let sizes: Vec<(f64, f64)> = model.ents.iter().map(|e| entity_size(&e.id, &e.attrs)).collect();
    let pairs: Vec<(usize, usize)> = model
        .rels
        .iter()
        .map(|r| (ids.iter().position(|i| *i == r.from).unwrap(), ids.iter().position(|i| *i == r.to).unwrap()))
        .collect();
    let labels: Vec<Option<String>> = model.rels.iter().map(|r| r.label.clone()).collect();
    let (placed, extent) = place(&sizes, &pairs, &labels);
    let (width, height) = (extent.0 + CANVAS_MARGIN, extent.1 + CANVAS_MARGIN);
    let mut doc = Document::new(width, height);

    let base = |i: usize| (placed[i].x, placed[i].y, sizes[i].0, sizes[i].1);
    let bounds = |i: usize| {
        let (x, y, w, h) = base(i);
        Bounds { x, y, w, h }
    };
    let edge_stroke = col("edge_stroke");
    let label_text = col("label_text");

    for (r, &(a, b)) in model.rels.iter().zip(&pairs) {
        let from = connection_point(&bounds(a), &bounds(b));
        let to = connection_point(&bounds(b), &bounds(a));
        let mut group = Elem::new("g").attr("id", format!("rel-{}_to_{}", r.from, r.to));
        let mut line = line_elem(&Ln { x1: from.x, y1: from.y, x2: to.x, y2: to.y }, &edge_stroke, "2");
        if !r.identifying {
            line = line.attr("stroke-dasharray", "5,5");
        }
        group.push(line);
        emit_marker(&marker(from, to, r.card_from), &edge_stroke, &mut group);
        emit_marker(&marker(to, from, r.card_to), &edge_stroke, &mut group);
        if let Some(text) = &r.label {
            let (x, y) = ((from.x + to.x) / 2.0, ((from.y + to.y) / 2.0) - 5.0);
            group.push(
                Elem::new("text")
                    .attr("fill", label_text.clone())
                    .num("x", x)
                    .num("y", y)
                    .content(text)
                    .attr("text-anchor", "middle")
                    .attr("font-family", "Arial, sans-serif")
                    .attr("font-size", font_size_value(small)),
            );
        }
        doc.add(group);
    }

    for (i, e) in model.ents.iter().enumerate() {
        let (x, y, w, h) = base(i);
        let styles = entity_styles(&e.classes, &model.class_defs);
        let attributed = !e.attrs.is_empty();
        let stroke = box_property(&styles, "stroke", attributed).unwrap_or_else(|| col("node_stroke"));
        let color = styles.iter().find(|(k, _)| k == "color").map(|(_, v)| v.clone()).unwrap_or_else(|| label_text.clone());
        let mut group = Elem::new("g").attr("id", format!("entity-{}", e.id));
        group.push(
            Elem::new("rect")
                .attr("fill", box_property(&styles, "fill", attributed).unwrap_or_else(|| col("node_fill")))
                .attr("stroke", stroke.clone())
                .attr("stroke-width", box_property(&styles, "stroke-width", attributed).unwrap_or_else(|| "2".into()))
                .num("x", x)
                .num("y", y)
                .num("width", w)
                .num("height", h),
        );
        let label_y = y + ROW_HEIGHT / 2.0 + large / 3.0;
        group.push(
            Elem::new("text")
                .attr("fill", color.clone())
                .num("x", x + w / 2.0)
                .num("y", label_y)
                .content(&e.id)
                .attr("text-anchor", "middle")
                .attr("font-family", "Arial, sans-serif")
                .attr("font-size", font_size_value(large))
                .attr("font-weight", "bold"),
        );
        let sep_y = label_y - large / 3.0 + ROW_HEIGHT / 2.0;
        group.push(line_elem(&Ln { x1: x, y1: sep_y, x2: x + w, y2: sep_y }, &stroke, "1"));
        for (index, a) in e.attrs.iter().enumerate() {
            let text: Vec<&str> = [a.key.as_deref(), a.ty.as_deref(), Some(a.name.as_str()), a.note.as_deref()]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect();
            let ay = sep_y + index as f64 * ROW_HEIGHT + ROW_HEIGHT / 2.0 + small / 3.0;
            group.push(
                Elem::new("text")
                    .attr("fill", color.clone())
                    .num("x", x + ENTITY_PADDING)
                    .num("y", ay)
                    .content(&text.join(" "))
                    .attr("font-family", "monospace")
                    .attr("font-size", font_size_value(small)),
            );
        }
        doc.add(group);
    }
    Ok(doc.to_xml())
}
