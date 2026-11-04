//! Block renderer: port of `Parser::Native::Builders::Block` +
//! `IRAdapters::Block` (model rules), `Layout::Block` (column grid placement)
//! and `Renderer::Block`.
//!
//! Ruby mixes Integer and Float arithmetic in the placement (an Integer
//! `20 / 2` floors, and the viewBox prints `20` where a Float prints `20.0`),
//! so geometry is carried as [`N`]. Declined to the Ruby path
//! (`Error::Unported`): sources with a frontmatter/directive preamble.
use super::pie::{items, strip, text_of};
use crate::svg::{ruby_float, ArrowPath, Arrowhead, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use serde_json::Value;
use std::ops::{Add, Div, Mul, Sub};

const DEFAULT_BLOCK_WIDTH: i64 = 100;
const DEFAULT_BLOCK_HEIGHT: i64 = 60;
const DEFAULT_SPACING: i64 = 20;
const DEFAULT_COMPOUND_PADDING: i64 = 8;
const COMPACT_TEXT_WIDTH_SCALE: f64 = 1.16;
const COMPACT_TEXT_WIDTH_FONT_SIZE: f64 = 16.0;

fn unported() -> Error {
    Error::Unported("block".into())
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

/// `Array#sum` of a mixed Integer/Float list (Kahan-Babuska once a Float appears).
fn nsum(values: &[N]) -> N {
    if values.iter().all(|v| matches!(v, I(_))) {
        return I(values.iter().map(|v| if let I(i) = v { *i } else { 0 }).sum());
    }
    F(super::pie::fsum(values.iter().map(|v| v.f())))
}

// ---- model ----

#[derive(Default, Clone)]
struct Blk {
    id: String,
    label: Option<String>,
    role: &'static str,
    span: i64,
    compound: bool,
    shape: String,
    direction: Option<String>,
    children: Vec<usize>,
}

struct Conn {
    from: String,
    to: String,
    kind: &'static str,
}

fn present(v: Option<&Value>) -> bool {
    v.is_some_and(|v| !v.is_null())
}

fn to_i(s: &str) -> i64 {
    // Ruby String#to_i: optional sign + leading digits
    let s = s.trim_start();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '_').filter(|c| *c != '_').collect();
    let n: i64 = digits.parse().unwrap_or(0);
    if neg { -n } else { n }
}

fn unquote(label: &str) -> String {
    // gsub(/^["']|["']$/, ""): per line anchors; labels are single-line in practice
    let mut s = label.to_string();
    if s.contains('\n') {
        return s;
    }
    if s.starts_with('"') || s.starts_with('\'') {
        s.remove(0);
    }
    if s.ends_with('"') || s.ends_with('\'') {
        s.pop();
    }
    s
}

struct Model {
    blocks: Vec<Blk>,
    top: Vec<usize>,
    columns: Option<i64>,
    conns: Vec<Conn>,
}

struct Raw {
    id: String,
    label: Option<String>,
    role: &'static str,
    span: i64,
    compound: bool,
    shape: String,
    direction: Option<String>,
    children: Vec<Raw>,
}

fn id_text(v: &Value) -> String {
    match v.get("string") {
        Some(s) => text_of(s),
        None => text_of(v),
    }
}

fn build_statements(stmts: &[&Value], parent_id: Option<&str>, offset: usize, conns: &mut Vec<(String, String, &'static str)>, columns: &mut Option<i64>, top: bool) -> Vec<Raw> {
    let mut out = vec![];
    for (i, stmt) in stmts.iter().enumerate() {
        let index = i + offset;
        let key = |k: &str| present(stmt.get(k));
        let anon = |kind: &str| match parent_id {
            Some(p) => format!("{p}-{kind}-{index}"),
            None => format!("{kind}-{index}"),
        };
        if key("columns_keyword") {
            if top && columns.is_none() && key("columns_value") {
                *columns = Some(to_i(&text_of(&stmt["columns_value"])));
            }
        } else if key("space_keyword") {
            out.push(Raw { id: anon("space"), label: None, role: "space", span: 1, compound: false, shape: "rect".into(), direction: None, children: vec![] });
        } else if key("arrow_id") {
            let direction = stmt.get("arrow_direction").filter(|v| !v.is_null()).map(|d| text_of(&d["direction"]));
            let label = stmt.get("arrow_label").filter(|v| !v.is_null()).map(text_of);
            out.push(Raw { id: text_of(&stmt["arrow_id"]), label, role: "arrow", span: 1, compound: false, shape: "rect".into(), direction, children: vec![] });
        } else if key("compound_keyword") {
            let id = if key("compound_id") { text_of(&stmt["compound_id"]) } else { anon("compound") };
            let nested = stmt.get("compound_statements").filter(|v| !v.is_null()).map(items).unwrap_or_default();
            let children = build_statements(&nested, Some(&id), 0, conns, columns, false);
            out.push(Raw { id, label: None, role: "block", span: 1, compound: true, shape: "rect".into(), direction: None, children });
        } else if key("block_id") {
            let id = id_text(&stmt["block_id"]);
            let span = stmt.get("block_width").filter(|v| !v.is_null()).map(|w| to_i(text_of(w).strip_prefix(':').unwrap_or(&text_of(w)))).unwrap_or(1);
            let (shape, label) = match stmt.get("block_shape").filter(|v| !v.is_null()) {
                None => ("rect".to_string(), Some(id.clone())),
                Some(sh) if sh.is_object() && key_of(sh, "open") && key_of(sh, "close") => {
                    let delims = format!("{}{}", text_of(&sh["open"]), text_of(&sh["close"]));
                    let shape = if delims == "(())" { "circle" } else { "rect" };
                    (shape.to_string(), Some(unquote(&text_of(&sh["label"]))))
                }
                Some(_) => ("rect".to_string(), None),
            };
            out.push(Raw { id, label, role: "block", span, compound: false, shape, direction: None, children: vec![] });
        } else if key("from") && key("to") {
            let kind = match stmt.get("arrow").filter(|v| !v.is_null()) {
                None => "arrow",
                Some(a) => {
                    let t = a.get("arrow_type").filter(|v| !v.is_null()).or_else(|| a.get("line_type").filter(|v| !v.is_null()));
                    match t.map(text_of).as_deref() {
                        Some("---") => "line",
                        _ => "arrow",
                    }
                }
            };
            conns.push((text_of(&stmt["from"]), text_of(&stmt["to"]), kind));
        }
    }
    out
}

fn key_of(v: &Value, k: &str) -> bool {
    present(v.get(k))
}

fn reserve(preferred: &str, occupied: &mut Vec<String>) -> String {
    let mut candidate = preferred.to_string();
    let mut suffix = 2;
    while occupied.contains(&candidate) {
        candidate = format!("{preferred}_{suffix}");
        suffix += 1;
    }
    occupied.push(candidate.clone());
    candidate
}

fn flatten(raws: Vec<Raw>, blocks: &mut Vec<Blk>, occupied: &mut Vec<String>, originals: &mut Vec<(String, usize)>) -> Vec<usize> {
    let mut ids = vec![];
    for r in raws {
        let idx = blocks.len();
        let adapted = reserve(&r.id, occupied);
        originals.push((r.id.clone(), idx));
        blocks.push(Blk {
            id: adapted,
            label: r.label,
            role: r.role,
            span: r.span,
            compound: r.compound,
            shape: r.shape,
            direction: r.direction,
            children: vec![],
        });
        let kids = flatten(r.children, blocks, occupied, originals);
        blocks[idx].children = kids;
        ids.push(idx);
    }
    ids
}

fn build(tree: &Value) -> Model {
    let stmts = items(tree.get("statements").unwrap_or(&Value::Null));
    let mut conns = vec![];
    let mut columns = None;
    let raws = build_statements(&stmts, None, 1, &mut conns, &mut columns, true);
    let mut blocks = vec![];
    let mut occupied = vec![];
    let mut originals = vec![];
    let top = flatten(raws, &mut blocks, &mut occupied, &mut originals);
    let mut by_source: Vec<(String, String)> = vec![];
    for (orig, idx) in &originals {
        let adapted = blocks[*idx].id.clone();
        match by_source.iter_mut().find(|(o, _)| o == orig) {
            Some(slot) => slot.1 = adapted,
            None => by_source.push((orig.clone(), adapted)),
        }
    }
    let conns = conns
        .into_iter()
        .filter_map(|(f, t, kind)| {
            let lookup = |k: &str| by_source.iter().find(|(o, _)| o == k).map(|(_, a)| a.clone());
            Some(Conn { from: lookup(&f)?, to: lookup(&t)?, kind })
        })
        .collect();
    Model { blocks, top, columns, conns }
}

// ---- layout ----

#[derive(Clone, Copy)]
struct Geo {
    x: N,
    y: N,
    w: N,
    h: N,
    row: Option<i64>,
}

#[derive(Clone, Copy)]
struct Dim {
    w: N,
    h: N,
}

struct Settings {
    auto_layout: bool,
    span_layout: bool,
    wfs: f64,
    column_width: Option<N>,
    grid: Option<(f64, f64)>,
}

struct Layout<'a> {
    m: &'a Model,
    normal: f64,
}

impl Layout<'_> {
    fn b(&self, i: usize) -> &Blk {
        &self.m.blocks[i]
    }
    fn span(&self, i: usize) -> i64 {
        self.b(i).span.max(1)
    }
    fn compound(&self, i: usize) -> bool {
        self.b(i).compound || !self.b(i).children.is_empty()
    }
    fn space(&self, i: usize) -> bool {
        self.b(i).role == "space"
    }
    fn arrow(&self, i: usize) -> bool {
        self.b(i).role == "arrow"
    }
    fn special(&self, i: usize) -> bool {
        self.arrow(i) || self.b(i).shape == "circle"
    }
    fn label_text(&self, i: usize) -> String {
        let b = self.b(i);
        let label = b.label.clone().unwrap_or_else(|| b.id.clone()).replace("&nbsp;", "\u{a0}");
        let cs: Vec<char> = label.chars().collect();
        if cs.len() >= 2 && (cs[0] == '"' || cs[0] == '\'') && cs[cs.len() - 1] == cs[0] {
            return cs[1..cs.len() - 1].iter().collect();
        }
        label
    }

    fn compact_dims(&self, i: usize) -> (f64, f64) {
        let m = text::measure(&self.label_text(i), self.normal, Face::Arial);
        let (w, h) = (m.width * COMPACT_TEXT_WIDTH_SCALE + 8.0, m.height + 13.0);
        if !self.compound(i) {
            return (w, h);
        }
        let dims: Vec<(f64, f64)> = self.b(i).children.iter().map(|&c| self.compact_dims(c)).collect();
        let cw = dims.iter().map(|d| d.0).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| if x > a { x } else { a }))).unwrap_or(0.0);
        let ch = dims.iter().map(|d| d.1).fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| if x > a { x } else { a }))).unwrap_or(0.0);
        let count = dims.len() as f64;
        let w2 = cw * count + (DEFAULT_COMPOUND_PADDING as f64) * (count + 1.0);
        let h2 = ch + (DEFAULT_COMPOUND_PADDING * 2) as f64;
        (if w2 > w { w2 } else { w }, if h2 > h { h2 } else { h })
    }

    fn block_dims(&self, i: usize, auto: bool, wfs: f64) -> Dim {
        if self.arrow(i) {
            return Dim { w: I(DEFAULT_BLOCK_WIDTH / 2), h: I(DEFAULT_BLOCK_HEIGHT / 2) };
        }
        let label = self.label_text(i);
        let m = text::measure(&label, self.normal, Face::Arial);
        let mut width = if auto && wfs != self.normal { text::measure(&label, wfs, Face::Arial).width } else { m.width };
        width += if auto { 8.0 } else { 40.0 };
        let height = m.height + if auto { 13.0 } else { 30.0 };
        let d = if auto {
            Dim { w: F(width), h: F(height) }
        } else {
            Dim { w: F(width).max(I(DEFAULT_BLOCK_WIDTH)), h: F(height).max(I(DEFAULT_BLOCK_HEIGHT)) }
        };
        if !self.compound(i) {
            return d;
        }
        let dims: Vec<Dim> = self.b(i).children.iter().map(|&c| self.block_dims(c, false, self.normal)).collect();
        let ws: Vec<N> = dims.iter().map(|d| d.w).collect();
        let cw = nsum(&ws) + I(DEFAULT_COMPOUND_PADDING * (dims.len() as i64 + 1));
        let mh = dims.iter().map(|d| d.h).fold(None, |a: Option<N>, x| Some(a.map_or(x, |a| a.max(x))));
        let ch = F(mh.map_or(0.0, |v| v.f())) + I(DEFAULT_COMPOUND_PADDING * 2);
        Dim { w: d.w.max(cw), h: d.h.max(ch) }
    }

    fn scaled(&self, i: usize, compact: bool, wfs: f64) -> Dim {
        let font = if self.special(i) { self.normal } else { wfs };
        self.block_dims(i, compact, font)
    }

    fn grid_for(&self, blocks: &[usize], auto: bool) -> Option<(f64, f64)> {
        if auto || !blocks.iter().any(|&b| self.compound(b)) {
            return None;
        }
        let dims: Vec<(f64, f64)> = blocks.iter().filter(|&&b| !self.space(b) && !self.special(b)).map(|&b| self.compact_dims(b)).collect();
        if dims.is_empty() {
            return None;
        }
        let max = |f: fn(&(f64, f64)) -> f64| dims.iter().map(f).fold(f64::NEG_INFINITY, |a, x| if x > a { x } else { a });
        Some((max(|d| d.0), max(|d| d.1)))
    }

    fn dims_in_grid(&self, i: usize, s: &Settings) -> Dim {
        let compact = (s.auto_layout || s.span_layout) && !self.compound(i);
        let d = self.scaled(i, compact, s.wfs);
        let Some((gw, gh)) = s.grid else { return d };
        if self.special(i) {
            return self.block_dims(i, true, self.normal);
        }
        let span = self.span(i);
        Dim { w: F(gw * span as f64 + (DEFAULT_COMPOUND_PADDING * (span - 1)) as f64), h: F(gh) }
    }

    fn compound_children(&self, parent: usize, px: N, py: N, compact: bool, parent_width: N, out: &mut Vec<(usize, Geo)>) {
        let mut child_x = px + I(DEFAULT_COMPOUND_PADDING);
        let kids = &self.b(parent).children;
        let compact_width = compact.then(|| {
            let count = kids.len() as i64;
            (parent_width - I(DEFAULT_COMPOUND_PADDING * (count + 1))) / I(count)
        });
        for &child in kids {
            let y = py + I(DEFAULT_COMPOUND_PADDING);
            let d = match compact_width {
                Some(cw) => {
                    let (_, h) = self.compact_dims(child);
                    Dim { w: cw, h: F(h) }
                }
                None => self.block_dims(child, false, self.normal),
            };
            let geo = Geo { x: child_x, y, w: d.w, h: d.h, row: None };
            out.push((child, geo));
            if self.compound(child) && !self.b(child).children.is_empty() {
                self.compound_children(child, geo.x, geo.y, false, geo.w, out);
            }
            child_x = child_x + geo.w + I(DEFAULT_COMPOUND_PADDING);
        }
    }

    fn column_layout(&self, blocks: &[usize], columns: i64, auto: bool, span_layout: bool) -> Vec<(usize, Geo)> {
        let wfs = if span_layout && columns == 1 { self.normal } else { COMPACT_TEXT_WIDTH_FONT_SIZE };
        let column_width = if span_layout {
            blocks
                .iter()
                .filter(|&&b| !self.space(b))
                .map(|&b| self.block_dims(b, !self.compound(b), wfs).w)
                .fold(None, |a: Option<N>, x| Some(a.map_or(x, |a| a.max(x))))
        } else {
            None
        };
        let s = Settings { auto_layout: auto, span_layout, wfs, column_width, grid: self.grid_for(blocks, auto) };
        let mut positioned: Vec<(usize, Geo)> = vec![];
        let (mut row, mut col) = (0i64, 0i64);
        let mut row_heights: Vec<Option<N>> = vec![];
        let mut col_widths: Vec<N> = vec![I(0); columns.max(0) as usize];
        for &b in blocks {
            let span = self.span(b);
            if self.space(b) {
                if let Some((_, gh)) = s.grid {
                    set_row(&mut row_heights, row, F(gh));
                }
                col += span;
                if col >= columns {
                    col = 0;
                    row += 1;
                }
                continue;
            }
            let d = self.dims_in_grid(b, &s);
            if !s.span_layout && col + span > columns {
                col = 0;
                row += 1;
            }
            let spacing = I(if s.auto_layout || s.span_layout || s.grid.is_some() { DEFAULT_COMPOUND_PADDING } else { DEFAULT_SPACING });
            let mut x = if col == 0 {
                spacing
            } else if let Some(cw) = s.column_width {
                spacing + I(col) * (cw + spacing)
            } else {
                let upto: Vec<N> = col_widths.iter().take(col as usize).copied().collect();
                nsum(&upto) + spacing * I(col + 1)
            };
            let mut y = if row == 0 {
                spacing
            } else {
                let upto: Vec<N> = row_heights.iter().take(row as usize).flatten().copied().collect();
                nsum(&upto) + spacing + spacing * I(row)
            };
            if let (Some((gw, gh)), true) = (s.grid, self.special(b)) {
                x = x + (F(gw) - d.w) / I(2);
                y = y + (F(gh) - d.h) / I(2);
            }
            let width = match s.column_width {
                None => d.w * I(span),
                Some(cw) => cw * I(span) + spacing * I(span - 1),
            };
            let geo = Geo { x, y, w: width, h: d.h, row: Some(row) };
            positioned.push((b, geo));
            for c in col..col + span {
                if c < columns {
                    let cur = col_widths[c as usize];
                    col_widths[c as usize] = cur.max(d.w);
                }
            }
            let current = row_heights.get(row as usize).copied().flatten().unwrap_or(I(0));
            let height = s.grid.map_or(d.h, |(_, gh)| F(gh));
            set_row(&mut row_heights, row, current.max(height));
            if self.compound(b) && !self.b(b).children.is_empty() {
                self.compound_children(b, geo.x, geo.y, s.grid.is_some(), geo.w, &mut positioned);
            }
            col += span;
            if col >= columns {
                col = 0;
                row += 1;
            }
        }
        positioned
    }
}

fn set_row(rows: &mut Vec<Option<N>>, row: i64, v: N) {
    let row = row.max(0) as usize;
    if rows.len() <= row {
        rows.resize(row + 1, None);
    }
    rows[row] = Some(v);
}

// ---- render ----

struct Paint {
    node_fill: String,
    node_stroke: String,
    edge_stroke: String,
    edge_color: String,
    label_text: String,
    stroke_width: Option<String>,
    font_family: Option<String>,
    font_size: Option<String>,
}

fn themed(e: Elem, p: &Paint) -> Elem {
    let mut e = e.attr("fill", p.node_fill.clone()).attr("stroke", p.node_stroke.clone());
    if let Some(w) = &p.stroke_width {
        e = e.attr("stroke-width", w.clone());
    }
    e
}

fn shape_elem(l: &Layout, i: usize, g: &Geo, p: &Paint) -> Elem {
    let b = l.b(i);
    if l.arrow(i) {
        let (x, y, w, h) = (g.x.f(), g.y.f(), g.w.f(), g.h.f());
        // The native builder hands `direction` over as a slice object, which no
        // `case ... when "up"` matches: every arrow block points right.
        let _ = &b.direction;
        let pts = [(x, y), (x + w, y + h / 2.0), (x, y + h)];
        let points = pts.iter().map(|(a, b)| format!("{},{}", ruby_float(*a), ruby_float(*b))).collect::<Vec<_>>().join(" ");
        return themed(Elem::new("polygon"), p).attr("points", points);
    }
    if b.shape == "circle" {
        let r = g.w.min(g.h) / I(2);
        return themed(Elem::new("circle"), p)
            .num("cx", (g.x + g.w / I(2)).f())
            .num("cy", (g.y + g.h / I(2)).f())
            .num("r", r.f());
    }
    themed(rect(g), p)
}

fn rect(g: &Geo) -> Elem {
    Elem::new("rect").num("x", g.x.f()).num("y", g.y.f()).num("width", g.w.f()).num("height", g.h.f())
}

fn block_group(l: &Layout, i: usize, g: &Geo, p: &Paint) -> Elem {
    let b = l.b(i);
    let mut group = Elem::new("g").attr("id", format!("block-{}", b.id));
    group.push(shape_elem(l, i, g, p));
    if b.label.as_deref().is_some_and(|s| !s.is_empty()) {
        let mut t = Elem::new("text")
            .num("x", (g.x + g.w / I(2)).f())
            .num("y", (g.y + g.h / I(2)).f())
            .content(&l.label_text(i))
            .attr("fill", p.label_text.clone());
        if let Some(f) = &p.font_family {
            t = t.attr("font-family", f.clone());
        }
        if let Some(s) = &p.font_size {
            t = t.attr("font-size", s.clone());
        }
        group.push(t.attr("text-anchor", "middle").attr("dominant-baseline", "middle"));
    }
    group
}

pub fn render(source: &str) -> Result<String, Error> {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    if !strip(preamble).is_empty() {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "block")?;
    let model = build(&tree);
    let theme = Theme::default_theme();
    let col = |k: &str| theme.color(k).unwrap_or("").to_string();
    let paint = Paint {
        node_fill: col("node_fill"),
        node_stroke: col("node_stroke"),
        edge_stroke: col("edge_stroke"),
        edge_color: col("edge_color"),
        label_text: col("label_text"),
        stroke_width: theme.number("shapes", "stroke_width").map(ruby_float),
        font_family: theme.string("typography", "font_family").map(str::to_string),
        font_size: theme.number("typography", "font_size_normal").map(ruby_float),
    };
    let normal = theme.number("typography", "font_size_normal").unwrap_or(14.0);
    let l = Layout { m: &model, normal };

    let columns_given = model.columns;
    let columns = columns_given.unwrap_or_else(|| model.top.iter().map(|&b| l.span(b)).sum::<i64>().max(1));
    let auto = columns_given.is_none();
    let span_layout = model.top.iter().any(|&b| l.span(b) > 1);
    let positioned = l.column_layout(&model.top, columns, auto, span_layout);
    let geo = |i: usize| positioned.iter().find(|(k, _)| *k == i).map(|(_, g)| *g);

    let (width, height) = if positioned.is_empty() {
        (I(DEFAULT_SPACING * 2), I(DEFAULT_SPACING * 2))
    } else {
        let mx = positioned.iter().map(|(_, g)| g.x + g.w).fold(None, |a: Option<N>, x| Some(a.map_or(x, |a| a.max(x)))).unwrap();
        let my = positioned.iter().map(|(_, g)| g.y + g.h).fold(None, |a: Option<N>, x| Some(a.map_or(x, |a| a.max(x)))).unwrap();
        (mx + I(DEFAULT_SPACING), my + I(DEFAULT_SPACING))
    };
    let mut doc = Document::new(width.f(), height.f());
    doc.view_box = Some(format!("0 0 {} {}", width.show(), height.show()));

    let index_of = |adapted: &str| model.blocks.iter().position(|b| b.id == adapted);
    for c in &model.conns {
        let (Some(fi), Some(ti)) = (index_of(&c.from), index_of(&c.to)) else { continue };
        let (Some(f), Some(t)) = (geo(fi), geo(ti)) else { continue };
        let (x1, y1, x2, y2) = if auto && f.row.is_some() && t.row.is_some() && f.row == t.row {
            (f.x + f.w, f.y + f.h / I(2), t.x, t.y + t.h / I(2))
        } else {
            (f.x + f.w / I(2), f.y + f.h, t.x + t.w / I(2), t.y)
        };
        let d = format!("M {} {} L {} {}", ruby_float(x1.f()), ruby_float(y1.f()), ruby_float(x2.f()), ruby_float(y2.f()));
        let mut group = Elem::new("g").attr("id", format!("connection-{}-{}", c.from, c.to));
        let mut path = Elem::new("path").attr("fill", "none").attr("stroke", paint.edge_stroke.clone());
        if let Some(w) = &paint.stroke_width {
            path = path.attr("stroke-width", w.clone());
        }
        path = path.attr("d", d.clone());
        group.push(path);
        if c.kind == "arrow" {
            for head in Arrowhead::for_path(&ArrowPath {
                d: &d,
                stroke: Some(&paint.edge_stroke),
                stroke_width: paint.stroke_width.as_deref(),
                marker_end: Some("url(#arrowhead)"),
                ..Default::default()
            }) {
                group.push(head);
            }
        }
        doc.add(group);
    }

    for &i in &model.top {
        if l.space(i) {
            continue;
        }
        let Some(g) = geo(i) else { return Err(unported()) };
        if l.compound(i) {
            let mut group = Elem::new("g").attr("id", format!("block-{}", l.b(i).id));
            group.push(
                rect(&g)
                    .attr("fill", "none")
                    .attr("stroke", paint.edge_color.clone())
                    .attr("stroke-width", "2")
                    .attr("stroke-dasharray", "5,5"),
            );
            for &c in &l.b(i).children {
                if l.space(c) {
                    continue;
                }
                let Some(cg) = positioned.iter().find(|(k, _)| *k == c).map(|(_, g)| *g) else { return Err(unported()) };
                group.push(block_group(&l, c, &cg, &paint));
            }
            doc.add(group);
        } else {
            doc.add(block_group(&l, i, &g, &paint));
        }
    }
    Ok(doc.to_xml())
}
