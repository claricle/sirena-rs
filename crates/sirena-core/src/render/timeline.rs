//! Timeline renderer: port of Ruby `Parser::Native::Builders::Timeline`,
//! `Notation::Mermaid::IRAdapters::Timeline`, `Layout::Timeline*` and
//! `Renderer::Timeline`.
use super::pie::{fsum, items, is_slice, strip};
use crate::svg::{read_num, round4, ruby_float, Document, Elem, Theme};
use crate::text::timeline as tt;
use crate::Error;
use serde_json::Value;

const FONT_FAMILY: &str = "Trebuchet MS, Verdana, Arial, sans-serif";

fn unported() -> Error {
    Error::Unported("timeline".into())
}

// ---- parse tree -> sections / periods (Builders::Timeline + IR adapter) ----

#[derive(Default)]
struct Period {
    time: String,
    descriptions: Vec<String>,
}

#[derive(Default)]
struct Section {
    name: String,
    periods: Vec<Period>,
}

#[derive(Default)]
struct Model {
    title: Option<String>,
    sections: Vec<Section>,
    periods: Vec<Period>,
}

/// Builder `extract_text`: a slice's text, `"[]"` for an empty capture; stripped.
fn extract(v: &Value) -> Result<String, Error> {
    match v {
        Value::Array(a) if a.is_empty() => Ok("[]".into()),
        v if is_slice(v) => Ok(strip(v["value"].as_str().unwrap_or("")).to_string()),
        _ => Err(unported()),
    }
}

fn is_empty_capture(v: &Value) -> bool {
    matches!(v, Value::Array(a) if a.is_empty())
}

fn descriptions(v: Option<&Value>) -> Result<Vec<String>, Error> {
    let mut out = vec![];
    let Some(v) = v else { return Ok(out) };
    let list: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        v => vec![v],
    };
    for d in list {
        let text = extract(&d["desc"])?;
        if !text.is_empty() {
            out.push(text);
        }
    }
    Ok(out)
}

fn build(tree: &Value) -> Result<Model, Error> {
    let mut m = Model::default();
    let mut statements: Vec<&Value> = vec![tree];
    if let Some(s) = tree.get("statements") {
        statements = vec![tree];
        statements.extend(items(s));
    }
    // Which section collects events; `None` = top level.
    let mut current: Option<usize> = None;
    let mut last: Option<(Option<usize>, usize)> = None;
    for item in statements {
        for key in ["acc_title", "acc_descr"] {
            if let Some(v) = item.get(key) {
                if is_empty_capture(v) {
                    return Err(unported()); // Declined: the Ruby path owns the "[]" text
                }
            }
        }
        if let Some(v) = item.get("title") {
            m.title = Some(extract(v)?);
        }
        if let Some(v) = item.get("acc_title") {
            extract(v)?;
        }
        if let Some(v) = item.get("acc_descr") {
            extract(v)?;
        }
        if let Some(v) = item.get("section") {
            m.sections.push(Section { name: extract(v)?, periods: vec![] });
            current = Some(m.sections.len() - 1);
            last = None;
        }
        let push = |m: &mut Model, current: Option<usize>, p: Period| -> (Option<usize>, usize) {
            let list = match current {
                Some(i) => &mut m.sections[i].periods,
                None => &mut m.periods,
            };
            list.push(p);
            (current, list.len() - 1)
        };
        if let Some(e) = item.get("event_entry") {
            if !e.is_null() {
                let p = Period { time: extract(&e["time"])?, descriptions: descriptions(e.get("descriptions"))? };
                last = Some(push(&mut m, current, p));
            }
        }
        if let Some(c) = item.get("continuation_entry") {
            if !c.is_null() {
                let d = descriptions(c.get("descriptions"))?;
                match last {
                    Some((sec, idx)) => {
                        let list = match sec {
                            Some(i) => &mut m.sections[i].periods,
                            None => &mut m.periods,
                        };
                        list[idx].descriptions.extend(d);
                    }
                    None => {
                        last = Some(push(&mut m, current, Period { time: String::new(), descriptions: d }));
                    }
                }
            }
        }
        if let Some(v) = item.get("task") {
            let name = extract(v)?;
            if !name.is_empty() {
                push(&mut m, current, Period { time: name, descriptions: vec![] });
            }
        }
    }
    Ok(m)
}

// ---- layout ----

/// `TimelineRounding.tidy`.
fn tidy(v: f64) -> f64 {
    round4(v)
}

/// `value.to_i == value ? value.to_i.to_s : value.to_s`
fn number(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.0e15 {
        format!("{}", v as i64)
    } else {
        ruby_float(v)
    }
}

#[derive(Clone, Copy, Default)]
struct Line {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    stroke_width: f64,
    dashed: bool,
}

impl Line {
    fn settle(&mut self) {
        self.x1 = tidy(self.x1);
        self.x2 = tidy(self.x2);
        self.y1 = tidy(self.y1);
        self.y2 = tidy(self.y2);
    }

    fn shift(&mut self, dx: f64, dy: f64) {
        self.x1 += dx;
        self.x2 += dx;
        self.y1 += dy;
        self.y2 += dy;
        self.settle();
    }
}

struct Run {
    content: String,
    x: f64,
    y: Option<f64>,
}

struct Card {
    kind: &'static str,
    lines: Vec<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    text_width: f64,
    color_index: usize,
    font_size: f64,
    connector: Option<Line>,
    outline: String,
    rule: Line,
    text_x: f64,
    text_y: f64,
    runs: Vec<Run>,
}

const CORNER: f64 = 5.0;
const TEXT_TOP: f64 = 10.0;
const FIRST_BASELINE: f64 = 21.0;
const LINE_PITCH: f64 = 17.6;

fn path_number(v: f64) -> String {
    number(tidy(v))
}

impl Card {
    fn shift(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        if let Some(c) = &mut self.connector {
            c.shift(dx, dy);
        }
        self.place();
    }

    fn place(&mut self) {
        self.x = tidy(self.x);
        self.y = tidy(self.y);
        self.width = tidy(self.width);
        self.height = tidy(self.height);
        self.text_width = tidy(self.text_width);
        if let Some(c) = &mut self.connector {
            c.settle();
        }
        self.outline = [
            format!("M{} {}", path_number(self.x), path_number(self.y + self.height - CORNER)),
            format!("v{}", path_number(CORNER + CORNER - self.height)),
            format!("q0,-{} {},-{}", number(CORNER), number(CORNER), number(CORNER)),
            format!("h{}", path_number(self.width - 10.0)),
            format!("q{},0 {},{}", number(CORNER), number(CORNER), number(CORNER)),
            format!("v{}", path_number(self.height - CORNER)),
            format!("H{}", path_number(self.x)),
            "Z".to_string(),
        ]
        .join(" ");
        let bottom = tidy(self.y + self.height);
        self.rule = Line { x1: self.x, y1: bottom, x2: tidy(self.x + self.width), y2: bottom, ..Line::default() };
        self.text_x = tidy(self.x + self.width / 2.0);
        self.text_y = tidy(self.y + TEXT_TOP + self.scaled(FIRST_BASELINE));
        let pitch = self.scaled(LINE_PITCH);
        let baselines: Vec<f64> = (1..self.lines.len()).map(|i| tidy(self.text_y + i as f64 * pitch)).collect();
        self.runs = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, content)| Run {
                content: content.clone(),
                x: self.text_x,
                y: if i == 0 { None } else { Some(baselines[i - 1]) },
            })
            .collect();
    }

    fn scaled(&self, v: f64) -> f64 {
        let size = if self.font_size > 0.0 { self.font_size } else { tt::FONT_SIZE };
        v * size / tt::FONT_SIZE
    }

    fn x_range(&self) -> (f64, f64) {
        let reach = self.width.max(self.text_width);
        let middle = self.x + self.width / 2.0;
        (middle - reach / 2.0, middle + reach / 2.0)
    }

    fn bottom(&self) -> f64 {
        let b = self.y + self.height;
        match &self.connector {
            Some(c) => b.max(c.y2),
            None => b,
        }
    }
}

const LEFT: f64 = 200.0;
const TOP: f64 = 50.0;
const COLUMN: f64 = 200.0;
const TEXT_WIDTH: f64 = 150.0;
const CARD_WIDTH: f64 = 190.0;
const MIN_EVENT: f64 = 50.0;
const EVENT_GAP: f64 = 10.0;
const EVENT_DROP: f64 = 200.0;
const PERIOD_MEASURE: &str = "[object Object]";

struct Group<'a> {
    name: &'a str,
    periods: Vec<&'a Period>,
}

struct Flow<'a> {
    groups: Vec<Group<'a>>,
    sectioned: bool,
    font_size: f64,
    periods: Vec<&'a Period>,
    section_max: f64,
    task_max: f64,
    event_stack_max: f64,
}

fn fmax(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| if v > m { v } else { m }))).unwrap_or(0.0)
}

impl<'a> Flow<'a> {
    fn new(groups: Vec<Group<'a>>, sectioned: bool, font_size: f64) -> Self {
        let periods: Vec<&Period> = groups.iter().flat_map(|g| g.periods.iter().copied()).collect();
        let mut flow = Flow { groups, sectioned, font_size, periods, section_max: 0.0, task_max: 0.0, event_stack_max: 0.0 };
        flow.section_max = if flow.sectioned {
            fmax(flow.groups.iter().map(|g| flow.measured(g.name) + tt::PADDING))
        } else {
            0.0
        };
        flow.task_max = if flow.periods.is_empty() { 0.0 } else { flow.measured(PERIOD_MEASURE) + 20.0 };
        flow.event_stack_max = fmax(flow.periods.iter().map(|p| flow.stack(&p.descriptions)));
        flow
    }

    fn measured(&self, text: &str) -> f64 {
        tt::card_height(&self.wrap(text, TEXT_WIDTH), self.font_size)
    }

    fn wrap(&self, text: &str, width: f64) -> Vec<String> {
        tt::wrap(text, width, self.font_size)
    }

    fn stack(&self, events: &[String]) -> f64 {
        if events.is_empty() {
            return 0.0;
        }
        fsum(events.iter().map(|t| self.measured(t))) + EVENT_GAP * (events.len() as f64 - 1.0)
    }

    fn axis_y(&self) -> f64 {
        if !self.sectioned {
            return self.task_max + 100.0;
        }
        self.section_max + self.task_max + 150.0
    }

    fn card(&self, kind: &'static str, lines: Vec<String>, b: [f64; 4], color: usize) -> Card {
        let text_width = lines.iter().map(|l| tt::width_of(l, self.font_size)).fold(f64::MIN, f64::max);
        let mut c = Card {
            kind,
            lines: lines.into_iter().filter(|l| !l.is_empty()).collect(),
            x: b[0],
            y: b[1],
            width: b[2],
            height: b[3],
            text_width,
            color_index: color,
            font_size: self.font_size,
            connector: None,
            outline: String::new(),
            rule: Line::default(),
            text_x: 0.0,
            text_y: 0.0,
            runs: vec![],
        };
        c.place();
        c
    }

    fn cards(&self) -> Vec<Card> {
        if !self.sectioned {
            return self.period_cards(&self.periods, 0, [LEFT, TOP]);
        }
        let mut left = LEFT;
        let mut out = vec![];
        for (index, group) in self.groups.iter().enumerate() {
            let columns = (group.periods.len().max(1)) as f64;
            let origin = [left, TOP + self.section_max + 50.0];
            let width = COLUMN * columns - 50.0;
            let lines = self.wrap(group.name, width);
            let height = tt::card_height(&lines, self.font_size).max(self.section_max);
            out.push(self.card("section", lines, [left, TOP, width + 40.0, height], index));
            out.extend(self.period_cards(&group.periods, index, origin));
            left += COLUMN * columns;
        }
        out
    }

    fn period_cards(&self, periods: &[&Period], color: usize, origin: [f64; 2]) -> Vec<Card> {
        let mut running = self.task_max;
        let mut out = vec![];
        for (index, period) in periods.iter().enumerate() {
            let lines = self.wrap(&period.time, TEXT_WIDTH);
            let height = tt::card_height(&lines, self.font_size).max(running);
            let mut card = self.card("period", lines, [origin[0] + COLUMN * index as f64, origin[1], CARD_WIDTH, height], color);
            running = running.max(card.height);
            card.connector = Some(Line {
                x1: card.x + 95.0,
                y1: origin[1] + running,
                x2: card.x + 95.0,
                y2: origin[1] + running + EVENT_DROP + self.event_stack_max,
                stroke_width: 2.0,
                dashed: true,
            });
            let (px, py) = (card.x, card.y);
            out.push(card);
            let mut top = py + EVENT_DROP;
            for text in &period.descriptions {
                let lines = self.wrap(text, TEXT_WIDTH);
                let height = tt::card_height(&lines, self.font_size).max(MIN_EVENT);
                out.push(self.card("event", lines, [px, top, CARD_WIDTH, height], color));
                top += EVENT_GAP + height;
            }
        }
        out
    }
}

const AXIS_X: f64 = 150.0;
const FRAME: f64 = 50.0;
const TITLE_Y: f64 = 20.0;
const TITLE_ASCENT: f64 = 31.0;
const TITLE_SIZE: f64 = 33.0;

struct Scene {
    width: f64,
    height: f64,
    title: Option<String>,
    title_x: f64,
    title_y: f64,
    cards: Vec<Card>,
    axis: Line,
}

fn layout(m: &Model, theme: Theme) -> Scene {
    let sectioned = !m.sections.is_empty();
    let groups: Vec<Group> = if sectioned {
        m.sections.iter().map(|s| Group { name: &s.name, periods: s.periods.iter().collect() }).collect()
    } else {
        vec![Group { name: "", periods: m.periods.iter().collect() }]
    };
    let font_size = match theme.number("typography", "font_size_large") {
        Some(v) if v > 0.0 => v,
        _ => tt::FONT_SIZE,
    };
    let flow = Flow::new(groups, sectioned, font_size);
    let cards = flow.cards();
    let (left, right) = if cards.is_empty() {
        (0.0, 0.0)
    } else {
        (
            cards.iter().map(|c| c.x_range().0).fold(f64::INFINITY, f64::min),
            cards.iter().map(|c| c.x_range().1).fold(f64::NEG_INFINITY, f64::max),
        )
    };
    let axis = Line { x1: AXIS_X, y1: flow.axis_y(), x2: (right - left) + 3.0 * AXIS_X, y2: flow.axis_y(), stroke_width: 4.0, dashed: false };
    let mut scene = Scene {
        width: 0.0,
        height: 0.0,
        title: m.title.clone(),
        title_x: ((right - left) / 2.0) - AXIS_X,
        title_y: TITLE_Y,
        cards,
        axis,
    };
    // shift_to_origin
    let mut left_edge = scene.cards.iter().map(|c| c.x_range().0).fold(f64::INFINITY, f64::min).min(AXIS_X);
    if scene.title.is_some() {
        left_edge = left_edge.min(scene.title_x);
    }
    let mut top_edge = scene.cards.iter().map(|c| c.y).fold(f64::INFINITY, f64::min).min(scene.axis.y1);
    if scene.title.is_some() {
        top_edge = top_edge.min(TITLE_Y - TITLE_ASCENT);
    }
    let (dx, dy) = (FRAME - left_edge, FRAME - top_edge);
    for c in &mut scene.cards {
        c.shift(dx, dy);
    }
    scene.axis.shift(dx, dy);
    scene.title_x = tidy(scene.title_x + dx);
    scene.title_y = tidy(scene.title_y + dy);
    let bottom = scene.cards.iter().map(Card::bottom).fold(scene.axis.y1, f64::max);
    scene.width = tidy(scene.axis.x2 + FRAME);
    scene.height = tidy(bottom + FRAME);
    scene
}

// ---- renderer ----

/// Svg `<br>` splitting of `LineBreakText.fill`: `/<br\s*\/?>/i`, keeping empties.
fn split_breaks(raw: &str) -> Vec<String> {
    let b = raw.as_bytes();
    let mut out = vec![];
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        if b[i] == b'<' && i + 3 <= b.len() && b[i + 1..i + 3].eq_ignore_ascii_case(b"br") {
            let mut j = i + 3;
            while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
                j += 1;
            }
            if j < b.len() && b[j] == b'/' {
                j += 1;
            }
            if j < b.len() && b[j] == b'>' {
                out.push(strip(&raw[start..i]).to_string());
                start = j + 1;
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out.push(strip(&raw[start..]).to_string());
    out
}

fn line_group(line: &Line, theme: Theme) -> Elem {
    let stroke = theme.color("edge_stroke").unwrap_or("");
    let mut path = Elem::new("path")
        .attr("fill", "none")
        .attr("stroke", stroke)
        .attr("stroke-width", number(line.stroke_width))
        .attr(
            "d",
            format!("M {} {} L {} {}", number(line.x1), number(line.y1), number(line.x2), number(line.y2)),
        );
    if line.dashed {
        path = path.attr("stroke-dasharray", "5,5");
    }
    let mut group = Elem::new("g").attr("class", "lineWrapper").child(path);
    if let Some(head) = arrowhead(line, stroke) {
        group.push(head);
    }
    group
}

/// `Svg::Arrowhead` for a straight `M x1 y1 L x2 y2` path.
fn arrowhead(line: &Line, stroke: &str) -> Option<Elem> {
    let named = !stroke.trim().is_empty() && !stroke.trim().eq_ignore_ascii_case("none");
    // The path carries `number(stroke_width)`, which Arrowhead reads back.
    let sw = read_num(&number(line.stroke_width)).filter(|w| w.is_finite() && *w >= 0.0).unwrap_or(1.0);
    if !named || sw <= 0.0 {
        return None;
    }
    let (x1, y1) = (read_num(&number(line.x1))?, read_num(&number(line.y1))?);
    let (x2, y2) = (read_num(&number(line.x2))?, read_num(&number(line.y2))?);
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = dx.hypot(dy);
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let (dx, dy) = (dx / len, dy / len);
    let (length, half) = (4.0 * sw, 2.0 * sw);
    let base = (x2 - dx * length, y2 - dy * length);
    let off = (dy * half, -dx * half);
    let pts = [(x2, y2), (base.0 - off.0, base.1 - off.1), (base.0 + off.0, base.1 + off.1)];
    if !pts.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
        return None;
    }
    let points: Vec<String> = pts
        .iter()
        .map(|(x, y)| format!("{},{}", crate::svg::write_num(*x), crate::svg::write_num(*y)))
        .collect();
    Some(Elem::new("polygon").attr("fill", stroke).attr("points", points.join(" ")))
}

fn render_card(card: &Card, theme: Theme, doc: &mut Document) {
    let fill = theme.palette("timeline_fill_palette", card.color_index).unwrap_or("");
    let text_color = theme.palette("timeline_text_palette", card.color_index).unwrap_or("");
    let rule = theme.palette("timeline_rule_palette", card.color_index).unwrap_or("");
    let mut text = Elem::new("text")
        .attr("fill", text_color)
        .num4("x", card.text_x)
        .num4("y", card.text_y)
        .attr("font-family", FONT_FAMILY)
        .attr("font-size", number(card.font_size))
        .attr("text-anchor", "middle");
    if card.runs.len() < 2 {
        text = text.content(card.runs.first().map_or("", |r| r.content.as_str()));
    } else {
        for run in &card.runs {
            let mut span = Elem::new("tspan").num4("x", run.x).content(&run.content);
            if let Some(y) = run.y {
                span = span.num4("y", y);
            }
            text.push(span);
        }
    }
    let background = Elem::new("g")
        .child(Elem::new("path").attr("class", "node-bkg").attr("fill", fill).attr("d", card.outline.clone()))
        .child(
            Elem::new("line")
                .attr("class", "node-line")
                .attr("stroke", rule)
                .attr("stroke-width", "3")
                .num4("x1", card.rule.x1)
                .num4("y1", card.rule.y1)
                .num4("x2", card.rule.x2)
                .num4("y2", card.rule.y2),
        );
    let node = Elem::new("g").attr("class", "timeline-node").child(background).child(text);
    let mut wrapper = Elem::new("g");
    match card.kind {
        "period" => wrapper = wrapper.attr("class", "taskWrapper"),
        "event" => wrapper = wrapper.attr("class", "eventWrapper"),
        _ => {}
    }
    doc.add(wrapper.child(node));
    if let Some(c) = &card.connector {
        doc.add(line_group(c, theme));
    }
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "timeline")?;
    let model = build(&tree)?;
    let theme = Theme::default_theme();
    let scene = layout(&model, theme);

    let mut doc = Document::new(scene.width, scene.height);
    for card in &scene.cards {
        render_card(card, theme, &mut doc);
    }
    if let Some(title) = &scene.title {
        let mut text = Elem::new("text")
            .attr("fill", theme.color("label_text").unwrap_or(""))
            .num4("x", scene.title_x)
            .num4("y", scene.title_y)
            .attr("font-family", FONT_FAMILY)
            .attr("font-size", number(TITLE_SIZE))
            .attr("font-weight", "bold");
        let lines = split_breaks(title);
        if lines.len() == 1 {
            text = text.content(title);
        } else {
            for (i, line) in lines.iter().enumerate() {
                let mut span = Elem::new("tspan").num4("x", scene.title_x).content(line);
                if i > 0 {
                    span = span.attr("line-shift", "1");
                }
                text.push(span);
            }
        }
        doc.add(text);
    }
    doc.add(line_group(&scene.axis, theme));
    Ok(doc.to_xml())
}
