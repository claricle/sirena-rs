//! `Layout::C4` node construction and `C4Placement` / `C4Bounds`.
use super::model::{Boundary, Diagram, Element};
use super::text_metrics::{self as tm, Sizes};
use regex::Regex;

const MARGIN: i64 = 50;
const AVAILABLE_WIDTH: f64 = 800.0;
const MARGIN_X: i64 = 50;
const MARGIN_Y: i64 = 10;

pub struct Line {
    pub text: String,
    pub offset: f64,
    pub font_size: i64,
    pub weight: &'static str,
    pub style: &'static str,
    pub anchor: Option<&'static str>,
}

pub struct Placed {
    pub id: String,
    pub kind: &'static str,
    pub external: bool,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub labels: Vec<Line>,
    pub stereotype: Option<String>,
    pub stereotype_offset: i64,
    pub image_offset: Option<i64>,
    pub outline: Option<(String, String)>,
    pub header: i64,
    pub children: Vec<Placed>,
}

pub struct Canvas {
    pub width: i64,
    pub height: i64,
    pub title_x: f64,
}

fn bracket(text: &str) -> String {
    if text.is_empty() { String::new() } else { format!("[{text}]") }
}

fn line_spread(index: usize, size: i64, count: usize) -> f64 {
    (index as i64 * size) as f64 - (size * (count as i64 - 1)) as f64 / 2.0
}

fn label_lines(text: &str, offset: Option<i64>, size: i64, weight: &'static str, style: &'static str, anchor: Option<&'static str>) -> Vec<Line> {
    let Some(offset) = offset else { return vec![] };
    if text.is_empty() {
        return vec![];
    }
    let lines = tm::lines(text);
    let count = lines.len();
    lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| Line { text, offset: offset as f64 + line_spread(i, size, count), font_size: size, weight, style, anchor })
        .collect()
}

fn stereotype_text(element_type: &str) -> Option<String> {
    if element_type.is_empty() {
        return None;
    }
    let external = element_type.ends_with("_Ext");
    let base = element_type.strip_suffix("_Ext").unwrap_or(element_type);
    let mut snake = String::new();
    let chars: Vec<char> = base.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && chars[i - 1].is_ascii_lowercase() && c.is_ascii_uppercase() {
            snake.push('_');
        }
        snake.push(*c);
    }
    Some(format!("<<{}{}>>", if external { "external_" } else { "" }, snake.to_lowercase()))
}

fn shape_of(element_type: &str) -> Option<&'static str> {
    let base = element_type.strip_suffix("_Ext").unwrap_or(element_type);
    if base.ends_with("Db") {
        Some("database")
    } else if base.ends_with("Queue") {
        Some("queue")
    } else {
        None
    }
}

fn outline(shape: &str, left: i64, top: i64, w: i64, h: i64) -> Option<(String, String)> {
    let f = super::f;
    let (l, t, w, h) = (left as f64, top as f64, w as f64, h as f64);
    match shape {
        "database" => {
            let half = f(w / 2.0);
            let hh = f(h);
            Some((
                format!("M{},{}c0,-10 {half},-10 {half},-10c0,0 {half},0 {half},10l0,{hh}c0,10 -{half},10 -{half},10c0,0 -{half},0 -{half},-10l0,-{hh}", f(l), f(t)),
                format!("M{},{}c0,10 {half},10 {half},10c0,0 {half},0 {half},-10", f(l), f(t)),
            ))
        }
        "queue" => {
            let half = f(h / 2.0);
            let w_ = f(w);
            Some((
                format!("M{},{}l{w_},0c5,0 5,{half} 5,{half}c0,0 0,{half} -5,{half}l-{w_},0c-5,0 -5,-{half} -5,-{half}c0,0 0,-{half} 5,-{half}", f(l), f(t)),
                format!("M{},{}c-5,0 -5,{half} -5,{half}c0,{half} 5,{half} 5,{half}", f(l + w), f(t)),
            ))
        }
        _ => None,
    }
}

fn element_node(e: &Element, s: &Sizes) -> Placed {
    let base = e.element_type.strip_suffix("_Ext").unwrap_or(&e.element_type);
    let person = base == "Person";
    let container = matches!(base, "Container" | "ContainerDb" | "ContainerQueue");
    let component = base == "Component";
    let label = e.label.clone().unwrap_or_default();
    let technology = e.technology.clone().unwrap_or_default();
    let description = e.description.clone().unwrap_or_default();

    // C4ShapeMetrics
    let stereotype_offset = 20;
    let mut offset = 20 + s.small + 2 - 4;
    let mut image_offset = None;
    if person {
        image_offset = Some(offset);
        offset += 48;
    }
    let label_offset = offset + 8;
    offset = label_offset + tm::height(&label, s.large);
    let mut technology_offset = None;
    if !technology.is_empty() {
        let o = offset + 5;
        technology_offset = Some(o);
        offset = o + tm::height(&bracket(&technology), s.normal);
    }
    let mut rect_width = tm::width(&label, s.large);
    let mut rect_height = offset;
    let mut description_offset = None;
    if !description.is_empty() {
        rect_width = rect_width.max(tm::width(&description, s.normal));
        let o = offset + 20;
        description_offset = Some(o);
        rect_height = o + tm::height(&description, s.normal) - (tm::lines(&description).len() as i64 * 5);
    }
    let width = (rect_width + 20).max(216);
    let height = rect_height.max(60);

    let mut labels = label_lines(&label, Some(label_offset), s.large, "bold", "normal", None);
    if labels.is_empty() {
        labels.push(Line { text: String::new(), offset: label_offset as f64, font_size: s.large, weight: "bold", style: "normal", anchor: None });
    }
    labels.extend(label_lines(&bracket(&technology), technology_offset, s.normal, "normal", "italic", Some("middle")));
    labels.extend(label_lines(&description, description_offset, s.normal, "normal", "normal", Some("middle")));

    let kind = if person {
        "person"
    } else if container {
        "container"
    } else if component {
        "component"
    } else {
        "system"
    };
    Placed {
        id: e.id.clone(),
        kind,
        external: e.external,
        x: 0,
        y: 0,
        w: width,
        h: height,
        labels,
        stereotype: stereotype_text(&e.element_type),
        stereotype_offset,
        image_offset,
        outline: None,
        header: 0,
        children: vec![],
    }
    .with_shape(shape_of(&e.element_type))
}

impl Placed {
    fn with_shape(self, shape: Option<&'static str>) -> Placed {
        // The outline needs the placed box; it is filled in after placement.
        let _ = shape;
        self
    }
}

fn boundary_type_text(b: &Boundary) -> String {
    match b.boundary_type.as_str() {
        "Enterprise_Boundary" => "ENTERPRISE".into(),
        "System_Boundary" => "SYSTEM".into(),
        "Container_Boundary" => "CONTAINER".into(),
        _ => match &b.type_param {
            Some(t) if !t.is_empty() => t.clone(),
            _ => "system".into(),
        },
    }
}

fn boundary_node(d: &Diagram, b: &Boundary, s: &Sizes) -> Placed {
    let label = b.label.clone().unwrap_or_default();
    let type_text = boundary_type_text(b);
    // C4BoundaryMetrics
    let label_offset = 8;
    let after_label = label_offset + tm::height(&label, s.large);
    let type_offset = after_label + 5;
    let header = type_offset + tm::height(&format!("[{type_text}]"), s.normal);
    let mut labels = label_lines(&label, Some(label_offset), s.large, "bold", "normal", None);
    labels.extend(label_lines(&bracket(&type_text), Some(type_offset), s.normal, "normal", "normal", Some("middle")));
    let mut children: Vec<Placed> =
        d.boundaries.iter().filter(|c| c.parent_id.as_deref() == Some(b.id.as_str())).map(|c| boundary_node(d, c, s)).collect();
    children.extend(d.elements.iter().filter(|e| e.boundary_id.as_deref() == Some(b.id.as_str())).map(|e| element_node(e, s)));
    Placed {
        id: b.id.clone(),
        kind: "boundary",
        external: false,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        labels,
        stereotype: None,
        stereotype_offset: 0,
        image_offset: None,
        outline: None,
        header,
        children,
    }
}

/// Root nodes: root boundaries, then root elements.
pub fn build_nodes(d: &Diagram, s: &Sizes) -> Option<Vec<Placed>> {
    let mut out: Vec<Placed> = d.boundaries.iter().filter(|b| b.parent_id.is_none()).map(|b| boundary_node(d, b, s)).collect();
    out.extend(d.elements.iter().filter(|e| e.boundary_id.is_none()).map(|e| element_node(e, s)));
    Some(out)
}

// ---- placement ----

#[derive(Default, Clone, Copy)]
struct Extent {
    startx: i64,
    stopx: i64,
    starty: i64,
    stopy: i64,
}

impl Extent {
    fn reset(&mut self, x: i64, y: i64) {
        *self = Extent { startx: x, stopx: x, starty: y, stopy: y };
    }
    fn cover(&mut self, l: i64, t: i64, r: i64, b: i64) {
        self.startx = self.startx.min(l);
        self.starty = self.starty.min(t);
        self.stopx = self.stopx.max(r);
        self.stopy = self.stopy.max(b);
    }
}

struct Bounds {
    width_limit: f64,
    per_row: i64,
    data: Extent,
    upcoming: Extent,
    count: i64,
}

impl Bounds {
    fn new(width_limit: f64, per_row: i64) -> Self {
        Bounds { width_limit, per_row, data: Extent::default(), upcoming: Extent::default(), count: 0 }
    }
    fn start_at(&mut self, x: i64, y: i64) {
        self.data.reset(x, y);
        self.upcoming.reset(x, y);
    }
    fn insert(&mut self, n: &mut Placed) {
        self.count += 1;
        let (left, top, right) = self.position_for(n);
        n.x = left;
        n.y = top;
        self.data.cover(left, top, right, top + n.h);
        self.upcoming.cover(left, top, right, top + n.h);
    }
    fn bump_last_margin(&mut self) {
        self.data.stopx += MARGIN;
        self.data.stopy += MARGIN;
    }
    fn absorb(&mut self, child: &Bounds) {
        self.data.stopy = (child.data.stopy + MARGIN).max(self.data.stopy);
        self.data.stopx = (child.data.stopx + MARGIN).max(self.data.stopx);
    }
    fn position_for(&mut self, n: &Placed) -> (i64, i64, i64) {
        let gap = if self.upcoming.startx == self.upcoming.stopx { MARGIN } else { MARGIN * 2 };
        let (left, top) = (self.upcoming.stopx + gap, self.upcoming.starty + MARGIN * 2);
        let right = left + n.w;
        if left as f64 >= self.width_limit || right as f64 >= self.width_limit || self.count > self.per_row {
            let left = self.upcoming.startx + MARGIN;
            let top = self.upcoming.stopy + MARGIN * 2;
            self.upcoming.stopx = left + n.w;
            self.upcoming.starty = self.upcoming.stopy;
            self.upcoming.stopy = top + n.h;
            self.count = 1;
            return (left, top, self.upcoming.stopx);
        }
        (left, top, right)
    }
}

struct Placer {
    shapes_in_row: i64,
    boundaries_in_row: i64,
    max_x: i64,
    max_y: i64,
}

fn row_limit(config: &str, name: &str, default: i64) -> i64 {
    let re = Regex::new(&format!(r"(?i){name}InRow\D*(\d+)")).unwrap();
    let v = re.captures(config).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(0);
    if v >= 1 { v } else { default }
}

impl Placer {
    fn place_boundaries(&mut self, parent: &mut Bounds, nodes: &mut [&mut Placed], global: bool) {
        let across = self.boundaries_in_row.min(nodes.len() as i64);
        let mut bounds = Bounds::new(parent.width_limit / across as f64, self.shapes_in_row);
        for (index, node) in nodes.iter_mut().enumerate() {
            if index as i64 % self.boundaries_in_row == 0 {
                bounds.start_at(parent.data.startx + MARGIN_X, parent.data.stopy + MARGIN_Y + node.header);
            } else {
                let d = bounds.data;
                let x = if d.stopx == d.startx { d.startx } else { d.stopx + MARGIN_X };
                bounds.start_at(x, d.starty);
            }
            self.place_contents(&mut bounds, node);
            if !(global && index == 0 && node.kind == "global") {
                let d = bounds.data;
                node.x = d.startx;
                node.y = d.starty;
                node.w = d.stopx - d.startx;
                node.h = d.stopy - d.starty;
            }
            parent.absorb(&bounds);
            self.max_x = self.max_x.max(parent.data.stopx);
            self.max_y = self.max_y.max(parent.data.stopy);
        }
    }

    fn place_contents(&mut self, bounds: &mut Bounds, node: &mut Placed) {
        let mut any_box = false;
        for c in node.children.iter_mut().filter(|c| c.kind != "boundary") {
            bounds.insert(c);
            any_box = true;
        }
        if any_box {
            bounds.bump_last_margin();
        }
        let mut bs: Vec<&mut Placed> = node.children.iter_mut().filter(|c| c.kind == "boundary").collect();
        if !bs.is_empty() {
            self.place_boundaries(bounds, &mut bs, false);
        }
    }
}

pub fn place(d: &Diagram, nodes: &mut Vec<Placed>) -> Canvas {
    let config = d.layout_config.clone().unwrap_or_default();
    let mut p = Placer {
        shapes_in_row: row_limit(&config, "Shape", 4),
        boundaries_in_row: row_limit(&config, "Boundary", 2),
        max_x: MARGIN_X,
        max_y: MARGIN_Y,
    };
    let mut screen = Bounds::new(AVAILABLE_WIDTH, p.shapes_in_row);
    screen.start_at(MARGIN_X, MARGIN_Y);
    let s = Sizes { large: 16, normal: 14, small: 12 };
    let _ = &s;
    // global boundary header: C4BoundaryMetrics(label "global", type "global") at 16/14
    let header = 8 + tm::height("global", 16) + 5 + tm::height("[global]", 14);
    let children = std::mem::take(nodes);
    let mut global = Placed {
        id: String::new(),
        kind: "global",
        external: false,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        labels: vec![],
        stereotype: None,
        stereotype_offset: 0,
        image_offset: None,
        outline: None,
        header,
        children,
    };
    p.place_boundaries(&mut screen, &mut [&mut global], true);
    *nodes = global.children;
    // outlines need the placed boxes
    fn shapes(nodes: &mut [Placed], d: &Diagram) {
        for n in nodes.iter_mut() {
            if n.kind != "boundary" {
                if let Some(e) = d.elements.iter().find(|e| e.id == n.id) {
                    if let Some(shape) = shape_of(&e.element_type) {
                        n.outline = outline(shape, n.x, n.y, n.w, n.h);
                    }
                }
            }
            shapes(&mut n.children, d);
        }
    }
    shapes(nodes, d);
    Canvas {
        width: p.max_x - MARGIN_X + 2 * MARGIN_X,
        height: p.max_y - MARGIN_Y + 2 * MARGIN_Y,
        title_x: ((p.max_x - MARGIN_X) as f64 / 2.0) - (4 * MARGIN_X) as f64,
    }
}
