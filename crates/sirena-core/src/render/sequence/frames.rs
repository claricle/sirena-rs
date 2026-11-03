//! `Layout::Sequence::FramePlacement`: control frames and `box` groups.
use super::model::{BoxGroup, Diagram, Frame, Section};
use super::num::{fl, i, N};
use super::text;
use std::collections::HashMap;

pub const OPEN_HEIGHT: i64 = 45;
pub const RECT_OPEN_HEIGHT: i64 = 10;
pub const CLOSE_HEIGHT: i64 = 10;
pub const RECT_CLOSE_HEIGHT: i64 = 8;
pub const SECTION_HEIGHT: i64 = 45;
pub const UNLABELLED_DROP: i64 = 20;
pub const SLOT_HALF: i64 = 34;
pub const SIDE_MARGIN: i64 = 11;
pub const NEST_MARGIN: i64 = 10;
pub const TAB_MIN_WIDTH: i64 = 50;
pub const TAB_CHAR_WIDTH: i64 = 8;
pub const TAB_PADDING: i64 = 18;
pub const TAB_HEIGHT: i64 = 20;
pub const BOX_PAD: i64 = 5;
pub const BOX_AFTER: i64 = 15;
pub const BOX_EDGE: i64 = 25;
pub const BOX_HEADER: i64 = 27;
pub const BOX_HEADER_BARE: i64 = 10;
pub const BOX_BELOW: i64 = 10;
pub const BOX_TOP: i64 = 5;
pub const BOX_TITLE_BASELINE: i64 = 21;
pub const BOX_BOTTOM_PAD: i64 = 10;
const TITLE_BASELINE: i64 = 18;
const KIND_BASELINE: i64 = 14;
const TITLE_PITCH: i64 = 19;
const TAB_SLANT: i64 = 8;
const TAB_SHOULDER: i64 = 13;
const TITLE_MIN_ROOM: i64 = 20;
const TITLE_MARGIN: i64 = 20;
const TITLE_LINE_ROOM: i64 = 17;
const TITLE_FONT_SIZE: f64 = 16.0;
const DIVIDER_OFFSET: i64 = 5;
const LOOP_WIDTH: i64 = 56;

/// What frames need to know about the placed notes.
pub trait NoteSlots {
    /// Height of the note slots at `index` written before `order`.
    fn slots_before(&self, index: usize, order: usize) -> i64;
    /// Left and right edge of the notes written between two frame edges.
    fn x_range(&self, from: usize, to: usize) -> Option<(N, N)>;
}

pub struct NoNotes;

impl NoteSlots for NoNotes {
    fn slots_before(&self, _index: usize, _order: usize) -> i64 {
        0
    }
    fn x_range(&self, _from: usize, _to: usize) -> Option<(N, N)> {
        None
    }
}

pub struct Divider {
    pub y: f64,
    pub text: String,
    pub text_y: f64,
    pub x1: f64,
    pub x2: f64,
    pub text_x: f64,
}

pub struct FrameShape {
    pub kind: &'static str,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: Option<String>,
    pub title: Option<String>,
    pub title_lines: Vec<String>,
    pub kind_x: f64,
    pub kind_y: f64,
    pub title_x: f64,
    pub title_ys: Vec<f64>,
    pub tab_points: String,
    pub dividers: Vec<Divider>,
}

pub struct BoxShape {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: Option<String>,
    pub title: Option<String>,
    pub title_x: f64,
    pub title_y: f64,
}

pub struct Frames<'a> {
    frames: &'a [Frame],
    boxes: &'a [BoxGroup],
    /// `[source, target]` ids of each message.
    spans: Vec<(&'a str, &'a str)>,
    ids: Vec<&'a str>,
    title_lines: HashMap<usize, Vec<String>>,
}

impl<'a> Frames<'a> {
    pub fn new(d: &'a Diagram) -> Frames<'a> {
        Frames {
            frames: &d.frames,
            boxes: &d.boxes,
            spans: d.messages.iter().map(|m| (m.from.as_str(), m.to.as_str())).collect(),
            ids: d.participants.iter().map(|p| p.id.as_str()).collect(),
            title_lines: HashMap::new(),
        }
    }

    /// Wraps every frame title at the title area's width.
    pub fn fit_titles(&mut self, positions: &HashMap<&str, N>) {
        let none = NoNotes;
        let lines = self
            .frames
            .iter()
            .map(|f| {
                let (left, right) = self.horizontal_extent(f, positions, &none);
                (f.open_order, self.title_lines_for(f, right - left))
            })
            .collect();
        self.title_lines = lines;
    }

    pub fn top_inset(&self) -> i64 {
        if self.boxes.is_empty() {
            return 0;
        }
        if self.boxes.iter().any(|b| !b.title.is_empty()) { BOX_HEADER } else { BOX_HEADER_BARE }
    }

    pub fn row_shift(&self, index: i64) -> i64 {
        if index < 0 {
            return 0;
        }
        (0..=index as usize).map(|at| self.room(at)).sum()
    }

    pub fn total_shift(&self) -> i64 {
        self.row_shift(self.spans.len() as i64)
    }

    pub fn bottom_inset(&self) -> i64 {
        if self.boxes.is_empty() { 0 } else { BOX_BELOW }
    }

    pub fn gap_before(&self, id: &str) -> i64 {
        self.boxes.iter().filter(|b| b.members.first().map(String::as_str) == Some(id)).count() as i64 * BOX_PAD
    }

    pub fn gap_after(&self, id: &str) -> i64 {
        self.boxes.iter().filter(|b| b.members.last().map(String::as_str) == Some(id)).count() as i64 * BOX_AFTER
    }

    pub fn extra_width(&self) -> i64 {
        self.boxes
            .iter()
            .map(|b| BOX_PAD + if b.members.last().map(String::as_str) == self.ids.last().copied() { BOX_PAD } else { BOX_AFTER })
            .sum()
    }

    /// Height of the frame edges at `index` that come before `order`.
    pub fn lead(&self, index: usize, order: usize) -> i64 {
        self.edges(index).iter().filter(|(o, _)| *o < order).map(|(_, h)| *h).sum()
    }

    pub fn frame_shapes(
        &self,
        positions: &HashMap<&str, N>,
        row_y: &dyn Fn(usize) -> f64,
        notes: &dyn NoteSlots,
    ) -> Vec<FrameShape> {
        (0..self.frames.len()).map(|k| self.frame_shape(k, positions, row_y, notes)).collect()
    }

    pub fn box_shapes(&self, left_of: &HashMap<&str, N>, widths: &HashMap<&str, N>, bottom: f64) -> Vec<BoxShape> {
        let mut out = vec![];
        for b in self.boxes {
            let members: Vec<&str> = b.members.iter().map(String::as_str).filter(|id| left_of.contains_key(id)).collect();
            if members.is_empty() {
                continue;
            }
            let lefts = members.iter().map(|id| left_of[id].f());
            let rights = members.iter().map(|id| (left_of[id] + widths[id]).f());
            let left = lefts.fold(f64::INFINITY, f64::min) - BOX_EDGE as f64;
            let right = rights.fold(f64::NEG_INFINITY, f64::max) + BOX_EDGE as f64;
            out.push(BoxShape {
                x: left,
                y: BOX_TOP as f64,
                width: right - left,
                height: bottom - BOX_TOP as f64,
                color: b.color.clone(),
                title: (!b.title.is_empty()).then(|| b.title.clone()),
                title_x: left + ((right - left) / 2.0),
                title_y: (BOX_TOP + BOX_TITLE_BASELINE) as f64,
            });
        }
        out
    }

    // ---- private ----

    fn opening(&self, index: usize) -> impl Iterator<Item = &Frame> {
        self.frames.iter().filter(move |f| f.start == index)
    }

    fn closing(&self, index: usize) -> impl Iterator<Item = &Frame> {
        self.frames.iter().filter(move |f| f.stop == index)
    }

    fn sections_at(&self, index: usize) -> Vec<&Section> {
        self.frames.iter().flat_map(|f| f.sections.iter()).filter(|s| s.start == index).collect()
    }

    fn room(&self, index: usize) -> i64 {
        self.opening(index).map(|f| self.open_height(f)).sum::<i64>()
            + self.closing(index).map(close_height).sum::<i64>()
            + self.sections_height(index)
    }

    fn open_height(&self, f: &Frame) -> i64 {
        if f.kind == "rect" {
            return RECT_OPEN_HEIGHT;
        }
        labelled_height(OPEN_HEIGHT, &f.label) + self.extra_title(f)
    }

    fn extra_title(&self, f: &Frame) -> i64 {
        let lines = self.title_lines.get(&f.open_order).map_or(0, Vec::len) as i64;
        (TITLE_MIN_ROOM).max(TITLE_LINE_ROOM * lines) - TITLE_MIN_ROOM
    }

    fn title_lines_for(&self, f: &Frame, width: N) -> Vec<String> {
        if f.kind == "rect" || f.label.is_empty() {
            return vec![];
        }
        let title = format!("[{}]", f.label);
        let widest_word = title
            .split(|c: char| c.is_whitespace())
            .filter(|w| !w.is_empty())
            .map(|w| text::width_of(&format!("{w} "), TITLE_FONT_SIZE))
            .max()
            .unwrap_or(0);
        let limit = (width.f() - (TAB_MIN_WIDTH + TITLE_MARGIN) as f64).max((widest_word + 1) as f64);
        text::wrap_lines(&title, limit, TITLE_FONT_SIZE)
    }

    fn section_height(&self, s: &Section) -> i64 {
        labelled_height(SECTION_HEIGHT, &s.label)
    }

    fn sections_height(&self, index: usize) -> i64 {
        self.sections_at(index).iter().map(|s| self.section_height(s)).sum()
    }

    fn closing_room(&self, at: usize) -> i64 {
        self.closing(at).map(close_height).sum()
    }

    fn edges(&self, index: usize) -> Vec<(usize, i64)> {
        let mut out: Vec<(usize, i64)> = self.opening(index).map(|f| (f.open_order, self.open_height(f))).collect();
        out.extend(self.closing(index).map(|f| (f.close_order, close_height(f))));
        out.extend(self.sections_at(index).iter().map(|s| (s.order, self.section_height(s))));
        out
    }

    fn base(&self, index: usize, row_y: &dyn Fn(usize) -> f64, notes: &dyn NoteSlots) -> f64 {
        row_y(index) - SLOT_HALF as f64 - self.room(index) as f64 - notes.slots_before(index, usize::MAX) as f64
    }

    fn frame_shape(&self, k: usize, positions: &HashMap<&str, N>, row_y: &dyn Fn(usize) -> f64, notes: &dyn NoteSlots) -> FrameShape {
        let f = &self.frames[k];
        let (left, right) = self.horizontal_extent(f, positions, notes);
        let top = self.frame_top(k, row_y, notes);
        let bottom = self.frame_bottom(f, row_y, notes).max(top + TAB_HEIGHT as f64);
        let (x, y) = (left.f(), top);
        let width = (right - left).f();
        let mut shape = FrameShape {
            kind: f.kind,
            x,
            y,
            width,
            height: bottom - top,
            color: None,
            title: None,
            title_lines: vec![],
            kind_x: 0.0,
            kind_y: 0.0,
            title_x: 0.0,
            title_ys: vec![],
            tab_points: String::new(),
            dividers: vec![],
        };
        if f.kind == "rect" {
            shape.color = Some(f.label.clone());
        } else {
            let tab_width = TAB_MIN_WIDTH.max(f.kind.len() as i64 * TAB_CHAR_WIDTH + TAB_PADDING) as f64;
            shape.title = (!f.label.is_empty()).then(|| format!("[{}]", f.label));
            shape.title_lines = self.title_lines.get(&f.open_order).cloned().unwrap_or_default();
            let tab_right = x + tab_width;
            let bottom_y = y + TAB_HEIGHT as f64;
            shape.tab_points = format!(
                "{},{} {},{} {},{} {},{} {},{}",
                fl(x).show(),
                fl(y).show(),
                fl(tab_right).show(),
                fl(y).show(),
                fl(tab_right).show(),
                fl(y + TAB_SHOULDER as f64).show(),
                fl(tab_right - TAB_SLANT as f64).show(),
                fl(bottom_y).show(),
                fl(x).show(),
                fl(bottom_y).show()
            );
            shape.kind_x = x + tab_width / 2.0;
            shape.kind_y = y + KIND_BASELINE as f64;
            shape.title_x = tab_right + (width - tab_width) / 2.0;
            let count = shape.title_lines.len().max(1);
            shape.title_ys = (0..count).map(|line| y + (TITLE_BASELINE + TITLE_PITCH * line as i64) as f64).collect();
        }
        shape.dividers = f
            .sections
            .iter()
            .map(|s| self.divider(s, row_y, notes, &shape))
            .collect();
        shape
    }

    fn frame_top(&self, k: usize, row_y: &dyn Fn(usize) -> f64, notes: &dyn NoteSlots) -> f64 {
        let f = &self.frames[k];
        let start = f.start;
        let opened_before: i64 = self.frames[..k].iter().filter(|o| o.start == start).map(|o| self.open_height(o)).sum();
        self.base(start, row_y, notes)
            + (self.closing_room(start) + self.sections_height(start) + opened_before + notes.slots_before(start, f.open_order)) as f64
    }

    fn frame_bottom(&self, f: &Frame, row_y: &dyn Fn(usize) -> f64, notes: &dyn NoteSlots) -> f64 {
        let deeper = self.closing(f.stop).filter(|o| o.depth > f.depth).count() as i64;
        self.base(f.stop, row_y, notes) + (CLOSE_HEIGHT * deeper + notes.slots_before(f.stop, f.close_order)) as f64
    }

    fn divider(&self, s: &Section, row_y: &dyn Fn(usize) -> f64, notes: &dyn NoteSlots, shape: &FrameShape) -> Divider {
        let all: Vec<&Section> = self.frames.iter().flat_map(|f| f.sections.iter()).collect();
        let rank = all
            .iter()
            .take_while(|o| !std::ptr::eq(**o, s))
            .filter(|o| o.start == s.start)
            .count();
        let at = s.start;
        let earlier: i64 = self.sections_at(at).iter().take(rank).map(|e| self.section_height(e)).sum();
        let top = self.base(at, row_y, notes) + (self.closing_room(at) + DIVIDER_OFFSET + earlier) as f64;
        let line_y = top + notes.slots_before(at, s.order) as f64;
        Divider {
            y: line_y,
            text: format!("[{}]", s.label),
            text_y: line_y + TITLE_BASELINE as f64,
            x1: shape.x,
            x2: shape.x + shape.width,
            text_x: shape.x + shape.width / 2.0,
        }
    }

    fn horizontal_extent(&self, f: &Frame, positions: &HashMap<&str, N>, notes: &dyn NoteSlots) -> (N, N) {
        let mut centres: Vec<N> = vec![];
        for index in f.start..f.stop {
            let Some((source, target)) = self.spans.get(index) else { continue };
            let (Some(s), Some(t)) = (positions.get(source), positions.get(target)) else { continue };
            centres.push(*s);
            centres.push(*t);
            if source == target {
                centres.push(*s + LOOP_WIDTH);
            }
        }
        if centres.is_empty() {
            centres = self.ids.iter().filter_map(|id| positions.get(id).copied()).collect();
        }
        let margin = SIDE_MARGIN + NEST_MARGIN * self.nesting_below(f);
        if let Some((a, b)) = notes.x_range(f.open_order, f.close_order) {
            centres.push(a);
            centres.push(b);
        }
        let low = centres.iter().copied().reduce(|a, b| if b.f() < a.f() { b } else { a }).unwrap_or(i(0));
        let high = centres.iter().copied().reduce(|a, b| if b.f() > a.f() { b } else { a }).unwrap_or(i(0));
        (low - margin, high + margin)
    }

    fn nesting_below(&self, f: &Frame) -> i64 {
        self.frames
            .iter()
            .filter(|o| o.depth > f.depth && o.start >= f.start && o.stop <= f.stop)
            .map(|o| (o.depth - f.depth) as i64)
            .max()
            .unwrap_or(0)
    }
}

fn close_height(f: &Frame) -> i64 {
    if f.kind == "rect" { RECT_CLOSE_HEIGHT } else { CLOSE_HEIGHT }
}

fn labelled_height(height: i64, label: &str) -> i64 {
    if label.is_empty() { height - UNLABELLED_DROP } else { height }
}
