//! Radar renderer: port of Ruby `Parser::Native::Builders::Radar`,
//! `Parser::Radar#create_diagram`, the IR adapter, `Layout::Radar` and
//! `Renderer::Radar`.
use super::pie::{is_slice, items, to_f};
use crate::svg::{round4, ruby_float, Document, Elem, Theme};
use crate::Error;
use serde_json::Value;
use std::f64::consts::PI;

const DEFAULT_RADIUS: f64 = 300.0;
const PADDING: f64 = 50.0;
const LABEL_SCALE: f64 = 1.05;
const CURVE_TENSION: f64 = 0.17;
const LEGEND_STEP: f64 = 20.0;
const LEGEND_INSET: f64 = (DEFAULT_RADIUS + PADDING) * 0.75;
const GRID_CIRCLES: usize = 5;
const MAX_TICKS: usize = 32;

fn unported() -> Error {
    Error::Unported("radar".into())
}

// ---- parse tree -> diagram ----

enum Values {
    Positional(Vec<f64>),
    Named(Vec<(String, f64)>),
}

struct Curve {
    label: String,
    values: Values,
}

#[derive(Default)]
struct Options {
    ticks: Option<i64>,
    show_legend: Option<bool>,
    graticule: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Default)]
struct Diagram {
    title: Option<String>,
    axes: Vec<(String, String)>,
    curves: Vec<Curve>,
    options: Options,
}

/// Ruby `capture.to_s` for a slice (empty capture is `[]`).
fn to_s(v: &Value) -> Result<String, Error> {
    match v {
        Value::Array(a) if a.is_empty() => Ok("[]".into()),
        v if is_slice(v) => Ok(v["value"].as_str().unwrap_or("").to_string()),
        _ => Err(unported()),
    }
}

/// Ruby `String#to_i`: leading integer, 0 when none.
fn to_i(s: &str) -> i64 {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start = i;
    while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b'_' && i > start)) {
        i += 1;
    }
    t[..i].replace('_', "").parse().unwrap_or(0)
}

fn build(tree: &Value) -> Result<Diagram, Error> {
    let mut d = Diagram::default();
    let Some(statements) = tree.get("statements") else { return Ok(d) };
    for item in items(statements) {
        if item.get("title").is_some() {
            d.title = Some(to_s(&item["title"])?);
        } else if item.get("acc_title").is_some() {
            to_s(&item["acc_title"])?;
        } else if item.get("acc_descr").is_some() {
            to_s(&item["acc_descr"])?;
        } else if let Some(axes) = item.get("axes") {
            for axis in items(axes) {
                let id = to_s(&axis["id"])?;
                let label = if axis.get("label").is_some() { to_s(&axis["label"])? } else { id.clone() };
                d.axes.push((id, label));
            }
        } else if let Some(values) = item.get("values") {
            let id = to_s(&item["id"])?;
            let label = if item.get("label").is_some() { to_s(&item["label"])? } else { id.clone() };
            let entries = items(values);
            let mut positional = vec![];
            let mut named = vec![];
            for e in &entries {
                let number = to_f(&to_s(&e["value"])?);
                if e.get("axis").is_some() {
                    named.push((to_s(&e["axis"])?, number));
                } else {
                    positional.push(number);
                }
            }
            // A lone named value, or a mix, takes a Ruby path that fails.
            let v = match (positional.is_empty(), named.is_empty()) {
                (_, true) => Values::Positional(positional),
                (true, false) if named.len() > 1 => Values::Named(named),
                _ => return Err(unported()),
            };
            d.curves.push(Curve { label, values: v });
        } else if item.get("ticks").is_some() {
            d.options.ticks = Some(to_i(&to_s(&item["ticks"])?));
        } else if item.get("show_legend").is_some() {
            d.options.show_legend = Some(to_s(&item["show_legend"])? == "true");
        } else if item.get("graticule").is_some() {
            d.options.graticule = Some(to_s(&item["graticule"])?);
        } else if item.get("min").is_some() {
            d.options.min = Some(to_f(&to_s(&item["min"])?));
        } else if item.get("max").is_some() {
            d.options.max = Some(to_f(&to_s(&item["max"])?));
        } else {
            return Err(unported());
        }
    }
    Ok(d)
}

// ---- layout + render ----

fn number_string(v: f64) -> String {
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
    class: &'static str,
}

fn label_element(l: &Label, theme: Theme) -> Elem {
    Elem::new("text")
        .attr("class", l.class)
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .num("x", l.x)
        .num("y", l.y)
        .attr("text-anchor", l.anchor)
        .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"))
        .attr("font-size", number_string(l.font_size))
        .content(&l.text)
}

fn point_list(points: &[(f64, f64)]) -> String {
    points.iter().map(|(x, y)| format!("{},{}", ruby_float(*x), ruby_float(*y))).collect::<Vec<_>>().join(" ")
}

fn closed_curve(points: &[(f64, f64)]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let n = points.len() as i64;
    let at = |i: i64| points[i.rem_euclid(n) as usize];
    let offset = |origin: (f64, f64), ahead: (f64, f64), behind: (f64, f64), scale: f64| {
        format!(
            "{},{}",
            ruby_float(origin.0 + ((ahead.0 - behind.0) * scale)),
            ruby_float(origin.1 + ((ahead.1 - behind.1) * scale))
        )
    };
    let segments: Vec<String> = (0..n)
        .map(|index| {
            let (before, from, to, after) = (at(index - 1), at(index), at(index + 1), at(index + 2));
            let first = offset(from, to, before, CURVE_TENSION);
            let second = offset(to, after, from, -CURVE_TENSION);
            format!("C{first} {second} {},{}", ruby_float(to.0), ruby_float(to.1))
        })
        .collect();
    format!("M{},{} {} Z", ruby_float(points[0].0), ruby_float(points[0].1), segments.join(" "))
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "radar")?;
    let d = build(&tree)?;
    let theme = Theme::default_theme();
    let font = |key: &str, fallback: f64| match theme.number("typography", key) {
        Some(v) if v > 0.0 => v,
        _ => fallback,
    };

    // value range
    let value_for = |curve: &Curve, axis_index: usize| -> f64 {
        let id = &d.axes[axis_index].0;
        match &curve.values {
            Values::Positional(vals) => {
                // later axes with the same id overwrite earlier ones
                let mut found = None;
                for (i, (aid, _)) in d.axes.iter().enumerate() {
                    if aid == id {
                        if let Some(v) = vals.get(i) {
                            found = Some(*v);
                        }
                    }
                }
                found.unwrap_or(0.0)
            }
            Values::Named(named) => named.iter().rev().find(|(a, _)| a == id).map_or(0.0, |(_, v)| *v),
        }
    };
    let n_axes = d.axes.len();
    let mut measurements: Vec<f64> = vec![];
    for c in &d.curves {
        for a in 0..n_axes {
            measurements.push(value_for(c, a));
        }
    }
    let min_value = d.options.min.unwrap_or(0.0);
    let mut max_value = d.options.max.unwrap_or_else(|| measurements.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| if v > m { v } else { m }))).unwrap_or(100.0));
    if max_value <= min_value {
        max_value = min_value + 1.0;
    }

    // axes
    let step = 360.0 / n_axes as f64;
    struct Axis {
        angle: f64,
        radians: f64,
        end: (f64, f64),
        label: (f64, f64),
    }
    let axes: Vec<Axis> = (0..n_axes)
        .map(|index| {
            let angle = (index as f64 * step) - 90.0;
            let radians = angle * PI / 180.0;
            Axis {
                angle,
                radians,
                end: (radians.cos() * DEFAULT_RADIUS, radians.sin() * DEFAULT_RADIUS),
                label: (radians.cos() * DEFAULT_RADIUS * LABEL_SCALE, radians.sin() * DEFAULT_RADIUS * LABEL_SCALE),
            }
        })
        .collect();

    let center = DEFAULT_RADIUS + PADDING;
    let size = center * 2.0;
    let ticks = match d.options.ticks {
        Some(t) if t > 0 => (t as usize).min(MAX_TICKS),
        _ => GRID_CIRCLES,
    };
    let polygon = d.options.graticule.as_deref() == Some("polygon");
    let color = |i: usize| theme.palette("radar_palette", i).unwrap_or("");

    let mut doc = Document::new(size, size);
    doc.view_box = Some("0 0 700 700".into());

    // grid
    let grid_line = theme.color("radar_grid_line").unwrap_or("");
    for index in 0..ticks {
        let radius = DEFAULT_RADIUS * ((index + 1) as f64 / ticks as f64);
        let ring = if polygon {
            let pts: Vec<String> = axes
                .iter()
                .map(|a| {
                    let r = a.angle * PI / 180.0;
                    format!("{},{}", ruby_float(center + r.cos() * radius), ruby_float(center + r.sin() * radius))
                })
                .collect();
            Elem::new("polygon").attr("points", pts.join(" "))
        } else {
            Elem::new("circle").num("cx", center).num("cy", center).num("r", radius)
        };
        doc.add(ring.attr("fill", "none").attr("stroke", grid_line).attr("stroke-width", "1").attr("class", "radarGraticule"));
    }
    // axes
    for (index, a) in axes.iter().enumerate() {
        doc.add(
            Elem::new("line")
                .attr("stroke", theme.color("radar_axis_line").unwrap_or(""))
                .attr("stroke-width", "1")
                .attr("class", "radarAxisLine")
                .num("x1", center)
                .num("y1", center)
                .num("x2", center + a.end.0)
                .num("y2", center + a.end.1),
        );
        doc.add(label_element(
            &Label {
                text: d.axes[index].1.clone(),
                x: center + a.label.0,
                y: round4(center + a.label.1),
                font_size: font("font_size_normal", 12.0),
                anchor: "middle",
                class: "radarAxisLabel",
            },
            theme,
        ));
    }
    // curves
    for (index, curve) in d.curves.iter().enumerate() {
        let points: Vec<(f64, f64)> = axes
            .iter()
            .enumerate()
            .map(|(ai, a)| {
                let value = value_for(curve, ai);
                let normalized = ((value - min_value) / (max_value - min_value)).max(0.0).min(1.0);
                let radius = normalized * DEFAULT_RADIUS;
                (center + a.radians.cos() * radius, center + a.radians.sin() * radius)
            })
            .collect();
        if points.is_empty() {
            continue;
        }
        let c = color(index);
        let shape = if polygon {
            Elem::new("polygon").attr("points", point_list(&points))
        } else {
            Elem::new("path").attr("d", closed_curve(&points))
        };
        doc.add(
            shape
                .attr("class", format!("radarCurve-{index}"))
                .attr("fill", c)
                .attr("fill-opacity", "0.3")
                .attr("stroke", c)
                .attr("stroke-width", "2"),
        );
        for (x, y) in &points {
            doc.add(
                Elem::new("circle")
                    .attr("fill", c)
                    .attr("stroke", theme.color("background").unwrap_or(""))
                    .attr("stroke-width", "1")
                    .num("cx", *x)
                    .num("cy", *y)
                    .num("r", 3.0),
            );
        }
    }
    // legend
    if d.options.show_legend != Some(false) {
        for (index, curve) in d.curves.iter().enumerate() {
            let left = center + LEGEND_INSET;
            let top = center - LEGEND_INSET + (index as f64 * LEGEND_STEP);
            doc.add(
                Elem::new("circle")
                    .attr("fill", color(index))
                    .attr("stroke", "none")
                    .num("cx", left + 6.0)
                    .num("cy", top + 6.0)
                    .num("r", 6.0),
            );
            doc.add(label_element(
                &Label {
                    text: curve.label.clone(),
                    x: left + 16.0,
                    y: round4(top),
                    font_size: font("font_size_small", 12.0),
                    anchor: "start",
                    class: "radarLegendText",
                },
                theme,
            ));
        }
    }
    if let Some(title) = &d.title {
        doc.add(label_element(
            &Label { text: title.clone(), x: center, y: 0.0, font_size: font("font_size_large", 16.0), anchor: "middle", class: "radarTitle" },
            theme,
        ));
    }
    Ok(doc.to_xml())
}
