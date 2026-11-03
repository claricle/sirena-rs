//! Mindmap renderer: port of Ruby `Parser::Native::Builders::Mindmap` (tree
//! rebuild by indent), `Layout::Mindmap`, `Layout::MindmapNodeSize`,
//! `Layout::MindmapCose*` (force-directed placement) and `Renderer::Mindmap`.
//!
//! Declined to the Ruby path (`Error::Unported`): multiple roots, and
//! round-shape content with comment lines or leading newlines.
use super::pie::{fsum, items, strip, text_of};
use super::user_journey::ruby_round;
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use serde_json::Value;

fn unported() -> Error {
    Error::Unported("mindmap".into())
}

// ---- model ----

struct RawNode {
    content: String,
    shape: &'static str,
    indent: usize,
}

struct MNode {
    label: String,
    shape: &'static str,
    parent: Option<usize>,
    depth: usize,
}

const SHAPE_NAMES: [&str; 6] = ["circle", "bang", "cloud", "hexagon", "square", "round"];

fn truthy(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Null) | Some(Value::Bool(false)))
}

fn raw_text(v: &Value) -> String {
    match v {
        Value::Array(a) if a.is_empty() => "[]".to_string(),
        Value::Array(a) => a.iter().map(text_of).collect(),
        v => text_of(v),
    }
}

fn parse_nodes(tree: &Value) -> Result<Vec<RawNode>, Error> {
    let mut out = vec![];
    let nodes = tree.get("nodes").map(items).unwrap_or_default();
    for n in nodes {
        let node_keys = ["content", "icon", "classes", "shape_circle", "shape_bang", "shape_cloud", "shape_hexagon", "shape_square", "shape_round"];
        if !node_keys.iter().any(|k| truthy(n.get(*k))) {
            continue;
        }
        if truthy(n.get("icon")) || truthy(n.get("classes")) {
            continue; // decorates the previous node; nothing is drawn from it
        }
        let shape = SHAPE_NAMES
            .iter()
            .find(|name| truthy(n.get(format!("shape_{name}").as_str())))
            .copied()
            .unwrap_or("default");
        let mut content = match n.get("content") {
            Some(v) if truthy(Some(v)) => raw_text(v),
            _ => String::new(),
        };
        if shape == "round" {
            if content.contains("%%") {
                return Err(unported()); // comment lines are stripped by the Ruby path
            }
            if !truthy(n.get("round_quoted")) {
                if let Some(rest) = content.strip_prefix("\r\n").or_else(|| content.strip_prefix('\n')) {
                    content = rest.to_string();
                }
            }
        }
        let indent = match n.get("indent") {
            None | Some(Value::Null) => 0,
            Some(Value::Array(a)) => a.iter().map(|v| text_of(v).chars().count()).sum(),
            Some(v) => text_of(v).chars().count(),
        };
        let shape: &'static str = match shape {
            "circle" => "circle",
            "bang" => "bang",
            "cloud" => "cloud",
            "hexagon" => "hexagon",
            "square" => "square",
            "round" => "round",
            _ => "default",
        };
        out.push(RawNode { content, shape, indent });
    }
    Ok(out)
}

/// Rebuilds the hierarchy by indent and flattens it depth first (IR order).
fn hierarchy(raw: &[RawNode]) -> Result<Vec<MNode>, Error> {
    let mut parents: Vec<Option<usize>> = vec![];
    let mut stack: Vec<(usize, usize)> = vec![];
    let mut root: Option<usize> = None;
    for (i, n) in raw.iter().enumerate() {
        while stack.last().is_some_and(|(ind, _)| *ind >= n.indent) {
            stack.pop();
        }
        match stack.last() {
            None => {
                if root.is_some() {
                    return Err(unported()); // "Multiple roots are illegal"
                }
                root = Some(i);
                parents.push(None);
            }
            Some((_, p)) => parents.push(Some(*p)),
        }
        stack.push((n.indent, i));
    }
    let Some(root) = root else { return Ok(vec![]) };
    let mut children: Vec<Vec<usize>> = vec![vec![]; raw.len()];
    for (i, p) in parents.iter().enumerate() {
        if let Some(p) = p {
            children[*p].push(i);
        }
    }
    let mut order: Vec<usize> = vec![];
    let mut depth = vec![0usize; raw.len()];
    fn walk(i: usize, children: &[Vec<usize>], depth: &mut [usize], order: &mut Vec<usize>) {
        order.push(i);
        for &c in &children[i] {
            depth[c] = depth[i] + 1;
            walk(c, children, depth, order);
        }
    }
    walk(root, &children, &mut depth, &mut order);
    let mut pos = vec![0usize; raw.len()];
    for (k, &i) in order.iter().enumerate() {
        pos[i] = k;
    }
    Ok(order
        .iter()
        .map(|&i| MNode {
            label: raw[i].content.clone(),
            shape: raw[i].shape,
            parent: parents[i].map(|p| pos[p]),
            depth: depth[i],
        })
        .collect())
}

// ---- node size ----

const FONT_SIZE: f64 = 16.0;
const LINE_HEIGHT_RATIO: f64 = 24.0 / 16.0;
const MAX_TEXT_WIDTH: f64 = 200.0;

struct Size {
    width: f64,
    height: f64,
    lines: Vec<String>,
}

fn ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

fn rb_strip(s: &str) -> &str {
    strip(s)
}

/// `label.split(/<br\s*\/?>|\r?\n/i).map(&:strip).reject(&:empty?)`
fn label_lines(label: &str) -> Vec<String> {
    let bytes = label.as_bytes();
    let mut parts: Vec<&str> = vec![];
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        let rest = &label[i..];
        let mut hit = 0usize;
        if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("<br") {
            let mut j = i + 3;
            while j < bytes.len() && ws(bytes[j] as char) {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'/' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'>' {
                hit = j + 1 - i;
            }
        }
        if hit == 0 && bytes[i] == b'\n' {
            hit = 1;
        } else if hit == 0 && bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            hit = 2;
        }
        if hit > 0 {
            parts.push(&label[start..i]);
            i += hit;
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push(&label[start..]);
    let lines: Vec<String> = parts.into_iter().map(rb_strip).filter(|p| !p.is_empty()).map(str::to_string).collect();
    if lines.is_empty() { vec![String::new()] } else { lines }
}

fn words(s: &str) -> Vec<&str> {
    s.split(ws).filter(|w| !w.is_empty()).collect()
}

fn measure(t: &str, shape: &str, size: f64) -> f64 {
    let width = text::width(t, size, Face::Trebuchet);
    if shape == "bang" || shape == "cloud" {
        return width;
    }
    width + text::trebuchet_kerning_adjustment(t, size)
}

fn node_size(label: &str, shape: &str, font_size: f64, line_height: f64) -> Size {
    let lines = label_lines(label);
    let widest_word = lines
        .iter()
        .flat_map(|l| words(l))
        .map(|w| measure(w, shape, font_size))
        .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| if v > m { v } else { m })))
        .unwrap_or(0.0);
    let wrap_width = if widest_word > MAX_TEXT_WIDTH { widest_word } else { MAX_TEXT_WIDTH };
    let mut wrapped: Vec<String> = vec![];
    for line in &lines {
        if measure(line, shape, font_size) <= wrap_width {
            wrapped.push(line.clone());
            continue;
        }
        let mut rows: Vec<String> = vec![];
        for word in words(line) {
            let joined = format!("{} {}", rows.last().map(String::as_str).unwrap_or(""), word);
            if !rows.is_empty() && measure(&joined, shape, font_size) <= wrap_width {
                *rows.last_mut().unwrap() = joined;
            } else {
                rows.push(word.to_string());
            }
        }
        wrapped.extend(rows);
    }
    let widest = wrapped
        .iter()
        .map(|l| measure(l, shape, font_size))
        .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| if v > m { v } else { m })))
        .unwrap_or(0.0);
    let text_width = if wrapped.len() > lines.len() && MAX_TEXT_WIDTH > widest { MAX_TEXT_WIDTH } else { widest };
    let text_height = wrapped.len() as f64 * font_size * line_height;
    let (wide, high) = match shape {
        "circle" => {
            let two_lines = font_size * line_height * 2.0;
            let side = if text_width > two_lines {
                text_width
            } else if text_height < text_width {
                text_width
            } else if text_height > two_lines {
                two_lines
            } else {
                text_height
            };
            (side + 20.0, side + 20.0)
        }
        "square" => (text_width + 40.0, text_height + 20.0),
        "round" => (text_width + 30.0, text_height + 30.0),
        "hexagon" => (text_width + 68.0, text_height + 20.0),
        "bang" => ((text_width * 1.25) + 62.4, text_height + 56.0),
        "cloud" => ((text_width * 0.74) + 48.5, (text_width * 0.49) + 38.3),
        _ => (text_width + 40.0, text_height + 10.0),
    };
    Size { width: wide, height: high, lines: wrapped }
}

// ---- trig: correctly rounded sin/cos, like the Ruby 60-digit series ----

#[derive(Clone, Copy)]
struct Dd(f64, f64);

fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

fn quick(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd(s, b - (s - a))
}

fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

fn dd_add(a: Dd, b: Dd) -> Dd {
    let (s1, s2) = two_sum(a.0, b.0);
    let (t1, t2) = two_sum(a.1, b.1);
    let s2 = s2 + t1;
    let Dd(s1, s2) = quick(s1, s2);
    let s2 = s2 + t2;
    quick(s1, s2)
}

fn dd_mul(a: Dd, b: Dd) -> Dd {
    let (p1, p2) = two_prod(a.0, b.0);
    quick(p1, p2 + (a.0 * b.1 + a.1 * b.0))
}

fn dd_neg(a: Dd) -> Dd {
    Dd(-a.0, -a.1)
}

fn dd_div_small(a: Dd, d: f64) -> Dd {
    let q1 = a.0 / d;
    let (p, e) = two_prod(q1, d);
    let r = dd_add(a, Dd(-p, -e));
    let q2 = r.0 / d;
    let (p, e) = two_prod(q2, d);
    let r = dd_add(r, Dd(-p, -e));
    let q3 = r.0 / d;
    let Dd(s, t) = quick(q1, q2);
    dd_add(Dd(s, t), Dd(q3, 0.0))
}

fn reduce(angle: f64) -> (i64, Dd) {
    let half_pi_1 = f64::from_bits(0x3FF921FB54442D18);
    let half_pi_2 = f64::from_bits(0x3C91A62633145C07);
    let half_pi_3 = f64::from_bits(0xB91F1976B7ED8FBC);
    let k = (angle / half_pi_1).round();
    let mut r = Dd(angle, 0.0);
    let (p, e) = two_prod(k, half_pi_1);
    r = dd_add(r, Dd(-p, -e));
    let (p, e) = two_prod(k, half_pi_2);
    r = dd_add(r, Dd(-p, -e));
    r = dd_add(r, Dd(-(k * half_pi_3), 0.0));
    (k as i64, r)
}

fn sin_series(r: Dd) -> Dd {
    let x2 = dd_mul(r, r);
    let (mut sum, mut term) = (r, r);
    for i in 1..40 {
        let d = ((2 * i) * (2 * i + 1)) as f64;
        term = dd_neg(dd_div_small(dd_mul(term, x2), d));
        if term.0 == 0.0 {
            break;
        }
        sum = dd_add(sum, term);
        if term.0.abs() < 1e-40 * sum.0.abs().max(1e-300) {
            break;
        }
    }
    sum
}

fn cos_series(r: Dd) -> Dd {
    let x2 = dd_mul(r, r);
    let (mut sum, mut term) = (Dd(1.0, 0.0), Dd(1.0, 0.0));
    for i in 1..40 {
        let d = ((2 * i - 1) * (2 * i)) as f64;
        term = dd_neg(dd_div_small(dd_mul(term, x2), d));
        if term.0 == 0.0 {
            break;
        }
        sum = dd_add(sum, term);
        if term.0.abs() < 1e-40 * sum.0.abs() {
            break;
        }
    }
    sum
}

fn finish(v: Dd) -> f64 {
    let x = v.0 + v.1;
    if x == 0.0 { 0.0 } else { x }
}

fn sin(radians: f64) -> f64 {
    let (k, r) = reduce(radians);
    match k.rem_euclid(4) {
        0 => finish(sin_series(r)),
        1 => finish(cos_series(r)),
        2 => finish(dd_neg(sin_series(r))),
        _ => finish(dd_neg(cos_series(r))),
    }
}

fn cos(radians: f64) -> f64 {
    let (k, r) = reduce(radians);
    match k.rem_euclid(4) {
        0 => finish(cos_series(r)),
        1 => finish(dd_neg(sin_series(r))),
        2 => finish(dd_neg(cos_series(r))),
        _ => finish(sin_series(r)),
    }
}

// ---- COSE placement ----

#[derive(Clone, Copy)]
struct R {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl R {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn cx(&self) -> f64 {
        self.x + (self.w / 2.0)
    }
    fn cy(&self) -> f64 {
        self.y + (self.h / 2.0)
    }
    fn hw(&self) -> f64 {
        self.w / 2.0
    }
    fn hh(&self) -> f64 {
        self.h / 2.0
    }
}

fn intersects(a: &R, b: &R) -> bool {
    !(a.right() < b.x || a.bottom() < b.y || b.right() < a.x || b.bottom() < a.y)
}

fn sgn(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else if v > 0.0 { 1.0 } else { 0.0 }
}

fn slope_between(a: &R, b: &R) -> f64 {
    let rise = b.cy() - a.cy();
    let run = b.cx() - a.cx();
    rise / run
}

fn clip_points(a: &R, b: &R) -> Option<[f64; 4]> {
    if intersects(a, b) {
        return None;
    }
    if a.cx() == b.cx() {
        let (x1, x2) = (a.cx(), b.cx());
        return Some(if a.cy() > b.cy() { [x1, a.y, x2, b.bottom()] } else { [x1, a.bottom(), x2, b.y] });
    }
    if a.cy() == b.cy() {
        let (y1, y2) = (a.cy(), b.cy());
        return Some(if a.cx() > b.cx() { [a.x, y1, b.right(), y2] } else { [a.right(), y1, b.x, y2] });
    }
    let slope = slope_between(a, b);
    let left_up = (a.cx() > b.cx(), a.cy() > b.cy());
    let p = border_point(a, slope, left_up, false);
    let q = border_point(b, slope, left_up, true);
    Some([p[0], p[1], q[0], q[1]])
}

fn border_point(b: &R, slope: f64, left_up: (bool, bool), second: bool) -> [f64; 2] {
    let other_left = if second { !left_up.0 } else { left_up.0 };
    let diagonal = b.h / b.w;
    if -diagonal == slope {
        return if other_left { [b.x, b.bottom()] } else { [b.right(), b.y] };
    }
    if diagonal == slope {
        return if other_left { [b.x, b.y] } else { [b.right(), b.bottom()] };
    }
    let (sign, mut line) = match left_up {
        (true, true) => (1.0, 4i64),
        (true, false) => (-1.0, 3),
        (false, true) => (-1.0, 1),
        (false, false) => (1.0, 2),
    };
    if second {
        line = ((line + 1) % 4) + 1;
    }
    let dir = if sign * diagonal > slope { line } else { 1 + (line % 4) };
    match dir {
        1 => [b.cx() + (-b.hh() / slope), b.y],
        2 => [b.right(), b.cy() + (b.hw() * slope)],
        3 => [b.cx() + (b.hh() / slope), b.bottom()],
        _ => [b.x, b.cy() + (-b.hw() * slope)],
    }
}

fn axis_overlap(low_a: f64, high_a: f64, low_b: f64, high_b: f64) -> f64 {
    let overlap = high_a.min(high_b) - low_a.max(low_b);
    if low_a <= low_b && high_a >= high_b {
        overlap + (low_b - low_a).min(high_a - high_b)
    } else if low_b <= low_a && high_b >= high_a {
        overlap + (low_a - low_b).min(high_b - high_a)
    } else {
        overlap
    }
}

fn separation(a: &R, b: &R, buffer: f64) -> (f64, f64) {
    let overlap_x = axis_overlap(a.x, a.right(), b.x, b.right());
    let overlap_y = axis_overlap(a.y, a.bottom(), b.y, b.bottom());
    let slope = if a.cy() == b.cy() && a.cx() == b.cx() { 1.0 } else { slope_between(a, b).abs() };
    let move_x = overlap_y / slope;
    let (mx, my) = if overlap_x < move_x { (overlap_x, slope * overlap_x) } else { (move_x, overlap_y) };
    let dir = |f: f64, s: f64| if f < s { -1.0 } else { 1.0 };
    let push = |d: f64, m: f64| (-1.0 * d) * ((m / 2.0) + buffer);
    (push(dir(a.cx(), b.cx()), mx), push(dir(a.cy(), b.cy()), my))
}

#[derive(Clone)]
struct Body {
    w: f64,
    h: f64,
    x: f64,
    y: f64,
    links: Vec<usize>,
    spring: (f64, f64),
    rep: (f64, f64),
    cells: [i64; 4],
    surrounding: Vec<usize>,
}

impl Body {
    fn r(&self) -> R {
        R { x: self.x, y: self.y, w: self.w, h: self.h }
    }
}

struct Link {
    source: usize,
    target: usize,
    lx: f64,
    ly: f64,
}

fn unit_floor(v: f64) -> f64 {
    if v.abs() < 1.0 { sgn(v) } else { v }
}

struct Cose {
    bodies: Vec<Body>,
    links: Vec<Link>,
}

const MARGIN: f64 = 15.0;
const IDEAL_LENGTH: f64 = 50.0;
const SPRING_STRENGTH: f64 = 0.45;
const REPULSION_STRENGTH: f64 = 4500.0;
const MIN_ITERATIONS: usize = 2500;
const CHECK_PERIOD: usize = 100;
const GRID_PERIOD: usize = 10;
const MAX_DISPLACEMENT: f64 = 300.0;
const DISPLACEMENT_PER_BODY: f64 = 1.5;
const MIN_REPULSION_DISTANCE: f64 = IDEAL_LENGTH / 10.0;
const SEPARATION_BUFFER: f64 = IDEAL_LENGTH / 2.0;
const REPULSION_RANGE: f64 = 2.0 * IDEAL_LENGTH;

fn bounding_box(bodies: &[Body]) -> (f64, f64, f64, f64) {
    let min = |f: &dyn Fn(&Body) -> f64| bodies.iter().map(f).fold(f64::INFINITY, f64::min);
    let max = |f: &dyn Fn(&Body) -> f64| bodies.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
    (min(&|b| b.x), min(&|b| b.y), max(&|b| b.r().right()), max(&|b| b.r().bottom()))
}

impl Cose {
    fn other_end(&self, link: usize, body: usize) -> usize {
        if self.links[link].source == body { self.links[link].target } else { self.links[link].source }
    }

    fn neighbors(&self, body: usize) -> Vec<usize> {
        let mut out: Vec<usize> = vec![];
        for &l in &self.bodies[body].links {
            let o = self.other_end(l, body);
            if !out.contains(&o) {
                out.push(o);
            }
        }
        out
    }

    fn shift_to(&mut self, world_x: f64, world_y: f64) {
        let (left, top, _, _) = bounding_box(&self.bodies);
        let (dx, dy) = (left - MARGIN, top - MARGIN);
        for b in &mut self.bodies {
            b.x = world_x + (b.x - dx);
            b.y = world_y + (b.y - dy);
        }
    }

    // seed
    fn seed(&mut self) -> (f64, f64) {
        let mut order = vec![0usize];
        let mut i = 0;
        while i < order.len() {
            for o in self.neighbors(order[i]) {
                if !order.contains(&o) {
                    order.push(o);
                }
            }
            i += 1;
        }
        let mut remaining = order.clone();
        while remaining.len() > 2 {
            remaining = remaining.iter().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, b)| *b).collect();
        }
        let centre = remaining[0];
        let diag = |b: &Body| ((b.w * b.w) + (b.h * b.h)).sqrt();
        let ring = order.iter().map(|&i| diag(&self.bodies[i])).fold(f64::MIN, f64::max).max(50.0);
        self.branch(centre, None, (0.0, 359.0), 0.0, ring);
        let (left, top, right, bottom) = bounding_box(&self.bodies);
        for b in &mut self.bodies {
            b.x -= left;
            b.y -= top;
        }
        (right - left, bottom - top)
    }

    fn branch(&mut self, body: usize, parent: Option<usize>, wedge: (f64, f64), distance: f64, ring: f64) {
        let radians = (wedge_bearing(wedge) * (2.0 * std::f64::consts::PI)) / 360.0;
        let cx = distance * cos(radians);
        let cy = distance * sin(radians);
        let b = &mut self.bodies[body];
        b.x = cx - (b.w / 2.0);
        b.y = cy - (b.h / 2.0);
        let mut links = self.bodies[body].links.clone();
        if let Some(p) = parent {
            let idx = links.iter().position(|&l| self.other_end(l, body) == p).unwrap_or(0);
            links.rotate_left(idx);
        }
        let children: Vec<usize> = links.iter().map(|&l| self.other_end(l, body)).filter(|o| Some(*o) != parent).collect();
        for (index, child) in children.iter().enumerate() {
            let w = wedge_slice(wedge, index, children.len());
            self.branch(*child, Some(body), w, distance + ring, ring);
        }
    }

    fn measure_link(&mut self, l: usize) -> Option<f64> {
        let (s, t) = (self.links[l].source, self.links[l].target);
        let clip = clip_points(&self.bodies[t].r(), &self.bodies[s].r())?;
        let lx = unit_floor(clip[0] - clip[2]);
        let ly = unit_floor(clip[1] - clip[3]);
        self.links[l].lx = lx;
        self.links[l].ly = ly;
        Some(((lx * lx) + (ly * ly)).sqrt())
    }

    // relaxation
    fn relax(&mut self) {
        let n = self.bodies.len();
        let max_iterations = (n * 5).max(MIN_ITERATIONS);
        let threshold = DISPLACEMENT_PER_BODY * n as f64;
        let final_cooling = CHECK_PERIOD as f64 / MIN_ITERATIONS as f64;
        let cooling_cycles = MIN_ITERATIONS as f64 / CHECK_PERIOD as f64;
        let mut cooling = 1.0f64;
        let (mut iterations, mut cycle) = (0usize, 0u32);
        let (mut displacement, mut previous) = (0.0f64, 0.0f64);
        let mut grid = Grid { cells: vec![] };
        loop {
            iterations += 1;
            if iterations == max_iterations {
                break;
            }
            if iterations % CHECK_PERIOD == 0 {
                let oscillating = iterations as f64 > max_iterations as f64 / 3.0 && (displacement - previous).abs() < 2.0;
                let converged = displacement < threshold;
                previous = displacement;
                if converged || oscillating {
                    break;
                }
                cycle += 1;
                let decay = (f64::from(cycle).powf((100.0 * (1.0 - final_cooling)).ln() / cooling_cycles.ln())) / 100.0;
                cooling = (1.0 - decay).max(final_cooling);
            }
            // step
            displacement = 0.0;
            for l in 0..self.links.len() {
                let Some(length) = self.measure_link(l) else { continue };
                if length == 0.0 {
                    continue;
                }
                let pull = SPRING_STRENGTH * (length - IDEAL_LENGTH);
                let (fx, fy) = (pull * (self.links[l].lx / length), pull * (self.links[l].ly / length));
                let (s, t) = (self.links[l].source, self.links[l].target);
                self.bodies[s].spring.0 += fx;
                self.bodies[s].spring.1 += fy;
                self.bodies[t].spring.0 -= fx;
                self.bodies[t].spring.1 -= fy;
            }
            let refresh = iterations % GRID_PERIOD == 1;
            if refresh {
                grid.refresh(&mut self.bodies);
            }
            let mut done = vec![false; n];
            for b in 0..n {
                if refresh {
                    grid.refresh_surrounding(&mut self.bodies, b, &done);
                }
                let surrounding = self.bodies[b].surrounding.clone();
                for o in surrounding {
                    self.repel(b, o);
                }
                done[b] = true;
            }
            for b in &mut self.bodies {
                displacement += b.step(cooling);
            }
        }
    }

    fn repel(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.bodies[a].r(), self.bodies[b].r());
        let (fx, fy) = if intersects(&ra, &rb) {
            let (x, y) = separation(&ra, &rb, SEPARATION_BUFFER);
            let share = 1.0 / 2.0;
            (share * (2.0 * x), share * (2.0 * y))
        } else {
            let clip = clip_points(&ra, &rb).unwrap_or([0.0; 4]);
            let gap = |g: f64| if g.abs() < MIN_REPULSION_DISTANCE { sgn(g) * MIN_REPULSION_DISTANCE } else { g };
            let (gx, gy) = (gap(clip[2] - clip[0]), gap(clip[3] - clip[1]));
            let squared = (gx * gx) + (gy * gy);
            let distance = squared.sqrt();
            let force = REPULSION_STRENGTH / squared;
            ((force * gx) / distance, (force * gy) / distance)
        };
        self.bodies[a].rep.0 -= fx;
        self.bodies[a].rep.1 -= fy;
        self.bodies[b].rep.0 += fx;
        self.bodies[b].rep.1 += fy;
    }
}

impl Body {
    fn step(&mut self, cooling: f64) -> f64 {
        let limit = cooling * MAX_DISPLACEMENT;
        let clamp = |d: f64| if d.abs() > limit { limit * sgn(d) } else { d };
        let mx = clamp(cooling * (self.spring.0 + self.rep.0));
        let my = clamp(cooling * (self.spring.1 + self.rep.1));
        self.x += mx;
        self.y += my;
        self.spring = (0.0, 0.0);
        self.rep = (0.0, 0.0);
        mx.abs() + my.abs()
    }
}

fn ruby_remainder(x: f64, y: f64) -> f64 {
    let mut z = x % y;
    if y * z < 0.0 {
        z += y;
    }
    if z != 0.0 && ((x < 0.0 && y > 0.0) || (x > 0.0 && y < 0.0)) {
        return z - y;
    }
    z
}

fn wedge_bearing(w: (f64, f64)) -> f64 {
    let half = ((w.1 - w.0) + 1.0) / 2.0;
    let mid = if half < 0.0 { half + 180.0 } else { half };
    ruby_remainder(mid + w.0, 360.0)
}

fn wedge_slice(w: (f64, f64), index: usize, count: usize) -> (f64, f64) {
    let step = (w.1 - w.0).abs() / count as f64;
    let first = ruby_remainder(w.0 + (index as f64 * step), 360.0);
    (first, ruby_remainder(first + step, 360.0))
}

struct Grid {
    cells: Vec<Vec<Vec<usize>>>,
}

impl Grid {
    fn refresh(&mut self, bodies: &mut [Body]) {
        let (l, t, r, b) = bounding_box(bodies);
        let (width, height) = (r - l, b - t);
        let (left, top, right, bottom) = (l - MARGIN, t - MARGIN, l + width + MARGIN, t + height + MARGIN);
        let cols = ((right - left) / REPULSION_RANGE).ceil().max(0.0) as usize;
        let rows = ((bottom - top) / REPULSION_RANGE).ceil().max(0.0) as usize;
        self.cells = vec![vec![vec![]; rows]; cols];
        let span = |start: f64, size: f64, origin: f64| {
            (((start - origin) / REPULSION_RANGE).floor() as i64, (((size + start) - origin) / REPULSION_RANGE).floor() as i64)
        };
        for (i, body) in bodies.iter_mut().enumerate() {
            let (fx, lx) = span(body.x, body.w, left);
            let (fy, ly) = span(body.y, body.h, top);
            for c in fx..=lx {
                for r in fy..=ly {
                    if let Some(cell) = self.cells.get_mut(c as usize).and_then(|col| col.get_mut(r as usize)) {
                        cell.push(i);
                    }
                }
            }
            body.cells = [fx, lx, fy, ly];
        }
    }

    fn refresh_surrounding(&self, bodies: &mut [Body], idx: usize, processed: &[bool]) {
        let [fx, lx, fy, ly] = bodies[idx].cells;
        let mut candidates: Vec<usize> = vec![];
        for c in (fx - 1)..=(lx + 1) {
            for r in (fy - 1)..=(ly + 1) {
                if c >= 0 && r >= 0 && (c as usize) < self.cells.len() && self.cells.first().is_some_and(|f| (r as usize) < f.len()) {
                    candidates.extend(self.cells[c as usize][r as usize].iter().filter(|&&o| o != idx));
                }
            }
        }
        let me = bodies[idx].r();
        let mut found: Vec<usize> = vec![];
        for o in candidates {
            if processed[o] || found.contains(&o) {
                continue;
            }
            let ob = bodies[o].r();
            let gap = |c: f64, oc: f64, s: f64, os: f64| (c - oc).abs() - ((s / 2.0) + (os / 2.0));
            let gx = gap(me.cx(), ob.cx(), me.w, ob.w);
            let gy = gap(me.cy(), ob.cy(), me.h, ob.h);
            if gx <= REPULSION_RANGE && gy <= REPULSION_RANGE {
                found.push(o);
            }
        }
        bodies[idx].surrounding = found;
    }
}

/// `MindmapCose.call`: centre of every body, in input order.
fn cose(sizes: &[(f64, f64)], edges: &[(usize, usize)]) -> Vec<(f64, f64)> {
    let bodies: Vec<Body> = sizes
        .iter()
        .map(|&(w, h)| Body { w, h, x: 0.0, y: 0.0, links: vec![], spring: (0.0, 0.0), rep: (0.0, 0.0), cells: [0; 4], surrounding: vec![] })
        .collect();
    let mut c = Cose { bodies, links: vec![] };
    if c.bodies.is_empty() {
        return vec![];
    }
    for &(s, t) in edges {
        if s == t || c.neighbors(s).contains(&t) {
            continue;
        }
        c.links.push(Link { source: s, target: t, lx: 0.0, ly: 0.0 });
        let l = c.links.len() - 1;
        c.bodies[s].links.push(l);
        c.bodies[t].links.push(l);
    }
    let (w, h) = c.seed();
    c.shift_to(1200.0 - (w / 2.0), 900.0 - (h / 2.0));
    c.relax();
    c.shift_to(0.0, 0.0);
    c.bodies.iter().map(|b| (b.r().cx(), b.r().cy())).collect()
}

// ---- layout + render ----

const PADDING: f64 = 10.0;
const EDGE_INSET: f64 = 15.0;
const LABEL_LINE_PITCH: f64 = 1.2;

fn f(v: f64) -> String {
    ruby_float(v)
}

fn lighten(color: &str, factor: f64) -> String {
    let c = color.trim_start_matches('#');
    let ch = |i: usize| {
        let v = i64::from_str_radix(&c[i..i + 2], 16).unwrap_or(0) as f64;
        (v + ((255.0 - v) * factor)).round() as i64
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(2), ch(4))
}

struct Placed {
    shape: &'static str,
    level: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    lines: Vec<String>,
}

fn blend(points: &[(f64, f64); 3], weights: [f64; 3], divisor: f64) -> String {
    let x = fsum(points.iter().zip(weights).map(|(p, w)| p.0 * w));
    let y = fsum(points.iter().zip(weights).map(|(p, w)| p.1 * w));
    format!("{} {}", f(x / divisor), f(y / divisor))
}

fn inset_point(from: (f64, f64), toward: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (toward.0 - from.0, toward.1 - from.1);
    let distance = dx.hypot(dy);
    if distance == 0.0 {
        return from;
    }
    let inset = |a: f64, b: f64| a + (((b - a) * EDGE_INSET) / distance);
    (inset(from.0, toward.0), inset(from.1, toward.1))
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "mindmap")?;
    let raw = parse_nodes(&tree)?;
    let nodes = hierarchy(&raw)?;
    let theme = Theme::default_theme();
    let font_size = theme.number("typography", "font_size_large").filter(|v| v.is_finite() && *v > 0.0).unwrap_or(FONT_SIZE);
    let line_height = theme.number("typography", "line_height").filter(|v| v.is_finite() && *v > 0.0).unwrap_or(LINE_HEIGHT_RATIO);

    if nodes.is_empty() {
        let d = Document::new(PADDING * 2.0, PADDING * 2.0);
        return Ok(d.to_xml());
    }
    let sizes: Vec<Size> = nodes.iter().map(|n| node_size(&n.label, n.shape, font_size, line_height)).collect();
    let edges: Vec<(usize, usize)> = nodes.iter().enumerate().filter_map(|(i, n)| n.parent.map(|p| (p, i))).collect();
    let centres = cose(&sizes.iter().map(|s| (s.width, s.height)).collect::<Vec<_>>(), &edges);

    // frame
    let mut placed: Vec<Placed> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| Placed {
            shape: n.shape,
            level: n.depth,
            x: centres[i].0,
            y: centres[i].1 - (sizes[i].height / 2.0),
            w: sizes[i].width,
            h: sizes[i].height,
            lines: sizes[i].lines.clone(),
        })
        .collect();
    let left = placed.iter().map(|n| n.x - (n.w / 2.0)).fold(f64::INFINITY, f64::min);
    let top = placed.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
    for n in &mut placed {
        n.x = n.x - left + PADDING;
        n.y = n.y - top + PADDING;
    }
    let right = placed.iter().map(|n| n.x + (n.w / 2.0)).fold(f64::NEG_INFINITY, f64::max);
    let bottom = placed.iter().map(|n| n.y + n.h).fold(f64::NEG_INFINITY, f64::max);
    let (width, height) = (right + PADDING, bottom + PADDING);

    let mut doc = Document::new(width, height);
    let level_color = |level: usize| theme.palette("mindmap_palette", level).unwrap_or("").to_string();

    // geometry per node
    struct Geo {
        x: f64,
        y: f64,
        center_x: f64,
        center_y: f64,
        radius: f64,
    }
    let geos: Vec<Geo> = placed
        .iter()
        .map(|n| {
            let radius = n.w.max(n.h) / 2.0;
            Geo { x: n.x - (n.w / 2.0), y: n.y, center_x: n.x, center_y: n.y + radius, radius }
        })
        .collect();

    // edges
    for (parent, child) in &edges {
        let (s, t) = (&placed[*parent], &placed[*child]);
        let (sg, tg) = (&geos[*parent], &geos[*child]);
        let from = (sg.center_x, sg.y + (s.h / 2.0));
        let to = (tg.center_x, tg.y + (t.h / 2.0));
        let (start, end) = (inset_point(from, to), inset_point(to, from));
        let mid = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
        let pts = [start, mid, end];
        let path = [
            format!("M {} {} L {}", f(start.0), f(start.1), blend(&pts, [5.0, 1.0, 0.0], 6.0)),
            format!("C {}, {}, {}", blend(&pts, [2.0, 1.0, 0.0], 3.0), blend(&pts, [1.0, 2.0, 0.0], 3.0), blend(&pts, [1.0, 4.0, 1.0], 6.0)),
            format!("C {}, {}, {}", blend(&pts, [0.0, 2.0, 1.0], 3.0), blend(&pts, [0.0, 1.0, 2.0], 3.0), blend(&pts, [0.0, 1.0, 5.0], 6.0)),
            format!("L {} {}", f(end.0), f(end.1)),
        ]
        .join(" ");
        doc.add(
            Elem::new("path")
                .attr("fill", "none")
                .attr("stroke", level_color(s.level))
                .attr("stroke-width", "2")
                .attr("d", path),
        );
    }

    // nodes
    let pitch = font_size * LABEL_LINE_PITCH;
    for (n, g) in placed.iter().zip(&geos) {
        let color = level_color(n.level);
        let paint = |e: Elem| e.attr("fill", lighten(&color, 0.9)).attr("stroke", color.clone()).attr("stroke-width", "2");
        let mut group = Elem::new("g").attr("class", "node mindmap-node");
        let rect = |with_radius: bool| {
            let mut e = Elem::new("rect").num("x", g.x).num("y", n.y).num("width", n.w).num("height", n.h);
            if with_radius {
                e = e.num("rx", 5.0).num("ry", 5.0);
            }
            e
        };
        match n.shape {
            "circle" => group.push(paint(Elem::new("circle").num("cx", g.center_x).num("cy", g.center_y).num("r", g.radius))),
            "cloud" | "bang" => {
                let half = n.w / 2.0;
                let (cx, top_y, height) = (g.center_x, n.y, n.h);
                let xs = [cx - half, cx - (half * 0.6), cx - (half * 0.2), cx + (half * 0.2), cx + (half * 0.6), cx + half];
                let ys = [top_y, top_y + (height * 0.1), top_y + (height * 0.2), top_y + (height * 0.3), top_y + (height * 0.6), top_y + height];
                let (left, inner_left, near_left, near_right, inner_right, right) = (f(xs[0]), f(xs[1]), f(xs[2]), f(xs[3]), f(xs[4]), f(xs[5]));
                let (t, high, upper, shoulder, lower, bottom) = (f(ys[0]), f(ys[1]), f(ys[2]), f(ys[3]), f(ys[4]), f(ys[5]));
                let center = f(cx);
                let d = [
                    format!("M {left} {lower} Q {left} {shoulder}, {inner_left} {upper} "),
                    format!("Q {inner_left} {t}, {near_left} {high} "),
                    format!("Q {near_left} {t}, {center} {t} "),
                    format!("Q {near_right} {t}, {near_right} {high} "),
                    format!("Q {inner_right} {t}, {inner_right} {upper} "),
                    format!("Q {right} {shoulder}, {right} {lower} "),
                    format!("Q {right} {bottom}, {center} {bottom} "),
                    format!("Q {left} {bottom}, {left} {lower} Z"),
                ]
                .join("");
                group.push(paint(Elem::new("path")).attr("d", d));
            }
            "hexagon" => {
                let offset = n.w * 0.2;
                let l = g.center_x - (n.w / 2.0);
                let r = g.center_x + (n.w / 2.0);
                let middle = n.y + (n.h / 2.0);
                let bottom = n.y + n.h;
                let pts = [(l + offset, n.y), (r - offset, n.y), (r, middle), (r - offset, bottom), (l + offset, bottom), (l, middle)];
                let points = pts.iter().map(|p| format!("{},{}", f(p.0), f(p.1))).collect::<Vec<_>>().join(" ");
                group.push(paint(Elem::new("polygon")).attr("points", points));
            }
            "square" => group.push(paint(rect(false))),
            _ => group.push(paint(rect(true))),
        }
        // label
        let center_y = if n.shape == "circle" { n.y + g.radius } else { n.y + (n.h / 2.0) };
        let baseline = center_y + 5.0;
        let lift = ((n.lines.len() as f64 - 1.0) * pitch) / 2.0;
        let y = baseline - lift;
        let mut text = Elem::new("text")
            .num("x", g.center_x)
            .num("y", y)
            .attr("text-anchor", "middle")
            .attr("fill", theme.color("label_text").unwrap_or(""))
            .attr("font-size", f(font_size))
            .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"));
        if n.lines.len() == 1 {
            text = text.content(&n.lines[0]);
        } else {
            for (i, line) in n.lines.iter().enumerate() {
                let t = if i == n.lines.len() - 1 { line.clone() } else { format!("{line} ") };
                let mut span = Elem::new("tspan").num("x", g.center_x);
                if i > 0 {
                    span = span.num("y", ruby_round(y + (i as f64 * pitch), 4));
                }
                text.push(span.content(&t));
            }
        }
        group.push(text);
        doc.add(group);
    }
    Ok(doc.to_xml())
}
