//! Flowchart renderer: the label-free, cluster-free subset of Ruby
//! `Parser::Flowchart` + `Layout::Flowchart` (`DirectedPlacement`,
//! `FlowchartEdgeRouter`) + `Renderer::Flowchart`.
//!
//! Anything outside the subset returns `Error::Unported`: subgraphs, edge
//! labels, styling/click/class statements, comments, markdown/html/quoted
//! text, long links, self loops, redeclared nodes, other node shapes.
#[path = "class_diagram/place.rs"]
#[allow(dead_code)]
mod place;

use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use std::collections::HashMap;

const FONT_SIZE: f64 = 16.0;
const NODE_LINE_HEIGHT: f64 = 1.5;
const PAGE_MARGIN: f64 = 8.0;
const ROUNDED_CORNER: f64 = 5.0;
const ARROW_LENGTH: f64 = 8.0;
const ARROW_HALF_WIDTH: f64 = 4.0;
const CIRCLE_HEAD_RADIUS: f64 = 5.5;
const CROSS_HEAD_HALF: f64 = 4.5;
const CIRCLE_HEAD_REACH: f64 = 6.6;
const CROSS_HEAD_REACH: f64 = 6.5;
const COINCIDENT: f64 = 0.01;
const RESERVED: &[&str] = &[
    "end", "graph", "flowchart", "subgraph", "style", "class", "classDef", "click", "linkStyle", "direction", "default", "interpolate", "call", "href", "accTitle", "accDescr", "swimlane",
];

fn unported(why: &str) -> Error {
    Error::Unported(format!("flowchart: {why}"))
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Rect,
    Rounded,
    Stadium,
    Circle,
    Rhombus,
    Hexagon,
}

struct Node {
    id: String,
    label: String,
    shape: Shape,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct Edge {
    src: usize,
    tgt: usize,
    kind: String,
}

struct Model {
    dir: &'static str,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

/// Ruby `Float#round(nd)` (round-half-up with Ruby's correction step).
fn rr(x: f64, nd: i32) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let binexp = x.abs().log2().floor() as i32 + 1;
    let limit = if binexp > 0 { binexp / 4 } else { binexp / 3 - 1 };
    if nd >= 15 - limit {
        return x;
    }
    let under = if binexp > 0 { binexp / 3 + 1 } else { binexp / 4 };
    if nd < -under {
        return 0.0;
    }
    let s = 10f64.powi(nd);
    let mut f = (x * s).round();
    if x > 0.0 {
        if (f + 0.5) / s <= x {
            f += 1.0;
        }
    } else if (f - 0.5) / s >= x {
        f -= 1.0;
    }
    f / s
}

fn plain_text(s: &str) -> Option<String> {
    if !s.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | ',')) {
        return None;
    }
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { None } else { Some(t) }
}

fn read_id(s: &str) -> Option<(&str, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len());
    let id = &s[..end];
    if RESERVED.iter().any(|r| id.starts_with(r)) {
        return None;
    }
    Some((id, &s[end..]))
}

/// A node reference: `id` or `id` + a plain-text shape. Returns the rest.
fn read_node<'a>(s: &'a str, model: &mut Model, index: &mut HashMap<String, usize>) -> Result<(usize, &'a str), Error> {
    let (id, rest) = read_id(s).ok_or_else(|| unported("node id"))?;
    let shapes: [(&str, &str, Shape); 6] = [
        ("((", "))", Shape::Circle),
        ("([", "])", Shape::Stadium),
        ("{{", "}}", Shape::Hexagon),
        ("[", "]", Shape::Rect),
        ("(", ")", Shape::Rounded),
        ("{", "}", Shape::Rhombus),
    ];
    let mut explicit = None;
    let mut rest = rest;
    for (open, close, shape) in shapes {
        if let Some(inner) = rest.strip_prefix(open) {
            let end = inner.find(close).ok_or_else(|| unported("shape close"))?;
            let label = plain_text(&inner[..end]).ok_or_else(|| unported("label text"))?;
            explicit = Some((label, shape));
            rest = &inner[end + close.len()..];
            break;
        }
    }
    if rest.starts_with(['(', '[', '{', '>']) {
        return Err(unported("shape"));
    }
    let i = match index.get(id) {
        Some(&i) => {
            if explicit.is_some() {
                return Err(unported("redeclared node"));
            }
            i
        }
        None => {
            let (label, shape) = explicit.unwrap_or((id.to_string(), Shape::Rect));
            model.nodes.push(Node { id: id.to_string(), label, shape, x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
            index.insert(id.to_string(), model.nodes.len() - 1);
            model.nodes.len() - 1
        }
    };
    Ok((i, rest))
}

const LINKS: [&str; 8] = ["-.->", "-.-", "==>", "===", "-->", "---", "--x", "--o"];

/// `Builders::Flowchart#link_type` for the supported tokens.
fn link_type(token: &str) -> String {
    let marker = |c: char| match c {
        '>' => Some("arrow"),
        'x' => Some("cross"),
        'o' => Some("circle"),
        _ => None,
    };
    let head = marker(token.chars().last().unwrap());
    let weight = if token.contains('.') {
        "dotted_"
    } else if token.contains('=') {
        "thick_"
    } else {
        ""
    };
    format!("{weight}{}", head.unwrap_or("line"))
}

/// `click ID ...` draws nothing; accepted only for a known node and a plain
/// argument list (callback / call f(x) / "url" / "tooltip" / target).
fn click_ok(args: &str, index: &HashMap<String, usize>) -> Result<(), Error> {
    let id = args.split([' ', '\t']).next().unwrap_or("");
    let plain = args.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '\t' | '"' | ':' | '/' | '.' | '(' | ')' | '<' | '>' | '_' | '-'));
    if !index.contains_key(id) || !plain || args.matches('"').count() % 2 != 0 {
        return Err(unported("click"));
    }
    Ok(())
}

fn parse(source: &str) -> Result<Model, Error> {
    if source.contains('\r') {
        return Err(unported("carriage return"));
    }
    let text = source.trim_start_matches([' ', '\t', '\n']);
    let (word, mut rest) = if let Some(r) = text.strip_prefix("flowchart") {
        ("flowchart", r)
    } else if let Some(r) = text.strip_prefix("graph") {
        ("graph", r)
    } else {
        return Err(unported("header"));
    };
    let _ = word;
    let mut dir = "DOWN";
    let trimmed = rest.trim_start_matches([' ', '\t']);
    if trimmed.len() < rest.len() {
        for (name, d) in [("TD", "DOWN"), ("TB", "DOWN"), ("BT", "UP"), ("LR", "RIGHT"), ("RL", "LEFT")] {
            if let Some(r) = trimmed.strip_prefix(name) {
                dir = d;
                rest = r;
                break;
            }
        }
    }
    let rest_t = rest.trim_start_matches([' ', '\t']);
    if !(rest_t.is_empty() || rest_t.starts_with([';', '\n'])) {
        return Err(unported("header tail"));
    }
    let mut model = Model { dir, nodes: vec![], edges: vec![] };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut stmts: Vec<&str> = vec![];
    for line in rest_t.split('\n') {
        if line.trim_matches([' ', '\t']).starts_with("click ") {
            stmts.push(line);
        } else {
            stmts.extend(line.split(';'));
        }
    }
    for stmt in stmts {
        let stmt = stmt.trim_matches([' ', '\t']);
        if stmt.is_empty() {
            continue;
        }
        if let Some(args) = stmt.strip_prefix("click ") {
            click_ok(args, &index)?;
            continue;
        }
        let (mut prev, mut tail) = read_node(stmt, &mut model, &mut index)?;
        loop {
            tail = tail.trim_start_matches([' ', '\t']);
            if tail.is_empty() {
                break;
            }
            let token = LINKS.iter().find(|l| tail.starts_with(**l)).ok_or_else(|| unported("link"))?;
            tail = &tail[token.len()..];
            if matches!(tail.chars().next(), Some('x' | 'o' | '>' | '-' | '.' | '=' | '|')) && !token.ends_with(['x', 'o']) {
                return Err(unported("link tail"));
            }
            tail = tail.trim_start_matches([' ', '\t']);
            let (next, after) = read_node(tail, &mut model, &mut index)?;
            if next == prev {
                return Err(unported("self loop"));
            }
            model.edges.push(Edge { src: prev, tgt: next, kind: link_type(token) });
            prev = next;
            tail = after;
        }
    }
    if model.nodes.is_empty() {
        return Err(unported("empty"));
    }
    Ok(model)
}

fn size_of(label: &str, shape: Shape) -> (f64, f64) {
    let lw = text::width(label, FONT_SIZE, Face::Trebuchet);
    let lh = FONT_SIZE * NODE_LINE_HEIGHT * 1.0;
    match shape {
        Shape::Rhombus => {
            let s = lw + lh + 30.0;
            (s, s)
        }
        Shape::Circle => {
            let s = lw + 15.0;
            (s, s)
        }
        Shape::Rounded => (lw + 30.0, lh + 30.0),
        Shape::Hexagon => (lw + (45.0 + 0.118 * (lh - FONT_SIZE * NODE_LINE_HEIGHT)), lh + 15.0),
        Shape::Rect | Shape::Stadium => (lw + 60.0, lh + 30.0),
    }
}

fn centre(n: &Node) -> (f64, f64) {
    (n.x + n.w / 2.0, n.y + n.h / 2.0)
}

fn axis_ratio(delta: f64, half: f64) -> f64 {
    if delta == 0.0 { 0.0 } else { delta.abs() / half }
}

fn box_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    let a = if dx == 0.0 { f64::INFINITY } else { hw / dx.abs() };
    let b = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
    if b < a { b } else { a }
}

fn cap_intersection(cx: f64, radius: f64, dx: f64, dy: f64, cy: f64) -> f64 {
    let a = dx * dx + dy * dy;
    let b = dx * cx + dy * cy;
    let c = (cx * cx + cy * cy) - radius * radius;
    let disc = b * b - a * c;
    (b + (if disc > 0.0 { disc } else { 0.0 }).sqrt()) / a
}

fn ellipse_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    let xr = axis_ratio(dx, hw);
    let yr = axis_ratio(dy, hh);
    1.0 / ((xr.powi(2)) + (yr.powi(2))).sqrt()
}

fn stadium_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    if hw == hh {
        return ellipse_scale(hw, hh, dx, dy);
    }
    if hw < hh {
        return stadium_scale(hh, hw, dy, dx);
    }
    let flat = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
    let straight = hw - hh;
    if (flat * dx).abs() <= straight {
        return flat;
    }
    cap_intersection(straight * if dx < 0.0 { -1.0 } else { 1.0 }, hh, dx, dy, 0.0)
}

fn rounded_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    let scale = box_scale(hw, hh, dx, dy);
    let inner_x = hw - ROUNDED_CORNER;
    let inner_y = hh - ROUNDED_CORNER;
    if (dx * scale).abs() <= inner_x || (dy * scale).abs() <= inner_y {
        return scale;
    }
    cap_intersection(inner_x * if dx < 0.0 { -1.0 } else { 1.0 }, ROUNDED_CORNER, dx, dy, inner_y * if dy < 0.0 { -1.0 } else { 1.0 })
}

fn node_boundary(n: &Node, fx: f64, fy: f64) -> (f64, f64) {
    let (cx, cy) = centre(n);
    let (dx, dy) = (fx - cx, fy - cy);
    if dx == 0.0 && dy == 0.0 {
        return (cx, cy);
    }
    let (hw, hh) = (n.w / 2.0, n.h / 2.0);
    let scale = match n.shape {
        Shape::Rhombus => 1.0 / (axis_ratio(dx, hw) + axis_ratio(dy, hh)),
        Shape::Circle => (if hw < hh { hw } else { hh }) / dx.hypot(dy),
        Shape::Hexagon => {
            let slope = 1.0 / (axis_ratio(dx, hw) + axis_ratio(dy, 2.0 * hh));
            let side = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
            if side < slope { side } else { slope }
        }
        Shape::Rounded => rounded_scale(hw, hh, dx, dy),
        Shape::Stadium => stadium_scale(hw, hh, dx, dy),
        Shape::Rect => box_scale(hw, hh, dx, dy),
    };
    (cx + dx * scale, cy + dy * scale)
}

fn unit_towards(tx: f64, ty: f64, fx: f64, fy: f64) -> (f64, f64) {
    let span = (fx - tx).hypot(fy - ty);
    if span == 0.0 { (0.0, 0.0) } else { ((fx - tx) / span, (fy - ty) / span) }
}

fn fmt1(p: (f64, f64)) -> String {
    format!("{},{}", ruby_float(rr(p.0, 1)), ruby_float(rr(p.1, 1)))
}

fn heads(kind: &str, route: (f64, f64, f64, f64), s: &Node, t: &Node, ink: &str) -> Vec<Elem> {
    let head_type = kind.trim_start_matches("thick_").trim_start_matches("dotted_");
    let (shape, _) = match head_type {
        "arrow" | "cross" | "circle" => (head_type, ()),
        _ => return vec![],
    };
    let (sx, sy, tx, ty) = route;
    // the target end: boundary of `t` seen from the route's source end
    let (tip, from) = (node_boundary(t, sx, sy), (sx, sy));
    let _ = (tx, ty, s);
    head_elem(shape, tip, from, ink)
}

fn head_elem(shape: &str, tip: (f64, f64), from: (f64, f64), ink: &str) -> Vec<Elem> {
    let (ax, ay) = unit_towards(tip.0, tip.1, from.0, from.1);
    match shape {
        "cross" => {
            let (bx, by) = (tip.0 + ax * CROSS_HEAD_REACH, tip.1 + ay * CROSS_HEAD_REACH);
            let (mut ux, uy) = unit_towards(bx, by, from.0, from.1);
            if ux == 0.0 && uy == 0.0 {
                ux = 1.0;
            }
            let arm_x = (ux - uy) * CROSS_HEAD_HALF;
            let arm_y = (uy + ux) * CROSS_HEAD_HALF;
            let mut g = vec![];
            for (lx, ly) in [(arm_x, arm_y), (arm_y, -arm_x)] {
                g.push(
                    Elem::new("line")
                        .attr("stroke", ink)
                        .attr("stroke-width", "2")
                        .num("x1", rr(bx - lx, 1))
                        .num("y1", rr(by - ly, 1))
                        .num("x2", rr(bx + lx, 1))
                        .num("y2", rr(by + ly, 1)),
                );
            }
            g
        }
        "circle" => {
            let (cx, cy) = (tip.0 + ax * CIRCLE_HEAD_REACH, tip.1 + ay * CIRCLE_HEAD_REACH);
            vec![Elem::new("circle").attr("fill", ink).attr("stroke", ink).attr("stroke-width", "1").num("cx", rr(cx, 1)).num("cy", rr(cy, 1)).num("r", CIRCLE_HEAD_RADIUS)]
        }
        _ => {
            let (bx, by) = (tip.0 + ax * ARROW_LENGTH, tip.1 + ay * ARROW_LENGTH);
            let pts = [tip, (bx - ay * ARROW_HALF_WIDTH, by + ax * ARROW_HALF_WIDTH), (bx + ay * ARROW_HALF_WIDTH, by - ax * ARROW_HALF_WIDTH)];
            let points = pts.iter().map(|p| fmt1(*p)).collect::<Vec<_>>().join(" ");
            vec![Elem::new("polygon").attr("fill", ink).attr("stroke", ink).attr("points", points)]
        }
    }
}

fn node_elem(n: &Node, theme: &Theme) -> Elem {
    let fill = theme.color("node_fill").unwrap_or("");
    let stroke = theme.color("node_stroke").unwrap_or("");
    let sw = ruby_float(theme.number("shapes", "stroke_width").unwrap_or(2.0));
    let paint = |e: Elem| e.attr("fill", fill).attr("stroke", stroke).attr("stroke-width", sw.clone());
    let (cx, cy) = centre(n);
    let shape = match n.shape {
        Shape::Rect | Shape::Rounded | Shape::Stadium => {
            let r = paint(Elem::new("rect")).num("x", n.x).num("y", n.y).num("width", n.w).num("height", n.h);
            match n.shape {
                Shape::Rounded => r.num("rx", ROUNDED_CORNER).num("ry", ROUNDED_CORNER),
                Shape::Stadium => {
                    let c = (if n.w < n.h { n.w } else { n.h }) / 2.0;
                    r.num("rx", c).num("ry", c)
                }
                _ => r,
            }
        }
        Shape::Circle => paint(Elem::new("circle")).num("cx", cx).num("cy", cy).num("r", (if n.w < n.h { n.w } else { n.h }) / 2.0),
        Shape::Rhombus => {
            let pts = [(cx, n.y), (n.x + n.w, cy), (cx, n.y + n.h), (n.x, cy)];
            paint(Elem::new("polygon")).attr("points", pts.iter().map(|p| format!("{},{}", ruby_float(p.0), ruby_float(p.1))).collect::<Vec<_>>().join(" "))
        }
        Shape::Hexagon => {
            let q = n.w / 4.0;
            let (l, il, ir, r) = (n.x, n.x + q, n.x + n.w - q, n.x + n.w);
            let pts = [(il, n.y), (ir, n.y), (r, cy), (ir, n.y + n.h), (il, n.y + n.h), (l, cy)];
            paint(Elem::new("polygon")).attr("points", pts.iter().map(|p| format!("{},{}", ruby_float(p.0), ruby_float(p.1))).collect::<Vec<_>>().join(" "))
        }
    };
    let label = Elem::new("text")
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .num("x", cx)
        .num("y", cy)
        .attr("text-anchor", "middle")
        .attr("dominant-baseline", "middle")
        .attr("font-family", theme.string("typography", "font_family").unwrap_or(""))
        .attr("font-size", ruby_float(FONT_SIZE))
        .content(&n.label);
    Elem::new("g").attr("id", format!("node-{}", n.id)).child(shape).child(label)
}

pub fn render(source: &str) -> Result<String, Error> {
    let mut model = parse(source)?;
    for n in &mut model.nodes {
        let (w, h) = size_of(&n.label, n.shape);
        n.w = w;
        n.h = h;
    }
    let sizes: Vec<(f64, f64)> = model.nodes.iter().map(|n| (n.w, n.h)).collect();
    let pedges: Vec<place::Edge> = model.edges.iter().map(|e| place::Edge { src: e.src, tgt: e.tgt, labels: vec![] }).collect();
    let pos = place::place(&sizes, &pedges, model.dir)?;
    for (n, (x, y)) in model.nodes.iter_mut().zip(pos) {
        n.x = x;
        n.y = y;
    }
    for e in &model.edges {
        let (a, b) = (centre(&model.nodes[e.src]), centre(&model.nodes[e.tgt]));
        if (a.0 - b.0).hypot(a.1 - b.1) < COINCIDENT {
            return Err(unported("coincident centres"));
        }
    }

    let theme = Theme::default_theme();
    let ink = theme.color("edge_stroke").unwrap_or("");
    let sw = theme.number("shapes", "stroke_width").unwrap_or(2.0);
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for n in &model.nodes {
        max_x = max_x.max(n.x + n.w);
        max_y = max_y.max(n.y + n.h);
    }
    let mut reach_x = f64::NEG_INFINITY;
    let mut reach_y = f64::NEG_INFINITY;
    let mut edge_elems = vec![];
    for e in &model.edges {
        let (s, t) = (&model.nodes[e.src], &model.nodes[e.tgt]);
        let (sc, tc) = (centre(s), centre(t));
        let sb = node_boundary(s, tc.0, tc.1);
        let tb = node_boundary(t, sc.0, sc.1);
        let route = (rr(sb.0, 1), rr(sb.1, 1), rr(tb.0, 1), rr(tb.1, 1));
        reach_x = reach_x.max(route.0).max(route.2);
        reach_y = reach_y.max(route.1).max(route.3);
        let d = format!("M {} {} L {} {}", ruby_float(rr(route.0, 2)), ruby_float(rr(route.1, 2)), ruby_float(rr(route.2, 2)), ruby_float(rr(route.3, 2)));
        let mut path = Elem::new("path").attr("fill", "none").attr("stroke", ink).attr("stroke-width", ruby_float(sw)).attr("d", d);
        if e.kind.starts_with("thick_") {
            let thick = theme.number("shapes", "stroke_width_thick").unwrap_or(sw * 3.5);
            path = path.attr("stroke-width", ruby_float(thick));
        }
        if e.kind.starts_with("dotted_") {
            path = path.attr("stroke-dasharray", theme.string("shapes", "dash_pattern_dotted").unwrap_or("2"));
        }
        let mut g = Elem::new("g").attr("id", format!("edge-{}_to_{}", s.id, t.id));
        g.push(path);
        for h in heads(&e.kind, route, s, t, ink) {
            g.push(h);
        }
        edge_elems.push(g);
    }
    let reach = |v: f64| if v.is_finite() { v + PAGE_MARGIN } else { PAGE_MARGIN };
    let width = (max_x.max(0.0) + PAGE_MARGIN).max(reach(reach_x));
    let height = (max_y.max(0.0) + PAGE_MARGIN).max(reach(reach_y));
    let mut doc = Document::new(width, height);
    doc.overflow = Some("hidden".into());
    for g in edge_elems {
        doc.add(g);
    }
    for n in &model.nodes {
        doc.add(node_elem(n, &theme));
    }
    Ok(doc.to_xml())
}
