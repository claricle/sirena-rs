//! User journey renderer: port of Ruby `Parser::Native::Builders::UserJourney`,
//! `Layout::UserJourney*` (grid geometry) and `Renderer::UserJourney`.
use super::pie::{items, strip, text_of};
use crate::svg::{round4, ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const TASK_WIDTH: f64 = 150.0;
const TASK_BOX_HEIGHT: f64 = 50.0;
const TASK_PITCH: f64 = 200.0;
const SECTION_Y: f64 = 50.0;
const TASK_Y: f64 = 110.0;
const LINE_END: f64 = 450.0;
const LEGEND_ROW_HEIGHT: f64 = 50.0;
const DIAGRAM_MARGIN: f64 = 100.0;
const TITLE_EXTRA: f64 = 70.0;
const TIMELINE_Y: f64 = 200.0;
const TEXT_SIZE: f64 = 14.0;
const DOT_RADIUS: f64 = 7.0;
const CENTRAL_BASELINE_EMS: f64 = 0.35;
const NO_SECTION_FILL: &str = "#CCC";
const TEXT_FONT: &str = "\"Open Sans\", sans-serif";
const GREY: &str = "#666";

const FACE_RADIUS: f64 = 15.0;
const EYE_RADIUS: f64 = 1.5;
const FACE_TOP: f64 = 300.0;
const FACE_STEP: f64 = 30.0;
const SMILE_OFFSET: f64 = 2.0;
const FROWN_OFFSET: f64 = 7.0;
const FLAT_HALF_WIDTH: f64 = 5.0;
const MOUTH_OUTER: f64 = 7.5;
const MOUTH_INNER: f64 = 6.818;

const LEGEND_FONT_SIZE: f64 = 16.0;
const LABEL_WIDTH: f64 = 360.0;
const BASE_MARGIN: f64 = 150.0;
const FIRST_ROW_Y: f64 = 60.0;
const ROW_STEP: f64 = 20.0;

/// Ruby `Float#round(nd)` (round half away from zero, with Ruby's correction).
pub(crate) fn ruby_round(x: f64, nd: i32) -> f64 {
    if nd == 4 {
        return round4(x);
    }
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

struct Task {
    name: String,
    score: i64,
    actors: Vec<String>,
    section: String,
}

fn rb_split_commas(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    s.split(',').map(|p| strip(p).to_string()).collect()
}

fn actors_of(node: &Value) -> Vec<String> {
    if node.get("actors").is_none() {
        return vec![];
    }
    let names = rb_split_commas(&text_of(&node["actors"]));
    if names.is_empty() {
        return vec![String::new()];
    }
    if names.len() == 1 {
        return names;
    }
    names.into_iter().filter(|n| !n.is_empty()).collect()
}

/// Ruby `String#to_i`.
fn to_i(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: i64 = digits.parse().unwrap_or(0);
    if neg { -n } else { n }
}

fn build(tree: &Value) -> Result<(Option<String>, Vec<Task>), Error> {
    let mut title = None;
    let mut section = String::new();
    let mut tasks = vec![];
    for item in items(tree) {
        if let Some(t) = item.get("task") {
            let score = to_i(&text_of(&t["score"]));
            if !(1..=5).contains(&score) {
                // The Ruby path owns the ScoreError text.
                return Err(Error::Unported("user_journey".into()));
            }
            tasks.push(Task {
                name: strip(&text_of(&t["name"])).to_string(),
                score,
                actors: actors_of(t),
                section: section.clone(),
            });
        } else if let Some(s) = item.get("section") {
            if !s.is_null() && !matches!(s, Value::Bool(false)) {
                section = strip(&text_of(&s["text"])).to_string();
            }
        } else if let Some(t) = item.get("title") {
            title = Some(strip(&text_of(&t["text"])).to_string());
        }
    }
    Ok((title, tasks))
}

// ---- legend ----

fn width(text: &str) -> f64 {
    text::width(text, LEGEND_FONT_SIZE, Face::Arial)
}

/// Ruby `String#split(" ", -1)`: awk-style whitespace split.
fn awk_split(s: &str) -> Vec<String> {
    let ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r');
    let mut out: Vec<String> = s.split(ws).filter(|p| !p.is_empty()).map(str::to_string).collect();
    if !s.is_empty() && s.ends_with(ws) && !out.is_empty() {
        out.push(String::new());
    }
    out
}

fn wrap(name: &str) -> Vec<String> {
    if width(name) <= LABEL_WIDTH {
        return vec![name.to_string()];
    }
    let mut lines: Vec<String> = vec![];
    let mut current = String::new();
    for word in awk_split(name) {
        let candidate = if current.is_empty() { word.clone() } else { format!("{current} {word}") };
        if width(&candidate) <= LABEL_WIDTH {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(current.clone());
        }
        if width(&word) > LABEL_WIDTH {
            let mut piece = String::new();
            for ch in word.chars() {
                if width(&format!("{piece}{ch}-")) > LABEL_WIDTH {
                    lines.push(format!("{piece}-"));
                    piece.clear();
                }
                piece.push(ch);
            }
            current = piece;
        } else {
            current = word;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

struct Row {
    name: String,
    index: usize,
    y: f64,
    lines: Vec<String>,
}

fn legend_rows(names: &[String]) -> Vec<Row> {
    let mut y = FIRST_ROW_Y;
    let mut rows = vec![];
    for (index, name) in names.iter().enumerate() {
        let lines = wrap(name);
        let step = ROW_STEP.max(lines.len() as f64 * ROW_STEP);
        rows.push(Row { name: name.clone(), index, y, lines });
        y += step;
    }
    rows
}

// ---- helpers ----

fn br_split(text: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)<br[ \t\r\n\x0b\x0c]*/?>").unwrap());
    if text.is_empty() {
        return vec![];
    }
    re.split(text).map(str::to_string).collect()
}

fn number_string(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { ruby_float(v) }
}

fn coord(v: f64) -> String {
    let r = ruby_round(v, 3);
    if r == r.trunc() { format!("{}", r as i64) } else { ruby_float(r) }
}

struct Style {
    kind: usize,
    fill: String,
    colour: &'static str,
}

fn run_style(theme: Theme, number: Option<usize>) -> Style {
    match number {
        None => Style { kind: 0, fill: NO_SECTION_FILL.into(), colour: "black" },
        Some(n) => {
            let size = palette_len(theme, "user_journey_section_palette");
            Style {
                kind: n % size,
                fill: theme.palette("user_journey_section_palette", n).unwrap_or("").to_string(),
                colour: "#fff",
            }
        }
    }
}

/// Length of a theme palette (palettes wrap, so find the first repeat).
fn palette_len(theme: Theme, name: &str) -> usize {
    let at = |i: usize| theme.palette(name, i);
    (1..256).find(|&n| at(n) == at(0) && at(n + 1) == at(1)).unwrap_or(1)
}

struct Box_ {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct LabelT {
    text: String,
    x: f64,
    y: f64,
}

fn centered_labels(text: &str, left: f64, w: f64, top: f64, height: f64, size: f64) -> Vec<LabelT> {
    let lines = br_split(text);
    let cx = left + w / 2.0;
    let cy = top + height / 2.0;
    let n = lines.len();
    lines
        .into_iter()
        .enumerate()
        .map(|(offset, line)| {
            let shift = (offset as f64 * size) - (size * (n as f64 - 1.0) / 2.0);
            LabelT { text: line, x: cx, y: round4((cy + shift) + (size * CENTRAL_BASELINE_EMS)) }
        })
        .collect()
}

fn dot_circle(theme: Theme, x: f64, y: f64, name_index: usize, colour: &str) -> Elem {
    Elem::new("circle")
        .attr("class", format!("actor-{name_index}"))
        .attr("fill", colour)
        .attr("stroke", theme.color("node_stroke").unwrap_or(""))
        .num("cx", x)
        .num("cy", y)
        .num("r", DOT_RADIUS)
}

fn secondary_paint(theme: Theme) -> String {
    let c = theme.color("foreground_secondary").unwrap_or("");
    if c == "#666666" { GREY.to_string() } else { c.to_string() }
}

fn band_rect(b: &Box_, class: &str, fill: &str, stroke: &str) -> Elem {
    Elem::new("rect")
        .attr("class", class)
        .attr("fill", fill)
        .attr("stroke", stroke)
        .num("x", b.x)
        .num("y", b.y)
        .num("width", b.w)
        .num("height", b.h)
        .num("rx", 3.0)
        .num("ry", 3.0)
}

fn box_text(l: &LabelT, class: &str, fill: &str, size: f64) -> Elem {
    Elem::new("text")
        .attr("class", class)
        .attr("fill", fill)
        .num("x", l.x)
        .num("y", l.y)
        .attr("text-anchor", "middle")
        .attr("font-family", TEXT_FONT)
        .attr("font-size", number_string(size))
        .content(&l.text)
}

fn fmt_mode(v: f64, fl: bool) -> String {
    if fl { ruby_float(v) } else { format!("{}", v as i64) }
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "user_journey")?;
    let (title, tasks) = build(&tree)?;
    let theme = Theme::default_theme();
    let normal = theme.number("typography", "font_size_normal").filter(|v| *v > 0.0).unwrap_or(TEXT_SIZE);
    let font_family = theme.string("typography", "font_family").unwrap_or("");
    let foreground = theme.color("foreground").unwrap_or("");
    let stroke = secondary_paint(theme);

    // legend
    let mut names: Vec<String> = tasks.iter().flat_map(|t| t.actors.iter().cloned()).collect();
    names.sort();
    names.dedup();
    let rows = legend_rows(&names);
    let widest = rows
        .iter()
        .flat_map(|r| r.lines.iter())
        .map(|l| width(l))
        .filter(|w| *w > BASE_MARGIN - *w)
        .fold(None, |m: Option<f64>, w| Some(m.map_or(w, |m| if w > m { w } else { m })));
    let fl = widest.is_some();
    let left = BASE_MARGIN + widest.unwrap_or(0.0);

    // boxes
    let boxes: Vec<Box_> = (0..tasks.len())
        .map(|p| Box_ { x: left + p as f64 * TASK_PITCH, y: TASK_Y, w: TASK_WIDTH, h: TASK_BOX_HEIGHT })
        .collect();
    let scene_width = if boxes.is_empty() {
        left * 2.0 + DIAGRAM_MARGIN
    } else {
        let right = boxes.iter().map(|b| b.x + b.w).fold(f64::MIN, f64::max);
        left + right + DIAGRAM_MARGIN / 2.0
    };
    let task_bottom = boxes.iter().map(|b| b.y + b.h).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v)))).unwrap_or(0.0);
    let stop_y = (rows.len() as f64 * LEGEND_ROW_HEIGHT).max(if boxes.is_empty() { 0.0 } else { LINE_END }).max(task_bottom);
    let has_title = title.as_deref().is_some_and(|t| !t.is_empty());
    let height = stop_y + 20.0 + if has_title { TITLE_EXTRA } else { 0.0 };

    let mut doc = Document::new(scene_width, height + 25.0);
    doc.view_box = Some(format!("0 -25 {} {}", fmt_mode(scene_width, fl), fmt_mode(height, false)));

    let actor_index = |name: &str| rows.iter().find(|r| r.name == name).map(|r| r.index).unwrap_or(0);
    let actor_colour = |i: usize| theme.palette("user_journey_actor_palette", i).unwrap_or("").to_string();

    // legend actors
    for r in &rows {
        doc.add(dot_circle(theme, 20.0, r.y, r.index, &actor_colour(r.index)));
        for (offset, line) in r.lines.iter().enumerate() {
            doc.add(
                Elem::new("text")
                    .attr("fill", foreground)
                    .num("x", 50.0)
                    .num("y", r.y + 7.0 + offset as f64 * 20.0)
                    .attr("font-family", font_family)
                    .attr("font-size", number_string(theme.number("typography", "font_size_large").filter(|v| *v > 0.0).unwrap_or(LEGEND_FONT_SIZE)))
                    .content(line),
            );
        }
    }

    // runs of equal section name
    let mut runs: Vec<(Vec<usize>, String, Option<usize>)> = vec![];
    let mut named: i64 = -1;
    for (i, t) in tasks.iter().enumerate() {
        match runs.last_mut() {
            Some(last) if tasks[last.0[0]].section == t.section => last.0.push(i),
            _ => {
                let number = if t.section.is_empty() {
                    None
                } else {
                    named += 1;
                    Some(named as usize)
                };
                runs.push((vec![i], t.section.clone(), number));
            }
        }
    }

    // section bands
    for (indexes, name, number) in &runs {
        if name.is_empty() {
            continue;
        }
        let style = run_style(theme, *number);
        let l = indexes.iter().map(|i| boxes[*i].x).fold(f64::MAX, f64::min);
        let r = indexes.iter().map(|i| boxes[*i].x + boxes[*i].w).fold(f64::MIN, f64::max);
        let span_w = r - l;
        let class = format!("journey-section section-type-{}", style.kind);
        let b = Box_ { x: l, y: SECTION_Y, w: span_w, h: TASK_BOX_HEIGHT };
        let mut g = Elem::new("g").child(band_rect(&b, &class, &style.fill, &stroke));
        for label in centered_labels(name, l, span_w, SECTION_Y, TASK_BOX_HEIGHT, normal) {
            g.push(box_text(&label, &class, style.colour, normal));
        }
        doc.add(g);
    }

    // tasks
    for (indexes, _, number) in &runs {
        let style = run_style(theme, *number);
        for &p in indexes {
            let t = &tasks[p];
            let b = &boxes[p];
            let mut g = Elem::new("g").attr("id", format!("task-task_{p}"));
            let center_x = b.x + b.w / 2.0;
            let center_y = FACE_TOP + (5.0 - t.score as f64) * FACE_STEP;
            g.push(
                Elem::new("line")
                    .attr("class", "task-line")
                    .attr("stroke", stroke.clone())
                    .attr("stroke-width", "1px")
                    .num("x1", center_x)
                    .num("y1", b.y)
                    .num("x2", center_x)
                    .num("y2", LINE_END)
                    .attr("stroke-dasharray", "4 2"),
            );
            g.push(
                Elem::new("circle")
                    .attr("class", "face")
                    .attr("fill", "#FFF8DC")
                    .attr("stroke", theme.color("node_stroke").unwrap_or(""))
                    .attr("stroke-width", "2")
                    .num("cx", center_x)
                    .num("cy", center_y)
                    .num("r", FACE_RADIUS),
            );
            let mut features = Elem::new("g");
            for side in [-1.0, 1.0] {
                features.push(
                    Elem::new("circle")
                        .attr("fill", foreground)
                        .attr("stroke", stroke.clone())
                        .attr("stroke-width", "2")
                        .num("cx", center_x + (side * FACE_RADIUS / 3.0))
                        .num("cy", center_y - (FACE_RADIUS / 3.0))
                        .num("r", EYE_RADIUS),
                );
            }
            if t.score == 3 {
                let row = center_y + FROWN_OFFSET;
                features.push(
                    Elem::new("line")
                        .attr("class", "mouth")
                        .attr("stroke", stroke.clone())
                        .attr("stroke-width", "1px")
                        .num("x1", center_x - FLAT_HALF_WIDTH)
                        .num("y1", row)
                        .num("x2", center_x + FLAT_HALF_WIDTH)
                        .num("y2", row),
                );
            } else {
                let smile = t.score > 3;
                let offset = if smile { SMILE_OFFSET } else { FROWN_OFFSET };
                let dir = if smile { 1.0 } else { -1.0 };
                let row = coord(center_y + offset);
                let at = |o: f64| format!("{},{}", coord(center_x + dir * o), row);
                let arc = |r: f64, sweep: i32| format!("A{},{},0,1,{}", ruby_float(r), ruby_float(r), sweep);
                let d = format!(
                    "M{}{},{}L{}{},{}Z",
                    at(MOUTH_OUTER),
                    arc(MOUTH_OUTER, 1),
                    at(-MOUTH_OUTER),
                    at(-MOUTH_INNER),
                    arc(MOUTH_INNER, 0),
                    at(MOUTH_INNER)
                );
                features.push(
                    Elem::new("path")
                        .attr("class", "mouth")
                        .attr("fill", foreground)
                        .attr("stroke", stroke.clone())
                        .attr("d", d),
                );
            }
            g.push(features);
            g.push(band_rect(b, &format!("task task-type-{}", style.kind), &style.fill, &stroke));
            for (offset, name) in t.actors.iter().enumerate() {
                let i = actor_index(name);
                g.push(dot_circle(theme, b.x + 14.0 + offset as f64 * 10.0, b.y, i, &actor_colour(i)));
            }
            for label in centered_labels(&t.name, b.x, b.w, b.y, b.h, normal) {
                g.push(box_text(&label, "task", style.colour, normal));
            }
            doc.add(g);
        }
    }

    // title + timeline
    if has_title {
        doc.add(
            Elem::new("text")
                .attr("fill", foreground)
                .num("x", left)
                .num("y", 25.0)
                .attr("font-family", font_family)
                .attr("font-size", "32")
                .attr("font-weight", "bold")
                .content(title.as_deref().unwrap_or("")),
        );
    }
    let end_x = scene_width - left - 4.0;
    let edge = theme.color("edge_stroke").unwrap_or("");
    doc.add(
        Elem::new("line")
            .attr("stroke", edge)
            .attr("stroke-width", "4")
            .num("x1", left)
            .num("y1", TIMELINE_Y)
            .num("x2", end_x)
            .num("y2", TIMELINE_Y),
    );
    doc.add(Elem::new("path").attr("fill", edge).attr(
        "d",
        format!(
            "M {},{} V {} L {},{} Z",
            fmt_mode(end_x - 20.0, fl),
            fmt_mode(TIMELINE_Y - 8.0, false),
            fmt_mode(TIMELINE_Y + 8.0, false),
            fmt_mode(end_x + 4.0, fl),
            fmt_mode(TIMELINE_Y, false)
        ),
    ));
    Ok(doc.to_xml())
}
