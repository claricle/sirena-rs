//! C4 renderer: port of `Parser::Builders::C4` (model rules),
//! `IRAdapters::C4`, `Layout::C4` (+ `C4Placement`, `C4Bounds`, `C4Text`,
//! `C4ShapeMetrics`, `C4ShapeOutline`, `C4Intersection`) and `Renderer::C4`.
//!
//! Declined to the Ruby path (`Error::Unported`): sources with a
//! frontmatter/directive preamble, elements/boundaries without an id, and a
//! relationship without a label (Ruby raises there).
use super::pie::{is_slice, strip, text_of};
use crate::svg::{round4, ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use regex::Regex;
use serde_json::{Map, Value};

mod layout;
mod model;
mod text_metrics;


use layout::{place, Placed};

use model::{build, Diagram};

fn unported() -> Error {
    Error::Unported("c4".into())
}

/// Ruby `Float#to_s` for a number printed in a Ruby string (`"#{x}"`).
fn f(x: f64) -> String {
    ruby_float(x)
}

const MARGIN_Y: i64 = 10;
const MIDDLE_BASELINE_EMS: f64 = 0.35;

struct Colours {
    bg: String,
    border: String,
    text: String,
}

struct Label {
    text: String,
    x: f64,
    y: f64,
    font_size: f64,
    weight: Option<&'static str>,
    style: Option<&'static str>,
    anchor: Option<&'static str>,
}

fn label_elem(l: &Label, fill: &str, font_family: &str) -> Elem {
    let mut e = Elem::new("text").num("x", l.x).num("y", l.y).content(&l.text).attr("fill", fill).attr("font-family", font_family).num("font-size", l.font_size);
    if let Some(w) = l.weight {
        e = e.attr("font-weight", w);
    }
    if let Some(s) = l.style {
        e = e.attr("font-style", s);
    }
    if let Some(a) = l.anchor {
        e = e.attr("text-anchor", a);
    }
    e
}

fn middle_baseline_y(y: f64, font_size: f64) -> f64 {
    round4(y + font_size * MIDDLE_BASELINE_EMS)
}

// ---- typed scene ----

struct Pt {
    x: f64,
    y: f64,
}

struct SceneEdge {
    id: String,
    start: Pt,
    end: Pt,
    control: Option<Pt>,
    heads: Vec<String>,
    labels: Vec<Label>,
}

fn centre(n: &Placed) -> (f64, f64) {
    (n.x as f64 + n.w as f64 / 2.0, n.y as f64 + n.h as f64 / 2.0)
}

fn vertical_exit_y(n: &Placed, y: f64) -> f64 {
    if (n.y as f64) < y { (n.y + n.h) as f64 } else { n.y as f64 }
}

fn leave(n: &Placed, target: (f64, f64)) -> (f64, f64) {
    let (tx, ty) = target;
    let (cx, cy) = centre(n);
    if n.y as f64 == ty {
        let x = if (n.x as f64) < tx { (n.x + n.w) as f64 } else { n.x as f64 };
        return (x, cy);
    }
    if n.x as f64 == tx {
        return (cx, vertical_exit_y(n, ty));
    }
    let dx = (n.x as f64 - tx).abs();
    let dy = (n.y as f64 - ty).abs();
    let slope = dy / dx;
    if n.h as f64 / n.w as f64 >= slope {
        let sign = if (n.y as f64) < ty { 1.0 } else { -1.0 };
        let side_x = if n.x as f64 > tx { n.x as f64 } else { (n.x + n.w) as f64 };
        (side_x, cy + (sign * slope * n.w as f64) / 2.0)
    } else {
        let ratio = dx / dy;
        let sign = if n.x as f64 > tx { -1.0 } else { 1.0 };
        (cx + (sign * ratio * n.h as f64) / 2.0, vertical_exit_y(n, ty))
    }
}

fn arrowhead(from: &Pt, tip: &Pt) -> Option<String> {
    let run = tip.x - from.x;
    let rise = tip.y - from.y;
    let length = crate::js_math::hypot2(run, rise);
    if length == 0.0 {
        return None;
    }
    let (u0, u1) = (run / length, rise / length);
    let (b0, b1) = (tip.x - u0 * 8.0, tip.y - u1 * 8.0);
    let (w0, w1) = ((-u1 * 8.0) / 2.0, (u0 * 8.0) / 2.0);
    let pts = [(tip.x, tip.y), (b0 + w0, b1 + w1), (b0 + -w0, b1 + -w1)];
    Some(pts.iter().map(|(a, b)| format!("{},{}", f(*a), f(*b))).collect::<Vec<_>>().join(" "))
}

pub fn render(source: &str) -> Result<String, Error> {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    if !strip(preamble).is_empty() {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "c4")?;
    let diagram: Diagram = build(&tree).ok_or_else(unported)?;
    let theme = Theme::default_theme();
    let size = |k: &str, d: i64| theme.number("typography", k).filter(|v| *v > 0.0 && v.fract() == 0.0).map_or(d, |v| v as i64);
    let (large, normal, small) = (size("font_size_large", 16), size("font_size_normal", 14), size("font_size_small", 12));
    let sizes = text_metrics::Sizes { large, normal, small };

    let mut placed = layout::build_nodes(&diagram, &sizes).ok_or_else(unported)?;
    let canvas = place(&diagram, &mut placed);

    let title = diagram.title.clone().filter(|t| !t.is_empty());
    let title_h = if title.is_some() { 60 } else { 0 };
    let width = canvas.width;
    let height = canvas.height + title_h;
    let view_box = format!("0 {} {} {}", -(MARGIN_Y + title_h), width, height);
    let mut doc = Document::new(width as f64, height as f64);
    doc.view_box = Some(view_box);

    let col = |k: &str| theme.color(k).unwrap_or("").to_string();
    let ff = "Arial, sans-serif";
    let colours = |n: &Placed| -> Colours {
        let (bg, border, text) = match (n.kind, n.external) {
            ("person", false) | ("system", false) | ("container", _) => ("primary", "edge_stroke", "background"),
            ("person", true) | ("system", true) => ("secondary", "edge_stroke", "background"),
            ("component", _) => ("surface_variant", "node_stroke", "foreground"),
            _ => ("background", "node_stroke", "foreground"),
        };
        Colours { bg: col(bg), border: col(border), text: col(text) }
    };

    fn rect_box(n: &Placed) -> Elem {
        Elem::new("rect").num("x", n.x as f64).num("y", n.y as f64).num("width", n.w as f64).num("height", n.h as f64)
    }
    let radius = |n: &Placed| if n.kind == "boundary" { 8.0 } else if n.kind == "person" { 10.0 } else { 5.0 };

    // boundaries
    fn boundaries<'a>(nodes: &'a [Placed], out: &mut Vec<&'a Placed>) {
        for n in nodes {
            if n.kind == "boundary" {
                out.push(n);
                boundaries(&n.children, out);
            }
        }
    }
    let mut bs = vec![];
    boundaries(&placed, &mut bs);
    for n in bs {
        let c = colours(n);
        let mut g = Elem::new("g").attr("id", format!("boundary-{}", n.id));
        let r = radius(n);
        g.push(
            rect_box(n)
                .attr("fill", c.bg.clone())
                .attr("stroke", c.border.clone())
                .attr("stroke-width", "2")
                .num("rx", r)
                .num("ry", r)
                .attr("stroke-dasharray", "10,5"),
        );
        if let Some(l) = n.labels.first() {
            g.push(label_elem(&node_label(l, n), &c.text, ff));
        }
        doc.add(g);
    }

    // relationships
    let mut index: Vec<(&str, &Placed)> = vec![];
    fn index_nodes<'a>(nodes: &'a [Placed], out: &mut Vec<(&'a str, &'a Placed)>) {
        for n in nodes {
            if n.kind != "boundary" {
                match out.iter_mut().find(|(k, _)| *k == n.id) {
                    Some(slot) => slot.1 = n,
                    None => out.push((n.id.as_str(), n)),
                }
            }
            index_nodes(&n.children, out);
        }
    }
    index_nodes(&placed, &mut index);
    let find = |id: &Option<String>| id.as_deref().and_then(|i| index.iter().find(|(k, _)| *k == i)).map(|(_, n)| *n);
    let edge_stroke = col("edge_stroke");
    let mut scene_edges: Vec<SceneEdge> = vec![];
    for (position, rel) in diagram.relationships.iter().enumerate() {
        let (Some(source), Some(target)) = (find(&rel.from), find(&rel.to)) else { continue };
        let (s, t) = (leave(source, centre(target)), leave(target, centre(source)));
        let (start, end) = (Pt { x: s.0, y: s.1 }, Pt { x: t.0, y: t.1 });
        let control = (position != 0).then(|| Pt { x: start.x + (end.x - start.x) / 4.0, y: start.y + (end.y - start.y) / 2.0 });
        let mut heads = vec![];
        heads.extend(arrowhead(control.as_ref().unwrap_or(&start), &end));
        if rel.rel_type == "BiRel" {
            heads.extend(arrowhead(control.as_ref().unwrap_or(&end), &start));
        }
        let Some(label) = rel.label.clone() else { return Err(unported()) };
        let tech = rel.technology.clone().filter(|t| !t.is_empty()).map(|t| format!("[{t}]"));
        let texts: Vec<String> = [Some(label), tech].into_iter().flatten().filter(|t| !t.is_empty()).collect();
        let labels = texts
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let middle = (start.y + end.y) / 2.0 + (i as i64 * (small + 5)) as f64;
                Label {
                    text,
                    x: (start.x + end.x) / 2.0,
                    y: middle_baseline_y(middle, small as f64),
                    font_size: small as f64,
                    weight: None,
                    style: Some(if i == 0 { "normal" } else { "italic" }),
                    anchor: Some("middle"),
                }
            })
            .collect();
        scene_edges.push(SceneEdge { id: format!("rel_{position}"), start, end, control, heads, labels });
    }
    for e in &scene_edges {
        let mut g = Elem::new("g").attr("id", e.id.clone());
        match &e.control {
            Some(c) => g.push(
                Elem::new("path")
                    .attr("fill", "none")
                    .attr("stroke", edge_stroke.clone())
                    .attr("stroke-width", "2")
                    .attr("d", format!("M{},{} Q{},{} {},{}", f(e.start.x), f(e.start.y), f(c.x), f(c.y), f(e.end.x), f(e.end.y))),
            ),
            None => g.push(
                Elem::new("line")
                    .attr("stroke", edge_stroke.clone())
                    .attr("stroke-width", "2")
                    .num("x1", e.start.x)
                    .num("y1", e.start.y)
                    .num("x2", e.end.x)
                    .num("y2", e.end.y),
            ),
        }
        for h in &e.heads {
            g.push(Elem::new("polygon").attr("fill", edge_stroke.clone()).attr("stroke", edge_stroke.clone()).attr("points", h.clone()));
        }
        for l in &e.labels {
            g.push(label_elem(l, &col("label_text"), ff));
        }
        doc.add(g);
    }

    // elements
    fn elements<'a>(nodes: &'a [Placed], out: &mut Vec<&'a Placed>) {
        for n in nodes {
            if n.kind == "boundary" {
                elements(&n.children, out);
            } else {
                out.push(n);
            }
        }
    }
    let mut es = vec![];
    elements(&placed, &mut es);
    for n in es {
        let c = colours(n);
        let mut g = Elem::new("g").attr("id", format!("element-{}", n.id));
        match &n.outline {
            Some((body, rim)) => {
                let mut shape = Elem::new("g");
                for (d, fill) in [(body, c.bg.clone()), (rim, "none".to_string())] {
                    shape.push(Elem::new("path").attr("d", d.clone()).attr("fill", fill).attr("stroke", c.border.clone()).attr("stroke-width", "2"));
                }
                g.push(shape);
            }
            None => {
                let r = radius(n);
                g.push(rect_box(n).attr("fill", c.bg.clone()).attr("stroke", c.border.clone()).attr("stroke-width", "2").num("rx", r).num("ry", r));
            }
        }
        if n.kind == "person" {
            let cx = n.x as f64 + n.w as f64 / 2.0;
            let cy = (n.y + n.image_offset.unwrap_or(0)) as f64 + 14.0;
            g.push(Elem::new("circle").attr("fill", c.text.clone()).num("cx", cx).num("cy", cy).num("r", 10.0));
            g.push(Elem::new("ellipse").attr("fill", c.text.clone()).num("cx", cx).num("cy", cy + 22.0).num("rx", 18.0).num("ry", 12.0));
        }
        if let Some(text) = &n.stereotype {
            let l = Label {
                text: text.clone(),
                x: n.x as f64 + n.w as f64 / 2.0,
                y: (n.y + n.stereotype_offset) as f64,
                font_size: small as f64,
                weight: Some("normal"),
                style: Some("italic"),
                anchor: Some("middle"),
            };
            g.push(label_elem(&l, &c.text, ff));
        }
        for l in &n.labels {
            g.push(label_elem(&node_label(l, n), &c.text, ff));
        }
        doc.add(g);
    }

    if let Some(t) = title {
        let _ = text::measure(&t, large as f64, Face::Arial);
        let l = Label { text: t, x: canvas.title_x, y: (MARGIN_Y * 2) as f64, font_size: large as f64, weight: Some("bold"), style: None, anchor: None };
        doc.add(label_elem(&l, &col("foreground"), ff));
    }
    Ok(doc.to_xml())
}

fn node_label(l: &layout::Line, n: &Placed) -> Label {
    Label {
        text: l.text.clone(),
        x: n.x as f64 + n.w as f64 / 2.0,
        y: middle_baseline_y((n.y as f64) + l.offset, l.font_size as f64),
        font_size: l.font_size as f64,
        weight: Some(l.weight),
        style: Some(l.style),
        anchor: l.anchor,
    }
}

// shared tree helpers
pub(super) fn present(v: Option<&Value>) -> bool {
    v.is_some_and(|v| !v.is_null())
}

pub(super) fn plain_object(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object().filter(|_| !is_slice(v))
}

pub(super) fn text(v: &Value) -> String {
    text_of(v)
}

#[allow(dead_code)]
fn _regex_used(_: &Regex) {}
