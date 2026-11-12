//! Treemap renderer: port of Ruby `Parser::Native::Builders::Treemap`,
//! `Parser::Builders::Treemap#build_diagram`, the IR adapter, `Layout::Treemap*`
//! (d3-style squarified tiling) and `Renderer::Treemap`.
use super::pie::{fsum, is_slice, items, strip, text_of, to_f};
use crate::svg::{Document, Elem, Theme};
use crate::Error;
use regex::Regex;
use serde_json::Value;

const CANVAS_WIDTH: f64 = 1000.0;
const CANVAS_HEIGHT: f64 = 400.0;
const INNER: f64 = 10.0;
const HEADER: f64 = 25.0;

/// Ruby `capture.to_s` for a slice or an empty capture.
fn to_s(v: &Value) -> Result<String, Error> {
    match v {
        Value::Array(a) if a.is_empty() => Ok("[]".into()),
        v if is_slice(v) => Ok(v["value"].as_str().unwrap_or("").to_string()),
        _ => Err(Error::Unported("treemap".into())),
    }
}

// ---- parse ----

struct Node {
    label: String,
    value: Option<f64>,
    css_class: Option<String>,
    children: Vec<Node>,
}

#[derive(Default)]
struct Diagram {
    title: Option<String>,
    roots: Vec<Node>,
    /// classDef name -> styles, first-insertion order, last value wins.
    class_defs: Vec<(String, String)>,
}

fn attach(roots: &mut Vec<Node>, path: &[usize], node: Node) {
    let mut list = roots;
    for &i in path {
        list = &mut list[i].children;
    }
    list.push(node);
}

fn build(tree: &Value) -> Result<Diagram, Error> {
    let mut d = Diagram::default();
    // (indent, path of child indices from the roots)
    let mut stack: Vec<(usize, Vec<usize>)> = vec![];
    if let Some(s) = tree.get("statements") {
        for item in items(s) {
            if let Some(t) = item.get("title") {
                d.title = Some(to_s(t)?);
            } else if let Some(c) = item.get("class_def") {
                let (name, styles) = (to_s(&c["class_name"])?, to_s(&c["class_styles"])?);
                match d.class_defs.iter_mut().find(|(n, _)| *n == name) {
                    Some(slot) => slot.1 = styles,
                    None => d.class_defs.push((name, styles)),
                }
            } else if let Some(n) = item.get("node") {
                let indent = text_of(&n["indent"]).chars().count();
                let node = Node {
                    label: text_of(&n["label"]["string"]),
                    value: n.get("value").map(|v| to_s(v).map(|s| to_f(&s))).transpose()?,
                    css_class: n.get("css_class").map(to_s).transpose()?,
                    children: vec![],
                };
                while stack.last().is_some_and(|(i, _)| *i >= indent) {
                    stack.pop();
                }
                let path = match stack.last() {
                    None => {
                        d.roots.push(node);
                        vec![d.roots.len() - 1]
                    }
                    Some((_, parent)) => {
                        let mut path = parent.clone();
                        let mut list = &mut d.roots;
                        for &i in &path {
                            list = &mut list[i].children;
                        }
                        let n = list.len();
                        attach(&mut d.roots, &path, node);
                        path.push(n);
                        path
                    }
                };
                stack.push((indent, path));
            }
        }
    }
    Ok(d)
}

// ---- layout ----

struct TNode {
    label: String,
    value: f64,
    css_class: Option<String>,
    children: Vec<TNode>,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

/// `partition_value`: the explicit value, else the Kahan sum of the children's.
fn partition_value(n: &Node) -> f64 {
    match n.value {
        Some(v) => v,
        None => fsum(n.children.iter().map(partition_value)),
    }
}

fn tree_nodes(nodes: &[Node]) -> Vec<TNode> {
    let mut out: Vec<(usize, TNode)> = nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let value = partition_value(n);
            if value < 0.0 {
                return None;
            }
            Some((
                i,
                TNode {
                    label: n.label.clone(),
                    value,
                    css_class: n.css_class.clone(),
                    children: tree_nodes(&n.children),
                    x0: 0.0,
                    y0: 0.0,
                    x1: 0.0,
                    y1: 0.0,
                },
            ))
        })
        .collect();
    out.sort_by(|a, b| (-a.1.value).partial_cmp(&(-b.1.value)).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    out.into_iter().map(|(_, n)| n).collect()
}

fn squarify(nodes: &mut [TNode], value: f64, bounds: [f64; 4]) {
    let ratio = (1.0 + 5.0f64.sqrt()) / 2.0;
    let (mut x0, mut y0, x1, y1) = (bounds[0], bounds[1], bounds[2], bounds[3]);
    let mut value = value;
    let positive: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].value > 0.0).collect();
    let zeros: Vec<usize> = (0..nodes.len()).filter(|&i| !(nodes[i].value > 0.0)).collect();

    let worst = |nodes: &[TNode], row: &[usize], alpha: f64| -> f64 {
        let values: Vec<f64> = row.iter().map(|&i| nodes[i].value).collect();
        let s = fsum(values.iter().copied());
        let beta = (s * s) * alpha;
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        (max / beta).max(beta / min)
    };

    let mut index = 0;
    while index < positive.len() {
        let width = x1 - x0;
        let height = y1 - y0;
        let alpha = (height / width).max(width / height) / (value * ratio);
        let mut stop = index + 1;
        while stop < positive.len() {
            let before = worst(nodes, &positive[index..stop], alpha);
            if worst(nodes, &positive[index..=stop], alpha) <= before {
                stop += 1;
            } else {
                break;
            }
        }
        let row = &positive[index..stop];
        let sum = fsum(row.iter().map(|&i| nodes[i].value));
        if (x1 - x0) < (y1 - y0) {
            let (bx0, by0, bx1) = (x0, y0, x1);
            y0 += (y1 - y0) * sum / value;
            let by1 = y0;
            let scale = (bx1 - bx0) / sum;
            let mut cx = bx0;
            for &i in row {
                nodes[i].x0 = cx;
                nodes[i].y0 = by0;
                nodes[i].y1 = by1;
                cx += nodes[i].value * scale;
                nodes[i].x1 = cx;
            }
        } else {
            let (bx0, by0, by1) = (x0, y0, y1);
            x0 += (x1 - x0) * sum / value;
            let bx1 = x0;
            let scale = (by1 - by0) / sum;
            let mut cy = by0;
            for &i in row {
                nodes[i].x0 = bx0;
                nodes[i].x1 = bx1;
                nodes[i].y0 = cy;
                cy += nodes[i].value * scale;
                nodes[i].y1 = cy;
            }
        }
        value -= sum;
        index = stop;
    }
    for &i in &zeros {
        nodes[i].x0 = x0;
        nodes[i].y0 = y0;
        nodes[i].x1 = x0;
        nodes[i].y1 = y1;
    }
}

fn collapse(low: f64, high: f64) -> (f64, f64) {
    if high < low {
        let m = (low + high) / 2.0;
        (m, m)
    } else {
        (low, high)
    }
}

fn place(node: &mut TNode, inset: f64) {
    (node.x0, node.x1) = collapse(node.x0 + inset, node.x1 - inset);
    (node.y0, node.y1) = collapse(node.y0 + inset, node.y1 - inset);
    if node.children.is_empty() {
        return;
    }
    let half = INNER / 2.0;
    let bounds = [node.x0 + half, node.y0 + HEADER + half, node.x1 - half, node.y1 - half];
    squarify(&mut node.children, node.value, bounds);
    for child in &mut node.children {
        place(child, INNER / 2.0);
    }
}

fn round_all(node: &mut TNode) {
    for e in [&mut node.x0, &mut node.y0, &mut node.x1, &mut node.y1] {
        *e = (*e + 0.5).floor();
    }
    for c in &mut node.children {
        round_all(c);
    }
}

// ---- cells & render ----

struct Palette {
    fill: Option<String>,
    stroke: Option<String>,
}

/// `format_value`: integral -> integer text, else one decimal.
fn format_value(v: f64) -> String {
    if v == v.trunc() {
        if v == 0.0 { "0".into() } else { format!("{v:.0}") }
    } else {
        format!("{v:.1}")
    }
}

fn section_total(n: &TNode) -> Option<f64> {
    let values: Vec<f64> = n
        .children
        .iter()
        .filter_map(|c| if c.children.is_empty() { Some(c.value) } else { section_total(c) })
        .collect();
    if values.is_empty() { None } else { Some(fsum(values)) }
}

fn style_value(classes: &[(String, Palette)], css: &Option<String>, stroke: bool) -> Option<String> {
    let name = css.as_ref()?;
    let (_, p) = classes.iter().find(|(n, _)| n == name)?;
    if stroke { p.stroke.clone() } else { p.fill.clone() }
}

fn text_el(theme: Theme, x: f64, y: f64, text: &str, size: &str, weight: Option<&str>, anchor: Option<&str>) -> Elem {
    let mut e = Elem::new("text").attr("fill", theme.color("label_text").unwrap_or("")).num4("x", x).num4("y", y);
    if let Some(a) = anchor {
        e = e.attr("text-anchor", a);
    }
    e = e
        .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"))
        .attr("font-size", size);
    if let Some(w) = weight {
        e = e.attr("font-weight", w);
    }
    e.content(text)
}

fn cell(n: &TNode, depth: usize, classes: &[(String, Palette)], theme: Theme) -> Elem {
    let (x, y) = (n.x0, n.y0);
    let (width, height) = (n.x1 - n.x0, n.y1 - n.y0);
    let label_y = y + 15.0;
    let fill = style_value(classes, &n.css_class, false)
        .unwrap_or_else(|| theme.palette("treemap_palette", depth).unwrap_or("").to_string());
    let stroke = style_value(classes, &n.css_class, true)
        .unwrap_or_else(|| theme.color("node_stroke").unwrap_or("#333").to_string());
    let mut g = Elem::new("g").child(
        Elem::new("rect")
            .attr("fill", fill)
            .attr("stroke", stroke)
            .attr("stroke-width", "2")
            .num4("x", x)
            .num4("y", y)
            .num4("width", width)
            .num4("height", height)
            .num4("rx", 4.0)
            .num4("ry", 4.0),
    );
    g.push(text_el(theme, x + 5.0, label_y, &n.label, "12", Some("bold"), None));
    if n.children.is_empty() {
        g.push(text_el(theme, x + 5.0, label_y + 15.0, &format_value(n.value), "10", None, None));
    } else if let Some(total) = section_total(n) {
        g.push(text_el(theme, x + width - 5.0, label_y, &format_value(total), "10", None, Some("end")));
    }
    for c in &n.children {
        g.push(cell(c, depth + 1, classes, theme));
    }
    g
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "treemap")?;
    let d = build(&tree)?;
    let theme = Theme::default_theme();

    // class_definitions: fill / stroke colours per classDef name.
    let (fill_re, stroke_re) = (
        Regex::new(r"fill:[ \t\r\n\x0c\x0b]*([^;,]+)").unwrap(),
        Regex::new(r"stroke:[ \t\r\n\x0c\x0b]*([^;,]+)").unwrap(),
    );
    let grab = |re: &Regex, s: &str| re.captures(s).map(|c| strip(&c[1]).to_string());
    let mut classes: Vec<(String, Palette)> = vec![];
    for (name, styles) in &d.class_defs {
        let p = Palette { fill: grab(&fill_re, styles), stroke: grab(&stroke_re, styles) };
        if p.fill.is_some() || p.stroke.is_some() {
            classes.push((name.clone(), p));
        }
    }

    let total = fsum(d.roots.iter().map(partition_value));
    let mut doc = Document::new(CANVAS_WIDTH, CANVAS_HEIGHT);
    doc.view_box = Some("0 0 1000 400".into());
    if let Some(title) = &d.title {
        doc.add(text_el(theme, CANVAS_WIDTH / 2.0, 25.0, title, "18", Some("bold"), Some("middle")));
    }
    if total > 0.0 {
        let mut root = TNode {
            label: String::new(),
            value: total,
            css_class: None,
            children: tree_nodes(&d.roots),
            x0: 0.0,
            y0: 0.0,
            x1: CANVAS_WIDTH,
            y1: CANVAS_HEIGHT,
        };
        place(&mut root, 0.0);
        round_all(&mut root);
        for c in &root.children {
            doc.add(cell(c, 0, &classes, theme));
        }
    }
    Ok(doc.to_xml())
}
