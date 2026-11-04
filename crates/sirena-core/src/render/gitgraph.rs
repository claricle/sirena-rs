//! Git graph renderer: port of Ruby `Parser::Native::Builders::GitGraph` (+ the
//! shared replay `State`), `Notation::Mermaid::IRAdapters::GitGraph`,
//! `Layout::GitGraph` and `Renderer::GitGraph`.
//!
//! Replay errors (unknown branch, repeated merge id, ...) return `Error::Unported`
//! so the Ruby path owns the error text.
use super::pie::{is_slice, items};
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text::{width as text_width, Face};
use crate::Error;
use serde_json::Value;
use std::collections::HashMap;

fn unported() -> Error {
    Error::Unported("gitgraph".into())
}

const COMMIT_SPACING: f64 = 50.0;
const LANE_SPACING: f64 = 90.0;
const COMMIT_RADIUS: f64 = 10.0;
const PADDING: f64 = 40.0;
const LABEL_HEADROOM: f64 = 1.2;
const BRANCH_LABEL_INSET: f64 = 8.5;
const EMPTY_BRANCH_LABEL_CLEARANCE: f64 = 50.0;
const BRANCH_LABEL_BASELINE_OFFSET: f64 = 10.5;
const VERTICAL_BRANCH_LABEL_CLEARANCE: f64 = 40.0;
const EXPLICIT_ID_BRANCH_LABEL_CLEARANCE: f64 = 47.0;
const CHERRY_PICK_BRANCH_LABEL_CLEARANCE: f64 = 28.0;
const MERMAID_LABEL_FONT_SIZE: f64 = 10.0;
const MERMAID_LABEL_WIDTH_SCALE: f64 = 0.86;
const VERTICAL_LANE_LABEL_RATIO: f64 = 0.85;
const VERTICAL_LANE_LABEL_HEADROOM: f64 = 12.5;
const VERTICAL_LANE_GAP: f64 = 85.0;
const SMALL_FONT: f64 = 12.0;
const BRANCH_FONT: f64 = 16.0;

fn meas(text: &str, size: f64) -> f64 {
    text_width(text, size, Face::Arial)
}

// ---- parse tree -> replayed graph (Builders::GitGraph::State) ----

#[derive(Clone, Default)]
struct Commit {
    id: String,
    message: Option<String>,
    typ: String,
    tags: Vec<String>,
    branch: String,
    parent_ids: Vec<String>,
    is_merge: bool,
    is_cherry_pick: bool,
    cherry_pick_parent: Option<String>,
}

struct Branch {
    name: String,
    order: Option<i64>,
    parent_branch: Option<String>,
    created_at: Option<String>,
}

#[derive(Default, Clone)]
struct Opts {
    id: Option<String>,
    typ: Option<String>,
    cherry_pick_parent: Option<String>,
    tags: Option<Vec<String>>,
    message: Option<String>,
    parent_ids: Option<Vec<String>>,
    is_merge: bool,
    merge_branch: Option<String>,
    is_cherry_pick: bool,
}

struct State {
    current: String,
    branches: Vec<Branch>,
    commits: Vec<Commit>,
    counter: usize,
}

/// Text of a captured value: a slice's text, `"[]"` for an empty capture (`Array#to_s`).
fn to_s(v: &Value) -> Result<String, Error> {
    match v {
        Value::Array(a) if a.is_empty() => Ok("[]".into()),
        Value::String(s) => Ok(s.clone()),
        v if is_slice(v) => Ok(v["value"].as_str().unwrap_or("").to_string()),
        _ => Err(unported()),
    }
}

/// `State.unquote`: an empty capture is "", `\x` escapes the next character.
fn unquote(v: Option<&Value>) -> Result<String, Error> {
    let text = match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(_)) => String::new(),
        Some(v) => to_s(v)?,
    };
    let mut out = String::new();
    let mut it = text.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('v') => out.push('\u{b}'),
                Some('0') => out.push('\0'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn extract_options(options: Option<&Value>) -> Result<Opts, Error> {
    let mut r = Opts::default();
    let Some(Value::Array(list)) = options else { return Ok(r) };
    for option in list {
        if !option.is_object() || is_slice(option) {
            continue;
        }
        if let Some(v) = option.get("id") {
            r.id = Some(to_s(v)?);
        }
        if let Some(v) = option.get("type") {
            r.typ = Some(to_s(v)?);
        }
        if let Some(v) = option.get("parent") {
            r.cherry_pick_parent = Some(to_s(v)?);
        }
        if let Some(v) = option.get("tag") {
            let mut value = to_s(v)?;
            if value == "\"\"" {
                value.clear();
            }
            r.tags.get_or_insert_with(Vec::new).push(value);
        }
        if let Some(v) = option.get("message") {
            r.message = Some(unquote(Some(v))?);
        }
    }
    Ok(r)
}

impl State {
    fn tip(&self, name: &str) -> Option<String> {
        self.commits
            .iter()
            .rev()
            .find(|c| c.branch == name)
            .map(|c| c.id.clone())
            .or_else(|| self.branches.iter().find(|b| b.name == name).and_then(|b| b.created_at.clone()))
    }

    fn has_branch(&self, name: &str) -> bool {
        self.branches.iter().any(|b| b.name == name)
    }

    fn add_commit(&mut self, o: Opts) -> Result<(), Error> {
        self.counter += 1;
        let id = o.id.clone().unwrap_or_else(|| format!("commit-{}", self.counter));
        if o.is_merge && self.commits.iter().any(|c| c.id == id) {
            return Err(unported());
        }
        let parent_ids = o.parent_ids.clone().unwrap_or_else(|| self.tip(&self.current).into_iter().collect());
        self.commits.push(Commit {
            id,
            message: o.message,
            typ: o.typ.unwrap_or_else(|| "NORMAL".into()),
            tags: o.tags.unwrap_or_default(),
            branch: self.current.clone(),
            parent_ids,
            is_merge: o.is_merge,
            is_cherry_pick: o.is_cherry_pick,
            cherry_pick_parent: o.cherry_pick_parent,
        });
        Ok(())
    }

    fn checkout(&mut self, name: String) -> Result<(), Error> {
        if !self.has_branch(&name) {
            return Err(unported());
        }
        self.current = name;
        Ok(())
    }

    fn merge(&mut self, name: String, mut o: Opts) -> Result<(), Error> {
        if !self.has_branch(&name) || name == self.current {
            return Err(unported());
        }
        let (source_tip, current_tip) = (self.tip(&name), self.tip(&self.current));
        // `tips.first && tips.uniq.length == 2`
        if source_tip.is_none() || source_tip == current_tip {
            return Err(unported());
        }
        o.is_merge = true;
        o.merge_branch = Some(name);
        o.parent_ids = Some(current_tip.into_iter().chain(source_tip).collect());
        self.add_commit(o)
    }

    fn cherry_pick(&mut self, mut o: Opts) -> Result<(), Error> {
        let target = self.commits.iter().find(|c| Some(&c.id) == o.id.as_ref()).ok_or_else(unported)?;
        let valid_parent = o.cherry_pick_parent.as_ref().is_some_and(|p| target.parent_ids.contains(p));
        if target.is_merge && !valid_parent {
            return Err(unported());
        }
        o.is_cherry_pick = true;
        self.add_commit(o)
    }

    fn process(&mut self, statement: &Value) -> Result<(), Error> {
        if let Some(data) = statement.get("commit") {
            let o = if data.is_object() && !is_slice(data) { extract_options(data.get("options"))? } else { Opts::default() };
            return self.add_commit(o);
        }
        let data_of = |key: &str| -> Result<Option<&Value>, Error> {
            match statement.get(key) {
                None => Ok(None),
                Some(d) if d.is_object() && !is_slice(d) => Ok(Some(d)),
                Some(_) => Err(unported()),
            }
        };
        if let Some(d) = data_of("branch")? {
            let order = match d.get("options") {
                None | Some(Value::Null) => None,
                Some(Value::Array(list)) => {
                    let first = list.first().ok_or_else(unported)?;
                    // `nil.to_i` is 0 when the first option carries no order.
                    Some(match first.get("order") {
                        Some(v) => ruby_to_i(&to_s(v)?),
                        None => 0,
                    })
                }
                Some(_) => return Err(unported()),
            };
            let name = unquote(d.get("name"))?;
            if !self.has_branch(&name) {
                self.branches.push(Branch {
                    name: name.clone(),
                    order,
                    parent_branch: Some(self.current.clone()),
                    created_at: self.tip(&self.current),
                });
            }
            return self.checkout(name);
        }
        for key in ["checkout", "switch"] {
            if let Some(d) = data_of(key)? {
                return self.checkout(unquote(d.get("branch"))?);
            }
        }
        if let Some(d) = data_of("merge")? {
            let branch = unquote(d.get("branch"))?;
            return self.merge(branch, extract_options(d.get("options"))?);
        }
        if let Some(d) = data_of("cherry_pick")? {
            return self.cherry_pick(extract_options(d.get("options"))?);
        }
        Ok(())
    }
}

/// Ruby `String#to_i` (base 10).
fn ruby_to_i(s: &str) -> i64 {
    let t = s.trim_start();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
    let v = n.parse::<i64>().unwrap_or(0);
    if neg { -v } else { v }
}

// ---- layout ----

struct LCommit {
    ir: usize,
    c: Commit,
    x: f64,
    y: f64,
}

struct LBranch {
    name: String,
    lane: usize,
    color: String,
    parent_branch: Option<String>,
    created_at: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Id,
    Tag,
    Branch,
}

#[derive(Clone)]
struct Label {
    text: String,
    x: f64,
    y: f64,
    anchor: &'static str,
    kind: Kind,
    branch: String,
    owner: Option<usize>,
    font_size: f64,
    rotation: Option<f64>,
    cx: Option<f64>,
    cy: Option<f64>,
    tx: Option<f64>,
}

struct Pos {
    x: f64,
    y: f64,
    anchor: &'static str,
    rotation: Option<f64>,
    cx: Option<f64>,
    cy: Option<f64>,
    tx: Option<f64>,
}

fn pos(x: f64, y: f64, anchor: &'static str) -> Pos {
    Pos { x, y, anchor, rotation: None, cx: None, cy: None, tx: None }
}

struct Conn {
    from: usize,
    to: usize,
    kind: &'static str, // normal | merge | cherry_pick
}

struct Graph {
    orientation: String,
    commits: Vec<LCommit>,
    branches: Vec<LBranch>,
    conns: Vec<Conn>,
    lane_positions: HashMap<usize, f64>,
    width: f64,
    height: f64,
}

fn vertical(o: &str) -> bool {
    o == "TB" || o == "BT"
}

fn generated_commit_id(id: &str) -> bool {
    // /\Acommit[-_]\d+\z/
    id.strip_prefix("commit").and_then(|r| r.strip_prefix(['-', '_'])).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

fn hyphen_generated_id(id: &str) -> bool {
    id.strip_prefix("commit-").is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

fn cherry_pick_label(c: &Commit) -> String {
    let label = format!("cherry-pick:{}", c.id);
    match &c.cherry_pick_parent {
        Some(p) => format!("{label}|parent:{p}"),
        None => label,
    }
}

fn tag_labels(c: &Commit) -> Vec<String> {
    if !c.tags.is_empty() {
        return c.tags.iter().filter(|t| !t.is_empty()).cloned().collect();
    }
    if !c.is_cherry_pick {
        return vec![];
    }
    vec![cherry_pick_label(c)]
}

impl Graph {
    fn generated_commits_only(&self) -> bool {
        self.commits.iter().all(|c| hyphen_generated_id(&c.c.id) && tag_labels(&c.c).is_empty())
    }

    fn compact_generated_graph(&self) -> bool {
        self.orientation == "LR" && self.commits.len() <= 3 && self.commits.iter().all(|c| generated_commit_id(&c.c.id))
    }

    fn compact_generated_semantic(&self) -> bool {
        self.compact_generated_graph() && self.commits.iter().any(|c| c.c.is_merge && !tag_labels(&c.c).is_empty())
    }

    fn simple_three_commit(&self) -> bool {
        self.commits.len() == 3
            && self.branches.len() == 1
            && self.conns.len() == 2
            && self.conns.iter().all(|c| c.kind == "normal")
            && self.commits.iter().all(|c| {
                c.c.message.is_none()
                    && c.c.tags.is_empty()
                    && c.c.typ == "NORMAL"
                    && !c.c.is_merge
                    && !c.c.is_cherry_pick
            })
    }

    fn all_tagged_vertical(&self) -> bool {
        vertical(&self.orientation)
            && self.branches.len() == 1
            && self.commits.len() > 1
            && self.commits.iter().all(|c| c.c.tags.iter().any(|t| !t.is_empty()))
    }

    fn style(&self) -> Style {
        if self.orientation == "LR" && self.commits.len() == 1 {
            return Style::Lone;
        }
        if self.compact_generated_semantic() {
            return Style::GeneratedDiagonal;
        }
        if self.all_tagged_vertical() {
            return Style::TaggedVertical;
        }
        if vertical(&self.orientation) || self.simple_three_commit() {
            return Style::Linear;
        }
        Style::Legacy
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Lone,
    GeneratedDiagonal,
    TaggedVertical,
    Linear,
    Legacy,
}

fn mermaid_label_width(text: &str) -> f64 {
    meas(text, MERMAID_LABEL_FONT_SIZE) * MERMAID_LABEL_WIDTH_SCALE
}

fn diagonal_id_label_position(width: f64, x: f64, y: f64) -> Pos {
    let shift_x = -7.5 - (((width + 10.0) / 25.0) * 9.5);
    let shift_y = 10.0 + ((width / 25.0) * 8.5);
    let diagonal = 2f64.sqrt();
    let (ox, oy) = ((shift_x - shift_y) / diagonal, (shift_x + shift_y) / diagonal);
    Pos {
        x: x - (width / 2.0) + ox,
        y: y + 25.0 + oy,
        anchor: "start",
        rotation: Some(-45.0),
        cx: Some(x - COMMIT_RADIUS),
        cy: Some(y),
        tx: None,
    }
}

fn vertical_label_position(text: &str, x: f64, y: f64, font_size: f64) -> Pos {
    let width = meas(text, font_size);
    let logical_x = x + 14.0;
    let visual_x = x - width - 12.0;
    Pos { x: logical_x, y: y - 1.0, anchor: "start", rotation: Some(-45.0), cx: Some(x), cy: Some(y), tx: Some(visual_x - logical_x) }
}

fn linear_label_position(text: &str, x: f64, y: f64, orientation: &str, font_size: Option<f64>) -> Pos {
    if vertical(orientation) {
        return vertical_label_position(text, x, y, font_size.unwrap_or(SMALL_FONT));
    }
    let width = meas(text, SMALL_FONT);
    pos(x - (width / 2.0), y + 24.0, "start")
}

fn legacy_label_position(text: &str, x: f64, y: f64, orientation: &str, kind: Kind) -> Pos {
    if vertical(orientation) {
        return if kind == Kind::Tag { pos(x - 14.0, y + 4.0, "end") } else { pos(x + 14.0, y + 4.0, "start") };
    }
    if kind == Kind::Tag {
        let width = mermaid_label_width(text);
        return pos(x - (width / 2.0), y - 15.0, "start");
    }
    diagonal_id_label_position(mermaid_label_width(text), x, y)
}

fn label_position(text: &str, c: &LCommit, orientation: &str, kind: Kind, style: Style) -> Pos {
    let (x, y) = (c.x, c.y);
    if style == Style::Lone {
        let width = mermaid_label_width(text);
        if kind == Kind::Tag {
            return pos(x - (width / 2.0), y - 16.0, "middle");
        }
        return diagonal_id_label_position(width, x, y);
    }
    let special = if kind == Kind::Id {
        match style {
            Style::Linear | Style::TaggedVertical => Some(linear_label_position(
                text,
                x,
                y,
                orientation,
                if style == Style::TaggedVertical { Some(MERMAID_LABEL_FONT_SIZE) } else { None },
            )),
            Style::GeneratedDiagonal => Some(diagonal_id_label_position(mermaid_label_width(text), x, y)),
            Style::Legacy if generated_commit_id(&c.c.id) => Some(pos(x, y + 20.0, "middle")),
            _ => None,
        }
    } else if style == Style::TaggedVertical && kind == Kind::Tag {
        Some(Pos { x: x + 19.0, y: y + 7.0, anchor: "start", rotation: Some(45.0), cx: Some(x + 14.0), cy: Some(y + 4.0), tx: None })
    } else {
        None
    };
    special.unwrap_or_else(|| legacy_label_position(text, x, y, orientation, kind))
}

fn positioned_label(text: &str, c: &LCommit, orientation: &str, kind: Kind, style: Style) -> Label {
    let p = label_position(text, c, orientation, kind, style);
    Label {
        text: text.to_string(),
        x: p.x + PADDING,
        y: p.y + PADDING,
        anchor: p.anchor,
        kind,
        branch: c.c.branch.clone(),
        owner: Some(c.ir),
        font_size: SMALL_FONT,
        rotation: p.rotation,
        cx: p.cx.map(|v| v + PADDING),
        cy: p.cy.map(|v| v + PADDING),
        tx: p.tx,
    }
}

fn commit_labels(g: &Graph) -> Vec<Label> {
    let style = g.style();
    let mut out = vec![];
    for c in &g.commits {
        if !(c.c.is_cherry_pick || c.c.id.starts_with("commit_")) {
            out.push(positioned_label(&c.c.id, c, &g.orientation, Kind::Id, style));
        }
        for tag in tag_labels(&c.c) {
            out.push(positioned_label(&tag, c, &g.orientation, Kind::Tag, style));
        }
    }
    out
}

struct Anchor {
    x: f64,
    y: f64,
}

fn coordinates(time: f64, lane: f64, orientation: &str) -> (f64, f64) {
    if vertical(orientation) { (lane, time) } else { (time, lane) }
}

fn branch_label_translation(g: &Graph) -> Option<f64> {
    let vert = vertical(&g.orientation);
    if g.simple_three_commit() && !vert && !g.generated_commits_only() {
        return Some(EMPTY_BRANCH_LABEL_CLEARANCE);
    }
    if !vert && g.commits.iter().any(|c| c.c.is_cherry_pick) {
        return Some(CHERRY_PICK_BRANCH_LABEL_CLEARANCE);
    }
    let explicit = !vert
        && g.commits.len() > 1
        && !g.generated_commits_only()
        && g.commits.iter().any(|c| !generated_commit_id(&c.c.id))
        && !g.commits.iter().any(|c| c.c.is_cherry_pick);
    if explicit || g.compact_generated_semantic() {
        return Some(EXPLICIT_ID_BRANCH_LABEL_CLEARANCE);
    }
    None
}

fn branch_labels(g: &Graph) -> Vec<Label> {
    // representative commit (or empty-branch anchor) per branch, in insertion order
    let mut reps: Vec<(String, Anchor)> = vec![];
    for c in &g.commits {
        match reps.iter_mut().find(|(b, _)| *b == c.c.branch) {
            Some(slot) => {
                if vertical(&g.orientation) {
                    slot.1 = Anchor { x: c.x, y: c.y };
                }
            }
            None => reps.push((c.c.branch.clone(), Anchor { x: c.x, y: c.y })),
        }
    }
    for b in &g.branches {
        if reps.iter().any(|(n, _)| *n == b.name) {
            continue;
        }
        let creation = g.commits.iter().find(|c| Some(&c.c.id) == b.created_at.as_ref());
        let anchor = match creation {
            Some(c) => {
                let time = if vertical(&g.orientation) { c.y } else { c.x };
                let lane = g.lane_positions[&b.lane];
                let (x, y) = coordinates(time, lane, &g.orientation);
                Some(Anchor { x, y })
            }
            None => {
                if g.commits.is_empty() || b.parent_branch.is_some() {
                    None
                } else {
                    let (x, y) = coordinates(0.0, b.lane as f64 * LANE_SPACING, &g.orientation);
                    Some(Anchor { x, y })
                }
            }
        };
        if let Some(a) = anchor {
            reps.push((b.name.clone(), a));
        }
    }
    let translation = branch_label_translation(g);
    reps.into_iter()
        .map(|(name, a)| {
            let width = meas(&name, BRANCH_FONT);
            let (x, y) = if !vertical(&g.orientation) {
                let mut inset = BRANCH_LABEL_INSET;
                if g.commits.len() == 1 || g.generated_commits_only() {
                    inset -= EMPTY_BRANCH_LABEL_CLEARANCE;
                }
                (inset - width, a.y - BRANCH_LABEL_BASELINE_OFFSET)
            } else {
                let boundary = if g.orientation == "TB" { COMMIT_RADIUS - VERTICAL_BRANCH_LABEL_CLEARANCE } else { g.height };
                (a.x - (width / 2.0), boundary)
            };
            Label {
                text: name.clone(),
                x: x + PADDING,
                y: y + PADDING,
                anchor: "start",
                kind: Kind::Branch,
                branch: name,
                owner: None,
                font_size: BRANCH_FONT,
                rotation: None,
                cx: None,
                cy: None,
                tx: translation.map(|t| -t),
            }
        })
        .collect()
}

fn label_span(l: &Label) -> (f64, f64) {
    let width = meas(&l.text, l.font_size) * LABEL_HEADROOM;
    let share = match l.anchor {
        "start" => 0.0,
        "middle" => 0.5,
        _ => 1.0,
    };
    let left = l.x + l.tx.unwrap_or(0.0) - (width * share);
    (left, left + width)
}

fn build_graph(orientation: String, state: State) -> Result<Graph, Error> {
    let State { branches, commits, .. } = state;

    // IR edges: parent refs resolve to the LAST commit carrying that id.
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    for (i, c) in commits.iter().enumerate() {
        by_id.insert(&c.id, i);
    }
    let mut edges: Vec<(usize, usize, &'static str)> = vec![];
    for (i, c) in commits.iter().enumerate() {
        for p in &c.parent_ids {
            if let Some(&src) = by_id.get(p.as_str()) {
                let role = if c.is_merge {
                    "merge"
                } else if c.is_cherry_pick {
                    "cherry_pick"
                } else {
                    "normal"
                };
                edges.push((src, i, role));
            }
        }
    }

    // lanes
    let main = branches[0].name.clone();
    let rest: Vec<&Branch> = branches.iter().filter(|b| b.name != main).collect();
    let mut ordered: Vec<(usize, &Branch)> = rest.iter().cloned().enumerate().collect();
    ordered.sort_by(|(ia, a), (ib, b)| {
        let key = |i: usize, b: &Branch| (u8::from(b.order.is_some()), b.order.unwrap_or(i as i64), b.name.clone());
        key(*ia, a).cmp(&key(*ib, b))
    });
    let mut lanes: HashMap<String, usize> = HashMap::new();
    lanes.insert(main.clone(), 0);
    for (n, (_, b)) in ordered.iter().enumerate() {
        lanes.insert(b.name.clone(), n + 1);
    }

    let mut lane_positions: HashMap<usize, f64> = HashMap::new();
    if !vertical(&orientation) {
        for lane in lanes.values() {
            lane_positions.insert(*lane, *lane as f64 * LANE_SPACING);
        }
    } else {
        let mut by_lane: Vec<(&String, usize)> = lanes.iter().map(|(n, l)| (n, *l)).collect();
        by_lane.sort_by_key(|(_, l)| *l);
        for (i, (_, lane)) in by_lane.iter().enumerate() {
            let p = if i == 0 {
                0.0
            } else {
                let (pb, pl) = by_lane[i - 1];
                let measured = meas(pb, BRANCH_FONT);
                let substitute = (measured * VERTICAL_LANE_LABEL_RATIO) + VERTICAL_LANE_LABEL_HEADROOM;
                let w = if measured >= substitute { measured } else { substitute };
                lane_positions[&pl] + VERTICAL_LANE_GAP + (w / 2.0)
            };
            lane_positions.insert(*lane, p);
        }
    }

    // visible commits
    let mut latest: HashMap<&str, usize> = HashMap::new();
    for (i, c) in commits.iter().enumerate() {
        if !c.is_cherry_pick {
            latest.insert(&c.id, i);
        }
    }
    let visible: Vec<usize> =
        (0..commits.len()).filter(|&i| commits[i].is_cherry_pick || latest.get(commits[i].id.as_str()) == Some(&i)).collect();
    let count = visible.len();
    let mut lcommits = vec![];
    for (idx, &i) in visible.iter().enumerate() {
        let c = commits[i].clone();
        let lane = *lanes.get(&c.branch).unwrap_or(&0);
        let step = if orientation == "BT" { count - idx - 1 } else { idx };
        let time = step as f64 * COMMIT_SPACING + COMMIT_RADIUS;
        let (x, y) = coordinates(time, lane_positions[&lane], &orientation);
        lcommits.push(LCommit { ir: i, c, x, y });
    }

    let pos_of: HashMap<usize, usize> = lcommits.iter().enumerate().map(|(n, c)| (c.ir, n)).collect();
    let conns: Vec<Conn> = edges
        .iter()
        .filter_map(|(s, t, role)| Some(Conn { from: *pos_of.get(s)?, to: *pos_of.get(t)?, kind: role }))
        .collect();

    // branch metadata
    let color = |i: usize| Theme::default_theme().palette("git_graph_palette", i).unwrap_or("").to_string();
    let mut lbranches = vec![LBranch { name: main.clone(), lane: lanes[&main], color: color(0), parent_branch: None, created_at: None }];
    for (i, b) in rest.iter().enumerate() {
        lbranches.push(LBranch {
            name: b.name.clone(),
            lane: lanes[&b.name],
            color: color(i + 1),
            parent_branch: b.parent_branch.clone(),
            created_at: b.created_at.clone(),
        });
    }

    let time_span = if count == 0 { COMMIT_SPACING * 2.0 } else { count as f64 * COMMIT_SPACING };
    let max_lane = *lanes.values().max().unwrap();
    let lane_span = lane_positions[&max_lane] + COMMIT_RADIUS * 2.0;
    let (width, height) = if vertical(&orientation) { (lane_span, time_span) } else { (time_span, lane_span) };
    Ok(Graph { orientation, commits: lcommits, branches: lbranches, conns, lane_positions, width, height })
}

// ---- rendering ----

struct Num {
    v: f64,
    int: bool,
}

impl Num {
    fn text(&self) -> String {
        if self.int { format!("{}", self.v as i64) } else { ruby_float(self.v) }
    }
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "gitgraph")?;
    let hash = tree.is_object() && !is_slice(&tree);
    let orientation = match tree.get("direction").filter(|_| hash) {
        Some(v) => to_s(v)?,
        None => "LR".into(),
    };
    let statements: Vec<&Value> = if hash { tree.get("statements").map(items).unwrap_or_default() } else { vec![] };
    let mut state = State {
        current: "main".into(),
        branches: vec![Branch { name: "main".into(), order: Some(0), parent_branch: None, created_at: None }],
        commits: vec![],
        counter: 0,
    };
    for s in statements {
        state.process(s)?;
    }
    let g = build_graph(orientation, state)?;
    let theme = Theme::default_theme();
    let color = |k: &str| theme.color(k).unwrap_or("").to_string();
    let family = theme.string("typography", "font_family").unwrap_or("Arial, sans-serif");

    // labels (positions still carry the +PADDING, no spill shift yet)
    let mut labels = commit_labels(&g);
    labels.extend(branch_labels(&g));
    let base_width = g.width + PADDING * 2.0;
    let (spill_left, spill_right, shift_is_int) = if labels.is_empty() {
        (0.0, 0.0, true)
    } else {
        let spans: Vec<(f64, f64)> = labels.iter().map(label_span).collect();
        let min_left = spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
        let max_right = spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
        let left = (-min_left).max(0.0);
        let right = (max_right - base_width).max(0.0);
        (left, right, min_left > 0.0)
    };
    let shift = PADDING + spill_left;
    let width = base_width + spill_left + spill_right;
    let height = g.height + PADDING * 2.0;

    let mut doc = Document::new(width, height);
    let branch_colour: HashMap<&str, &str> = g.branches.iter().map(|b| (b.name.as_str(), b.color.as_str())).collect();
    let edge_stroke = color("edge_stroke");

    // connections
    for c in &g.conns {
        let (from, to) = (&g.commits[c.from], &g.commits[c.to]);
        let colour = if c.kind == "cherry_pick" {
            edge_stroke.clone()
        } else {
            branch_colour
                .get(from.c.branch.as_str())
                .or_else(|| branch_colour.get(to.c.branch.as_str()))
                .map(|s| s.to_string())
                .unwrap_or_else(|| edge_stroke.clone())
        };
        let (fx, fy, tx, ty) = (from.x + shift, from.y + PADDING, to.x + shift, to.y + PADDING);
        if c.kind == "normal" {
            doc.add(
                Elem::new("line")
                    .attr("fill", "none")
                    .attr("stroke", colour)
                    .attr("stroke-width", "2")
                    .num("x1", fx)
                    .num("y1", fy)
                    .num("x2", tx)
                    .num("y2", ty),
            );
            continue;
        }
        // Path numbers print as Ruby Integer when every term was an Integer.
        let x_int = !vertical(&g.orientation) && shift_is_int;
        let (nfx, nfy, ntx, nty) = (
            Num { v: fx, int: x_int },
            Num { v: fy, int: true },
            Num { v: tx, int: x_int },
            Num { v: ty, int: true },
        );
        let d = if c.kind == "merge" && fy != ty {
            let control_x = fx + ((tx - fx) * 0.5);
            format!(
                "M {} {} C {} {}, {} {}, {} {}",
                nfx.text(),
                nfy.text(),
                ruby_float(control_x),
                nfy.text(),
                ruby_float(control_x),
                nty.text(),
                ntx.text(),
                nty.text()
            )
        } else {
            format!("M {} {} L {} {}", nfx.text(), nfy.text(), ntx.text(), nty.text())
        };
        doc.add(
            Elem::new("path")
                .attr("fill", "none")
                .attr("stroke", colour)
                .attr("stroke-width", "2")
                .attr("d", d)
                .attr("stroke-dasharray", if c.kind == "merge" { "5,3" } else { "2,4" }),
        );
    }

    // commits
    let mut order: Vec<&LCommit> = g.commits.iter().collect();
    if g.orientation == "BT" {
        order.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    }
    let primary = color("primary");
    for c in order {
        let branch = branch_colour.get(c.c.branch.as_str()).map(|s| s.to_string()).unwrap_or_else(|| primary.clone());
        let fill = match c.c.typ.as_str() {
            "HIGHLIGHT" => color("highlight"),
            "REVERSE" => color("background"),
            _ => branch.clone(),
        };
        let (x, y) = (c.x + shift, c.y + PADDING);
        let mut circle = Elem::new("circle")
            .attr("fill", fill)
            .attr("stroke", branch)
            .attr("stroke-width", "2")
            .num("cx", x)
            .num("cy", y)
            .num("r", COMMIT_RADIUS);
        if c.c.is_cherry_pick {
            circle = circle.attr("class", "commit-cherry-pick");
            doc.add(circle);
            for dx in [-3.0, 3.0] {
                doc.add(
                    Elem::new("circle")
                        .attr("fill", "#fff")
                        .attr("class", "commit-cherry-pick")
                        .num("cx", x + dx)
                        .num("cy", y + 2.0)
                        .num("r", 2.75),
                );
            }
        } else {
            doc.add(circle);
        }
    }

    // labels
    let extra = shift - PADDING;
    let text_elem = |l: &Label, fill: String, bold: bool| -> Elem {
        let class = match l.kind {
            Kind::Tag => Some("tag-label"),
            _ if l.rotation.is_some() => Some("commit-label"),
            _ => None,
        };
        let mut transforms: Vec<String> = vec![];
        if let Some(r) = l.rotation {
            transforms.push(format!(
                "rotate({} {} {})",
                ruby_float(r),
                ruby_float(l.cx.unwrap_or(0.0) + extra),
                ruby_float(l.cy.unwrap_or(0.0))
            ));
        }
        if let Some(t) = l.tx {
            transforms.push(format!("translate({} 0)", ruby_float(t)));
        }
        let mut e = Elem::new("text")
            .attr("fill", fill)
            .num("x", l.x + extra)
            .num("y", l.y)
            .attr("text-anchor", l.anchor)
            .attr("font-family", family)
            .attr("font-size", ruby_float(l.font_size))
            .content(&l.text);
        if let Some(c) = class {
            e = e.attr("class", c);
        }
        if !transforms.is_empty() {
            e = e.attr("transform", transforms.join(" "));
        }
        if bold {
            e = e.attr("font-weight", "bold");
        }
        e
    };
    for c in &g.commits {
        for l in labels.iter().filter(|l| l.kind != Kind::Branch && l.owner == Some(c.ir)) {
            let tag = l.kind == Kind::Tag;
            doc.add(text_elem(l, if tag { color("accent") } else { color("label_text") }, tag));
        }
    }
    for b in &g.branches {
        if let Some(l) = labels.iter().find(|l| l.kind == Kind::Branch && l.branch == b.name) {
            doc.add(text_elem(l, b.color.clone(), true));
        }
    }
    Ok(doc.to_xml())
}
