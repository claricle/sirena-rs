//! Pie renderer: port of Ruby `Parser::Native::Builders::Pie`, `Layout::Pie`,
//! `Layout::PieLegend` and `Renderer::Pie`.
use crate::svg::{ruby_float, round4, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use serde_json::Value;

// ---- shared native-tree helpers (Parser::Native::Text), used by the other group-A renderers ----

/// A terminal slice object `{value, line, column, offset, length}`.
pub(crate) fn is_slice(v: &Value) -> bool {
    v.as_object().is_some_and(|o| o.contains_key("offset") && o.contains_key("value"))
}

/// `Text.of`: empty capture (array) -> "", slice -> its text.
pub(crate) fn text_of(v: &Value) -> String {
    match v {
        Value::Array(_) | Value::Null => String::new(),
        Value::String(s) => s.clone(),
        v if is_slice(v) => v["value"].as_str().unwrap_or("").to_string(),
        v => v.to_string(),
    }
}

/// `Text.items`: the statement hashes of a tree, whatever shape.
pub(crate) fn items(tree: &Value) -> Vec<&Value> {
    let hash = |v: &&Value| v.is_object() && !is_slice(v);
    match tree {
        Value::Array(a) => {
            let mut out = vec![];
            for x in a {
                match x {
                    Value::Array(inner) => out.extend(inner.iter().filter(hash)),
                    x if hash(&x) => out.push(x),
                    _ => {}
                }
            }
            out
        }
        v if hash(&v) => vec![v],
        _ => vec![],
    }
}

/// Ruby `String#strip`.
pub(crate) fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\0'))
}

/// Ruby `Array#sum` over floats (Kahan-Babuska).
pub(crate) fn fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut f, mut c) = (0.0f64, 0.0f64);
    for x in values {
        let t = f + x;
        if !t.is_finite() {
            // Ruby falls back to plain addition for non-finite values.
            f = t;
            c = 0.0;
            continue;
        }
        if f.abs() >= x.abs() {
            c += (f - t) + x;
        } else {
            c += (x - t) + f;
        }
        f = t;
    }
    f + c
}

/// Ruby `String#to_f`: the longest numeric prefix, 0.0 when none.
pub(crate) fn to_f(s: &str) -> f64 {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let st = *i;
        while *i < b.len() && (b[*i].is_ascii_digit() || (b[*i] == b'_' && *i > st)) {
            *i += 1;
        }
        *i > st
    };
    let int = digits(&mut i);
    let mut end = i;
    if int && i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        if digits(&mut j) {
            i = j;
            end = i;
        }
    }
    if int && i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) {
            end = j;
        }
    }
    s[..end].replace('_', "").parse().unwrap_or(0.0)
}

// ---- pie ----

const RADIUS: f64 = 185.0;
const CENTER: f64 = 225.0;
const OUTER_RADIUS: f64 = RADIUS + 1.0;
const LABEL_OFFSET: f64 = RADIUS * 0.75;
const TITLE_Y: f64 = CENTER - 200.0;
const HEIGHT: f64 = 450.0;
const TEXT_SIZE: f64 = 17.0;
const TITLE_SIZE: f64 = 25.0;
const MIN_PERCENT: f64 = 1.0;
const LEGEND_LEFT: f64 = 441.0;
const ROW_HEIGHT: f64 = 22.0;
const SWATCH: f64 = 18.0;
const TEXT_GAP: f64 = 4.0;
const TEXT_BASELINE: f64 = 14.0;
const RIGHT_MARGIN: f64 = 49.0;

#[derive(Default)]
struct Pie {
    title: Option<String>,
    show_data: bool,
    slices: Vec<(String, f64)>,
}

fn title_text(node: &Value) -> Result<String, Error> {
    let t = &node["text"];
    if text_of(t).is_empty() {
        return Err(Error::Unported("pie".into()));
    }
    Ok(strip(&text_of(t)).to_string())
}

fn build(tree: &Value) -> Result<Pie, Error> {
    let mut pie = Pie::default();
    for item in items(tree) {
        if item.get("show_data").is_some() {
            pie.show_data = true;
        }
        if let Some(t) = item.get("title") {
            // Ruby declines an empty title to the Parslet builder, which titles it "[]".
            pie.title = Some(title_text(t).unwrap_or_else(|_| "[]".into()));
        }
        if let Some(entry) = item.get("entry") {
            let label = strip(&text_of(&entry["label"]["string"])).to_string();
            let value = to_f(&text_of(&entry["value"]));
            if value < 0.0 {
                return Err(Error::Parse {
                    class: "ParseError".into(),
                    message: "Negative values are not allowed in pie charts.".into(),
                    line: 0,
                    col: 0,
                });
            }
            pie.slices.push((label, value));
        } else if let Some(t) = item.get("standalone_title") {
            pie.title = Some(title_text(t)?);
        } else if item.get("acc_title").is_some() {
            title_text(&item["acc_title"])?;
        } else if item.get("acc_descr").is_some() {
            title_text(&item["acc_descr"])?;
        }
    }
    Ok(pie)
}

struct Seg {
    index: usize,
    label: String,
    value: f64,
    percentage: f64,
    rank: usize,
    sweep: f64,
}

fn circle_point(angle: f64, radius: f64) -> (f64, f64) {
    let r = angle * std::f64::consts::PI / 180.0;
    (round4(CENTER + radius * crate::js_math::cos(r)), round4(CENTER + radius * crate::js_math::sin(r)))
}

fn f(v: f64) -> String {
    ruby_float(v)
}

fn slice_path(start: f64, finish: f64) -> String {
    let sweep = finish - start;
    let (sx, sy) = circle_point(start, RADIUS);
    if sweep >= 360.0 {
        let (hx, hy) = circle_point(start + 180.0, RADIUS);
        return format!("M 225 225 L {} {} A 185 185 0 1 1 {} {} A 185 185 0 1 1 {} {} Z", f(sx), f(sy), f(hx), f(hy), f(sx), f(sy));
    }
    let (fx, fy) = circle_point(finish, RADIUS);
    let large = if sweep > 180.0 { 1 } else { 0 };
    format!("M 225 225 L {} {} A 185 185 0 {} 1 {} {} Z", f(sx), f(sy), large, f(fx), f(fy))
}

fn text_elem(theme: Theme, x: f64, y: f64, content: &str, size: f64) -> Elem {
    Elem::new("text")
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .num4("x", x)
        .num4("y", y)
        .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"))
        .attr("font-size", size_str(size))
        .content(content)
}

fn size_str(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { ruby_float(v) }
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "pie")?;
    let pie = build(&tree)?;
    let theme = Theme::default_theme();

    let total = fsum(pie.slices.iter().map(|s| s.1));
    // Slices are keyed by label (first wins); the id keeps the input index.
    let mut segs: Vec<Seg> = vec![];
    for (i, (label, value)) in pie.slices.iter().enumerate() {
        if segs.iter().any(|s| &s.label == label) {
            continue;
        }
        let share = if total == 0.0 { 0.0 } else { value / total };
        segs.push(Seg { index: i, label: label.clone(), value: *value, percentage: share * 100.0, rank: 0, sweep: 0.0 });
    }
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by(|&a, &b| segs[b].value.partial_cmp(&segs[a].value).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
    for (rank, &i) in order.iter().enumerate() {
        segs[i].rank = rank;
    }
    let shown_total = fsum(segs.iter().filter(|s| s.percentage >= MIN_PERCENT).map(|s| s.value));
    let scale = if shown_total == 0.0 { 0.0 } else { 360.0 / shown_total };
    for s in &mut segs {
        s.sweep = scale * if s.percentage >= MIN_PERCENT { s.value } else { 0.0 };
    }

    // Legend.
    let n = segs.len() as f64;
    let top = CENTER - (n * ROW_HEIGHT / 2.0);
    let captions: Vec<String> = segs
        .iter()
        .map(|s| {
            if pie.show_data {
                let v = ruby_float(s.value);
                format!("{} [{}]", s.label, v.strip_suffix(".0").unwrap_or(&v))
            } else {
                s.label.clone()
            }
        })
        .collect();
    let legend_width = if segs.is_empty() {
        0.0
    } else {
        let widest = captions.iter().map(|c| text::width(c, TEXT_SIZE, Face::Arial)).fold(f64::MIN, f64::max);
        LEGEND_LEFT + SWATCH + TEXT_GAP + widest + RIGHT_MARGIN
    };
    let wide = legend_width > HEIGHT;
    let width = if wide { legend_width } else { HEIGHT };

    let mut doc = Document::new(width, HEIGHT);
    doc.view_box = Some(format!("0 0 {} 450", if wide { ruby_float(width) } else { "450".into() }));
    doc.add(
        Elem::new("circle")
            .attr("fill", "none")
            .attr("stroke", theme.color("label_text").unwrap_or(""))
            .attr("stroke-width", "2")
            .num4("cx", CENTER)
            .num4("cy", CENTER)
            .num4("r", OUTER_RADIUS),
    );
    if let Some(t) = &pie.title {
        doc.add(text_elem(theme, CENTER, TITLE_Y, t, TITLE_SIZE).attr("text-anchor", "middle").attr("font-weight", "bold"));
    }
    // Angles accumulate in rank order; slices are emitted in input order.
    let mut angles: Vec<(f64, f64)> = vec![(0.0, 0.0); segs.len()];
    let mut start = -90.0;
    for &i in &order {
        let finish = start + segs[i].sweep;
        angles[i] = (start, finish);
        start = finish;
    }
    for (i, s) in segs.iter().enumerate() {
        if s.sweep == 0.0 {
            continue;
        }
        let (a, b) = angles[i];
        doc.add(
            Elem::new("path")
                .attr("id", format!("slice-{}", s.index))
                .attr("fill", theme.palette("pie_palette", s.rank).unwrap_or(""))
                .attr("stroke", theme.color("node_stroke").unwrap_or(""))
                .attr("stroke-width", "2")
                .attr("d", slice_path(a, b)),
        );
        let (lx, ly) = circle_point((a + b) / 2.0, LABEL_OFFSET);
        doc.add(
            text_elem(theme, lx, ly, &format!("{}%", s.percentage.round() as i64), TEXT_SIZE)
                .attr("text-anchor", "middle")
                .attr("dominant-baseline", "middle"),
        );
    }
    for (i, s) in segs.iter().enumerate() {
        let y = top + (i as f64 * ROW_HEIGHT);
        let color = theme.palette("pie_palette", s.rank).unwrap_or("");
        doc.add(
            Elem::new("rect")
                .attr("fill", color)
                .attr("stroke", color)
                .num4("x", LEGEND_LEFT)
                .num4("y", y)
                .num4("width", SWATCH)
                .num4("height", SWATCH),
        );
        doc.add(text_elem(theme, LEGEND_LEFT + SWATCH + TEXT_GAP, y + TEXT_BASELINE, &captions[i], TEXT_SIZE));
    }
    Ok(doc.to_xml())
}
