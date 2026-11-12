//! State diagram renderer: port of Ruby `Parser::Builders::StateDiagram` (model),
//! `Layout::StateDiagram` + `Layout::Grid` (placement, which does not call Elkrb)
//! and `Renderer::StateDiagram`.
//!
//! Declined to the Ruby path (`Error::Unported`): a frontmatter/directive
//! preamble, a top-level terminal/fork/join whose position stays on the grid
//! (its Ruby integer-ness decides the viewBox text), and a note whose target is
//! a hidden bare `state X` declaration.
use super::pie::{items, strip, text_of};
use crate::svg::{ruby_float, ArrowPath, Arrowhead, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

const MARGIN: f64 = 50.0;
const MIN_CELL_WIDTH: f64 = 250.0;
const MIN_CELL_HEIGHT: f64 = 200.0;
const CELL_GAP: f64 = 30.0;
const CLUSTER_PADDING: f64 = 20.0;
const COLUMNS: usize = 3;
const MMDC_STATE_FONT_SIZE: f64 = 16.0;
const MMDC_STATE_HEIGHT: f64 = 40.0;
const MMDC_STATE_PADDING: f64 = 16.0;
const MMDC_COMPOSITE_SIDE_PADDING: f64 = 48.3515625;
const MMDC_COMPOSITE_START_Y: f64 = 29.5;
const MMDC_COMPOSITE_FIRST_STATE_Y: f64 = 126.5;
const MMDC_COMPOSITE_SECOND_STATE_Y: f64 = 265.5;
const MMDC_COMPOSITE_BOTTOM_PADDING: f64 = 37.5;
const MMDC_NOTE_WIDTH: f64 = 230.0;
const MMDC_NOTE_HEIGHT: f64 = 78.0;
const MMDC_MULTILINE_NOTE_PADDING: f64 = 30.0;
const MMDC_MULTILINE_NOTE_LINE_HEIGHT: f64 = 24.0;
const MMDC_DISCONNECTED_PAIR_INSET: f64 = 8.0;
const MMDC_DISCONNECTED_PAIR_GAP: f64 = 50.0;
const CR_OUTER_PADDING: f64 = 35.0;
const CR_TOP: f64 = 37.5;
const CR_WIDTH_PADDING: f64 = 70.0;
const CR_HEIGHT: f64 = 140.0;
const CR_CHILD_TOP: f64 = 50.0;
const CR_GAP: f64 = 50.0;

fn unported() -> Error {
    Error::Unported("state_diagram".into())
}

// ---- model (builder) ----

#[derive(Clone, Default)]
struct St {
    id: String,
    label: Option<String>,
    ty: String,
    description: Option<String>,
    descriptions: Vec<String>,
    children: Vec<St>,
    decl_only: bool,
    note_target: Option<String>,
    note_pos: Option<String>,
}

struct Tr {
    from: String,
    to: String,
    trigger: Option<String>,
    guard: Option<String>,
}

struct Note {
    id: String,
    text: String,
    target: String,
    position: String,
}

#[derive(Default)]
struct Model {
    states: Vec<St>,
    trans: Vec<Tr>,
    notes: Vec<Note>,
    direction: Option<String>,
    state_counter: usize,
    region_counter: usize,
    cur: Vec<usize>,
}

fn at<'a>(states: &'a mut Vec<St>, path: &[usize]) -> &'a mut St {
    let (first, rest) = path.split_first().expect("non-empty path");
    let s = &mut states[*first];
    if rest.is_empty() { s } else { at(&mut s.children, rest) }
}

fn dfs(states: &[St], id: &str, prefix: &mut Vec<usize>) -> Option<Vec<usize>> {
    for (i, s) in states.iter().enumerate() {
        prefix.push(i);
        if s.id == id {
            return Some(prefix.clone());
        }
        if let Some(p) = dfs(&s.children, id, prefix) {
            return Some(p);
        }
        prefix.pop();
    }
    None
}

fn txt(v: &Value) -> String {
    match v {
        Value::Object(o) if !(o.contains_key("value") && o.contains_key("offset")) => {
            for k in ["string", "marker_type", "dir_value", "label_text"] {
                if let Some(x) = o.get(k).filter(|x| !x.is_null()) {
                    return txt(x);
                }
            }
            o.values().next().map(txt).unwrap_or_default()
        }
        v => text_of(v),
    }
}

fn has<'a>(s: &'a Value, k: &str) -> Option<&'a Value> {
    s.get(k).filter(|v| !v.is_null())
}

impl Model {
    fn container(&mut self) -> &mut Vec<St> {
        if self.cur.is_empty() {
            &mut self.states
        } else {
            let cur = self.cur.clone();
            &mut at(&mut self.states, &cur).children
        }
    }

    fn container_ref(&self) -> &Vec<St> {
        let mut states = &self.states;
        for i in &self.cur {
            states = &states[*i].children;
        }
        states
    }

    fn find_state(&self, id: &str) -> Option<Vec<usize>> {
        if let Some(i) = self.container_ref().iter().position(|s| s.id == id) {
            let mut p = self.cur.clone();
            p.push(i);
            return Some(p);
        }
        dfs(&self.states, id, &mut vec![])
    }

    fn node(&mut self, path: &[usize]) -> &mut St {
        at(&mut self.states, path)
    }

    fn push_state(&mut self, st: St) -> Vec<usize> {
        let cur = self.cur.clone();
        let c = self.container();
        c.push(st);
        let mut p = cur;
        p.push(c.len() - 1);
        p
    }

    fn build_state(id: &str, label: Option<String>, ty: &str) -> St {
        St { id: id.into(), label, ty: ty.into(), ..Default::default() }
    }

    fn apply(&mut self, tree: &Value) -> Result<(), Error> {
        for item in items(tree) {
            if let Some(d) = has(item, "direction") {
                if let Some(v) = has(d, "dir_value") {
                    self.direction = Some(txt(v));
                }
            }
            if has(item, "header").is_none() && has(item, "direction").is_none() {
                self.statement(item)?;
            }
        }
        Ok(())
    }

    fn statement(&mut self, s: &Value) -> Result<(), Error> {
        if !s.is_object() {
            return Ok(());
        }
        if s.get("keyword").map(txt).as_deref() == Some("state") && has(s, "state_id").is_some() {
            return self.state_declaration(s);
        }
        if has(s, "from").is_some() && has(s, "to").is_some() {
            return self.transition(s);
        }
        if has(s, "note_keyword").is_some() || has(s, "style_keyword").is_some() {
            if has(s, "note_keyword").is_some() && has(s, "note_target").is_some() {
                self.note(s);
            }
            return Ok(());
        }
        if has(s, "state_id").is_some() && has(s, "keyword").is_none() {
            let id = txt(&s["state_id"]);
            let p = self.ensure_state_exists(&id);
            if let Some(d) = has(s, "description") {
                let d = txt(d);
                let st = self.node(&p);
                st.description = Some(d.clone());
                if !d.is_empty() {
                    st.descriptions.push(d);
                }
            }
        }
        Ok(())
    }

    fn note(&mut self, s: &Value) {
        let target = txt(&s["note_target"]);
        if self.find_state(&target).is_none() {
            return;
        }
        self.notes.push(Note {
            id: format!("{target}----note"),
            text: strip(&txt(&s["note_text"])).to_string(),
            target,
            position: txt(&s["position"]),
        });
    }

    fn state_declaration(&mut self, s: &Value) -> Result<(), Error> {
        let id = txt(&s["state_id"]);
        let label = has(s, "state_label").map(|l| strip(&txt(l)).to_string()).filter(|l| !l.is_empty());
        if let Some(marker) = has(s, "marker") {
            let ty = txt(&marker["marker_type"]);
            if let Some(p) = self.find_state(&id) {
                self.node(&p).decl_only = false;
            } else {
                let p = self.ensure_state_exists(&id);
                let st = self.node(&p);
                st.label = None;
                st.ty = ty;
            }
        } else if let Some(c) = has(s, "composite") {
            let p = self.add_or_update_state(&id, label.clone());
            self.composite_statements(&p, c)?;
            let st = self.node(&p);
            st.decl_only = false;
            if let Some(l) = label.filter(|l| !l.is_empty()) {
                st.descriptions.push(l);
            }
        } else {
            let description = has(s, "description").map(txt);
            let existing = self.find_state(&id);
            let p = self.add_or_update_state(&id, label.clone());
            let st = self.node(&p);
            st.decl_only = existing.is_none() && label.is_none() && description.is_none();
            if let Some(l) = label.filter(|l| !l.is_empty()) {
                st.descriptions.push(l);
            }
            if let Some(d) = description {
                st.description = Some(d.clone());
                if !d.is_empty() {
                    st.descriptions.push(d);
                }
            }
        }
        Ok(())
    }

    fn composite_statements(&mut self, parent: &[usize], composite: &Value) -> Result<(), Error> {
        let statements: Vec<&Value> = items(composite);
        let saved = std::mem::replace(&mut self.cur, parent.to_vec());
        let result = if !statements.iter().any(|s| has(s, "concurrent_sep").is_some()) {
            statements.iter().try_for_each(|s| self.statement(s))
        } else {
            let mut groups: Vec<Vec<&Value>> = vec![vec![]];
            for s in &statements {
                if has(s, "concurrent_sep").is_some() {
                    groups.push(vec![]);
                } else {
                    groups.last_mut().unwrap().push(s);
                }
            }
            groups.iter().try_for_each(|group| {
                self.region_counter += 1;
                let region = Self::build_state(&format!("concurrent_region_{}", self.region_counter), None, "normal");
                let rp = self.push_state(region);
                let inner = std::mem::replace(&mut self.cur, rp);
                let r = group.iter().try_for_each(|s| self.statement(s));
                self.cur = inner;
                r
            })
        };
        self.cur = saved;
        result
    }

    fn add_or_update_state(&mut self, id: &str, label: Option<String>) -> Vec<usize> {
        if let Some(p) = self.find_state(id) {
            if let Some(l) = label.filter(|l| !l.is_empty()) {
                self.node(&p).label = Some(l);
            }
            return p;
        }
        let l = label.unwrap_or_else(|| id.to_string());
        self.push_state(Self::build_state(id, Some(l), "normal"))
    }

    fn ensure_state_exists(&mut self, id: &str) -> Vec<usize> {
        if let Some(p) = self.find_state(id) {
            self.node(&p).decl_only = false;
            return p;
        }
        self.push_state(Self::build_state(id, Some(id.to_string()), "normal"))
    }

    fn marker_state(&mut self, ty: &str) -> String {
        if let Some(s) = self.container_ref().iter().find(|s| s.ty == ty) {
            return s.id.clone();
        }
        self.state_counter += 1;
        let id = format!("{ty}_{}", self.state_counter);
        self.push_state(Self::build_state(&id, Some("[*]".into()), ty));
        id
    }

    fn state_id(&mut self, v: &Value, source: bool) -> String {
        let s = txt(v);
        if strip(&s) != "[*]" {
            return s;
        }
        self.marker_state(if source { "start" } else { "end" })
    }

    fn transition(&mut self, s: &Value) -> Result<(), Error> {
        let from = self.state_id(&s["from"], true);
        let to = self.state_id(&s["to"], false);
        let (trigger, guard) = parse_label(s.get("label").filter(|v| !v.is_null()))?;
        self.create_transition(from, to.clone(), trigger, guard);
        let mut current = to;
        for item in items(s.get("chain").unwrap_or(&Value::Null)) {
            if let Some(c) = has(item, "chain_to") {
                let to = self.state_id(c, false);
                self.create_transition(current.clone(), to.clone(), None, None);
                current = to;
            }
        }
        Ok(())
    }

    fn create_transition(&mut self, from: String, to: String, trigger: Option<String>, guard: Option<String>) {
        for id in [&from, &to] {
            if !(id.starts_with("start_") || id.starts_with("end_")) {
                self.ensure_state_exists(id);
            }
        }
        self.trans.push(Tr { from, to, trigger, guard });
    }
}

fn parse_label(label: Option<&Value>) -> Result<(Option<String>, Option<String>), Error> {
    let Some(label) = label else { return Ok((None, None)) };
    let text = strip(&txt(label)).to_string();
    if text.is_empty() {
        return Ok((None, None));
    }
    if text.contains('\n') {
        return Err(unported());
    }
    let re = Regex::new(r"^(.+?)\s*\[(.+?)\]\s*$").unwrap();
    Ok(match re.captures(&text) {
        Some(c) => (Some(strip(&c[1]).to_string()), Some(strip(&c[2]).to_string())),
        None => (Some(text), None),
    })
}

// ---- layout ----

#[derive(Clone, Default)]
struct Ln {
    id: String,
    w: f64,
    h: f64,
    x: Option<f64>,
    y: Option<f64>,
    /// Ruby Integer-typed (vs Float) geometry, which decides viewBox text.
    xi: bool,
    yi: bool,
    wi: bool,
    hi: bool,
    labels: Vec<String>,
    shape: String,
    cluster: bool,
    note_target: Option<String>,
    note_pos: Option<String>,
    children: Vec<Ln>,
}

impl Ln {
    fn xv(&self) -> f64 {
        self.x.unwrap_or(0.0)
    }
    fn yv(&self) -> f64 {
        self.y.unwrap_or(0.0)
    }
    fn set_x(&mut self, v: f64, int: bool) {
        self.x = Some(v);
        self.xi = int;
    }
    fn set_y(&mut self, v: f64, int: bool) {
        self.y = Some(v);
        self.yi = int;
    }
    fn set_w(&mut self, v: f64, int: bool) {
        self.w = v;
        self.wi = int;
    }
    fn set_h(&mut self, v: f64, int: bool) {
        self.h = v;
        self.hi = int;
    }
}

struct Edge {
    id: String,
    source: String,
    target: String,
    label: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Down,
    Right,
    Left,
    Up,
}

struct Layout {
    nf: f64,
    sf: f64,
    dir: Dir,
    edges: Vec<Edge>,
}

fn shape_type(s: &St) -> String {
    if s.ty == "note" {
        return "note".into();
    }
    if s.descriptions.iter().any(|d| !d.is_empty()) {
        return "normal".into();
    }
    if s.description.as_ref().is_some_and(|d| !d.is_empty()) {
        return "normal".into();
    }
    s.ty.clone()
}

fn state_texts(s: &St) -> Vec<String> {
    let d: Vec<String> = s.descriptions.iter().filter(|x| !x.is_empty()).cloned().collect();
    if !d.is_empty() {
        return d;
    }
    [s.label.clone(), s.description.clone()].into_iter().flatten().filter(|x| !x.is_empty()).collect()
}

fn arial(t: &str, size: f64) -> text::Measure {
    text::measure(t, size, Face::Arial)
}

fn mmdc_state_text_width(t: &str) -> f64 {
    let measured = text::width(t, MMDC_STATE_FONT_SIZE, Face::Trebuchet);
    (((measured * 64.0).round() + 1.0) / 64.0) + MMDC_STATE_PADDING
}

fn mmdc_note_text_width(t: &str) -> f64 {
    let measured = text::width(t, MMDC_STATE_FONT_SIZE, Face::Trebuchet);
    ((measured * 32.0).round() + 1.0) / 32.0
}

impl Layout {
    fn node(&self, s: &St) -> Ln {
        let texts = state_texts(s);
        let shape = shape_type(s);
        let label = arial(texts.first().map_or(s.id.as_str(), |t| t.as_str()), self.nf);
        let descs = &texts[texts.len().min(1)..];
        let (w, h, wi, hi) = match shape.as_str() {
            "start" | "end" => (30.0, 30.0, true, true),
            "choice" => {
                let size = label.width.max(label.height) + 40.0;
                (size, size, false, false)
            }
            "fork" | "join" => (100.0, 10.0, true, true),
            "note" => {
                let multiline = s.note_target.is_some() && !descs.is_empty() && self.nf == 14.0 && self.sf == 12.0;
                if multiline {
                    let width = texts.iter().map(|l| mmdc_note_text_width(l)).fold(f64::MIN, f64::max);
                    (
                        width + MMDC_MULTILINE_NOTE_PADDING,
                        texts.len() as f64 * MMDC_MULTILINE_NOTE_LINE_HEIGHT + MMDC_MULTILINE_NOTE_PADDING,
                        false,
                        false,
                    )
                } else {
                    let ms: Vec<_> = descs.iter().map(|d| arial(d, self.sf)).collect();
                    (normal_width(&label, &ms), normal_height(&label, &ms), false, false)
                }
            }
            _ => {
                if descs.is_empty() && s.children.is_empty() {
                    (mmdc_state_text_width(texts.first().map_or(s.id.as_str(), |t| t.as_str())), MMDC_STATE_HEIGHT, false, false)
                } else {
                    let ms: Vec<_> = descs.iter().map(|d| arial(d, self.sf)).collect();
                    let (w, h) = (normal_width(&label, &ms), normal_height(&label, &ms));
                    (w.max(100.0), h.max(50.0), 100.0 > w, 50.0 > h)
                }
            }
        };
        Ln {
            id: s.id.clone(),
            w,
            h,
            wi,
            hi,
            labels: texts,
            cluster: !s.children.is_empty(),
            note_target: s.note_target.clone(),
            note_pos: s.note_pos.clone(),
            children: s.children.iter().map(|c| self.node(c)).collect(),
            shape,
            ..Default::default()
        }
    }
}

fn normal_width(label: &text::Measure, ms: &[text::Measure]) -> f64 {
    ms.iter().map(|m| m.width).chain(std::iter::once(label.width)).fold(f64::MIN, f64::max) + 40.0
}

fn normal_height(label: &text::Measure, ms: &[text::Measure]) -> f64 {
    label.height + 30.0 + ms.iter().map(|m| m.height + 10.0).sum::<f64>()
}

// -- Grid --

fn grid_place(children: &mut [Ln], lay: &Layout) {
    for c in children.iter_mut() {
        if c.cluster {
            grid_pack(c, lay);
        }
    }
    grid_arrange(children);
}

fn grid_pack(cluster: &mut Ln, lay: &Layout) {
    for c in cluster.children.iter_mut() {
        if c.cluster {
            grid_pack(c, lay);
        }
    }
    grid_arrange(&mut cluster.children);
    grid_fit(cluster, lay);
}

fn label_size(c: &Ln, axis_h: bool, lay: &Layout) -> f64 {
    match c.labels.first() {
        None => 0.0,
        Some(t) => {
            let m = arial(t, lay.nf);
            (if axis_h { m.height } else { m.width }).trunc()
        }
    }
}

fn grid_fit(cluster: &mut Ln, lay: &Layout) {
    let band = label_size(cluster, true, lay) + CLUSTER_PADDING * 2.0;
    let titled = label_size(cluster, false, lay) + CLUSTER_PADDING * 2.0;
    let contents = &mut cluster.children;
    if contents.is_empty() {
        cluster.w = titled;
        cluster.h = band + CLUSTER_PADDING;
        cluster.wi = true;
        cluster.hi = true;
        return;
    }
    let low_x = contents.iter().map(|c| c.xv().trunc()).fold(f64::MAX, f64::min);
    let low_y = contents.iter().map(|c| c.yv().trunc()).fold(f64::MAX, f64::min);
    for c in contents.iter_mut() {
        let (dx, dy) = (CLUSTER_PADDING - low_x, band - low_y);
        c.set_x(c.xv().trunc() + dx, true);
        c.set_y(c.yv().trunc() + dy, true);
    }
    let hx = contents.iter().map(|c| c.xv().trunc() + c.w.trunc()).fold(f64::MIN, f64::max);
    let hy = contents.iter().map(|c| c.yv().trunc() + c.h.trunc()).fold(f64::MIN, f64::max);
    cluster.w = (hx + CLUSTER_PADDING).max(titled);
    cluster.h = hy + CLUSTER_PADDING;
    cluster.wi = true;
    cluster.hi = true;
}

fn grid_arrange(children: &mut [Ln]) {
    let n_rows = children.len().div_ceil(COLUMNS);
    let widths: Vec<f64> = (0..COLUMNS)
        .map(|col| {
            let mut m = MIN_CELL_WIDTH;
            for r in 0..n_rows {
                if let Some(c) = children.get(r * COLUMNS + col).filter(|c| c.cluster) {
                    m = m.max(c.w.trunc() + CELL_GAP);
                }
            }
            m
        })
        .collect();
    let heights: Vec<f64> = (0..n_rows)
        .map(|r| {
            let mut m = MIN_CELL_HEIGHT;
            for c in children.iter().skip(r * COLUMNS).take(COLUMNS).filter(|c| c.cluster) {
                m = m.max(c.h.trunc() + CELL_GAP);
            }
            m
        })
        .collect();
    for (i, c) in children.iter_mut().enumerate() {
        let (r, col) = (i / COLUMNS, i % COLUMNS);
        if c.x.is_some() && c.y.is_some() {
            continue;
        }
        c.set_x(MARGIN + widths.iter().take(col).sum::<f64>(), true);
        c.set_y(MARGIN + heights.iter().take(r).sum::<f64>(), true);
    }
}

// -- hacks --

fn connected(edges: &[Edge], a: &Ln, b: &Ln) -> bool {
    edges.iter().any(|e| e.source == a.id && e.target == b.id)
}

fn visit(nodes: &mut [Ln], f: &mut dyn FnMut(&mut Ln)) {
    for n in nodes.iter_mut() {
        f(n);
        visit(&mut n.children, f);
    }
}

fn index(nodes: &[Ln], out: &mut HashMap<String, (f64, f64, f64, f64)>) {
    for n in nodes {
        out.insert(n.id.clone(), (n.xv(), n.yv(), n.w, subtree_height(n)));
        index(&n.children, out);
    }
}

impl Layout {
    fn plain_nested_graph(&self) -> bool {
        self.nf == 14.0 && self.sf == 12.0 && self.dir == Dir::Down
    }

    fn place(&self, top: &mut Vec<Ln>) -> Result<(), Error> {
        grid_place(top, self);
        // place_disconnected_normal_pair
        if top.len() == 2 && self.edges.is_empty() && top.iter().all(|s| s.shape == "normal" && s.children.is_empty()) {
            let mut x = MMDC_DISCONNECTED_PAIR_INSET;
            for s in top.iter_mut() {
                s.set_x(x, false);
                s.set_y(MMDC_DISCONNECTED_PAIR_INSET, false);
                x += s.w + MMDC_DISCONNECTED_PAIR_GAP;
            }
        }
        // orient_grid_terminals
        if self.dir == Dir::Down {
            visit(top, &mut |s| {
                if s.shape == "start" || s.shape == "end" {
                    let (x, y, xi, yi) = (s.x, s.y, s.xi, s.yi);
                    s.x = y;
                    s.y = x;
                    s.xi = yi;
                    s.yi = xi;
                }
            });
        }
        self.chain(top);
        self.concurrent(top);
        self.plain_composite(top);
        self.note_composite(top);
        self.attach_notes(top);
        Ok(())
    }

    fn chain(&self, top: &mut [Ln]) {
        if !matches!(self.dir, Dir::Down | Dir::Right) || top.is_empty() {
            return;
        }
        if !top.iter().all(|n| n.children.is_empty() && matches!(n.shape.as_str(), "start" | "end" | "normal")) {
            return;
        }
        let start: Vec<usize> = (0..top.len()).filter(|&i| top[i].shape == "start").collect();
        let finish: Vec<usize> = (0..top.len()).filter(|&i| top[i].shape == "end").collect();
        let states: Vec<usize> = (0..top.len()).filter(|&i| top[i].shape == "normal").collect();
        if !(start.len() == 1 && finish.len() <= 1 && !states.is_empty()) {
            return;
        }
        // ordered chain
        let mut ordered: Vec<usize> = vec![];
        let mut current = start[0];
        loop {
            let remaining: Vec<usize> = states.iter().copied().filter(|i| !ordered.contains(i)).collect();
            let targets: Vec<&String> =
                self.edges.iter().filter(|e| e.source == top[current].id).map(|e| &e.target).collect();
            let cands: Vec<usize> = remaining.into_iter().filter(|&i| targets.contains(&&top[i].id)).collect();
            if cands.len() > 1 {
                return;
            }
            match cands.first() {
                None => break,
                Some(&c) => {
                    current = c;
                    ordered.push(c);
                }
            }
        }
        if ordered.len() != states.len() {
            return;
        }
        let s0 = start[0];
        if self.dir == Dir::Right {
            let center = ordered.iter().map(|&i| top[i].h).fold(f64::MIN, f64::max) / 2.0;
            let sh = top[s0].h;
            top[s0].set_x(0.0, false);
            top[s0].set_y(center - sh / 2.0, false);
            let mut pos = 72.0;
            for &i in &ordered {
                let h = top[i].h;
                top[i].set_x(pos, false);
                top[i].set_y(center - h / 2.0, false);
                pos += top[i].w + 50.0;
            }
            if let Some(&f) = finish.first() {
                let fh = top[f].h;
                top[f].set_x(pos - 8.0, false);
                top[f].set_y(center - fh / 2.0, false);
            }
        } else {
            let center = ordered.iter().map(|&i| top[i].w).fold(f64::MIN, f64::max) / 2.0;
            let sw = top[s0].w;
            top[s0].set_x(center - sw / 2.0, false);
            top[s0].set_y(0.0, false);
            let mut pos = 72.0;
            for &i in &ordered {
                let w = top[i].w;
                top[i].set_x(center - w / 2.0, false);
                top[i].set_y(pos, false);
                pos += top[i].h + 50.0;
            }
            if let Some(&f) = finish.first() {
                let fw = top[f].w;
                top[f].set_x(center - fw / 2.0, false);
                top[f].set_y(pos - 8.0, false);
            }
        }
    }

    fn concurrent(&self, top: &mut [Ln]) {
        if !self.plain_nested_graph() || !self.edges.is_empty() || top.len() != 1 || !top[0].cluster {
            return;
        }
        let re = Regex::new(r"\Aconcurrent_region_\d+\z").unwrap();
        let regions = &top[0].children;
        if !(regions.len() > 1
            && regions.iter().all(|r| {
                r.cluster && re.is_match(&r.id) && r.children.len() == 1 && !r.children[0].cluster
            }))
        {
            return;
        }
        let mut position = CR_OUTER_PADDING;
        for region in top[0].children.iter_mut() {
            let cw = region.children[0].w;
            let width = cw + CR_WIDTH_PADDING;
            region.set_x(position, false);
            region.set_y(CR_TOP, false);
            region.set_w(width, false);
            region.set_h(CR_HEIGHT, false);
            region.children[0].set_x(CR_OUTER_PADDING, false);
            region.children[0].set_y(CR_CHILD_TOP, false);
            position += width + CR_GAP;
        }
        let c = &mut top[0];
        c.set_x(MARGIN, true);
        c.set_y(MARGIN, true);
        c.set_w(position - CR_GAP + CR_OUTER_PADDING, false);
        c.set_h(CR_TOP + CR_HEIGHT + CR_TOP, false);
    }

    /// `nested_note_nodes`: indices of (start, first, second) inside `composite`.
    fn nested_nodes(&self, composite: &Ln) -> Option<(usize, usize, usize)> {
        let ch = &composite.children;
        if ch.len() != 3 {
            return None;
        }
        let start = ch.iter().position(|n| n.shape == "start")?;
        let states: Vec<usize> = (0..3).filter(|&i| i != start).collect();
        if !states.iter().all(|&i| ch[i].shape == "normal") {
            return None;
        }
        let first = states.iter().copied().find(|&i| connected(&self.edges, &ch[start], &ch[i]))?;
        let second = states.iter().copied().find(|&i| i != first)?;
        if connected(&self.edges, &ch[first], &ch[second]) && connected(&self.edges, &ch[second], &ch[first]) {
            Some((start, first, second))
        } else {
            None
        }
    }

    fn size_nested(composite: &mut Ln, first: usize, second: usize) {
        for i in [first, second] {
            let w = mmdc_state_text_width(composite.children[i].labels.first().map_or("", |s| s.as_str()));
            composite.children[i].set_w(w, false);
            composite.children[i].set_h(MMDC_STATE_HEIGHT, false);
        }
    }

    fn place_nested_children(composite: &mut Ln, width: f64, s: usize, f: usize, sec: usize) {
        let ch = &mut composite.children;
        let sw = ch[s].w;
        ch[s].set_x((width - sw) / 2.0, false);
        ch[s].set_y(MMDC_COMPOSITE_START_Y, false);
        let fw = ch[f].w;
        ch[f].set_x((width - fw) / 2.0, false);
        ch[f].set_y(MMDC_COMPOSITE_FIRST_STATE_Y, false);
        let sew = ch[sec].w;
        ch[sec].set_x((width - sew) / 2.0, false);
        ch[sec].set_y(MMDC_COMPOSITE_SECOND_STATE_Y, false);
    }

    fn composite_dims(composite: &Ln, f: usize, sec: usize) -> (f64, f64) {
        let ch = &composite.children;
        (
            ch[f].w.max(ch[sec].w) + MMDC_COMPOSITE_SIDE_PADDING * 2.0,
            MMDC_COMPOSITE_SECOND_STATE_Y + ch[sec].h + MMDC_COMPOSITE_BOTTOM_PADDING,
        )
    }

    fn plain_composite(&self, top: &mut [Ln]) {
        if !self.plain_nested_graph() || top.len() != 1 || !top[0].cluster {
            return;
        }
        let Some((s, f, sec)) = self.nested_nodes(&top[0]) else { return };
        let c = &mut top[0];
        Self::size_nested(c, f, sec);
        let (width, height) = Self::composite_dims(c, f, sec);
        c.set_x(MARGIN, true);
        c.set_y(MARGIN, true);
        c.set_w(width, false);
        c.set_h(height, false);
        Self::place_nested_children(c, width, s, f, sec);
    }

    fn note_composite(&self, top: &mut [Ln]) {
        if !self.plain_nested_graph() || top.len() != 3 {
            return;
        }
        let Some(ci) = top.iter().position(|n| n.cluster) else { return };
        let Some(ri) = top.iter().position(|n| n.shape == "start") else { return };
        let comp_id = top[ci].id.clone();
        let Some(ni) = top.iter().position(|n| {
            n.note_target.as_deref() == Some(comp_id.as_str()) && n.note_pos.as_deref() == Some("right") && {
                let w = text::width(n.labels.first().map_or("", |s| s.as_str()), MMDC_STATE_FONT_SIZE, Face::Trebuchet);
                w > 200.0 && w <= 400.0
            }
        }) else {
            return;
        };
        if !connected(&self.edges, &top[ri], &top[ci]) {
            return;
        }
        let Some((s, f, sec)) = self.nested_nodes(&top[ci]) else { return };
        Self::size_nested(&mut top[ci], f, sec);
        top[ni].set_w(MMDC_NOTE_WIDTH, false);
        top[ni].set_h(MMDC_NOTE_HEIGHT, false);
        let (width, height) = Self::composite_dims(&top[ci], f, sec);
        let c = &mut top[ci];
        c.set_x(MARGIN, true);
        c.set_y(MARGIN + 72.0, true);
        c.set_w(width, false);
        c.set_h(height, false);
        let cx = c.xv();
        Self::place_nested_children(c, width, s, f, sec);
        let rw = top[ri].w;
        top[ri].set_x(cx + (width - rw) / 2.0, false);
        top[ri].set_y(MARGIN, true);
    }

    fn attach_notes(&self, top: &mut Vec<Ln>) {
        // id -> (x, y, w, subtree height); later duplicates win, like Ruby's to_h
        let mut targets: HashMap<String, (f64, f64, f64, f64)> = HashMap::new();
        index(top, &mut targets);
        visit(top, &mut |n| {
            let Some(t) = n.note_target.as_ref().and_then(|t| targets.get(t)) else { return };
            let (tx, ty, tw, th) = *t;
            let x = tx + (tw - n.w) / 2.0;
            n.set_x(x, false);
            n.set_y(ty + th + 75.0, false);
        });
    }
}

fn subtree_height(s: &Ln) -> f64 {
    let extent = s.children.iter().map(|c| c.yv() + subtree_height(c)).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
    extent.map_or(s.h, |e| s.h.max(e))
}

// ---- render ----

struct Abs {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn collect(nodes: &[Ln], px: f64, py: f64, out: &mut HashMap<String, Abs>) {
    for n in nodes {
        let (x, y) = (n.xv() + px, n.yv() + py);
        out.insert(n.id.clone(), Abs { x, y, w: n.w, h: n.h });
        collect(&n.children, x, y, out);
    }
}

fn coord(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { ruby_float(v) }
}

fn fmt_size(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { ruby_float(v) }
}

fn font_text(label: &str, x: f64, y: f64, size: f64, fill: &str) -> Elem {
    Elem::new("text")
        .attr("fill", fill)
        .num("x", x)
        .num("y", y)
        .attr("text-anchor", "middle")
        .attr("font-family", "Arial, sans-serif")
        .attr("font-size", fmt_size(size))
        .content(label)
}

struct Style {
    node_fill: &'static str,
    node_stroke: &'static str,
    edge_stroke: &'static str,
    label_text: &'static str,
}

fn render_state(n: &Ln, px: f64, py: f64, lay: &Layout, st: &Style) -> Elem {
    let (x, y) = (n.xv() + px, n.yv() + py);
    let (cx, cy) = (x + n.w / 2.0, y + n.h / 2.0);
    let mut g = Elem::new("g").attr("id", format!("state-{}", n.id));
    let rect = |x: f64, y: f64, w: f64, h: f64| Elem::new("rect").num("x", x).num("y", y).num("width", w).num("height", h);
    let normal = |x, y, w, h| {
        rect(x, y, w, h)
            .num("rx", 10.0)
            .num("ry", 10.0)
            .attr("fill", st.node_fill)
            .attr("stroke", st.node_stroke)
            .attr("stroke-width", "2")
    };
    let radius = if matches!(n.shape.as_str(), "start" | "end") { 7.0 } else { n.w.min(n.h) / 2.0 };
    match n.shape.as_str() {
        "start" => g.push(
            Elem::new("circle").attr("fill", st.node_stroke).attr("stroke", "none").num("cx", cx).num("cy", cy).num("r", radius),
        ),
        "end" => {
            let mut inner = Elem::new("g");
            inner.push(
                Elem::new("circle")
                    .attr("fill", "none")
                    .attr("stroke", st.node_stroke)
                    .attr("stroke-width", "2")
                    .num("cx", cx)
                    .num("cy", cy)
                    .num("r", radius),
            );
            inner.push(
                Elem::new("circle").attr("fill", st.node_stroke).attr("stroke", "none").num("cx", cx).num("cy", cy).num("r", radius - 5.0),
            );
            g.push(inner);
        }
        "choice" => {
            let pts = [
                format!("{},{}", ruby_float(cx), ruby_float(y)),
                format!("{},{}", ruby_float(x + n.w), ruby_float(cy)),
                format!("{},{}", ruby_float(cx), ruby_float(y + n.h)),
                format!("{},{}", ruby_float(x), ruby_float(cy)),
            ]
            .join(" ");
            g.push(
                Elem::new("polygon")
                    .attr("fill", st.node_fill)
                    .attr("stroke", st.node_stroke)
                    .attr("stroke-width", "2")
                    .attr("points", pts),
            );
        }
        "fork" | "join" => g.push(
            Elem::new("rect")
                .attr("fill", st.node_stroke)
                .attr("stroke", "none")
                .num("x", x)
                .num("y", y + n.h / 2.0 - 5.0)
                .num("width", n.w)
                .num("height", 10.0),
        ),
        "note" => g.push(
            rect(x, y, n.w, n.h).attr("fill", "#fff5ad").attr("stroke", "#aaaa33").attr("stroke-width", "1"),
        ),
        _ => g.push(normal(x, y, n.w, n.h)),
    }
    let stack = (n.labels.len() as f64 - 1.0) * 10.0;
    for (i, l) in n.labels.iter().enumerate() {
        let size = if i == 0 { lay.nf } else { lay.sf };
        g.push(font_text(l, cx, cy - stack + i as f64 * 20.0, size, st.label_text).attr("dominant-baseline", "middle"));
    }
    for c in &n.children {
        g.push(render_state(c, x, y, lay, st));
    }
    g
}

pub fn render(source: &str) -> Result<String, Error> {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    if ["layout", "theme", "init", "---", "config"].iter().any(|k| preamble.contains(k)) {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "state_diagram")?;
    let mut model = Model::default();
    model.apply(&tree)?;

    let theme = Theme::default_theme();
    let st = Style {
        node_fill: theme.color("node_fill").unwrap_or(""),
        node_stroke: theme.color("node_stroke").unwrap_or(""),
        edge_stroke: theme.color("edge_stroke").unwrap_or(""),
        label_text: theme.color("label_text").unwrap_or(""),
    };
    let dir = match model.direction.as_deref() {
        Some("LR") => Dir::Right,
        Some("RL") => Dir::Left,
        Some("BT") => Dir::Up,
        _ => Dir::Down,
    };

    // IR adapter: bare `state X` declarations stay hidden, children of hidden ones too.
    fn visible(states: &[St]) -> Vec<St> {
        states
            .iter()
            .filter(|s| !s.decl_only)
            .map(|s| St { children: visible(&s.children), ..s.clone() })
            .collect()
    }
    fn ids(states: &[St], out: &mut Vec<String>) {
        for s in states {
            out.push(s.id.clone());
            ids(&s.children, out);
        }
    }
    let shown = visible(&model.states);
    let mut shown_ids = vec![];
    ids(&shown, &mut shown_ids);
    let mut top = shown;
    for n in &model.notes {
        if !shown_ids.contains(&n.target) {
            return Err(unported());
        }
        let lines = split_note(&n.text);
        top.push(St {
            id: n.id.clone(),
            ty: "note".into(),
            descriptions: lines,
            note_target: Some(n.target.clone()),
            note_pos: Some(n.position.clone()),
            ..Default::default()
        });
    }
    let edges: Vec<Edge> = model
        .trans
        .iter()
        .map(|t| {
            let mut parts = vec![];
            if let Some(tr) = t.trigger.as_ref().filter(|s| !s.is_empty()) {
                parts.push(tr.clone());
            }
            if let Some(g) = t.guard.as_ref().filter(|s| !s.is_empty()) {
                parts.push(format!("[{g}]"));
            }
            let label = parts.join(" ");
            Edge {
                id: format!("{}_to_{}", t.from, t.to),
                source: t.from.clone(),
                target: t.to.clone(),
                label: if label.is_empty() { None } else { Some(label) },
            }
        })
        .collect();

    let lay = Layout {
        nf: theme.number("typography", "font_size_normal").unwrap_or(14.0),
        sf: theme.number("typography", "font_size_small").unwrap_or(12.0),
        dir,
        edges,
    };
    let mut nodes: Vec<Ln> = top.iter().map(|s| lay.node(s)).collect();
    lay.place(&mut nodes)?;

    // scene geometry (Ruby Integer-vs-Float typing decides the viewBox text)
    let measure = |extent: &dyn Fn(&Ln) -> (f64, bool), default: f64| -> (f64, bool) {
        let mut best: Option<(f64, bool)> = None;
        for n in &nodes {
            let (v, int) = extent(n);
            if best.is_none_or(|b| v > b.0) {
                best = Some((v, int));
            }
        }
        best.unwrap_or((default, true))
    };
    let (cw, cwi) = measure(&|n| (n.xv() + n.w, n.xi && n.wi), 800.0);
    let (ch, chi) = measure(&|n| (n.yv() + n.h, n.yi && n.hi), 600.0);
    if !nodes.is_empty() && (cwi || chi) {
        return Err(unported());
    }
    let (width, height) = (cw + 100.0, ch + 100.0);
    let fmt_dim = |v: f64, int: bool| if int { format!("{}", v as i64) } else { ruby_float(v) };
    let mut doc = Document::new(width, height);
    doc.view_box = Some(format!("0 0 {} {}", fmt_dim(width, cwi), fmt_dim(height, chi)));

    let mut abs = HashMap::new();
    collect(&nodes, 0.0, 0.0, &mut abs);
    for e in &lay.edges {
        let (Some(s), Some(t)) = (abs.get(&e.source), abs.get(&e.target)) else { continue };
        let (sx, sy) = (s.x + s.w / 2.0, s.y + s.h / 2.0);
        let (tx, ty) = (t.x + t.w / 2.0, t.y + t.h / 2.0);
        let d = format!("M {} {} L {} {}", coord(sx), coord(sy), coord(tx), coord(ty));
        let mut g = Elem::new("g").attr("id", format!("transition-{}", e.id));
        g.push(
            Elem::new("path")
                .attr("fill", "none")
                .attr("stroke", st.edge_stroke)
                .attr("stroke-width", "2")
                .attr("d", d.clone()),
        );
        for head in Arrowhead::for_path(&ArrowPath {
            d: &d,
            stroke: Some(st.edge_stroke),
            stroke_width: Some("2"),
            marker_end: Some("url(#arrowhead)"),
            ..Default::default()
        }) {
            g.push(head);
        }
        if let Some(l) = &e.label {
            g.push(font_text(l, (sx + tx) / 2.0, (sy + ty) / 2.0 - 8.0, lay.sf, st.label_text));
        }
        doc.add(g);
    }
    for n in &nodes {
        doc.add(render_state(n, 0.0, 0.0, &lay, &st));
    }
    Ok(doc.to_xml())
}

/// `note_lines`: split on `<br>` variants, strip, drop empties.
fn split_note(text: &str) -> Vec<String> {
    let re = Regex::new(r"(?i)\s*<br\s*/?\s*>\s*").unwrap();
    re.split(text).map(|s| strip(s).to_string()).filter(|s| !s.is_empty()).collect()
}
