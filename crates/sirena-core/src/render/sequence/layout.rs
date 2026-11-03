//! `Layout::Sequence` geometry and `Renderer::Sequence` drawing.
use super::frames::{BoxShape, FrameShape, Frames, NoteSlots, BOX_BOTTOM_PAD};
use super::model::{Diagram, Message, Note, Participant};
use super::num::{fl, i, nsum, N};
use super::text;
use crate::svg::{Document, Elem, Theme};
use std::collections::HashMap;

const FONT: f64 = 16.0;
const ACTOR_WIDTH: i64 = 150;
const ACTOR_HEIGHT: i64 = 65;
const DIAGRAM_MARGIN_X: i64 = 50;
const DIAGRAM_MARGIN_Y: i64 = 10;
const MESSAGE_PITCH: i64 = 44;
const NOTE_MARGIN: i64 = 10;
const LIFELINE_TAIL: i64 = 20;
const CANVAS_TRIM: i64 = 1;
const FIRST_ROW: i64 = DIAGRAM_MARGIN_Y + ACTOR_HEIGHT;
const LABEL_PADDING: i64 = 20;
const PARTICIPANT_MARGIN: i64 = 50;
const CANVAS_PAD: i64 = 40;
const ARROW_SIZE: i64 = 8;
const SELF_LOOP_WIDTH: i64 = 60;
const SELF_LOOP_HEIGHT: i64 = 20;
const ACTOR_LABEL_OFFSET: i64 = 55;
const TEXT_PADDING: i64 = 10;
const WRAP_LIMIT: f64 = (ACTOR_WIDTH - 2 * TEXT_PADDING) as f64;
const LINE_ADVANCE: i64 = 17; // (16 * 1.09).round
const LINE_PITCH: i64 = 19; // (16 * 1.2).round
const ACTIVATION_EDGE: i64 = 2;
const HEAD_INSET: i64 = 3;
const ROW_PADDING: i64 = 20;
const SELF_DROP: i64 = 30;
const EMPTY_DROP: i64 = 34;
const NOTE_PADDING: i64 = 20;
const NOTE_SIDE_GAP: i64 = 25;

#[derive(Clone, Copy)]
struct Pos {
    x: N,
    y: N,
    cx: N,
}

// ---- participant widths and gaps ----

fn participant_width(p: &Participant) -> N {
    let lines = text::split_breaks(&p.label);
    let padded = (text::widest(&lines, FONT) + LABEL_PADDING) as f64;
    i(ACTOR_WIDTH).max(fl(padded))
}

fn neighbour(len: usize, at: usize, step: i64) -> Option<usize> {
    let to = at as i64 + step;
    (to >= 0 && (to as usize) < len).then_some(to as usize)
}

fn claim_width(source: &str, wrap: bool) -> i64 {
    let body = text::body(source);
    let lines = if text::wrapped(source, wrap) {
        text::wrap_lines(&body, WRAP_LIMIT, FONT)
    } else {
        text::split_breaks(&body)
    };
    text::widest(&lines, FONT) + 2 * TEXT_PADDING
}

/// The participants a `create` statement declares: the row of the
/// message that creates each, and how far short of its centre that
/// message's arrow stops.
struct Creations {
    rows: Vec<(usize, usize)>,
    gaps: HashMap<usize, N>,
}

impl Creations {
    fn new(d: &Diagram, widths: &[N]) -> Creations {
        let mut rows = vec![];
        let mut gaps = HashMap::new();
        for (k, p) in d.participants.iter().enumerate() {
            if !p.created {
                continue;
            }
            let Some(row) = d.messages.iter().position(|m| m.to == p.id) else { continue };
            rows.push((k, row));
            let half = if p.actor { i(18) } else { fl(widths[k].f() / 2.0) };
            gaps.insert(k, half + 3);
        }
        Creations { rows, gaps }
    }

    fn rows(&self) -> Vec<usize> {
        self.rows.iter().map(|(_, r)| *r).collect()
    }

    fn has(&self, k: usize) -> bool {
        self.rows.iter().any(|(p, _)| *p == k)
    }

    fn row(&self, k: usize) -> Option<usize> {
        self.rows.iter().find(|(p, _)| *p == k).map(|(_, r)| *r)
    }
}

struct Gaps {
    after: Vec<N>,
}

impl Gaps {
    fn new(d: &Diagram, widths: &[N], creations: &Creations) -> Gaps {
        let n = d.participants.len();
        let index: HashMap<&str, usize> =
            d.participants.iter().enumerate().map(|(k, p)| (p.id.as_str(), k)).collect();
        let mut claims: Vec<(usize, f64)> = vec![];
        for m in &d.messages {
            let (Some(&from), Some(&to)) = (index.get(m.from.as_str()), index.get(m.to.as_str())) else { continue };
            let width = claim_width(&m.text, d.wrap) as f64;
            if from == to {
                claims.push((from, width / 2.0));
            } else if neighbour(n, to, 1) == Some(from) {
                claims.push((to, width));
            } else if neighbour(n, to, -1) == Some(from) {
                claims.push((from, width));
            }
        }
        for note in &d.notes {
            let ids: Vec<usize> = note.participant_ids.iter().filter_map(|id| index.get(id.as_str()).copied()).collect();
            let (Some(&first), Some(&last)) = (ids.first(), ids.last()) else { continue };
            let width = claim_width(&note.text, d.wrap) as f64;
            let (before, after) = (neighbour(n, last, -1), neighbour(n, last, 1));
            match note.position {
                "right_of" => {
                    if after.is_some() {
                        claims.push((first, width));
                    }
                }
                "left_of" => {
                    if let Some(b) = before {
                        claims.push((b, width));
                    }
                }
                _ => {
                    let half = width / 2.0;
                    if let Some(b) = before {
                        claims.push((b, half));
                    }
                    if after.is_some() {
                        claims.push((first, half));
                    }
                }
            }
        }
        let mut best: HashMap<usize, f64> = HashMap::new();
        for (k, w) in claims {
            let e = best.entry(k).or_insert(w);
            if w > *e {
                *e = w;
            }
        }
        let after = (0..n)
            .map(|k| match neighbour(n, k, 1) {
                None => i(0),
                Some(next) => {
                    let half_boxes = (widths[k] + widths[next]).f() / 2.0;
                    let claim = best.get(&k).copied().unwrap_or(0.0);
                    fl(claim - half_boxes).max(i(0))
                        + if creations.has(next) { fl(widths[next].f() / 2.0) } else { i(0) }
                }
            })
            .collect();
        Gaps { after }
    }
}

// ---- message rows ----

struct Rows {
    lines: Vec<Vec<String>>,
    wrap: bool,
    self_msg: Vec<bool>,
    closers: Vec<i64>,
    creators: Vec<usize>,
}

impl Rows {
    fn new(d: &Diagram, cx: &HashMap<&str, N>, creations: &Creations) -> Rows {
        let lines = d.messages.iter().map(|m| Self::edge_lines(d.wrap, m, cx)).collect();
        let self_msg = d.messages.iter().map(|m| m.from == m.to).collect();
        let closers = d.frames.iter().map(|f| f.stop as i64 - 1).collect();
        Rows { lines, wrap: d.wrap, self_msg, closers, creators: creations.rows() }
    }

    fn edge_lines(wrap: bool, m: &Message, cx: &HashMap<&str, N>) -> Vec<String> {
        let body = text::body(&m.text);
        let raw = if text::wrapped(&m.text, wrap) {
            text::wrap_lines(&body, Self::limit(m, cx).f(), FONT)
        } else {
            text::split_breaks(&body)
        };
        raw.iter().map(|l| text::decode(l)).collect()
    }

    fn limit(m: &Message, cx: &HashMap<&str, N>) -> N {
        (Self::bounded(m, cx) + ROW_PADDING).max(i(ACTOR_WIDTH))
    }

    fn bounded(m: &Message, cx: &HashMap<&str, N>) -> N {
        if m.from == m.to {
            return i(0);
        }
        let (Some(a), Some(b)) = (cx.get(m.from.as_str()), cx.get(m.to.as_str())) else { return i(0) };
        let flat = matches!(m.head_style, "none" | "stick_top" | "stick_bottom");
        let reverse = m.head_side == "source";
        let mut trim = ACTIVATION_EDGE;
        if !(flat || reverse) {
            trim += HEAD_INSET;
        }
        let turned = matches!(m.head_style, "half_top" | "half_bottom");
        if m.head_side == "both" || (reverse && turned) {
            trim += HEAD_INSET;
        }
        ((*a - *b).abs() - trim).max(i(0))
    }

    fn lines(&self, index: usize) -> &[String] {
        self.lines.get(index).map(Vec::as_slice).unwrap_or(&[])
    }

    fn extra(&self, index: usize) -> i64 {
        if self.lines(index).iter().all(|l| crate::render::pie::strip(l).is_empty()) {
            return -EMPTY_DROP;
        }
        (self.lines(index).len() as i64 - 1).max(0) * LINE_ADVANCE
    }

    fn self_drop(&self, index: usize) -> i64 {
        if !self.self_msg.get(index).copied().unwrap_or(false) {
            return 0;
        }
        if self.closers.contains(&(index as i64)) { 2 * SELF_DROP } else { SELF_DROP }
    }

    /// Half a box below the message that creates a participant.
    fn creation_drop(&self, index: usize) -> N {
        if self.creators.contains(&index) { fl(ACTOR_HEIGHT as f64 / 2.0) } else { i(0) }
    }

    fn extra_before(&self, index: usize) -> N {
        nsum((0..index).map(|at| i(self.extra(at) + self.self_drop(at)) + self.creation_drop(at)))
    }

    fn extra_through(&self, index: usize) -> N {
        self.extra_before(index) + self.extra(index)
    }

    fn total_extra(&self) -> N {
        self.extra_before(self.lines.len())
    }
}

// ---- notes ----

struct NoteGeom {
    order: usize,
    x: N,
    y: N,
    width: N,
    height: i64,
    lines: Vec<(String, N, N)>,
}

struct NoteBox {
    lines: Vec<String>,
    width: N,
    left: N,
}

fn note_box(note: &Note, centers: &[N], wrap: bool, actor_width: N) -> NoteBox {
    let body = text::body(&note.text);
    let wrapped = !body.is_empty() && text::wrapped(&note.text, wrap);
    let plain = text::split_breaks(&body);
    let widest = |lines: &[String]| fl(text::widest(lines, FONT) as f64);
    let unwrapped_width = || (widest(&plain) + NOTE_PADDING).max(i(ACTOR_WIDTH));
    let min_width = i(ACTOR_WIDTH);
    let (width, left) = match note.position {
        "left_of" | "right_of" => {
            let side: i64 = if note.position == "right_of" { 1 } else { -1 };
            let wide = if !wrapped {
                unwrapped_width()
            } else {
                let text_w = widest(&text::wrap_lines(&body, ACTOR_WIDTH as f64, FONT));
                (text_w + if side < 0 { NOTE_PADDING } else { 0 }).max(min_width)
            };
            if side > 0 {
                (wide, centers[0] + NOTE_SIDE_GAP)
            } else {
                (wide, centers[0] - NOTE_SIDE_GAP - wide)
            }
        }
        _ => {
            let min = centers.iter().copied().fold(centers[0], |a, b| if b.f() < a.f() { b } else { a });
            let max = centers.iter().copied().fold(centers[0], |a, b| if b.f() > a.f() { b } else { a });
            let span = max - min;
            let width = if wrapped && span.f() > 0.0 {
                span + 2 * NOTE_SIDE_GAP
            } else if wrapped {
                actor_width.max(min_width)
            } else {
                unwrapped_width().max(span + 2 * NOTE_SIDE_GAP)
            };
            (width, fl((min + max).f() / 2.0) - width.half())
        }
    };
    let lines = if wrapped && !centers.is_empty() {
        text::wrap_lines(&body, (width - NOTE_PADDING).f(), FONT)
    } else {
        plain
    };
    NoteBox { lines, width, left }
}

struct Placement {
    notes: Vec<NoteGeom>,
    slots: Vec<i64>,
    message_index: Vec<usize>,
    orders: Vec<usize>,
}

impl Placement {
    fn new(d: &Diagram, pos: &HashMap<&str, Pos>, rows: &Rows, frames: &Frames) -> Placement {
        let boxes: Vec<(NoteBox, Vec<N>)> = d
            .notes
            .iter()
            .map(|n| {
                let centers: Vec<N> = n.participant_ids.iter().filter_map(|id| pos.get(id.as_str()).map(|p| p.cx)).collect();
                let actor_width = n
                    .participant_ids
                    .first()
                    .and_then(|id| pos.get(id.as_str()))
                    .map(|p| (p.cx - p.x) * 2)
                    .unwrap_or(i(ACTOR_WIDTH));
                let b = if centers.is_empty() {
                    NoteBox { lines: text::split_breaks(&text::body(&n.text)), width: i(0), left: i(0) }
                } else {
                    note_box(n, &centers, rows.wrap, actor_width)
                };
                (b, centers)
            })
            .collect();
        let height = |lines: usize| lines as i64 * LINE_PITCH + NOTE_PADDING;
        let slots: Vec<i64> = boxes.iter().map(|(b, _)| height(b.lines.len()) + NOTE_MARGIN).collect();
        let mut notes = vec![];
        for (index, (b, centers)) in boxes.iter().enumerate() {
            if centers.is_empty() {
                continue;
            }
            let own = d.notes[index].message_index;
            let above: i64 = (0..index).filter(|at| d.notes[*at].message_index <= own).map(|at| slots[at]).sum();
            let lead = frames.lead(own, d.notes[index].order);
            let frame_offset = frames.top_inset()
                + frames.row_shift(own as i64 - 1)
                + lead
                + if lead == 0 { 0 } else { NOTE_MARGIN };
            let top = i(FIRST_ROW + own as i64 * MESSAGE_PITCH + above + NOTE_MARGIN) + rows.extra_before(own) + frame_offset;
            let center = b.left + b.width.half();
            let lines = b
                .lines
                .iter()
                .enumerate()
                .map(|(row, line)| {
                    let y = top + (NOTE_PADDING / 2) + fl(LINE_PITCH as f64 * (row as f64 + 0.5));
                    (line.clone(), center, y)
                })
                .collect();
            // `Layout::Sequence::Note` types its geometry as Float.
            notes.push(NoteGeom { order: d.notes[index].order, x: fl(b.left.f()), y: fl(top.f()), width: fl(b.width.f()), height: height(b.lines.len()), lines });
        }
        Placement {
            notes,
            slots,
            message_index: d.notes.iter().map(|n| n.message_index).collect(),
            orders: d.notes.iter().map(|n| n.order).collect(),
        }
    }

    fn total_height(&self) -> i64 {
        self.slots.iter().sum()
    }

    fn shift_for(&self, index: usize) -> i64 {
        (0..self.slots.len()).filter(|at| self.message_index[*at] <= index).map(|at| self.slots[at]).sum()
    }

    fn overhang(&self) -> N {
        let Some(left) = self.notes.iter().map(|n| n.x).reduce(|a, b| if b.f() < a.f() { b } else { a }) else { return i(0) };
        if left.f() < DIAGRAM_MARGIN_X as f64 { i(DIAGRAM_MARGIN_X) - left } else { i(0) }
    }

    fn right_edge(&self) -> N {
        self.notes.iter().map(|n| n.x + n.width).reduce(|a, b| if b.f() > a.f() { b } else { a }).unwrap_or(i(0))
    }
}

impl NoteSlots for Placement {
    fn slots_before(&self, index: usize, order: usize) -> i64 {
        (0..self.slots.len())
            .filter(|at| self.message_index[*at] == index && self.orders[*at] < order)
            .map(|at| self.slots[at])
            .sum()
    }

    fn x_range(&self, from: usize, to: usize) -> Option<(N, N)> {
        let inside: Vec<&NoteGeom> = self.notes.iter().filter(|n| n.order > from && n.order < to).collect();
        let low = inside.iter().map(|n| n.x).reduce(|a, b| if b.f() < a.f() { b } else { a })?;
        let high = inside.iter().map(|n| n.x + n.width).reduce(|a, b| if b.f() > a.f() { b } else { a })?;
        Some((low, high))
    }
}

// ---- scene ----

enum Head {
    Poly(String),
    Lines(Vec<[f64; 4]>),
}

struct Msg {
    id: String,
    dotted: bool,
    shaft: Option<[f64; 4]>,
    loop_path: Option<String>,
    heads: Vec<Head>,
    label: Option<(f64, f64, String, Vec<(f64, String)>)>,
}

struct Part {
    id: String,
    x: N,
    y: N,
    width: N,
    cx: N,
    actor: bool,
    shape: Option<String>,
    label: String,
}

pub struct Scene {
    width: N,
    height: N,
    view_box: String,
    lifelines: Vec<[N; 4]>,
    messages: Vec<Msg>,
    participants: Vec<Part>,
    notes: Vec<NoteGeom>,
    frames: Vec<FrameShape>,
    boxes: Vec<BoxShape>,
}

fn positions(d: &Diagram, widths: &[N], gaps: &Gaps, frames: &Frames, shift: N) -> Vec<Pos> {
    let mut cursor = i(DIAGRAM_MARGIN_X) + shift;
    let mut out = vec![];
    for (k, p) in d.participants.iter().enumerate() {
        let left = cursor + frames.gap_before(&p.id);
        let width = widths[k];
        cursor = left + (width + PARTICIPANT_MARGIN + frames.gap_after(&p.id) + gaps.after[k]);
        out.push(Pos { x: left, y: i(DIAGRAM_MARGIN_Y + frames.top_inset()), cx: left + width.half() });
    }
    out
}

fn by_id<'a>(d: &'a Diagram, pos: &[Pos]) -> HashMap<&'a str, Pos> {
    d.participants.iter().zip(pos).map(|(p, q)| (p.id.as_str(), *q)).collect()
}

pub fn scene(d: &Diagram) -> Scene {
    let widths: Vec<N> = d.participants.iter().map(participant_width).collect();
    let creations = Creations::new(d, &widths);
    let gaps = Gaps::new(d, &widths, &creations);
    let mut frames = Frames::new(d);
    let first = positions(d, &widths, &gaps, &frames, i(0));
    let first_map = by_id(d, &first);
    let cx: HashMap<&str, N> = first_map.iter().map(|(k, p)| (*k, p.cx)).collect();
    frames.fit_titles(&cx);
    let rows = Rows::new(d, &cx, &creations);
    let mut placement = Placement::new(d, &first_map, &rows, &frames);
    let shift = placement.overhang();
    let mut pos = first;
    if !shift.is_zero() {
        pos = positions(d, &widths, &gaps, &frames, shift);
        placement = Placement::new(d, &by_id(d, &pos), &rows, &frames);
    }
    let map = by_id(d, &pos);
    let cx: HashMap<&str, N> = map.iter().map(|(k, p)| (*k, p.cx)).collect();
    let count = d.messages.len() as i64;
    let stack = rows.total_extra() + placement.total_height();
    let origin_y = DIAGRAM_MARGIN_Y + frames.top_inset();

    // canvas
    let n = d.participants.len() as i64;
    let canvas_w = if n == 0 {
        i(0)
    } else {
        let span = nsum(widths.iter().copied())
            + (n - 1) * PARTICIPANT_MARGIN
            + nsum(gaps.after.iter().take(d.participants.len() - 1).copied());
        span + (2 * DIAGRAM_MARGIN_X) - CANVAS_PAD + frames.extra_width()
    };
    let lifeline_length = |note_height: N| note_height + (count * MESSAGE_PITCH + LIFELINE_TAIL + frames.total_shift());
    let canvas_h = if n == 0 {
        0
    } else {
        2 * DIAGRAM_MARGIN_Y
            + ACTOR_HEIGHT * 2
            + lifeline_length(i(0)).f() as i64
            + frames.top_inset()
            + frames.bottom_inset()
            + CANVAS_TRIM
            - CANVAS_PAD
    };
    let wide = canvas_w + CANVAS_PAD + shift;
    let edge = if placement.notes.is_empty() { i(0) } else { placement.right_edge() + DIAGRAM_MARGIN_X };
    let width = wide.max(edge);
    let height = stack + (canvas_h + CANVAS_PAD);

    let lifeline_end = i(origin_y + ACTOR_HEIGHT) + lifeline_length(stack);
    let message_vertical =
        |index: usize| origin_y + ACTOR_HEIGHT + (index as i64 + 1) * MESSAGE_PITCH + frames.row_shift(index as i64);
    // A created participant sits on its creating message's row.
    let created: Vec<Pos> = pos
        .iter()
        .enumerate()
        .map(|(k, p)| match creations.row(k) {
            Some(row) => {
                let line_y = i(message_vertical(row) + placement.shift_for(row)) + rows.extra_through(row);
                Pos { y: line_y - fl(ACTOR_HEIGHT as f64 / 2.0), ..*p }
            }
            None => *p,
        })
        .collect();
    let lifelines = created.iter().map(|p| [p.cx, p.y + ACTOR_HEIGHT, p.cx, lifeline_end]).collect();

    let index_of: HashMap<&str, usize> = d.participants.iter().enumerate().map(|(k, p)| (p.id.as_str(), k)).collect();
    let messages = d
        .messages
        .iter()
        .enumerate()
        .map(|(index, m)| {
            let s = map[m.from.as_str()].cx;
            let mut t = map[m.to.as_str()].cx;
            let target = index_of[m.to.as_str()];
            if creations.row(target) == Some(index) {
                let direction = if s.f() <= t.f() { -1 } else { 1 };
                t = t + creations.gaps[&target] * direction;
            }
            let vertical = i(message_vertical(index) + placement.shift_for(index)) + rows.extra_through(index);
            build_message(m, index, s, t, vertical, &rows)
        })
        .collect();

    let row_y = |index: usize| (i(message_vertical(index) + placement.shift_for(index)) + rows.extra_before(index)).f();
    let frame_shapes = frames.frame_shapes(&cx, &row_y, &placement);
    let lefts: HashMap<&str, N> = map.iter().map(|(k, p)| (*k, p.x)).collect();
    let width_of: HashMap<&str, N> =
        d.participants.iter().zip(&widths).map(|(p, w)| (p.id.as_str(), *w)).collect();
    let box_shapes = frames.box_shapes(&lefts, &width_of, (lifeline_end + BOX_BOTTOM_PAD).f());

    let participants = d
        .participants
        .iter()
        .zip(&created)
        .zip(&widths)
        .map(|((p, q), w)| Part {
            id: p.id.clone(),
            x: q.x,
            y: q.y,
            width: *w,
            cx: q.cx,
            actor: p.actor,
            shape: p.shape.clone().filter(|s| matches!(s.as_str(), "boundary" | "control" | "entity")),
            label: p.label.clone(),
        })
        .collect();

    Scene {
        view_box: format!("0 0 {} {}", width.show(), height.show()),
        width,
        height,
        lifelines,
        messages,
        participants,
        notes: placement.notes,
        frames: frame_shapes,
        boxes: box_shapes,
    }
}

fn cmp_dir(a: N, b: N) -> i64 {
    a.cmp(b)
}

fn head_ends(head: &str, side: &str) -> Vec<bool> {
    // true = target end
    if head == "none" {
        return vec![];
    }
    match side {
        "source" => vec![false],
        "both" => vec![false, true],
        _ => vec![true],
    }
}

fn head_geometry(target: bool, span: [N; 4], head: &str) -> Head {
    let [x1, y1, x2, y2] = span;
    let (tip_x, tip_y, from_x) = if target { (x2, y2, x1) } else { (x1, y1, x2) };
    let side = cmp_dir(tip_x, from_x);
    let size = ARROW_SIZE;
    match head {
        "cross" => {
            let half = size / 2;
            Head::Lines(vec![
                [(tip_x - half).f(), (tip_y - half).f(), (tip_x + half).f(), (tip_y + half).f()],
                [(tip_x - half).f(), (tip_y + half).f(), (tip_x + half).f(), (tip_y - half).f()],
            ])
        }
        "open" => {
            let dir = if tip_x.f() >= from_x.f() { 1 } else { -1 };
            let back = tip_x - dir * size;
            let notch = tip_x - fl((dir * size) as f64 * 0.6);
            Head::Poly(format!(
                "{},{} {},{} {},{} {},{}",
                tip_x.show(),
                tip_y.show(),
                back.show(),
                (tip_y - size / 2).show(),
                notch.show(),
                tip_y.show(),
                back.show(),
                (tip_y + size / 2).show()
            ))
        }
        "half_bottom" | "half_top" => {
            let oriented = if head.ends_with("bottom") { side } else { -side };
            let dir = if tip_x.f() >= from_x.f() { 1 } else { -1 };
            let back = tip_x - dir * size;
            Head::Poly(format!(
                "{},{} {},{} {},{}",
                tip_x.show(),
                tip_y.show(),
                back.show(),
                (tip_y + oriented * size / 2).show(),
                back.show(),
                tip_y.show()
            ))
        }
        "stick_bottom" | "stick_top" => {
            let oriented = if head.ends_with("bottom") { side } else { -side };
            let dir = if tip_x.f() >= from_x.f() { 1 } else { -1 };
            Head::Lines(vec![[
                tip_x.f(),
                tip_y.f(),
                (tip_x - dir * size).f(),
                (tip_y + oriented * size / 2).f(),
            ]])
        }
        _ => {
            let dir = if tip_x.f() > from_x.f() { 1 } else { -1 };
            let back = tip_x - dir * size;
            Head::Poly(format!(
                "{},{} {},{} {},{}",
                tip_x.show(),
                tip_y.show(),
                back.show(),
                (tip_y - size / 2).show(),
                back.show(),
                (tip_y + size / 2).show()
            ))
        }
    }
}

fn build_message(m: &Message, index: usize, s: N, t: N, vertical: N, rows: &Rows) -> Msg {
    let dotted = m.line_style == "dotted";
    let ends = head_ends(m.head_style, m.head_side);
    let (shaft, loop_path, heads) = if s.eq(t) {
        let top = vertical;
        let bottom = vertical + SELF_LOOP_HEIGHT;
        let reach = s + SELF_LOOP_WIDTH;
        let heads = ends
            .iter()
            .map(|target| {
                let edge_y = if *target { bottom } else { top };
                head_geometry(true, [s + ARROW_SIZE, edge_y, s, edge_y], m.head_style)
            })
            .collect();
        let path = format!(
            "M {},{} C {},{} {},{} {},{}",
            s.show(),
            top.show(),
            reach.show(),
            (top - 10).show(),
            reach.show(),
            (bottom + 10).show(),
            s.show(),
            bottom.show()
        );
        (None, Some(path), heads)
    } else {
        let span = [s, vertical, t, vertical];
        let flush = matches!(m.head_style, "cross" | "open" | "stick_top" | "stick_bottom");
        let direction = t.cmp(s);
        let inset = (if flush { 0 } else { ARROW_SIZE }) * direction;
        let x1 = s + if ends.contains(&false) { inset } else { direction };
        let x2 = t - if ends.contains(&true) { inset } else { 0 };
        let heads = ends.iter().map(|target| head_geometry(*target, span, m.head_style)).collect();
        (Some([x1.f(), vertical.f(), x2.f(), vertical.f()]), None, heads)
    };
    let shown = text::display(&m.text);
    let label = (!shown.is_empty()).then(|| {
        let x = (s + t).half().f();
        let top = (vertical - 10 - rows.extra(index)).f();
        let drawn = rows.lines(index);
        let runs = if drawn.len() > 1 {
            drawn.iter().enumerate().map(|(row, l)| (top + (row as i64 * LINE_PITCH) as f64, l.clone())).collect()
        } else {
            vec![]
        };
        (x, top, shown, runs)
    });
    Msg { id: format!("message-{index}"), dotted, shaft, loop_path, heads, label }
}

// ---- drawing ----

struct Colors {
    edge: &'static str,
    node_fill: &'static str,
    node_stroke: &'static str,
    label: &'static str,
}

fn line_elem(l: [f64; 4], width: &str, dash: Option<&str>, stroke: &str) -> Elem {
    let mut e = Elem::new("line")
        .attr("stroke", stroke)
        .attr("stroke-width", width)
        .num("x1", l[0])
        .num("y1", l[1])
        .num("x2", l[2])
        .num("y2", l[3]);
    if let Some(d) = dash {
        e = e.attr("stroke-dasharray", d);
    }
    e
}

fn text_elem(c: &Colors, x: f64, y: f64, content: &str) -> Elem {
    Elem::new("text")
        .attr("fill", c.label)
        .num("x", x)
        .num("y", y)
        .content(content)
        .attr("text-anchor", "middle")
        .attr("font-family", "Arial, sans-serif")
        .attr("font-size", "16")
}

fn circle(c: &Colors, cx: f64, cy: f64, r: f64, fill: &str) -> Elem {
    Elem::new("circle")
        .attr("fill", fill)
        .attr("stroke", c.node_stroke)
        .attr("stroke-width", "2")
        .num("cx", cx)
        .num("cy", cy)
        .num("r", r)
}

impl Scene {
    pub fn draw(&self) -> String {
        let theme = Theme::default_theme();
        let c = Colors {
            edge: theme.color("edge_stroke").unwrap_or("#000000"),
            node_fill: theme.color("node_fill").unwrap_or("#ffffff"),
            node_stroke: theme.color("node_stroke").unwrap_or("#000000"),
            label: theme.color("label_text").unwrap_or("#000000"),
        };
        let mut doc = Document::new(self.width.f(), self.height.f());
        doc.view_box = Some(self.view_box.clone());
        for (index, b) in self.boxes.iter().enumerate() {
            doc.add(box_group(index, b, &c));
        }
        for l in &self.lifelines {
            doc.add(line_elem([l[0].f(), l[1].f(), l[2].f(), l[3].f()], "1", Some("5,5"), c.edge));
        }
        for (index, f) in self.frames.iter().enumerate() {
            doc.add(frame_group(index, f, &c));
        }
        for m in &self.messages {
            doc.add(self.message(m, &c));
        }
        for p in &self.participants {
            doc.add(self.participant(p, &c));
        }
        for n in &self.notes {
            doc.add(note_group(n));
        }
        doc.to_xml()
    }

    fn message(&self, m: &Msg, c: &Colors) -> Elem {
        let dash = m.dotted.then_some("5,5");
        let mut g = Elem::new("g").attr("id", m.id.clone());
        if let Some(path) = &m.loop_path {
            let mut p = Elem::new("path").attr("fill", "none").attr("stroke", c.edge).attr("stroke-width", "2").attr("d", path.clone());
            if let Some(d) = dash {
                p = p.attr("stroke-dasharray", d);
            }
            g.push(p);
        } else if let Some(s) = m.shaft {
            g.push(line_elem(s, "2", dash, c.edge));
        }
        for head in &m.heads {
            match head {
                Head::Poly(points) => g.push(
                    Elem::new("polygon").attr("fill", c.edge).attr("stroke", c.edge).attr("points", points.clone()),
                ),
                Head::Lines(lines) => lines.iter().for_each(|l| g.push(line_elem(*l, "2", None, c.edge))),
            }
        }
        if let Some((x, y, shown, runs)) = &m.label {
            let t = if runs.is_empty() {
                text_elem(c, *x, *y, shown)
            } else {
                let mut t = text_elem(c, *x, *y, "");
                for (ry, line) in runs {
                    t.push(Elem::new("tspan").num("x", *x).num("y", *ry).content(line));
                }
                t
            };
            g.push(t);
        }
        g
    }

    fn participant(&self, p: &Part, c: &Colors) -> Elem {
        let mut g = Elem::new("g").attr("id", format!("participant-{}", p.id));
        let label_y;
        if let Some(shape) = &p.shape {
            let (cx, top) = (p.cx.f(), p.y.f());
            let (circle_dy, offset) = match shape.as_str() {
                "boundary" => (33.0, 63),
                "control" => (32.0, 61),
                _ => (31.0, 63),
            };
            g.push(circle(c, cx, top + circle_dy, 22.0, c.node_fill));
            let lines: Vec<[f64; 4]> = match shape.as_str() {
                "boundary" => vec![[cx - 55.0, top + 33.0, cx - 15.0, top + 33.0], [cx - 55.0, top + 23.0, cx - 55.0, top + 43.0]],
                "control" => vec![[cx - 4.0, top + 13.0, cx + 4.0, top + 13.0], [cx + 4.0, top + 13.0, cx + 1.0, top + 17.0]],
                _ => vec![[cx - 22.0, top + 53.0, cx + 22.0, top + 53.0]],
            };
            for l in lines {
                g.push(line_elem(l, "2", None, c.node_stroke));
            }
            label_y = p.y + offset;
        } else if p.actor {
            let (cx, top) = (p.cx.f(), p.y.f());
            g.push(circle(c, cx, top + 10.0, 15.0, "none"));
            let body_top = top + 25.0;
            let body_bottom = body_top + 20.0;
            for l in [
                [cx, body_top, cx, body_bottom],
                [cx - 18.0, body_top + 8.0, cx + 18.0, body_top + 8.0],
                [cx, body_bottom, cx - 18.0, body_bottom + 15.0],
                [cx, body_bottom, cx + 16.0, body_bottom + 15.0],
            ] {
                g.push(line_elem(l, "2", None, c.node_stroke));
            }
            label_y = p.y + ACTOR_LABEL_OFFSET;
        } else {
            g.push(
                Elem::new("rect")
                    .attr("fill", c.node_fill)
                    .attr("stroke", c.node_stroke)
                    .attr("stroke-width", "2")
                    .num("x", p.x.f())
                    .num("y", p.y.f())
                    .num("width", p.width.f())
                    .num("height", ACTOR_HEIGHT as f64)
                    .num("rx", 5.0)
                    .num("ry", 5.0),
            );
            label_y = p.y + ACTOR_HEIGHT / 2;
        }
        g.push(text_elem(c, p.cx.f(), label_y.f(), &p.label).attr("dominant-baseline", "middle"));
        g
    }
}

fn note_group(n: &NoteGeom) -> Elem {
    let mut g = Elem::new("g");
    g.push(
        Elem::new("rect")
            .num("x", n.x.f())
            .num("y", n.y.f())
            .num("width", n.width.f())
            .num("height", n.height as f64)
            .attr("fill", "#fff5ad")
            .attr("stroke", "#aaaa33")
            .attr("stroke-width", "1"),
    );
    for (line, x, y) in &n.lines {
        g.push(
            Elem::new("text")
                .num("x", x.f())
                .num("y", y.f())
                .content(line)
                .attr("font-size", "16")
                .attr("fill", "#000000")
                .attr("font-family", "Arial, sans-serif")
                .attr("text-anchor", "middle")
                .attr("dominant-baseline", "middle"),
        );
    }
    g
}

const FRAME_STROKE: &str = "#333333";
const TAB_FILL: &str = "#eaeaea";
const DEFAULT_RECT_FILL: &str = "#cccccc";

fn bounds_rect(x: f64, y: f64, width: f64, height: f64) -> Elem {
    Elem::new("rect").num("x", x).num("y", y).num("width", width).num("height", height)
}

fn frame_text(c: &Colors, content: &str, x: f64, y: f64) -> Elem {
    text_elem(c, x, y, content)
}

fn box_group(index: usize, b: &BoxShape, c: &Colors) -> Elem {
    let mut g = Elem::new("g").attr("id", format!("box-{index}"));
    g.push(
        bounds_rect(b.x, b.y, b.width, b.height)
            .attr("fill", b.color.clone().unwrap_or_else(|| "none".into()))
            .attr("stroke", c.node_stroke)
            .attr("stroke-opacity", "0.5")
            .attr("class", "box"),
    );
    if let Some(title) = &b.title {
        g.push(frame_text(c, title, b.title_x, b.title_y));
    }
    g
}

fn frame_group(index: usize, f: &FrameShape, c: &Colors) -> Elem {
    let mut g = Elem::new("g").attr("id", format!("frame-{index}"));
    if f.kind == "rect" {
        let color = f.color.clone().unwrap_or_default();
        let fill = if color.is_empty() { DEFAULT_RECT_FILL.to_string() } else { color };
        g.push(bounds_rect(f.x, f.y, f.width, f.height).attr("fill", fill).attr("class", "frame-rect"));
        return g;
    }
    g.push(
        bounds_rect(f.x, f.y, f.width, f.height)
            .attr("fill", "none")
            .attr("stroke", FRAME_STROKE)
            .attr("stroke-width", "2")
            .attr("class", "frame-outline"),
    );
    g.push(Elem::new("polygon").attr("points", f.tab_points.clone()).attr("fill", TAB_FILL).attr("stroke", FRAME_STROKE));
    g.push(frame_text(c, f.kind, f.kind_x, f.kind_y));
    if let Some(title) = &f.title {
        let lines: Vec<&String> = if f.title_lines.is_empty() { vec![title] } else { f.title_lines.iter().collect() };
        for (line, y) in lines.iter().zip(&f.title_ys) {
            g.push(frame_text(c, line, f.title_x, *y));
        }
    }
    for d in &f.dividers {
        g.push(
            Elem::new("line")
                .num("x1", d.x1)
                .num("y1", d.y)
                .num("x2", d.x2)
                .num("y2", d.y)
                .attr("stroke", FRAME_STROKE)
                .attr("stroke-width", "2")
                .attr("stroke-dasharray", "3,3"),
        );
        g.push(frame_text(c, &d.text, d.text_x, d.text_y));
    }
    g
}
