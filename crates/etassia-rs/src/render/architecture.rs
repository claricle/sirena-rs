//! Architecture renderer: port of `Layout::Architecture` (grid, block and row
//! placement, group bounds), `Renderer::ArchitectureEdgeRouter` and
//! `Renderer::Architecture`.
//!
//! Ruby mixes Integer and Float arithmetic in the placement (the viewBox prints
//! `370` where a Float prints `176.0`), so geometry is carried as `N`.
//! Declined to the Ruby path (`Error::Unported`): a frontmatter/directive
//! preamble, duplicate ids, references to undeclared groups, parent cycles,
//! empty labels, and an edge-router grid that holds the same coordinate as both
//! an Integer and a Float (Ruby's `uniq` keeps both).
use super::pie::{items, strip, text_of};
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::Error;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::{Add, Div, Mul, Sub};

const DEFAULT_SERVICE: i64 = 80;
const FALLBACK_SERVICE_WIDTH: f64 = 90.0;
const FALLBACK_SERVICE_HEIGHT: f64 = 85.0;
const SERVICE_LABEL_OFFSET: i64 = 16;
const GROUP_PADDING: f64 = 42.5;
const GROUP_LABEL_ALLOWANCE: i64 = 17;
const EMPTY_GROUP_SIZE: i64 = 112;
const EMPTY_GROUP_GAP: i64 = 98;
const GRID_PITCH: i64 = 201;
const GRID_ORIGIN: i64 = 60;
const COMPONENT_GAP: i64 = 100;
const BLOCK_GAP: i64 = 100;
const SPACING: i64 = 40;
const JUNCTION: i64 = 12;
const FOUR_SERVICE_OFFSETS: [(i64, i64); 4] = [(143, 296), (0, 155), (95, 0), (250, 131)];

fn unported() -> Error {
    Error::Unported("architecture".into())
}

// ---- Ruby Integer/Float number ----

#[derive(Clone, Copy, Debug)]
enum N {
    I(i64),
    F(f64),
}
use N::{F, I};

impl N {
    fn f(self) -> f64 {
        match self {
            I(i) => i as f64,
            F(f) => f,
        }
    }
    fn show(self) -> String {
        match self {
            I(i) => i.to_string(),
            F(f) => ruby_float(f),
        }
    }
    /// `[self, other].max` (a tie keeps the receiver).
    fn max(self, other: N) -> N {
        if other.f() > self.f() { other } else { self }
    }
    fn min(self, other: N) -> N {
        if other.f() < self.f() { other } else { self }
    }
}
impl Add for N {
    type Output = N;
    fn add(self, o: N) -> N {
        match (self, o) {
            (I(a), I(b)) => I(a + b),
            (a, b) => F(a.f() + b.f()),
        }
    }
}
impl Sub for N {
    type Output = N;
    fn sub(self, o: N) -> N {
        match (self, o) {
            (I(a), I(b)) => I(a - b),
            (a, b) => F(a.f() - b.f()),
        }
    }
}
impl Mul for N {
    type Output = N;
    fn mul(self, o: N) -> N {
        match (self, o) {
            (I(a), I(b)) => I(a * b),
            (a, b) => F(a.f() * b.f()),
        }
    }
}
impl Div for N {
    type Output = N;
    fn div(self, o: N) -> N {
        match (self, o) {
            (I(a), I(b)) => I(a.div_euclid(b) - if b < 0 && a.rem_euclid(b) != 0 { 1 } else { 0 }),
            (a, b) => F(a.f() / b.f()),
        }
    }
}

/// `list.max` over Integer/Float values (first maximum wins).
fn nmax(values: impl IntoIterator<Item = N>) -> Option<N> {
    values.into_iter().reduce(N::max)
}
fn nmin(values: impl IntoIterator<Item = N>) -> Option<N> {
    values.into_iter().reduce(N::min)
}

// ---- model ----

struct Grp {
    id: String,
    label: Option<String>,
    icon: Option<String>,
    parent: Option<String>,
}
struct Svc {
    id: String,
    label: Option<String>,
    icon: Option<String>,
    group: Option<String>,
}
struct Jn {
    id: String,
    group: Option<String>,
}
struct Edg {
    from: String,
    to: String,
    from_pos: Option<String>,
    to_pos: Option<String>,
    label: Option<String>,
}
#[derive(Default)]
struct Dia {
    groups: Vec<Grp>,
    services: Vec<Svc>,
    junctions: Vec<Jn>,
    edges: Vec<Edg>,
}

/// `Text#text`: unwrap a one-key hash, then `strip`.
fn txt(v: &Value) -> String {
    let inner = match v.as_object() {
        Some(o) if !o.contains_key("value") => o.values().next().unwrap_or(&Value::Null),
        _ => v,
    };
    strip(&text_of(inner)).to_string()
}

fn build(statements: &Value) -> Dia {
    let mut d = Dia::default();
    for s in items(statements) {
        let Some(o) = s.as_object() else { continue };
        if ["title", "acc_title", "acc_descr"].iter().any(|k| o.contains_key(*k)) {
            continue;
        }
        let get = |k: &str| o.get(k).map(txt);
        if o.contains_key("from") {
            d.edges.push(Edg {
                from: get("from").unwrap_or_default(),
                to: get("to").unwrap_or_default(),
                from_pos: get("from_pos"),
                to_pos: get("to_pos"),
                label: get("label"),
            });
            continue;
        }
        let id = get("id").unwrap_or_default();
        match get("stmt_type").as_deref() {
            Some("group") => d.groups.push(Grp { id, label: get("label"), icon: get("icon"), parent: get("parent") }),
            Some("service") => d.services.push(Svc { id, label: get("label"), icon: get("icon"), group: get("group") }),
            Some("junction") => d.junctions.push(Jn { id, group: get("group") }),
            _ => {}
        }
    }
    d
}

/// Declines shapes the Ruby diagram model refuses or the port does not model.
fn check(d: &Dia) -> Result<(), Error> {
    let mut seen = HashSet::new();
    let ids = d.groups.iter().map(|g| &g.id).chain(d.services.iter().map(|s| &s.id)).chain(d.junctions.iter().map(|j| &j.id));
    for id in ids {
        if id.is_empty() || !seen.insert(id.as_str()) {
            return Err(unported());
        }
    }
    let known: HashSet<&str> = d.groups.iter().map(|g| g.id.as_str()).collect();
    let ok = |r: &Option<String>| r.as_deref().is_none_or(|r| known.contains(r));
    if !d.groups.iter().all(|g| ok(&g.parent)) || !d.services.iter().all(|s| ok(&s.group)) || !d.junctions.iter().all(|j| ok(&j.group)) {
        return Err(unported());
    }
    for g in &d.groups {
        let mut cur = g.parent.as_deref();
        let mut steps = 0;
        while let Some(p) = cur {
            if p == g.id || steps > d.groups.len() {
                return Err(unported());
            }
            cur = d.groups.iter().find(|x| x.id == p).and_then(|x| x.parent.as_deref());
            steps += 1;
        }
    }
    let empty = |o: &Option<String>| o.as_deref().is_some_and(str::is_empty);
    if d.groups.iter().any(|g| empty(&g.label) || empty(&g.icon))
        || d.services.iter().any(|s| empty(&s.label) || empty(&s.icon))
    {
        return Err(unported());
    }
    Ok(())
}

// ---- layout ----

#[derive(Clone, Copy)]
struct Bx {
    x: N,
    y: N,
    w: N,
    h: N,
}

#[derive(Clone)]
struct Info {
    id: String,
    svc: Option<usize>,
    group: Option<String>,
    b: Bx,
}

struct Gb {
    id: String,
    b: Bx,
}

struct Nd {
    id: String,
    svc: Option<usize>,
    w: N,
    h: N,
    group: Option<String>,
}

type Cell = (i64, i64);
type Links = HashMap<String, Vec<(String, Cell)>>;

fn valid_side<'a>(pos: &'a Option<String>, default: &'a str) -> &'a str {
    match pos.as_deref() {
        Some(p @ ("L" | "R" | "T" | "B")) => p,
        _ => default,
    }
}

fn step_of(side: &str) -> Cell {
    match side {
        "R" => (1, 0),
        "L" => (-1, 0),
        "T" => (0, -1),
        _ => (0, 1),
    }
}

fn overlap(a: &Bx, b: &Bx) -> bool {
    a.x.f() < b.x.f() + b.w.f() && b.x.f() < a.x.f() + a.w.f() && a.y.f() < b.y.f() + b.h.f() && b.y.f() < a.y.f() + a.h.f()
}

struct Layout<'a> {
    d: &'a Dia,
    /// Edges whose endpoints are both services/junctions (the IR drops the rest).
    edges: Vec<&'a Edg>,
}

impl<'a> Layout<'a> {
    fn new(d: &'a Dia) -> Self {
        let node = |id: &str| d.services.iter().any(|s| s.id == id) || d.junctions.iter().any(|j| j.id == id);
        let edges = d.edges.iter().filter(|e| node(&e.from) && node(&e.to)).collect();
        Layout { d, edges }
    }

    fn group(&self, id: &str) -> Option<&Grp> {
        self.d.groups.iter().find(|g| g.id == id)
    }

    fn descendants(&self, id: &str) -> Vec<String> {
        let mut ids = vec![id.to_string()];
        let mut i = 0;
        while i < ids.len() {
            let cur = ids[i].clone();
            for g in &self.d.groups {
                if g.parent.as_deref() == Some(cur.as_str()) && !ids.contains(&g.id) {
                    ids.push(g.id.clone());
                }
            }
            i += 1;
        }
        ids
    }

    fn ancestors(&self, id: &str) -> Vec<String> {
        let mut ids: Vec<String> = vec![];
        let mut cur = self.group(id).and_then(|g| g.parent.clone());
        while let Some(c) = cur {
            if ids.contains(&c) {
                break;
            }
            cur = self.group(&c).and_then(|g| g.parent.clone());
            ids.push(c);
        }
        ids
    }

    fn nodes(&self) -> Vec<Nd> {
        let mut out: Vec<Nd> = self
            .d
            .services
            .iter()
            .enumerate()
            .map(|(i, s)| Nd { id: s.id.clone(), svc: Some(i), w: I(DEFAULT_SERVICE), h: I(DEFAULT_SERVICE), group: s.group.clone() })
            .collect();
        out.extend(self.d.junctions.iter().map(|j| Nd { id: j.id.clone(), svc: None, w: I(JUNCTION), h: I(JUNCTION), group: j.group.clone() }));
        out
    }

    // -- grid --

    fn edge_links(edges: &[&Edg], key: impl Fn(&Edg) -> (String, String)) -> Links {
        let mut links: Links = HashMap::new();
        for e in edges {
            let (from, to) = key(e);
            let step = step_of(valid_side(&e.from_pos, "R"));
            links.entry(from.clone()).or_default().push((to.clone(), step));
            links.entry(to).or_default().push((from, (-step.0, -step.1)));
        }
        links
    }

    fn components(ids: &[String], links: &Links) -> Vec<Vec<String>> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = vec![];
        for id in ids {
            if seen.contains(id) {
                continue;
            }
            let mut queue = vec![id.clone()];
            seen.insert(id.clone());
            let mut i = 0;
            while i < queue.len() {
                let cur = queue[i].clone();
                for (n, _) in links.get(&cur).into_iter().flatten() {
                    if seen.insert(n.clone()) {
                        queue.push(n.clone());
                    }
                }
                i += 1;
            }
            out.push(queue);
        }
        out
    }

    fn place_component(component: &[String], links: &Links) -> Vec<(String, Cell)> {
        let mut placed: Vec<(String, Cell)> = vec![(component[0].clone(), (0, 0))];
        let mut taken: HashSet<Cell> = HashSet::from([(0, 0)]);
        for id in component {
            for (n, step) in links.get(id).into_iter().flatten() {
                if placed.iter().any(|(k, _)| k == n) {
                    continue;
                }
                let from = placed.iter().find(|(k, _)| k == id).map(|(_, c)| *c).unwrap_or((0, 0));
                let mut cell = (from.0 + step.0, from.1 + step.1);
                while taken.contains(&cell) {
                    cell = (cell.0 + step.0, cell.1 + step.1);
                }
                taken.insert(cell);
                placed.push((n.clone(), cell));
            }
        }
        placed
    }

    /// Each connected component's cells, moved so its leftmost column and
    /// topmost row are 0.
    fn placed_components(ids: &[String], links: &Links) -> Vec<Vec<(String, Cell)>> {
        Self::components(ids, links)
            .iter()
            .map(|c| {
                let placed = Self::place_component(c, links);
                let min_col = placed.iter().map(|(_, c)| c.0).min().unwrap_or(0);
                let min_row = placed.iter().map(|(_, c)| c.1).min().unwrap_or(0);
                placed.into_iter().map(|(k, c)| (k, (c.0 - min_col, c.1 - min_row))).collect()
            })
            .collect()
    }

    /// Pixel cell centres per node id; components sit side by side.
    fn grid_cells(&self, ids: &[String], edges: &[&Edg]) -> Vec<(String, Cell)> {
        let known: Vec<&Edg> = edges.iter().copied().filter(|e| ids.contains(&e.from) && ids.contains(&e.to)).collect();
        let links = Self::edge_links(&known, |e| (e.from.clone(), e.to.clone()));
        let mut cells = vec![];
        let mut offset = GRID_ORIGIN;
        for placed in Self::placed_components(ids, &links) {
            let max_col = placed.iter().map(|(_, c)| c.0).max().unwrap_or(0);
            cells.extend(placed.into_iter().map(|(k, (c, r))| (k, (offset + c * GRID_PITCH, GRID_ORIGIN + r * GRID_PITCH))));
            offset += max_col * GRID_PITCH + DEFAULT_SERVICE + COMPONENT_GAP;
        }
        cells
    }

    fn grid_entry(n: &Nd, x: f64, y: f64) -> Info {
        Info {
            id: n.id.clone(),
            svc: n.svc,
            group: n.group.clone(),
            b: Bx { x: F(x - n.w.f() / 2.0), y: F(y - n.h.f() / 2.0), w: n.w, h: n.h },
        }
    }

    fn split(entries: Vec<Info>) -> (Vec<Info>, Vec<Info>) {
        entries.into_iter().partition(|i| i.svc.is_some())
    }

    fn position_on_grid(&self) -> (Vec<Info>, Vec<Info>) {
        let nodes = self.nodes();
        let ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
        let cells = self.grid_cells(&ids, &self.edges);
        let entries = cells
            .iter()
            .map(|(id, (x, y))| Self::grid_entry(nodes.iter().find(|n| &n.id == id).unwrap(), *x as f64, *y as f64))
            .collect();
        Self::split(entries)
    }

    fn block_owner(&self, n: &Nd) -> String {
        match &n.group {
            None => n.id.clone(),
            Some(g) => {
                let mut chain = vec![g.clone()];
                chain.extend(self.ancestors(g));
                chain.last().cloned().unwrap_or_default()
            }
        }
    }

    fn position_in_blocks(&self) -> (Vec<Info>, Vec<Info>) {
        let nodes = self.nodes();
        let owner: HashMap<String, String> = nodes.iter().map(|n| (n.id.clone(), self.block_owner(n))).collect();
        let mut blocks: Vec<String> = vec![];
        for n in &nodes {
            let o = &owner[&n.id];
            if !blocks.contains(o) {
                blocks.push(o.clone());
            }
        }
        let (inner, outer): (Vec<&Edg>, Vec<&Edg>) = self.edges.iter().copied().partition(|e| owner[&e.from] == owner[&e.to]);
        let members = |b: &String| -> Vec<String> { nodes.iter().filter(|n| &owner[&n.id] == b).map(|n| n.id.clone()).collect() };
        let nd = |id: &str| nodes.iter().find(|n| n.id == id).unwrap();

        // block => (right extent, bottom extent) of its own grid
        let extents: Vec<(f64, f64)> = blocks
            .iter()
            .map(|b| {
                let ids = members(b);
                let cells = self.grid_cells(&ids, &inner);
                let right = cells.iter().map(|(id, c)| c.0 as f64 + nd(id).w.f() / 2.0).fold(f64::MIN, f64::max);
                let bottom = cells.iter().map(|(id, c)| c.1 as f64 + nd(id).h.f() / 2.0).fold(f64::MIN, f64::max);
                (right, bottom)
            })
            .collect();

        let links = Self::edge_links(&outer, |e| (owner[&e.from].clone(), owner[&e.to].clone()));
        let mut block_cells: HashMap<String, Cell> = HashMap::new();
        let mut offset = 0;
        for placed in Self::placed_components(&blocks, &links) {
            let max_col = placed.iter().map(|(_, c)| c.0).max().unwrap_or(0);
            for (b, (c, r)) in placed {
                block_cells.insert(b, (offset + c, r));
            }
            offset += max_col + 1;
        }
        let starts = |axis: usize| -> HashMap<i64, f64> {
            let mut tracks: Vec<i64> = block_cells.values().map(|c| if axis == 0 { c.0 } else { c.1 }).collect();
            tracks.sort_unstable();
            tracks.dedup();
            let mut start = 0.0;
            let mut out = HashMap::new();
            for t in tracks {
                out.insert(t, start);
                let size = blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| {
                        let c = block_cells[*b];
                        (if axis == 0 { c.0 } else { c.1 }) == t
                    })
                    .map(|(i, _)| if axis == 0 { extents[i].0 } else { extents[i].1 })
                    .fold(f64::MIN, f64::max);
                start += size + BLOCK_GAP as f64;
            }
            out
        };
        let (xs, ys) = (starts(0), starts(1));
        let mut entries = vec![];
        for b in &blocks {
            let (col, row) = block_cells[b];
            let (sx, sy) = (xs[&col], ys[&row]);
            let ids = members(b);
            for (id, (x, y)) in self.grid_cells(&ids, &inner) {
                entries.push(Self::grid_entry(nd(&id), x as f64 + sx, y as f64 + sy));
            }
        }
        Self::split(entries)
    }

    // -- rows --

    fn position_by_rows(&self) -> (Vec<Info>, Vec<Info>) {
        let mut row_groups: Vec<Option<String>> = vec![None];
        row_groups.extend(self.d.groups.iter().map(|g| Some(g.id.clone())));
        let mut services: Vec<Info> = vec![];
        let mut current_y = I(SPACING);
        for g in &row_groups {
            let members: Vec<usize> = (0..self.d.services.len()).filter(|&i| &self.d.services[i].group == g).collect();
            if members.is_empty() {
                continue;
            }
            let mut x = I(SPACING);
            for i in members {
                services.push(Info {
                    id: self.d.services[i].id.clone(),
                    svc: Some(i),
                    group: g.clone(),
                    b: Bx { x, y: current_y, w: I(DEFAULT_SERVICE), h: I(DEFAULT_SERVICE) },
                });
                x = x + I(DEFAULT_SERVICE) + I(SPACING);
            }
            current_y = current_y + I(DEFAULT_SERVICE) + I(SPACING * 2);
        }
        let mut junctions: Vec<Info> = vec![];
        let mut cy = match nmax(services.iter().map(|s| s.b.y + s.b.h)) {
            None => I(SPACING),
            Some(m) => m + I(SPACING),
        };
        for g in &row_groups {
            let js: Vec<&Jn> = self.d.junctions.iter().filter(|j| &j.group == g).collect();
            if js.is_empty() {
                continue;
            }
            let gs: Vec<&Info> = services.iter().filter(|s| &s.group == g).collect();
            let (mut rx, ry) = if gs.is_empty() {
                (I(SPACING), cy)
            } else {
                let right = nmax(gs.iter().map(|s| s.b.x + s.b.w)).unwrap() + I(SPACING);
                let top = nmin(gs.iter().map(|s| s.b.y)).unwrap();
                (right, top + F((DEFAULT_SERVICE - JUNCTION) as f64 / 2.0))
            };
            for j in js {
                junctions.push(Info { id: j.id.clone(), svc: None, group: g.clone(), b: Bx { x: rx, y: ry, w: I(JUNCTION), h: I(JUNCTION) } });
                rx = rx + I(JUNCTION) + I(SPACING);
            }
            cy = if gs.is_empty() { cy + I(JUNCTION + SPACING) } else { cy.max(ry + I(JUNCTION) + I(SPACING)) };
        }
        (services, junctions)
    }

    fn position_four(&self) -> Vec<Info> {
        self.d
            .services
            .iter()
            .zip(FOUR_SERVICE_OFFSETS)
            .enumerate()
            .map(|(i, (s, (ox, oy)))| Info {
                id: s.id.clone(),
                svc: Some(i),
                group: None,
                b: Bx { x: I(SPACING + ox), y: I(SPACING + oy), w: I(DEFAULT_SERVICE), h: I(DEFAULT_SERVICE) },
            })
            .collect()
    }

    // -- group bounds --

    fn depth(&self, id: &str) -> usize {
        self.ancestors(id).len()
    }

    fn group_bounds(&self, services: &[Info], junctions: &[Info]) -> Vec<Gb> {
        let members: Vec<&Info> = services.iter().chain(junctions).collect();
        let mut order: Vec<usize> = (0..self.d.groups.len()).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(self.depth(&self.d.groups[i].id)), i));
        let mut bounds: Vec<Gb> = vec![];
        for i in order {
            let g = &self.d.groups[i];
            let mut boxes: Vec<Bx> = members.iter().filter(|m| m.group.as_deref() == Some(g.id.as_str())).map(|m| m.b).collect();
            for child in self.d.groups.iter().filter(|c| c.parent.as_deref() == Some(g.id.as_str())) {
                if let Some(b) = bounds.iter().find(|b| b.id == child.id) {
                    boxes.push(b.b);
                }
            }
            if boxes.is_empty() {
                continue;
            }
            let low_x = nmin(boxes.iter().map(|b| b.x)).unwrap();
            let high_x = nmax(boxes.iter().map(|b| b.x + b.w)).unwrap();
            let low_y = nmin(boxes.iter().map(|b| b.y)).unwrap();
            let high_y = nmax(boxes.iter().map(|b| b.y + b.h)).unwrap();
            let pad = F(GROUP_PADDING);
            bounds.push(Gb {
                id: g.id.clone(),
                b: Bx {
                    x: low_x - pad,
                    y: low_y - pad,
                    w: (high_x - low_x) + pad * I(2),
                    h: (high_y - low_y) + pad * I(2) + I(GROUP_LABEL_ALLOWANCE),
                },
            });
        }
        // an isolated empty group is drawn as a small box beside the diagram
        let mut all: Vec<N> = members.iter().map(|m| m.b.x + m.b.w).collect();
        all.extend(bounds.iter().map(|b| b.b.x + b.b.w));
        let mut right = nmax(all);
        let centre_y = if members.is_empty() {
            I(GRID_ORIGIN) + I(EMPTY_GROUP_SIZE) / I(2)
        } else {
            let top = nmin(members.iter().map(|m| m.b.y)).unwrap();
            let bottom = nmax(members.iter().map(|m| m.b.y + m.b.h)).unwrap();
            (top + bottom) / F(2.0)
        };
        for g in &self.d.groups {
            let isolated = !bounds.iter().any(|b| b.id == g.id) && g.parent.is_none() && !self.d.groups.iter().any(|o| o.parent.as_deref() == Some(g.id.as_str()));
            if !isolated {
                continue;
            }
            let x = match right {
                Some(r) => r + I(EMPTY_GROUP_GAP),
                None => I(GRID_ORIGIN),
            };
            bounds.push(Gb { id: g.id.clone(), b: Bx { x, y: centre_y - F(EMPTY_GROUP_SIZE as f64 / 2.0), w: I(EMPTY_GROUP_SIZE), h: I(EMPTY_GROUP_SIZE) } });
            right = Some(x + I(EMPTY_GROUP_SIZE));
        }
        bounds
    }

    fn groups_separate(&self, services: &[Info], junctions: &[Info], bounds: &[Gb]) -> bool {
        let nodes: Vec<&Info> = services.iter().chain(junctions).collect();
        bounds.iter().all(|gb| {
            let inside = self.descendants(&gb.id);
            let nodes_ok = nodes.iter().all(|n| n.group.as_ref().is_some_and(|g| inside.contains(g)) || !overlap(&gb.b, &n.b));
            let mut related = inside.clone();
            for a in self.ancestors(&gb.id) {
                if !related.contains(&a) {
                    related.push(a);
                }
            }
            nodes_ok && !bounds.iter().any(|o| !related.contains(&o.id) && overlap(&gb.b, &o.b))
        })
    }
}

struct Placed {
    services: Vec<Info>,
    junctions: Vec<Info>,
    groups: Vec<Gb>,
}

fn place(l: &Layout) -> Placed {
    let bounded = |(services, junctions): (Vec<Info>, Vec<Info>)| {
        let groups = l.group_bounds(&services, &junctions);
        Placed { services, junctions, groups }
    };
    if l.d.services.len() == 4 && l.edges.is_empty() && l.d.groups.is_empty() && l.d.junctions.is_empty() {
        return bounded((l.position_four(), vec![]));
    }
    if !l.edges.is_empty() {
        let p = bounded(l.position_on_grid());
        if l.groups_separate(&p.services, &p.junctions, &p.groups) {
            return p;
        }
        let p = bounded(l.position_in_blocks());
        if l.groups_separate(&p.services, &p.junctions, &p.groups) {
            return p;
        }
    }
    bounded(l.position_by_rows())
}

// ---- edge router ----

type Pt = (N, N);

/// An obstacle box; edges keep their Integer/Float type for the grid's `uniq`.
#[derive(Clone, Copy)]
struct Rb {
    x: N,
    y: N,
    r: N,
    b: N,
}

impl From<&Bx> for Rb {
    fn from(b: &Bx) -> Rb {
        Rb { x: b.x, y: b.y, r: b.x + b.w, b: b.y + b.h }
    }
}

struct End {
    point: Pt,
    b: Rb,
    side: String,
}

fn axis_interval(first: f64, second: f64, near: f64, far: f64) -> Option<(f64, f64)> {
    if first == second {
        return (first >= near && first <= far && first != near && first != far).then_some((f64::NEG_INFINITY, f64::INFINITY));
    }
    let step = second - first;
    let (a, b) = ((near - first) / step, (far - first) / step);
    Some(if a <= b { (a, b) } else { (b, a) })
}

fn crosses(b: &Rb, p: (f64, f64), q: (f64, f64)) -> bool {
    let Some(xi) = axis_interval(p.0, q.0, b.x.f(), b.r.f()) else { return false };
    let Some(yi) = axis_interval(p.1, q.1, b.y.f(), b.b.f()) else { return false };
    let lo = xi.0.max(yi.0).max(0.0);
    let hi = xi.1.min(yi.1).min(1.0);
    lo < hi
}

fn straight_clear(p: (f64, f64), q: (f64, f64), obstacles: &[Rb]) -> bool {
    !obstacles.iter().any(|b| crosses(b, p, q))
}

const GRID_STEPS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const TURN_PENALTY: i64 = 5;
const MARGIN: i64 = 20;

fn face(side: &str) -> (i32, i32) {
    match side {
        "L" => (-1, 0),
        "R" => (1, 0),
        "T" => (0, -1),
        _ => (0, 1),
    }
}

type State = (usize, usize, Option<(i32, i32)>);

/// The Ruby `MinHeap`, tie order included.
#[derive(Default)]
struct MinHeap {
    entries: Vec<(i64, State)>,
}

impl MinHeap {
    fn push(&mut self, cost: i64, item: State) {
        self.entries.push((cost, item));
        let mut index = self.entries.len() - 1;
        while index > 0 {
            let parent = (index - 1) / 2;
            if self.entries[parent].0 <= self.entries[index].0 {
                break;
            }
            self.entries.swap(parent, index);
            index = parent;
        }
    }
    fn pop(&mut self) -> (i64, State) {
        let top = self.entries[0];
        let last = self.entries.pop().unwrap();
        if !self.entries.is_empty() {
            self.entries[0] = last;
            let size = self.entries.len();
            let mut index = 0;
            loop {
                let left = 2 * index + 1;
                let mut smallest = index;
                for c in [left, left + 1] {
                    if c < size && self.entries[c].0 < self.entries[smallest].0 {
                        smallest = c;
                    }
                }
                if smallest == index {
                    break;
                }
                self.entries.swap(index, smallest);
                index = smallest;
            }
        }
        top
    }
}

/// `xs.uniq.sort`; `None` when one value is held as both Integer and Float.
fn uniq_sorted(values: &[N]) -> Option<Vec<N>> {
    let mut out: Vec<N> = vec![];
    for v in values {
        match out.iter().find(|o| o.f() == v.f()) {
            Some(o) if std::mem::discriminant(o) != std::mem::discriminant(v) => return None,
            Some(_) => {}
            None => out.push(*v),
        }
    }
    out.sort_by(|a, b| a.f().partial_cmp(&b.f()).unwrap_or(Ordering::Equal));
    Some(out)
}

fn build_grid(from: &Pt, to: &Pt, obstacles: &[Rb], margin: bool) -> Option<(Vec<N>, Vec<N>)> {
    let mut xs = vec![from.0, to.0];
    let mut ys = vec![from.1, to.1];
    for o in obstacles {
        xs.extend([o.x, o.r]);
        ys.extend([o.y, o.b]);
    }
    if margin {
        // `[min - MARGIN, 0].max` and `max + MARGIN`
        let (lo_x, hi_x) = (nmin(xs.iter().copied()).unwrap(), nmax(xs.iter().copied()).unwrap());
        let (lo_y, hi_y) = (nmin(ys.iter().copied()).unwrap(), nmax(ys.iter().copied()).unwrap());
        let floor = |v: N| (v - I(MARGIN)).max(I(0));
        let (a, b, c, d) = (floor(lo_x), hi_x + I(MARGIN), floor(lo_y), hi_y + I(MARGIN));
        xs.extend([a, b]);
        ys.extend([c, d]);
    }
    Some((uniq_sorted(&xs)?, uniq_sorted(&ys)?))
}

enum Search {
    Path(Vec<Pt>),
    NoPath,
    Decline,
}

fn search_grid(grid: &(Vec<N>, Vec<N>), from: &End, to: &End, obstacles: &[Rb]) -> Option<Vec<Pt>> {
    let (xs, ys) = grid;
    let find = |v: &[N], n: N| v.iter().position(|c| c.f() == n.f()).unwrap_or(0);
    let start = (find(xs, from.point.0), find(ys, from.point.1));
    let goal = (find(xs, to.point.0), find(ys, to.point.1));
    if start == goal {
        return None;
    }
    let required_first = face(&from.side);
    let tf = face(&to.side);
    let required_last = (-tf.0, -tf.1);
    let goal_state: State = (goal.0, goal.1, Some(required_last));
    let mut frontier = MinHeap::default();
    let start_state: State = (start.0, start.1, None);
    frontier.push(0, start_state);
    let mut previous: HashMap<State, State> = HashMap::new();
    let mut distances: HashMap<State, i64> = HashMap::from([(start_state, 0)]);
    while !frontier.entries.is_empty() {
        let (cost, state) = frontier.pop();
        if cost > distances.get(&state).copied().unwrap_or(i64::MAX) {
            continue;
        }
        if state == goal_state {
            let mut path = vec![state];
            while let Some(p) = previous.get(path.last().unwrap()) {
                path.push(*p);
            }
            path.reverse();
            return Some(path.into_iter().map(|(xi, yi, _)| (xs[xi], ys[yi])).collect());
        }
        let (xi, yi, dir) = state;
        for step in GRID_STEPS {
            if dir.is_none() && step != required_first {
                continue;
            }
            let (nx, ny) = (xi as i64 + step.0 as i64, yi as i64 + step.1 as i64);
            if nx < 0 || ny < 0 || nx as usize >= xs.len() || ny as usize >= ys.len() {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !straight_clear((xs[xi].f(), ys[yi].f()), (xs[nx].f(), ys[ny].f()), obstacles) {
                continue;
            }
            let step_cost = if dir.is_some_and(|d| d != step) { 1 + TURN_PENALTY } else { 1 };
            let next: State = (nx, ny, Some(step));
            let new_cost = cost + step_cost;
            if new_cost >= distances.get(&next).copied().unwrap_or(i64::MAX) {
                continue;
            }
            distances.insert(next, new_cost);
            previous.insert(next, state);
            frontier.push(new_cost, next);
        }
    }
    None
}

fn shortest_path(from: &End, to: &End, obstacles: &[Rb]) -> Search {
    for margin in [false, true] {
        let Some(grid) = build_grid(&from.point, &to.point, obstacles, margin) else { return Search::Decline };
        if let Some(path) = search_grid(&grid, from, to, obstacles) {
            return Search::Path(path);
        }
    }
    Search::NoPath
}

fn collapse(points: Vec<Pt>) -> Vec<Pt> {
    if points.len() <= 2 {
        return points;
    }
    let dir = |a: &Pt, b: &Pt| (b.0.f().partial_cmp(&a.0.f()), b.1.f().partial_cmp(&a.1.f()));
    let mut out = vec![points[0]];
    for w in points.windows(3) {
        if dir(&w[0], &w[1]) != dir(&w[1], &w[2]) {
            out.push(w[1]);
        }
    }
    out.push(*points.last().unwrap());
    out
}

fn route(from: &End, to: &End, obstacles: &[Rb]) -> Result<Vec<Pt>, Error> {
    let mut all = obstacles.to_vec();
    all.push(from.b);
    all.push(to.b);
    let pf = (from.point.0.f(), from.point.1.f());
    let pt = (to.point.0.f(), to.point.1.f());
    if straight_clear(pf, pt, &all) {
        return Ok(vec![from.point, to.point]);
    }
    match shortest_path(from, to, &all) {
        Search::Path(p) => Ok(collapse(p)),
        Search::NoPath => Ok(vec![from.point, to.point]),
        Search::Decline => Err(unported()),
    }
}

// ---- scene + render ----

fn connection(i: &Info, side: &str) -> Pt {
    let (b, two) = (&i.b, I(2));
    match side {
        "L" => (b.x, b.y + b.h / two),
        "T" => (b.x + b.w / two, b.y),
        "B" => (b.x + b.w / two, b.y + b.h),
        _ => (b.x + b.w, b.y + b.h / two),
    }
}

struct Routed<'a> {
    edge: &'a Edg,
    points: Vec<Pt>,
}

fn glyph(icon: &str) -> &'static str {
    let mut parts: Vec<&str> = icon.split(':').collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    match parts.last().copied() {
        Some("database") => "⬢",
        Some("server") => "▦",
        Some("disk") => "◎",
        Some("cloud") => "☁",
        Some("internet") => "◈",
        Some("browser") => "⊞",
        _ => "?",
    }
}

fn text_el(content: &str, x: f64, y: f64, size: &str, fill: &str) -> Elem {
    Elem::new("text").attr("fill", fill).num("x", x).num("y", y).attr("font-size", size).content(content)
}

pub fn render(source: &str) -> Result<String, Error> {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    if !strip(preamble).is_empty() {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "architecture")?;
    let statements = tree.get("statements").cloned().unwrap_or(Value::Null);
    let d = build(&statements);
    check(&d)?;
    let theme = Theme::default_theme();
    let col = |k: &str| theme.color(k).unwrap_or("").to_string();
    let (node_fill, node_stroke, edge_stroke, edge_color, label_text) =
        (col("node_fill"), col("node_stroke"), col("edge_stroke"), col("edge_color"), col("label_text"));
    let stroke_width = theme.number("shapes", "stroke_width").map(ruby_float);
    let font_family = theme.string("typography", "font_family").unwrap_or("Arial, Helvetica, sans-serif").to_string();
    let bold = theme.string("typography", "font_weight_bold").unwrap_or("bold").to_string();

    let l = Layout::new(&d);
    let p = place(&l);

    // -- edges: positioned, then routed --
    let nodes: Vec<&Info> = p.services.iter().chain(&p.junctions).collect();
    let node = |id: &str| nodes.iter().find(|n| n.id == id).copied();
    let icon_less = |i: &Info| i.svc.is_some_and(|s| d.services[s].icon.as_deref().is_none_or(str::is_empty));
    let ancestor_groups = |i: &Info| -> Vec<String> {
        let mut ids: Vec<String> = vec![];
        let mut cur = i.group.clone();
        while let Some(c) = cur {
            if ids.contains(&c) {
                break;
            }
            cur = p.groups.iter().find(|g| g.id == c).and_then(|_| l.group(&c)).and_then(|g| g.parent.clone());
            ids.push(c);
        }
        ids
    };
    let mut routed: Vec<Routed> = vec![];
    for e in &l.edges {
        let (Some(from), Some(to)) = (node(&e.from), node(&e.to)) else { continue };
        let end = |i: &Info, side: &str| {
            let mut point = connection(i, side);
            if icon_less(i) {
                if side == "T" {
                    point.1 = point.1 - I(5);
                } else if side == "R" {
                    point.0 = point.0 + I(10);
                }
            }
            End { point, b: (&i.b).into(), side: side.to_string() }
        };
        let (fs, ts) = (valid_side(&e.from_pos, "R"), valid_side(&e.to_pos, "L"));
        let (a, b) = (end(from, fs), end(to, ts));
        let mut excluded = ancestor_groups(from);
        for g in ancestor_groups(to) {
            if !excluded.contains(&g) {
                excluded.push(g);
            }
        }
        let mut obstacles: Vec<Rb> = nodes.iter().filter(|n| n.id != e.from && n.id != e.to).map(|n| (&n.b).into()).collect();
        obstacles.extend(p.groups.iter().filter(|g| !excluded.contains(&g.id)).map(|g| Rb::from(&g.b)));
        routed.push(Routed { edge: e, points: route(&a, &b, &obstacles)? });
    }

    // -- canvas --
    let extent = |bs: Vec<Bx>, x: bool| nmax(bs.iter().map(|b| if x { b.x + b.w } else { b.y + b.h })).unwrap_or(I(0));
    let sb = || p.services.iter().map(|i| i.b).collect::<Vec<_>>();
    let jb = || p.junctions.iter().map(|i| i.b).collect::<Vec<_>>();
    let gb = || p.groups.iter().map(|g| g.b).collect::<Vec<_>>();
    let total = |x: bool| nmax([extent(sb(), x), extent(jb(), x), extent(gb(), x)]).unwrap() + I(SPACING);
    let width = nmax(std::iter::once(total(true)).chain(routed.iter().flat_map(|r| r.points.iter().map(|q| q.0)))).unwrap();
    let height = nmax(std::iter::once(total(false)).chain(routed.iter().flat_map(|r| r.points.iter().map(|q| q.1)))).unwrap();
    let mut doc = Document::new(width.f(), height.f());
    doc.view_box = Some(format!("0 0 {} {}", width.show(), height.show()));

    // -- groups --
    for gb in &p.groups {
        let Some(g) = l.group(&gb.id) else { continue };
        let (x, y, w, h) = (gb.b.x.f(), gb.b.y.f(), gb.b.w.f(), gb.b.h.f());
        let mut el = Elem::new("g").attr("id", format!("group-{}", g.id));
        el.push(
            Elem::new("rect")
                .attr("fill", col("group_background"))
                .attr("fill-opacity", "0.3")
                .attr("stroke", col("architecture_group_border"))
                .attr("stroke-width", "2")
                .num("x", x)
                .num("y", y)
                .num("width", w)
                .num("height", h)
                .num("rx", 8.0)
                .num("ry", 8.0)
                .attr("stroke-dasharray", "5,5"),
        );
        if let Some(label) = &g.label {
            el.push(text_el(label, x + 10.0, y + 20.0, "14", &col("architecture_group_label")).attr("font-weight", bold.clone()));
        }
        if let Some(icon) = &g.icon {
            el.push(text_el(glyph(icon), x + w - 30.0, y + 25.0, "20", &edge_color));
        }
        doc.add(el);
    }

    // -- edges --
    for r in &routed {
        let mut el = Elem::new("g").attr("id", format!("edge-{}-{}", r.edge.from, r.edge.to));
        let d_attr = r
            .points
            .iter()
            .enumerate()
            .map(|(i, q)| format!("{} {} {}", if i == 0 { "M" } else { "L" }, ruby_float(q.0.f()), ruby_float(q.1.f())))
            .collect::<Vec<_>>()
            .join(" ");
        let mut path = Elem::new("path").attr("fill", "none").attr("stroke", edge_stroke.clone());
        if let Some(w) = &stroke_width {
            path = path.attr("stroke-width", w.clone());
        }
        el.push(path.attr("d", d_attr));
        if let Some(label) = r.edge.label.as_deref().filter(|s| !s.is_empty()) {
            let (a, b) = (&r.points[0], &r.points[1]);
            let x = (a.0.f() + b.0.f()) / 2.0;
            let y = (a.1.f() + b.1.f()) / 2.0 - 5.0;
            el.push(text_el(label, x, y, "10", &edge_color).attr("text-anchor", "middle"));
        }
        doc.add(el);
    }

    // -- services and junctions --
    for i in nodes {
        let (x, y, w, h) = (i.b.x.f(), i.b.y.f(), i.b.w.f(), i.b.h.f());
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        let Some(si) = i.svc else {
            let mut el = Elem::new("g").attr("id", format!("junction-{}", i.id));
            let mut c = Elem::new("circle").attr("fill", node_fill.clone()).attr("stroke", node_stroke.clone());
            if let Some(sw) = &stroke_width {
                c = c.attr("stroke-width", sw.clone());
            }
            el.push(c.num("cx", cx).num("cy", cy).num("r", w / 2.0));
            doc.add(el);
            continue;
        };
        let s = &d.services[si];
        let mut el = Elem::new("g").attr("id", format!("service-{}", s.id));
        if icon_less(i) {
            let (ny, nw, nh) = (y - 5.0, FALLBACK_SERVICE_WIDTH, FALLBACK_SERVICE_HEIGHT);
            let (top, right, bottom) = (ny + 5.0, x + nw, ny + nh);
            let f = ruby_float;
            let path = format!(
                "M {} {} V {} Q {} {} {} {} H {} Q {} {} {} {} V {} H {} Z",
                f(x), f(bottom), f(top), f(x), f(ny), f(x + 5.0), f(ny), f(right - 5.0), f(right), f(ny), f(right), f(top), f(bottom), f(x)
            );
            el.push(
                Elem::new("path")
                    .attr("fill", "none")
                    .attr("stroke", node_stroke.clone())
                    .attr("stroke-width", "2")
                    .attr("d", path)
                    .attr("stroke-dasharray", "8"),
            );
        } else {
            let mut r = Elem::new("rect").attr("fill", node_fill.clone()).attr("stroke", node_stroke.clone());
            if let Some(sw) = &stroke_width {
                r = r.attr("stroke-width", sw.clone());
            }
            el.push(r.num("x", x).num("y", y).num("width", w).num("height", h).num("rx", 5.0).num("ry", 5.0));
        }
        if let Some(icon) = &s.icon {
            el.push(text_el(glyph(icon), cx, cy, "24", &edge_color).attr("text-anchor", "middle").attr("dominant-baseline", "middle"));
        }
        if let Some(label) = &s.label {
            el.push(
                text_el(label, cx, y + h + SERVICE_LABEL_OFFSET as f64, "12", &label_text)
                    .attr("text-anchor", "middle")
                    .attr("font-family", font_family.clone())
                    .attr("dominant-baseline", "middle"),
            );
        }
        doc.add(el);
    }
    Ok(doc.to_xml())
}
