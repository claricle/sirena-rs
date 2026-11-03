//! Requirement renderer: port of Ruby `Parser::Builders::Requirement` (model
//! rules), `Layout::Requirement` (grid placement) and `Renderer::Requirement`.
//!
//! Styling statements (`style`, `classDef`, `class`) never reach the SVG in
//! Ruby, so they are not modelled. Declined to the Ruby path
//! (`Error::Unported`): diagrams where two entities share a name.
use super::pie::{fsum, items, strip, text_of};
use crate::svg::{ruby_float, ArrowPath, Arrowhead, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use serde_json::Value;

const PADDING_X: f64 = 10.0;
const TOP_PADDING: i64 = 8;
const SECTION_GAP: i64 = 20;
const BOTTOM_PADDING: i64 = 12;
const HEADER_ROWS: usize = 2;
const HEADER_HEIGHT: f64 = (TOP_PADDING + (HEADER_ROWS as i64 * 24) + SECTION_GAP / 2) as f64;
const BOLD_WIDTH_FACTOR: f64 = 1.06;
const NODE_SEPARATION: f64 = 50.0;
const RANK_SEPARATION: i64 = 74;
const CANVAS_MARGIN: i64 = 8;
const LABEL_FONT_SIZE: f64 = 16.0;
const TEXT_WRAP_WIDTH: f64 = 300.0;
const BROWSER_WIDTH_SCALE: f64 = 64.0;
const TREBUCHET_KERNING: [(&str, i32); 4] = [("Re", -38), ("Te", -119), ("Ty", -113), ("Ve", -86)];
const RISK_LEVELS: [&str; 3] = ["high", "medium", "low"];

/// A frontmatter/directive layout choice (e.g. `layout: elk`) selects another
/// placement engine, which is not ported.
fn source_layout(source: &str) -> bool {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    preamble.contains("layout")
}

fn unported() -> Error {
    Error::Unported("requirement".into())
}

// ---- model (builder) ----

#[derive(Default, Clone)]
struct Req {
    name: String,
    kind: String,
    id: Option<String>,
    text: Option<String>,
    risk: Option<String>,
    verifymethod: Option<String>,
}

#[derive(Default, Clone)]
struct Elt {
    name: String,
    kind: Option<String>,
    docref: Option<String>,
}

struct Rel {
    source: String,
    target: String,
    kind: Option<String>,
}

fn present(v: Option<&Value>) -> bool {
    v.is_some_and(|v| !v.is_null())
}

fn list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().filter(|x| x.is_object()).collect(),
        Some(v) if v.is_object() => vec![v],
        _ => vec![],
    }
}

/// `value.to_s.strip` where an empty capture (`[]`) prints as `"[]"`.
fn property_text(v: &Value) -> String {
    match v {
        Value::Array(_) => "[]".to_string(),
        v => strip(&text_of(v)).to_string(),
    }
}

fn to_s(v: &Value) -> String {
    match v {
        Value::Array(_) => "[]".to_string(),
        v => text_of(v),
    }
}

fn build(tree: &Value) -> (Vec<Req>, Vec<Elt>, Vec<Rel>) {
    let (mut reqs, mut elts, mut rels) = (vec![], vec![], vec![]);
    for stmt in items(tree.get("statements").unwrap_or(&Value::Null)) {
        if present(stmt.get("req_type")) {
            let mut r = Req {
                name: to_s(&stmt["req_name"]),
                kind: to_s(&stmt["req_type"]),
                ..Default::default()
            };
            for prop in list(stmt.get("req_properties")) {
                let key = text_of(&prop["key"]["prop_key"]);
                let value = prop.get("value").filter(|v| !v.is_null()).map(property_text);
                match key.as_str() {
                    "id" => r.id = value,
                    "text" => r.text = value,
                    "risk" => r.risk = value,
                    "verifymethod" => r.verifymethod = value,
                    _ => {}
                }
            }
            reqs.push(r);
        } else if present(stmt.get("elem_keyword")) {
            let mut e = Elt { name: to_s(&stmt["elem_name"]), ..Default::default() };
            for prop in list(stmt.get("elem_properties")) {
                let key = text_of(&prop["key"]["prop_key"]);
                let value = prop.get("value").filter(|v| !v.is_null()).map(property_text);
                match key.as_str() {
                    "type" => e.kind = value,
                    "docref" => e.docref = value,
                    _ => {}
                }
            }
            elts.push(e);
        } else if present(stmt.get("rel_source")) && present(stmt.get("rel_target")) {
            let kind = stmt.get("rel_type").and_then(|t| t.get("type")).filter(|v| !v.is_null()).map(to_s);
            rels.push(Rel { source: to_s(&stmt["rel_source"]), target: to_s(&stmt["rel_target"]), kind });
        }
    }
    (reqs, elts, rels)
}

// ---- layout ----

#[derive(Clone)]
struct Row {
    text: String,
    bold: bool,
    risk: bool,
}

fn row(text: impl Into<String>) -> Row {
    Row { text: text.into(), bold: false, risk: false }
}

fn bold(text: impl Into<String>) -> Row {
    Row { text: text.into(), bold: true, risk: false }
}

fn type_label(kind: &str) -> &str {
    match kind {
        "requirement" => "Requirement",
        "functionalRequirement" => "Functional Requirement",
        "interfaceRequirement" => "Interface Requirement",
        "performanceRequirement" => "Performance Requirement",
        "physicalRequirement" => "Physical Requirement",
        "designConstraint" => "Design Constraint",
        other => other,
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect(),
    }
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

fn wrap_text(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for word in text.split(is_space).filter(|w| !w.is_empty()) {
        let candidate = match lines.pop() {
            Some(prev) => format!("{prev} {word}"),
            None => word.to_string(),
        };
        if text::measure(&candidate, LABEL_FONT_SIZE, Face::Arial).width <= TEXT_WRAP_WIDTH {
            lines.push(candidate);
        } else {
            let (previous, current) = match candidate.rfind(' ') {
                Some(i) => (candidate[..i].to_string(), candidate[i + 1..].to_string()),
                None => (String::new(), candidate.clone()),
            };
            if !previous.is_empty() {
                lines.push(previous);
            }
            lines.push(current);
        }
    }
    lines
}

fn requirement_rows(r: &Req) -> Vec<Row> {
    let mut rows = vec![row(format!("<<{}>>", type_label(&r.kind))), bold(r.name.clone())];
    if let Some(id) = &r.id {
        rows.push(row(format!("ID: {id}")));
    }
    if let Some(t) = &r.text {
        rows.extend(wrap_text(&format!("Text: {t}")).into_iter().map(row));
    }
    if let Some(risk) = &r.risk {
        rows.push(Row { text: format!("Risk: {}", capitalize(risk)), bold: false, risk: true });
    }
    if let Some(v) = &r.verifymethod {
        rows.push(row(format!("Verification: {}", capitalize(v))));
    }
    rows
}

fn element_rows(e: &Elt) -> Vec<Row> {
    let mut rows = vec![row("<<Element>>"), bold(e.name.clone())];
    if let Some(t) = &e.kind {
        rows.push(row(format!("Type: {t}")));
    }
    if let Some(d) = &e.docref {
        rows.push(row(format!("Doc Ref: {d}")));
    }
    rows
}

fn browser_text_width(s: &str) -> f64 {
    let mut width = text::measure(s, LABEL_FONT_SIZE, Face::Trebuchet).width;
    let chars: Vec<char> = s.chars().collect();
    let mut adjustment: i64 = 0;
    for pair in chars.windows(2) {
        let key: String = pair.iter().collect();
        if let Some((_, v)) = TREBUCHET_KERNING.iter().find(|(k, _)| *k == key) {
            adjustment += i64::from(*v);
        }
    }
    width += (adjustment * 16) as f64 / 1000.0;
    (width * BROWSER_WIDTH_SCALE).round() / BROWSER_WIDTH_SCALE
}

struct Node {
    name: String,
    is_req: bool,
    risk: Option<String>,
    rows: Vec<Row>,
    width: f64,
    height: i64,
    x: f64,
    y: i64,
}

fn size_node(name: &str, is_req: bool, risk: Option<String>, rows: Vec<Row>) -> Node {
    let widest = rows
        .iter()
        .map(|r| browser_text_width(&r.text) * if r.bold { BOLD_WIDTH_FACTOR } else { 1.0 })
        .fold(f64::NEG_INFINITY, f64::max);
    let height = TOP_PADDING + SECTION_GAP + BOTTOM_PADDING + rows.len() as i64 * 24;
    Node {
        name: name.to_string(),
        is_req,
        risk,
        width: widest + 2.0 * PADDING_X,
        height,
        rows,
        x: 0.0,
        y: 0,
    }
}

/// Nodes on a cycle never resolve; they share the level below the rest.
fn node_levels(names: &[String], rels: &[Rel]) -> Vec<i64> {
    let edges: Vec<&Rel> = rels
        .iter()
        .filter(|r| names.contains(&r.source) && names.contains(&r.target) && r.source != r.target)
        .collect();
    let mut levels: Vec<Option<i64>> = vec![None; names.len()];
    let idx = |n: &str| names.iter().position(|x| x == n).unwrap();
    loop {
        let ready: Vec<usize> = (0..names.len())
            .filter(|&i| levels[i].is_none())
            .filter(|&i| edges.iter().filter(|e| e.target == names[i]).all(|e| levels[idx(&e.source)].is_some()))
            .collect();
        if ready.is_empty() {
            break;
        }
        for i in ready {
            let level = edges
                .iter()
                .filter(|e| e.target == names[i])
                .map(|e| levels[idx(&e.source)].unwrap() + 1)
                .max()
                .unwrap_or(0);
            levels[i] = Some(level);
        }
    }
    let stuck = levels.iter().flatten().max().map_or(0, |m| m + 1);
    levels.into_iter().map(|l| l.unwrap_or(stuck)).collect()
}

// ---- svg ----

fn risk_color(theme: Theme, risk: Option<&str>) -> Option<String> {
    let risk = risk?;
    let lower = risk.to_lowercase();
    match RISK_LEVELS.iter().position(|l| *l == lower) {
        None => Some(theme.color("requirement_unknown_risk_border").unwrap_or("").to_string()),
        Some(i) => Some(theme.palette("requirement_risk_palette", i).unwrap_or("").to_string()),
    }
}

struct Label {
    text: String,
    x: f64,
    y: f64,
    font_size: f64,
    weight: Option<&'static str>,
    anchor: Option<&'static str>,
    role: &'static str,
}

fn label_elem(l: &Label) -> Elem {
    let mut e = Elem::new("text").num("x", l.x).num("y", l.y).content(&l.text).attr("font-size", ruby_float(l.font_size));
    if let Some(w) = l.weight {
        e = e.attr("font-weight", w);
    }
    if let Some(a) = l.anchor {
        e = e.attr("text-anchor", a);
    }
    e
}

fn rect_elem(x: f64, y: f64, w: f64, h: f64) -> Elem {
    Elem::new("rect").num("x", x).num("y", y).num("width", w).num("height", h)
}

fn row_labels(n: &Node, header_role: &'static str, body_role: &'static str, prop_size: f64) -> Vec<Label> {
    n.rows
        .iter()
        .enumerate()
        .map(|(index, r)| {
            let header = index < HEADER_ROWS;
            let default_role = if header { header_role } else { body_role };
            let gap = if index < HEADER_ROWS { 0 } else { SECTION_GAP };
            let center_y = (n.y + TOP_PADDING + gap + index as i64 * 24 + 12) as f64;
            let font_size = if default_role == "property" || r.risk {
                prop_size
            } else {
                match (default_role, index) {
                    ("element", 1) => 14.0,
                    ("header", 0) => 12.0,
                    _ => 11.0,
                }
            };
            Label {
                text: r.text.clone(),
                x: if header { n.x + n.width / 2.0 } else { n.x + PADDING_X },
                y: if default_role == "property" { center_y + 5.0 } else { center_y },
                font_size,
                weight: if r.bold { Some("bold") } else { None },
                anchor: if header { Some("middle") } else { None },
                role: if r.risk { "risk" } else { default_role },
            }
        })
        .collect()
}

pub fn render(source: &str) -> Result<String, Error> {
    if source_layout(source) {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "requirement")?;
    let (reqs, elts, rels) = build(&tree);
    let theme = Theme::default_theme();
    let prop_size = theme.number("typography", "font_size_small").unwrap_or(12.0);

    let mut names: Vec<String> = reqs.iter().map(|r| r.name.clone()).collect();
    names.extend(elts.iter().map(|e| e.name.clone()));
    for (i, n) in names.iter().enumerate() {
        if names[..i].contains(n) {
            return Err(unported());
        }
    }
    let mut nodes: Vec<Node> = reqs
        .iter()
        .map(|r| size_node(&r.name, true, r.risk.clone(), requirement_rows(r)))
        .chain(elts.iter().map(|e| size_node(&e.name, false, None, element_rows(e))))
        .collect();

    if nodes.is_empty() {
        let mut doc = Document::new(20.0, 40.0);
        doc.view_box = Some("0 0 20 40".to_string());
        return Ok(doc.to_xml());
    }
    // ranks, in level order; within a level, node order
    let levels = node_levels(&names, &rels);
    let mut level_keys: Vec<i64> = vec![];
    for l in &levels {
        if !level_keys.contains(l) {
            level_keys.push(*l);
        }
    }
    level_keys.sort();
    let ranks: Vec<Vec<usize>> =
        level_keys.iter().map(|k| (0..nodes.len()).filter(|&i| levels[i] == *k).collect()).collect();
    let rank_width = |rank: &[usize], nodes: &[Node]| -> f64 {
        fsum(rank.iter().map(|&i| nodes[i].width)) + NODE_SEPARATION * (rank.len() as f64 - 1.0)
    };
    let canvas_width = ranks.iter().map(|r| rank_width(r, &nodes)).fold(f64::NEG_INFINITY, f64::max);
    let mut top = CANVAS_MARGIN;
    let mut order: Vec<usize> = vec![];
    for rank in &ranks {
        let mut left = CANVAS_MARGIN as f64 + (canvas_width - rank_width(rank, &nodes)) / 2.0;
        for &i in rank {
            nodes[i].x = left;
            nodes[i].y = top;
            left += nodes[i].width + NODE_SEPARATION;
            order.push(i);
        }
        top += rank.iter().map(|&i| nodes[i].height).max().unwrap() + RANK_SEPARATION;
    }
    let cw = canvas_width + (2 * CANVAS_MARGIN) as f64;
    let ch = top - RANK_SEPARATION + CANVAS_MARGIN;

    let mut doc = Document::new(cw, ch as f64);
    doc.view_box = Some(format!("0 0 {} {}", ruby_float(cw), ch));
    let fg = |k: &str| theme.color(k).unwrap_or("").to_string();

    // relationships
    for rel in &rels {
        let (Some(si), Some(ti)) = (names.iter().position(|n| *n == rel.source), names.iter().position(|n| *n == rel.target)) else {
            continue;
        };
        let (s, t) = (&nodes[si], &nodes[ti]);
        let (sx, sy) = (s.x + s.width / 2.0, (s.y + s.height) as f64);
        let (ex, ey) = (t.x + t.width / 2.0, t.y as f64);
        let offset = (ey - sy).abs() / 3.0;
        let (b0y, b1y) = (sy + offset, ey - offset);
        let d = format!(
            "M {} {} C {} {}, {} {}, {} {}",
            ruby_float(sx),
            ruby_float(sy),
            ruby_float(sx),
            ruby_float(b0y),
            ruby_float(ex),
            ruby_float(b1y),
            ruby_float(ex),
            ruby_float(ey)
        );
        let stroke = fg("edge_color");
        let mut group = Elem::new("g").attr("id", format!("relationship-{}-{}", rel.source, rel.target));
        group.push(Elem::new("path").attr("fill", "none").attr("stroke", stroke.clone()).attr("stroke-width", "2").attr("d", d.clone()));
        for head in Arrowhead::for_path(&ArrowPath {
            d: &d,
            stroke: Some(&stroke),
            stroke_width: Some("2"),
            marker_end: Some("url(#arrowhead)"),
            ..Default::default()
        }) {
            group.push(head);
        }
        if let Some(kind) = &rel.kind {
            let (mx, my) = ((sx + ex) / 2.0, (sy + ey) / 2.0);
            let width = (kind.chars().count() as i64 + 4) * 7;
            group.push(
                rect_elem(mx - (width / 2) as f64, my - 10.0, width as f64, 18.0)
                    .attr("fill", "#fff")
                    .attr("stroke", stroke.clone())
                    .attr("stroke-width", "1")
                    .num("rx", 3.0),
            );
            let l = Label { text: format!("<<{kind}>>"), x: mx, y: my, font_size: 10.0, weight: None, anchor: Some("middle"), role: "relationship" };
            group.push(label_elem(&l).attr("fill", fg("requirement_text")).attr("dominant-baseline", "middle"));
        }
        doc.add(group);
    }

    // nodes: requirements then elements, each in placement order
    for want_req in [true, false] {
        for &i in order.iter().filter(|&&i| nodes[i].is_req == want_req) {
            let n = &nodes[i];
            let (x, y, w, h) = (n.x, n.y as f64, n.width, n.height as f64);
            if want_req {
                let risk = n.risk.as_deref();
                let rc = risk_color(theme, risk);
                let labels = row_labels(n, "header", "property", prop_size);
                let mut group = Elem::new("g").attr("id", format!("requirement-{}", n.name));
                group.push(
                    rect_elem(x, y, w, h)
                        .attr("fill", fg("node_fill"))
                        .attr("stroke", rc.clone().unwrap_or_else(|| fg("requirement_box_border")))
                        .attr("stroke-width", "2")
                        .num("rx", 5.0)
                        .num("ry", 5.0),
                );
                let mut header = Elem::new("g");
                header.push(
                    rect_elem(x, y, w, HEADER_HEIGHT)
                        .attr("fill", rc.clone().unwrap_or_else(|| fg("node_fill")))
                        .attr("opacity", "0.3"),
                );
                for l in labels.iter().filter(|l| l.role == "header") {
                    header.push(label_elem(l).attr("fill", fg("requirement_text")).attr("dominant-baseline", "middle"));
                }
                group.push(header);
                for l in labels.iter().filter(|l| l.role != "header") {
                    let fill = if l.role == "risk" { rc.clone() } else { Some(fg("requirement_text")) };
                    let mut e = label_elem(l);
                    if let Some(f) = fill {
                        e = e.attr("fill", f);
                    }
                    group.push(e);
                }
                doc.add(group);
            } else {
                let mut group = Elem::new("g").attr("id", format!("element-{}", n.name));
                let right = x + w;
                let bottom = n.y + n.height;
                let points = format!(
                    "{},{} {},{} {},{} {},{}",
                    ruby_float(x),
                    n.y,
                    ruby_float(right),
                    n.y,
                    ruby_float(right),
                    bottom,
                    ruby_float(x),
                    bottom
                );
                group.push(
                    Elem::new("polygon")
                        .attr("fill", fg("node_fill"))
                        .attr("stroke", fg("requirement_element_border"))
                        .attr("stroke-width", "2")
                        .attr("points", points),
                );
                for l in row_labels(n, "element", "detail", prop_size) {
                    let fill = if l.role == "detail" { fg("requirement_detail_text") } else { fg("requirement_text") };
                    group.push(label_elem(&l).attr("fill", fill).attr("dominant-baseline", "middle"));
                }
                doc.add(group);
            }
        }
    }
    Ok(doc.to_xml())
}
