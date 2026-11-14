//! XY chart renderer: port of Ruby `Parser::Native::Builders::XyChart`,
//! `Layout::XyChart` and `Renderer::XyChart`.
use super::pie::{items, text_of, to_f};
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::Error;
use serde_json::Value;

const WIDTH: i64 = 700;
const HEIGHT: i64 = 500;
const MARGIN_TOP: i64 = 51;
const MARGIN_BOTTOM: i64 = 41;
const MARGIN_LEFT: f64 = 82.71875190734863;
const GRID_LINES: i64 = 5;
const BAR_PADDING_RATIO: f64 = 0.95;
const CATEGORICAL_INSET: f64 = 17.0;
const BAR_BASELINE_OFFSET: f64 = 8.0;

#[derive(Clone)]
enum Val {
    Num(f64),
    Text(String),
}

struct Dataset {
    bar: bool,
    label: String,
    values: Vec<f64>,
}

#[derive(Default)]
struct Chart {
    title: Option<String>,
    x_label: Option<String>,
    x_values: Option<Vec<Val>>,
    y: Option<(Option<String>, f64, f64)>,
    datasets: Vec<Dataset>,
}

/// `/\A\d+(\.\d+)?\z/`
fn numeric(s: &str) -> bool {
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    digits(int) && frac.is_none_or(digits)
}

fn values(node: Option<&Value>) -> Vec<Val> {
    let Some(node) = node else { return vec![] };
    items(node)
        .into_iter()
        .map(|e| {
            let t = text_of(&e["value"]);
            if numeric(&t) { Val::Num(to_f(&t)) } else { Val::Text(t) }
        })
        .collect()
}

fn string_of(v: &Value) -> String {
    text_of(&v["string"])
}

fn empty_label(item: &Value) -> bool {
    ["title", "x_label", "y_label", "dataset_label"]
        .iter()
        .any(|k| item.get(*k).is_some_and(|v| text_of(&v["string"]).is_empty()))
}

fn build(tree: &Value) -> Chart {
    let mut c = Chart::default();
    let stmts = tree.get("statements").map(items).unwrap_or_default();
    for item in stmts {
        if empty_label(item) {
            continue;
        }
        if let Some(t) = item.get("title") {
            c.title = Some(string_of(t));
        } else if item.get("x_values").is_some() {
            c.x_label = item.get("x_label").map(string_of);
            c.x_values = Some(values(item.get("x_values")));
        } else if item.get("y_min").is_some() {
            let label = item.get("y_label").map(string_of);
            c.y = Some((label, to_f(&text_of(&item["y_min"])), to_f(&text_of(&item["y_max"]))));
        } else {
            let (bar, label, vals) = if item.get("line_values").is_some() {
                (false, "Line".to_string(), values(item.get("line_values")))
            } else if item.get("bar_values").is_some() {
                (true, "Bar".to_string(), values(item.get("bar_values")))
            } else {
                (false, string_of(&item["dataset_label"]), values(item.get("dataset_values")))
            };
            let values = vals
                .into_iter()
                .map(|v| match v {
                    Val::Num(n) => n,
                    Val::Text(t) => to_f(&t),
                })
                .collect();
            c.datasets.push(Dataset { bar, label, values });
        }
    }
    c
}

/// Ruby `Float#round(1)` (round half away from zero, with Ruby's correction).
fn round1(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= 1.0e14 {
        return x;
    }
    let s = 10.0;
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

struct Position {
    label: String,
    position: f64,
}

enum XAxis {
    Numeric,
    Categorical(Vec<Position>),
}

struct Label {
    text: String,
    x: f64,
    y: f64,
    anchor: &'static str,
    size: f64,
    weight: Option<&'static str>,
    transform: Option<String>,
}

fn label_elem(theme: Theme, l: &Label) -> Elem {
    let mut e = Elem::new("text").num("x", l.x).num("y", l.y).attr("text-anchor", l.anchor);
    if let Some(t) = &l.transform {
        e = e.attr("transform", t.clone());
    }
    e = e
        .content(&l.text)
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .attr("font-size", ruby_float(l.size))
        .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"));
    if let Some(w) = l.weight {
        e = e.attr("font-weight", w);
    }
    e
}

fn line_elem(theme: Theme, x1: f64, y1: f64, x2: f64, y2: f64, grid: bool) -> Elem {
    let e = Elem::new("line").num("x1", x1).num("y1", y1).num("x2", x2).num("y2", y2);
    if grid {
        e.attr("stroke", theme.color("xy_grid_line").unwrap_or(""))
            .attr("stroke-width", "1")
            .attr("stroke-dasharray", "3,3")
    } else {
        e.attr("stroke", theme.color("xy_axis_line").unwrap_or("")).attr("stroke-width", "2")
    }
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "xychart")?;
    let chart = build(&tree);
    let theme = Theme::default_theme();
    let large = theme.number("typography", "font_size_large").unwrap_or(16.0);
    let normal = theme.number("typography", "font_size_normal").unwrap_or(14.0);
    let small = theme.number("typography", "font_size_small").unwrap_or(12.0);

    let width = WIDTH as f64 - MARGIN_LEFT - 0.0;
    let height_i = HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
    let height = height_i as f64;
    let plot_x = MARGIN_LEFT;
    let plot_y = MARGIN_TOP as f64;

    // x axis
    let x_axis_label = chart.x_label.clone();
    let x_axis = match &chart.x_values {
        Some(vals) if !vals.iter().all(|v| matches!(v, Val::Num(_))) => {
            let labels: Vec<String> = vals
                .iter()
                .map(|v| match v {
                    Val::Num(n) => ruby_float(*n),
                    Val::Text(t) => t.clone(),
                })
                .collect();
            let n = labels.len();
            if n == 1 {
                XAxis::Categorical(vec![Position { label: labels[0].clone(), position: width / 2.0 }])
            } else {
                let spacing = (width - CATEGORICAL_INSET * 2.0) / (n as f64 - 1.0);
                XAxis::Categorical(
                    labels
                        .into_iter()
                        .enumerate()
                        .map(|(i, label)| Position { label, position: CATEGORICAL_INSET + i as f64 * spacing })
                        .collect(),
                )
            }
        }
        _ => XAxis::Numeric,
    };
    // y axis
    let (y_label, y_min, y_max) = match &chart.y {
        Some((l, a, b)) => (l.clone(), *a, *b),
        None => (None, 0.0, 100.0),
    };

    let mut doc = Document::new(WIDTH as f64, HEIGHT as f64);
    doc.view_box = Some(format!("0 0 {WIDTH} {HEIGHT}"));

    if let Some(t) = &chart.title {
        doc.add(label_elem(
            theme,
            &Label {
                text: t.clone(),
                x: (WIDTH / 2) as f64,
                y: 30.0,
                anchor: "middle",
                size: large,
                weight: Some("bold"),
                transform: None,
            },
        ));
    }

    // lines
    let row = |i: i64| plot_y + ((i * height_i) / GRID_LINES) as f64;
    for i in 0..GRID_LINES {
        let y = row(i);
        doc.add(line_elem(theme, plot_x, y, plot_x + width, y, true));
    }
    if let XAxis::Categorical(ps) = &x_axis {
        for p in ps {
            let x = plot_x + p.position;
            doc.add(line_elem(theme, x, plot_y, x, plot_y + height, true));
        }
    }
    doc.add(line_elem(theme, plot_x, plot_y + height, plot_x + width, plot_y + height, false));
    doc.add(line_elem(theme, plot_x, plot_y, plot_x, plot_y + height, false));

    // series
    let bar_width = match &x_axis {
        XAxis::Categorical(ps) if ps.len() >= 2 => {
            let inner = ps[ps.len() - 1].position - ps[0].position;
            let tick = inner / ps.len() as f64;
            let m = if CATEGORICAL_INSET * 2.0 <= tick { CATEGORICAL_INSET * 2.0 } else { tick };
            m * BAR_PADDING_RATIO
        }
        _ => 20.0,
    };
    for (index, ds) in chart.datasets.iter().enumerate() {
        let color = theme.palette("xy_palette", index).unwrap_or("");
        let points: Vec<(f64, f64)> = ds
            .values
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let x = match &x_axis {
                    XAxis::Categorical(ps) => ps.get(i).map_or(0.0, |p| p.position),
                    XAxis::Numeric => {
                        let spacing = width / ((i + 1).max(1) as f64);
                        i as f64 * spacing
                    }
                };
                let normalized = if y_max == y_min { 0.0 } else { ((v - y_min) / (y_max - y_min)).clamp(0.0, 1.0) };
                (x, height - normalized * height)
            })
            .collect();
        if ds.bar {
            for &(px, py) in &points {
                let h = height - py + BAR_BASELINE_OFFSET;
                doc.add(
                    Elem::new("rect")
                        .attr("fill", color)
                        .attr("fill-opacity", "0.8")
                        .attr("stroke", color)
                        .attr("stroke-width", "1")
                        .num("x", plot_x + px - bar_width / 2.0)
                        .num("y", plot_y + py)
                        .num("width", bar_width)
                        .num("height", h),
                );
            }
        } else if !points.is_empty() {
            let pts: Vec<(f64, f64)> = points.iter().map(|&(x, y)| (plot_x + x, plot_y + y)).collect();
            let poly = pts.iter().map(|(x, y)| format!("{},{}", ruby_float(*x), ruby_float(*y))).collect::<Vec<_>>().join(" ");
            doc.add(
                Elem::new("polyline")
                    .attr("fill", "none")
                    .attr("stroke", color)
                    .attr("stroke-width", "2")
                    .attr("points", poly),
            );
            for (x, y) in pts {
                doc.add(
                    Elem::new("circle")
                        .attr("fill", color)
                        .attr("stroke", theme.color("background").unwrap_or(""))
                        .attr("stroke-width", "2")
                        .num("cx", x)
                        .num("cy", y)
                        .num("r", 4.0),
                );
            }
        }
    }

    // labels
    if let Some(t) = &x_axis_label {
        doc.add(label_elem(
            theme,
            &Label {
                text: t.clone(),
                x: plot_x + width / 2.0,
                y: plot_y + height + 60.0,
                anchor: "middle",
                size: normal,
                weight: Some("bold"),
                transform: None,
            },
        ));
    }
    if let XAxis::Categorical(ps) = &x_axis {
        for p in ps {
            doc.add(label_elem(
                theme,
                &Label {
                    text: p.label.clone(),
                    x: plot_x + p.position,
                    y: plot_y + height + 20.0,
                    anchor: "middle",
                    size: small,
                    weight: None,
                    transform: None,
                },
            ));
        }
    }
    if let Some(t) = &y_label {
        let cy_i = MARGIN_TOP + height_i / 2;
        let cy = cy_i as f64;
        doc.add(label_elem(
            theme,
            &Label {
                text: t.clone(),
                x: 20.0,
                y: cy,
                anchor: "middle",
                size: normal,
                weight: Some("bold"),
                transform: Some(format!("rotate(-90, 20, {})", cy_i)),
            },
        ));
    }
    let range = y_max - y_min;
    for i in 0..GRID_LINES {
        let value = round1(y_max - (i as f64 * range / GRID_LINES as f64));
        doc.add(label_elem(
            theme,
            &Label {
                text: ruby_float(value),
                x: plot_x - 10.0,
                y: row(i) + 4.0,
                anchor: "end",
                size: small,
                weight: None,
                transform: None,
            },
        ));
    }

    // legends
    for (index, ds) in chart.datasets.iter().enumerate() {
        let color = theme.palette("xy_palette", index).unwrap_or("");
        let y = (60 + index as i64 * 25) as f64;
        doc.add(
            Elem::new("rect")
                .attr("fill", color)
                .attr("stroke", "none")
                .num("x", (WIDTH - 150) as f64)
                .num("y", y - 8.0)
                .num("width", 15.0)
                .num("height", 15.0),
        );
        doc.add(label_elem(
            theme,
            &Label {
                text: ds.label.clone(),
                x: (WIDTH - 130) as f64,
                y: y + 4.0,
                anchor: "start",
                size: small,
                weight: None,
                transform: None,
            },
        ));
    }
    Ok(doc.to_xml())
}
