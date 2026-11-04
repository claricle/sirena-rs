//! Quadrant-chart renderer: port of Ruby `Parser::Native::Builders::Quadrant`,
//! `Parser::Builders::Quadrant`, the IR adapter, `Layout::Quadrant` and
//! `Renderer::Quadrant`.
use super::pie::{is_slice, items, strip, to_f};
use crate::svg::{round4, ruby_float, Document, Elem, Theme};
use crate::Error;
use serde_json::Value;

const CHART_SIZE: f64 = 500.0;
const PADDING: f64 = 5.0;
const AXIS_LABEL_PADDING: f64 = 5.0;
const QUADRANT_TEXT_TOP_PADDING: f64 = 5.0;
const TITLE_PADDING: f64 = 10.0;
const POINT_TEXT_PADDING: f64 = 5.0;
const EXTERNAL_BORDER: f64 = 2.0;
const INTERNAL_BORDER: f64 = 1.0;
const HALF_BORDER: f64 = EXTERNAL_BORDER / 2.0;
const PLOT_SIZE: f64 = CHART_SIZE - 2.0 * PADDING;

fn unported() -> Error {
    Error::Unported("quadrant".into())
}

// ---- parse tree -> diagram ----

#[derive(Default)]
struct Point {
    label: String,
    x: f64,
    y: f64,
    radius: Option<f64>,
    color: Option<String>,
    stroke_color: Option<String>,
    stroke_width: Option<f64>,
}

#[derive(Default)]
struct Diagram {
    title: Option<String>,
    x_left: Option<String>,
    x_right: Option<String>,
    y_bottom: Option<String>,
    y_top: Option<String>,
    quadrant_labels: [Option<String>; 4],
    points: Vec<Point>,
}

fn is_hash(v: &Value) -> bool {
    v.is_object() && !is_slice(v)
}

/// Ruby `value.to_s` of a leaf capture (slice text; empty capture `[]`).
fn leaf_s(v: &Value) -> Result<String, Error> {
    match v {
        Value::Null => Ok(String::new()),
        Value::Array(a) if a.is_empty() => Ok("[]".into()),
        v if is_slice(v) => Ok(v["value"].as_str().unwrap_or("").to_string()),
        _ => Err(unported()),
    }
}

/// `capture_string`: empty capture -> "".
fn capture_string(v: &Value) -> Result<String, Error> {
    match v {
        Value::Array(_) => Ok(String::new()),
        v => leaf_s(v),
    }
}

/// Builder `extract_text`.
fn extract_text(v: &Value) -> Result<String, Error> {
    let text = if is_hash(v) {
        match v.get("string") {
            Some(s) if !s.is_null() => capture_string(s)?,
            _ => match v.as_object().and_then(|o| o.values().next()) {
                Some(first) => leaf_s(first)?,
                None => return Err(unported()),
            },
        }
    } else {
        leaf_s(v)?
    };
    Ok(strip(&text).to_string())
}

/// Builder `extract_float`.
fn extract_float(v: &Value) -> Result<f64, Error> {
    let s = if is_hash(v) {
        match v.as_object().and_then(|o| o.values().next()) {
            Some(first) => leaf_s(first)?,
            None => return Err(unported()),
        }
    } else {
        leaf_s(v)?
    };
    Ok(to_f(&s))
}

fn build(tree: &Value) -> Result<Diagram, Error> {
    let mut d = Diagram::default();
    let mut statements: Vec<&Value> = vec![tree];
    if let Some(s) = tree.get("statements") {
        statements.extend(items(s));
    }
    for item in statements {
        if item.get("class_style").is_some() {
            return Err(unported()); // classDef validation belongs to the Ruby path
        }
        if let Some(t) = item.get("title") {
            if !t.is_null() {
                let text = extract_text(t)?;
                if !text.is_empty() {
                    d.title = Some(text);
                }
            }
        }
        if item.get("x_axis_left").is_some() {
            d.x_left = Some(extract_text(&item["x_axis_left"])?);
            d.x_right = Some(extract_text(&item["x_axis_right"])?);
        }
        if item.get("y_axis_bottom").is_some() {
            d.y_bottom = Some(extract_text(&item["y_axis_bottom"])?);
            d.y_top = Some(extract_text(&item["y_axis_top"])?);
        }
        if item.get("quadrant_label").is_some() {
            let number = leaf_s(&item["quadrant_number"])?;
            if let Some(i) = ["1", "2", "3", "4"].iter().position(|n| *n == number) {
                d.quadrant_labels[i] = Some(extract_text(&item["quadrant_label"])?);
            }
        }
        if item.get("data_point").is_some() {
            let label = extract_text(&item["label"])?;
            let coords = &item["coordinates"];
            if label.is_empty() || coords.is_null() {
                continue;
            }
            let mut p = Point { label, x: extract_float(&coords["x"])?, y: extract_float(&coords["y"])?, ..Point::default() };
            let styling = &item["styling"];
            let list: Vec<&Value> = match styling {
                Value::Array(a) => a.iter().collect(),
                v if is_hash(v) => vec![v],
                _ => vec![],
            };
            for s in list.into_iter().filter(|s| is_hash(s)) {
                if let Some(v) = s.get("radius").filter(|v| !v.is_null()) {
                    p.radius = Some(extract_float(v)?);
                }
                if let Some(v) = s.get("color").filter(|v| !v.is_null()) {
                    p.color = Some(extract_text(v)?);
                }
                if let Some(v) = s.get("stroke_color").filter(|v| !v.is_null()) {
                    p.stroke_color = Some(extract_text(v)?);
                }
                if let Some(v) = s.get("stroke_width").filter(|v| !v.is_null()) {
                    p.stroke_width = Some(extract_float(v)?);
                }
            }
            d.points.push(p);
        }
    }
    Ok(d)
}

// ---- layout + render ----

/// Renderer `number_string`: rounded to 4 places, integral -> integer text.
fn number_string(v: f64) -> String {
    let v = round4(v);
    if v.is_finite() && v == v.trunc() && v.abs() < 9.0e15 {
        format!("{}", v as i64)
    } else {
        ruby_float(v)
    }
}

struct Label {
    text: String,
    x: f64,
    y: f64,
    font_size: f64,
    anchor: &'static str,
    baseline: &'static str,
    rotation: f64,
}

struct Space {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    title: f64,
    x_axis_below: bool,
}

impl Space {
    fn half_width(&self) -> f64 {
        self.width / 2.0
    }
    fn half_height(&self) -> f64 {
        self.height / 2.0
    }
    fn right(&self) -> f64 {
        self.left + self.width
    }
    fn bottom(&self) -> f64 {
        self.top + self.height
    }
    fn mid_x(&self) -> f64 {
        self.left + self.half_width()
    }
    fn mid_y(&self) -> f64 {
        self.top + self.half_height()
    }
}

struct Line {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    stroke_width: f64,
}

fn text_element(label: &Label, theme: Theme) -> Elem {
    let mut e = Elem::new("text");
    if label.rotation != 0.0 {
        e = e.attr(
            "transform",
            format!("rotate({} {} {})", number_string(label.rotation), number_string(label.x), number_string(label.y)),
        );
    }
    e.attr("fill", theme.color("label_text").unwrap_or(""))
        .num("x", label.x)
        .num("y", label.y)
        .attr("dominant-baseline", label.baseline)
        .attr("text-anchor", label.anchor)
        .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"))
        .attr("font-size", number_string(label.font_size))
        .content(&label.text)
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "quadrant")?;
    let d = build(&tree)?;
    let theme = Theme::default_theme();
    let typo = |k: &str, default: f64| theme.number("typography", k).unwrap_or(default);
    let (title_size, large, small) = (typo("font_size_title", 20.0), typo("font_size_large", 16.0), typo("font_size_small", 12.0));

    let axis = |o: &Option<String>| o.clone().unwrap_or_default();
    let (x_left, x_right, y_bottom, y_top) = (axis(&d.x_left), axis(&d.x_right), axis(&d.y_bottom), axis(&d.y_top));
    let room = |a: &str, b: &str| if a.is_empty() && b.is_empty() { 0.0 } else { 2.0 * AXIS_LABEL_PADDING + large };
    let (x_room, y_room) = (room(&x_left, &x_right), room(&y_bottom, &y_top));
    let title = d.title.clone().unwrap_or_default();
    let title_space = if title.is_empty() { 0.0 } else { title_size + 2.0 * TITLE_PADDING };
    let below = !d.points.is_empty();
    let space = Space {
        left: PADDING + y_room,
        top: PADDING + if below { 0.0 } else { x_room } + title_space,
        width: PLOT_SIZE - y_room,
        height: PLOT_SIZE - x_room - title_space,
        title: title_space,
        x_axis_below: below,
    };

    let mk = |text: &str, x: f64, y: f64, size: f64, anchor: &'static str, baseline: &'static str, rotation: f64| Label {
        text: text.to_string(),
        x,
        y,
        font_size: size,
        anchor,
        baseline,
        rotation,
    };

    let mut doc = Document::new(CHART_SIZE, CHART_SIZE);
    doc.view_box = Some("0 0 500 500".into());

    // quadrants 1..4
    let mut quadrants = Elem::new("g").attr("class", "quadrants");
    for number in 1..=4usize {
        let x = if number == 1 || number == 4 { space.left + space.half_width() } else { space.left };
        let y = if number == 3 || number == 4 { space.top + space.half_height() } else { space.top };
        let fill = theme.palette("quadrant_palette", number - 1).unwrap_or("");
        let mut cell = Elem::new("g").attr("class", "quadrant").child(
            Elem::new("rect")
                .attr("fill", fill)
                .num("x", x)
                .num("y", y)
                .num("width", space.half_width())
                .num("height", space.half_height()),
        );
        if let Some(text) = &d.quadrant_labels[number - 1] {
            let label = if d.points.is_empty() {
                mk(text, x + space.half_width() / 2.0, y + space.half_height() / 2.0, large, "middle", "middle", 0.0)
            } else {
                mk(text, x + space.half_width() / 2.0, y + QUADRANT_TEXT_TOP_PADDING, large, "middle", "hanging", 0.0)
            };
            cell.push(text_element(&label, theme));
        }
        quadrants.push(cell);
    }
    doc.add(quadrants);

    // border lines
    let (al, ar) = (space.left - HALF_BORDER, space.right() + HALF_BORDER);
    let (dt, db) = (space.top + HALF_BORDER, space.bottom() - HALF_BORDER);
    let hl = |y: f64, a: f64, b: f64, w: f64| Line { x1: a, y1: y, x2: b, y2: y, stroke_width: w };
    let vl = |x: f64, a: f64, b: f64, w: f64| Line { x1: x, y1: a, x2: x, y2: b, stroke_width: w };
    let lines = [
        hl(space.top, al, ar, EXTERNAL_BORDER),
        vl(space.right(), dt, db, EXTERNAL_BORDER),
        hl(space.bottom(), al, ar, EXTERNAL_BORDER),
        vl(space.left, dt, db, EXTERNAL_BORDER),
        vl(space.mid_x(), dt, db, INTERNAL_BORDER),
        hl(space.mid_y(), space.left + HALF_BORDER, space.right() - HALF_BORDER, INTERNAL_BORDER),
    ];
    let mut border = Elem::new("g").attr("class", "border");
    for l in &lines {
        border.push(
            Elem::new("line")
                .attr("stroke", theme.color("quadrant_grid_line").unwrap_or(""))
                .attr("stroke-width", number_string(l.stroke_width))
                .num("x1", l.x1)
                .num("y1", l.y1)
                .num("x2", l.x2)
                .num("y2", l.y2),
        );
    }
    doc.add(border);

    // points
    let mut points = Elem::new("g").attr("class", "data-points");
    for (index, p) in d.points.iter().enumerate() {
        let svg_x = space.left + p.x * space.width;
        let svg_y = space.top + (1.0 - p.y) * space.height;
        let quadrant = if p.y >= 0.5 { if p.x >= 0.5 { 1 } else { 2 } } else if p.x < 0.5 { 3 } else { 4 };
        let default_fill = match quadrant {
            2 => "secondary",
            3 => "accent",
            4 => "success",
            _ => "primary",
        };
        let fill = p.color.clone().unwrap_or_else(|| theme.color(default_fill).unwrap_or("").to_string());
        let stroke = p.stroke_color.clone().unwrap_or_else(|| theme.color("node_stroke").unwrap_or("").to_string());
        let circle = Elem::new("circle")
            .attr("id", format!("point_{index}"))
            .attr("fill", fill)
            .attr("stroke", stroke)
            .attr("stroke-width", number_string(p.stroke_width.unwrap_or(2.0)))
            .num("cx", svg_x)
            .num("cy", svg_y)
            .num("r", p.radius.unwrap_or(5.0));
        let label = mk(&p.label, svg_x, svg_y + POINT_TEXT_PADDING, small, "middle", "hanging", 0.0);
        points.push(Elem::new("g").attr("class", "data-point").child(circle).child(text_element(&label, theme)));
    }
    doc.add(points);

    // axis labels (only the written ones)
    let mut axis_labels: Vec<Label> = vec![];
    {
        let middle = !x_right.is_empty();
        let centre = space.left + if middle { space.half_width() / 2.0 } else { 0.0 };
        let y_pos = if space.x_axis_below {
            AXIS_LABEL_PADDING + space.top + space.height + PADDING
        } else {
            AXIS_LABEL_PADDING + space.title
        };
        let anchor = if middle { "middle" } else { "start" };
        for (text, along) in [(&x_left, centre), (&x_right, centre + space.half_width())] {
            if !text.is_empty() {
                axis_labels.push(mk(text, along, y_pos, large, anchor, "hanging", 0.0));
            }
        }
        let middle = !y_top.is_empty();
        let lift = if middle { space.half_height() / 2.0 } else { 0.0 };
        let anchor = if middle { "middle" } else { "start" };
        for (text, along) in [(&y_bottom, space.bottom() - lift), (&y_top, space.mid_y() - lift)] {
            if !text.is_empty() {
                axis_labels.push(mk(text, AXIS_LABEL_PADDING, along, large, anchor, "hanging", -90.0));
            }
        }
    }
    let mut labels = Elem::new("g").attr("class", "labels");
    for l in &axis_labels {
        labels.push(Elem::new("g").attr("class", "label").child(text_element(l, theme)));
    }
    doc.add(labels);

    let mut title_group = Elem::new("g").attr("class", "title");
    if !title.is_empty() {
        let label = mk(&title, CHART_SIZE / 2.0, TITLE_PADDING, title_size, "middle", "hanging", 0.0);
        title_group.push(text_element(&label, theme));
    }
    doc.add(title_group);
    Ok(doc.to_xml())
}
