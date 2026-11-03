//! Class diagram renderer: port of Ruby `Parser::Builders::ClassDiagram` (model),
//! `Layout::ClassDiagram` + `ClassCellPlacement` (placement) and
//! `Renderer::ClassDiagram` (scene path).
//!
//! Three or more classes are placed by the layered placement (`place`, which
//! calls `crate::elk::layout`); one or two by the cell grid.
//!
//! Declined to the Ruby path (`Error::Unported`): a frontmatter/directive
//! preamble, a `note` inside a namespaced diagram, and any model the Ruby
//! layout rejects as invalid. `namespace` blocks are placed by `namespace`.
use super::pie::{items, strip, text_of};
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

mod namespace;
mod place;

const HEADER_PADDING: f64 = 24.0;
const MMDC_FONT_SIZE: f64 = 16.0;
const EMPTY_CLASS_HEIGHT: f64 = 84.0;
const STEREOTYPE_HEIGHT: f64 = 24.0;
const MEMBER_HEIGHT: f64 = 24.0;
const ATTRIBUTES_BASE: f64 = 96.0;
const METHODS_ONLY_BASE: f64 = 102.0;
const RENDER_LINE_HEIGHT: f64 = 18.0;
const COMPARTMENT_PADDING: f64 = 10.0;
const DIAGRAM_PADDING: f64 = 20.0;
const GRID_MARGIN: f64 = 50.0;
const CELL_GAP: f64 = 50.0;
const COLUMNS: usize = 3;
const ARROW_SIZE: f64 = 10.0;
const DIAMOND_SIZE: f64 = 12.0;
const DART_NEAR: f64 = 3.0;
const DART_FAR: f64 = 14.0;
const DART_WIDTH: f64 = 5.0;
const FALLBACK_RATIO: f64 = 1.05;

fn unported() -> Error {
    Error::Unported("class_diagram".into())
}

/// `ClassNameAdvances::TABLE` (per-mille advances of the bold class name).
fn name_advance(c: char) -> Option<i64> {
    Some(match c {
        '-' => 365, '<' => 608, '>' => 608, '_' => 580,
        '0' => 582, '1' => 579, '2' => 577, '3' => 576, '4' => 574,
        '5' => 572, '6' => 571, '7' => 569, '8' => 568, '9' => 567,
        'A' => 632, 'B' => 601, 'C' => 629, 'D' => 653, 'E' => 568,
        'F' => 577, 'G' => 679, 'H' => 685, 'I' => 273, 'J' => 524,
        'K' => 610, 'L' => 536, 'M' => 737, 'N' => 659, 'O' => 701,
        'P' => 577, 'R' => 613, 'S' => 516, 'T' => 549, 'U' => 687,
        'W' => 892, 'Z' => 564, 'a' => 524, 'b' => 570, 'c' => 520,
        'd' => 585, 'e' => 569, 'f' => 368, 'g' => 501, 'h' => 584,
        'i' => 298, 'j' => 378, 'k' => 544, 'l' => 304, 'm' => 858,
        'n' => 580, 'o' => 554, 'p' => 586, 'q' => 580, 'r' => 419,
        's' => 425, 't' => 400, 'u' => 588, 'v' => 526, 'w' => 748,
        'x' => 544, 'y' => 525,
        _ => return None,
    })
}

// ---- text helpers (Ruby GenericText, String ops) ----

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
}

/// `gsub(/\s+/, " ")` (ASCII whitespace).
fn squeeze(s: &str) -> String {
    let mut out = String::new();
    let mut in_ws = false;
    for c in s.chars() {
        if is_ws(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `GenericText.display`: `~T~` pairs become `<T>`.
fn display(text: &str) -> String {
    let pair = Regex::new(r"~([^~]+)~").unwrap();
    let mut shown = text.to_string();
    while pair.is_match(&shown) {
        shown = pair.replace_all(&shown, "<${1}>").into_owned();
    }
    shown
}

/// `CGI.unescapeHTML`.
fn unescape_html(s: &str) -> String {
    let re = Regex::new(r"&(apos|amp|quot|gt|lt|#[0-9]+|#[xX][0-9A-Fa-f]+);").unwrap();
    re.replace_all(s, |c: &regex::Captures| {
        let body = &c[1];
        match body {
            "apos" => "'".to_string(),
            "amp" => "&".to_string(),
            "quot" => "\"".to_string(),
            "gt" => ">".to_string(),
            "lt" => "<".to_string(),
            _ => {
                let n = if let Some(h) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
                    u32::from_str_radix(h, 16).ok()
                } else {
                    body[1..].parse::<u32>().ok()
                };
                match n.and_then(char::from_u32) {
                    Some(ch) => ch.to_string(),
                    None => c[0].to_string(),
                }
            }
        }
    })
    .into_owned()
}

/// `GenericText.display_member`.
fn display_member(text: &str) -> String {
    let text = unescape_html(text);
    match text.strip_prefix('~') {
        Some(rest) => format!("~{}", display(rest)),
        None => display(&text),
    }
}

// ---- model (builder) ----

#[derive(Clone, Default)]
#[allow(dead_code)] // `ty` and `visibility` mirror the Ruby model; display uses `text`
struct Attr {
    name: String,
    ty: Option<String>,
    visibility: String,
    text: String,
}

impl Attr {
    fn display_text(&self) -> String {
        let mut t = self.text.clone();
        if t.ends_with('*') || t.ends_with('$') {
            t.pop();
            while t.ends_with(is_ws) {
                t.pop();
            }
        }
        display_member(&squeeze(&t))
    }
}

#[derive(Clone, Default)]
#[allow(dead_code)]
struct Meth {
    name: String,
    parameters: Option<String>,
    return_type: Option<String>,
    visibility: String,
    text: String,
}

impl Meth {
    fn display_text(&self) -> String {
        let head_end = self.text.find('(').unwrap_or(self.text.len());
        let head = squeeze(&self.text[..head_end]);
        let head = head.trim_end_matches(|c: char| is_ws(c) || c == '\0');
        let mut row = format!("{}({})", head, self.parameters.as_deref().unwrap_or(""));
        if let Some(r) = &self.return_type {
            row.push_str(&format!(" : {r}"));
        }
        display_member(&row)
    }
}

#[derive(Clone, Default)]
struct Ent {
    id: String,
    name: String,
    stereotype: Option<String>,
    attrs: Vec<Attr>,
    meths: Vec<Meth>,
}

#[derive(Clone, Default)]
struct Rel {
    from: String,
    to: String,
    ty: String,
    label: Option<String>,
    source_card: Option<String>,
    target_card: Option<String>,
    start_marker: Option<String>,
    end_marker: Option<String>,
    dashed: Option<bool>,
    reversed: bool,
}

struct Model {
    ents: Vec<Ent>,
    rels: Vec<Rel>,
    direction: Option<String>,
    notes: Vec<(String, Option<String>)>,
    /// `namespace` blocks in source order: name and the class ids named in the body.
    blocks: Vec<(String, Vec<String>)>,
    count: usize,
    created_in: HashMap<String, usize>,
    body: String,
    raw_method: Regex,
    raw_visibility: Regex,
    mixed: Regex,
}

fn has<'a>(s: &'a Value, k: &str) -> Option<&'a Value> {
    s.get(k).filter(|v| !v.is_null())
}

fn is_slice(v: &Value) -> bool {
    v.get("offset").is_some() && v.get("length").is_some() && v.get("value").is_some()
}

fn collect_slices(v: &Value, out: &mut Vec<(usize, usize)>) {
    match v {
        Value::Object(m) if is_slice(v) => {
            let _ = m;
            out.push((v["offset"].as_u64().unwrap_or(0) as usize, v["length"].as_u64().unwrap_or(0) as usize));
        }
        Value::Object(m) => m.values().for_each(|x| collect_slices(x, out)),
        Value::Array(a) => a.iter().for_each(|x| collect_slices(x, out)),
        _ => {}
    }
}

fn relationship_type(op: &str) -> &'static str {
    match op {
        "<|--" | "--|>" => "inheritance",
        "*--" | "--*" => "composition",
        "o--" | "--o" => "aggregation",
        "..|>" | "<|.." => "realization",
        "..>" | "<.." => "dependency",
        _ => "association",
    }
}

fn marker_type(glyph: &str) -> Option<&'static str> {
    match glyph {
        "<|" | "|>" => Some("inheritance"),
        "*" => Some("composition"),
        "o" => Some("aggregation"),
        "<" | ">" => Some("dependency"),
        _ => None,
    }
}

fn visibility_of(symbol: &str) -> &'static str {
    match symbol {
        "-" => "private",
        "#" => "protected",
        "~" => "package",
        _ => "public",
    }
}

impl Model {
    fn new(body: String) -> Model {
        Model {
            ents: vec![],
            rels: vec![],
            direction: None,
            notes: vec![],
            blocks: vec![],
            count: 0,
            created_in: HashMap::new(),
            body,
            raw_method: Regex::new(r"^(?P<name>.*)\((?P<params>[^)]*)\)(?P<rest>.*)$").unwrap(),
            raw_visibility: Regex::new(r"(?s)^([-+#~])[ \t\r\n\x0b\x0c]*([^ \t\r\n\x0b\x0c].*)$").unwrap(),
            mixed: Regex::new(r"^(<\||\|>|\*|o|<|>)(--|\.\.)(<\||\|>|\*|o|<|>)$").unwrap(),
        }
    }

    fn apply(&mut self, tree: &Value) -> Result<(), Error> {
        let mut stmts: Vec<Value> = vec![];
        if let Some(statements) = tree.get("statements") {
            for s in items(statements) {
                stmts.push(s.clone());
            }
        }
        for s in &stmts {
            if s.get("namespace_keyword").is_some() {
                self.namespace(s)?;
                continue;
            }
            if let Some(t) = s.get("note_text") {
                let text = note_text(&text_of(t));
                // a multi-line note renders as tspans: not ported
                if text.contains('\n') {
                    return Err(unported());
                }
                self.notes.push((text, s.get("note_for").map(text_of)));
                continue;
            }
            self.item(s)?;
        }
        Ok(())
    }

    /// `process_namespace`: the body statements are built like top-level ones;
    /// the ids they name are kept for `ClassNamespaces.collect`.
    fn namespace(&mut self, s: &Value) -> Result<(), Error> {
        if has(s, "namespace_name").is_none() {
            return Err(unported());
        }
        self.count += 1;
        let mut ids = vec![];
        let body: Vec<Value> = match has(s, "namespace_body") {
            Some(b) => items(b).into_iter().cloned().collect(),
            None => vec![],
        };
        for st in &body {
            if st.get("namespace_keyword").is_some() || st.get("note_text").is_some() {
                return Err(unported());
            }
            self.count += 1;
            self.statement(st)?;
            if let Some(v) = has(st, "class_id") {
                ids.push(text_of(v).replace('`', ""));
            }
            for k in ["from_id", "to_id"] {
                if let Some(v) = has(st, k) {
                    ids.push(text_of(v).replace('`', ""));
                }
            }
        }
        let name = text_of(has(s, "namespace_name").unwrap());
        self.blocks.push((name, ids));
        Ok(())
    }

    fn item(&mut self, s: &Value) -> Result<(), Error> {
        if let Some(d) = has(s, "direction") {
            if let Some(v) = has(d, "dir_value") {
                self.direction = Some(text_of(v));
            }
        }
        if has(s, "header").is_some() || has(s, "direction").is_some() {
            return Ok(());
        }
        self.count += 1;
        self.statement(s)
    }

    fn statement(&mut self, s: &Value) -> Result<(), Error> {
        let class_id = has(s, "class_id");
        let keyword_is_class = has(s, "keyword").is_some_and(|k| text_of(k) == "class");
        if keyword_is_class && class_id.is_some() {
            return self.class_declaration(s);
        }
        if has(s, "stereotype").is_some() && class_id.is_some() && has(s, "keyword").is_none() {
            let ent = self.find_or_create(&class_id_text(class_id.unwrap()));
            let v = s["stereotype"].get("stereotype_value").ok_or_else(unported)?;
            if self.ents[ent].stereotype.is_none() {
                self.ents[ent].stereotype = Some(text_of(v));
            }
            return Ok(());
        }
        if class_id.is_some() && (has(s, "member").is_some() || has(s, "raw_member").is_some() || has(s, "body_stereotype").is_some()) {
            let ent = self.find_or_create(&class_id_text(class_id.unwrap()));
            self.apply_generic(ent, has(s, "generic"));
            return self.add_member(ent, s);
        }
        if has(s, "from_id").is_some() && has(s, "to_id").is_some() && has(s, "operator").is_some() {
            return self.relationship(s);
        }
        if let Some(d) = has(s, "direction_keyword") {
            let _ = d;
            self.direction = has(s, "dir_value").map(text_of);
            return Ok(());
        }
        if has(s, "link_keyword").is_some() || has(s, "callback_keyword").is_some() || has(s, "ignored").is_some() {
            return Ok(());
        }
        if let (Some(id), None) = (class_id, has(s, "keyword")) {
            let ent = self.find_or_create(&class_id_text(id));
            self.apply_generic(ent, has(s, "generic"));
        }
        Ok(())
    }

    fn find_or_create(&mut self, id: &str) -> usize {
        if let Some(i) = self.ents.iter().position(|e| e.id == id) {
            return i;
        }
        self.ents.push(Ent { id: id.to_string(), name: id.to_string(), ..Default::default() });
        self.created_in.insert(id.to_string(), self.count);
        self.ents.len() - 1
    }

    fn apply_generic(&mut self, ent: usize, generic: Option<&Value>) {
        let creating = self.created_in.remove(&self.ents[ent].id) == Some(self.count);
        if let (true, Some(g)) = (creating, generic) {
            if g.is_object() {
                if let Some(t) = has(g, "generic_type") {
                    self.ents[ent].name = format!("{}~{}~", self.ents[ent].name, text_of(t));
                }
            }
        }
    }

    fn class_declaration(&mut self, s: &Value) -> Result<(), Error> {
        let ent = self.find_or_create(&class_id_text(&s["class_id"]));
        let mut label: Option<String> = None;
        if let Some(tl) = has(s, "text_label").filter(|v| v.is_object()) {
            let l = text_of(tl.get("string").unwrap_or(&Value::Null));
            if !l.is_empty() && strip(&l).is_empty() {
                return Err(unported());
            }
            if !l.is_empty() {
                label = Some(l);
            }
        }
        if let Some(v) = s.get("stereotype").and_then(|st| has(st, "stereotype_value")) {
            if self.ents[ent].stereotype.is_none() {
                self.ents[ent].stereotype = Some(text_of(v));
            }
        }
        if label.is_none() {
            self.apply_generic(ent, has(s, "generic"));
        }
        if let Some(l) = label {
            self.ents[ent].name = l;
        }
        if let Some(body) = has(s, "body") {
            if body.is_array() || body.is_object() {
                for m in items(body) {
                    self.add_member(ent, m)?;
                }
            }
        }
        Ok(())
    }

    fn add_member(&mut self, ent: usize, item: &Value) -> Result<(), Error> {
        if let Some(b) = has(item, "body_stereotype") {
            if self.ents[ent].stereotype.is_none() {
                self.ents[ent].stereotype = Some(text_of(b));
            }
            return Ok(());
        }
        if let Some(raw) = has(item, "raw_member") {
            return self.add_raw_member(ent, &text_of(raw));
        }
        let vis = match has(item, "visibility") {
            Some(v) => visibility_of(&text_of(v.get("vis_symbol").unwrap_or(&Value::Null))),
            None => "public",
        };
        let member = has(item, "member").ok_or_else(unported)?;
        // member_source: the source from the first to the last slice
        let mut slices = vec![];
        for k in ["visibility", "member"] {
            if let Some(v) = item.get(k) {
                collect_slices(v, &mut slices);
            }
        }
        let first = slices.iter().map(|s| s.0).min().ok_or_else(unported)?;
        let last = slices.iter().map(|s| s.0 + s.1).max().ok_or_else(unported)?;
        let source_text = self.body.get(first..last).ok_or_else(unported)?.to_string();
        if let Some(name) = has(member, "method_name") {
            let parameters = has(member, "parameters").map_or(String::new(), text_of);
            let return_type = member.get("return_type").and_then(|r| has(r, "type")).map(text_of);
            self.ents[ent].meths.push(Meth {
                name: text_of(name),
                parameters: Some(parameters),
                return_type,
                visibility: vis.into(),
                text: source_text,
            });
        } else {
            let name = text_of(member.get("attr_name").unwrap_or(&Value::Null));
            let ty = has(member, "type").map(text_of);
            self.ents[ent].attrs.push(Attr { name, ty, visibility: vis.into(), text: source_text });
        }
        Ok(())
    }

    fn add_raw_member(&mut self, ent: usize, text: &str) -> Result<(), Error> {
        let source_text = strip(text).to_string();
        let (vis, rest) = match self.raw_visibility.captures(&source_text) {
            Some(c) => (visibility_of(&c[1]), c[2].to_string()),
            None => ("public", source_text.clone()),
        };
        if let Some(call) = self.raw_method.captures(&rest) {
            if !strip(&call["name"]).is_empty() {
                let mut ret = call["rest"].to_string();
                if ret.starts_with('*') || ret.starts_with('$') {
                    ret.remove(0);
                }
                let mut ret = ret.trim_start_matches(is_ws).to_string();
                if ret.ends_with('*') || ret.ends_with('$') {
                    ret.pop();
                    while ret.ends_with(is_ws) {
                        ret.pop();
                    }
                } else {
                    // `\s*[*$]\z` leaves the text alone without the symbol
                }
                self.ents[ent].meths.push(Meth {
                    name: strip(&call["name"]).to_string(),
                    parameters: Some(call["params"].to_string()),
                    return_type: if ret.is_empty() { None } else { Some(ret) },
                    visibility: vis.into(),
                    text: source_text,
                });
                return Ok(());
            }
        }
        if rest.contains(')') {
            return Err(unported());
        }
        let mut name = rest.clone();
        if name.ends_with('*') || name.ends_with('$') {
            name.pop();
            while name.ends_with(is_ws) {
                name.pop();
            }
        }
        self.ents[ent].attrs.push(Attr { name, ty: None, visibility: vis.into(), text: source_text });
        Ok(())
    }

    fn relationship(&mut self, s: &Value) -> Result<(), Error> {
        let from = class_id_text(&s["from_id"]);
        let to = class_id_text(&s["to_id"]);
        let op = text_of(s["operator"].get("arrow").unwrap_or(&Value::Null));
        let a = self.find_or_create(&from);
        self.apply_generic(a, has(s, "from_generic"));
        let b = self.find_or_create(&to);
        self.apply_generic(b, has(s, "to_generic"));
        let card = |k: &str| has(s, k).map(|v| text_of(v.get("string").unwrap_or(&Value::Null)));
        let (sc, tc) = (card("source_card"), card("target_card"));
        let left = matches!(op.as_str(), "<|--" | "<--" | "<|.." | "<..");
        let (from, to, sc, tc) = if left { (to, from, tc, sc) } else { (from, to, sc, tc) };
        let label = if let Some(l) = s.get("pipe_label").and_then(|p| has(p, "label_text")) {
            let re = Regex::new(r#"(?m)^["']|["']$"#).unwrap();
            Some(re.replace_all(&text_of(l), "").into_owned())
        } else {
            s.get("colon_label").and_then(|p| has(p, "label_text")).map(|l| strip(&text_of(l)).to_string())
        };
        let mut rel = Rel {
            from,
            to,
            ty: relationship_type(&op).into(),
            label,
            source_card: sc,
            target_card: tc,
            reversed: left,
            ..Default::default()
        };
        if op == "o.." {
            rel.start_marker = Some("aggregation".into());
            rel.dashed = Some(true);
        } else if let Some(m) = self.mixed.captures(&op) {
            rel.start_marker = marker_type(&m[1]).map(String::from);
            rel.end_marker = marker_type(&m[3]).map(String::from);
            rel.dashed = Some(&m[2] == "..");
        }
        self.rels.push(rel);
        Ok(())
    }

    fn valid(&self) -> bool {
        self.ents.iter().all(|e| {
            !e.id.is_empty()
                && !e.name.is_empty()
                && e.attrs.iter().all(|a| !a.name.is_empty())
                && e.meths.iter().all(|m| !m.name.is_empty())
        }) && self.rels.iter().all(|r| !r.from.is_empty() && !r.to.is_empty() && !r.ty.is_empty())
    }
}

/// `ClassNotes#note_text`: quotes off, `<br>` to newline, tags removed.
fn note_text(slice: &str) -> String {
    let text = slice.trim_matches(is_ws);
    let text = match text.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        Some(inner) => inner,
        None => text,
    };
    let brk = Regex::new(r"(?i)<br\s*/?>").unwrap();
    let tag = Regex::new(r"<[^>]*>").unwrap();
    tag.replace_all(&brk.replace_all(text, "\n"), "").into_owned()
}

fn class_id_text(v: &Value) -> String {
    let t = text_of(v);
    if t.len() >= 2 && t.starts_with('`') && t.ends_with('`') {
        t[1..t.len() - 1].to_string()
    } else {
        t
    }
}

// ---- layout ----

struct Sizes {
    name_font: f64,
    small_font: f64,
}

fn valid_font(v: Option<f64>, default: f64) -> f64 {
    match v {
        Some(f) if f.is_finite() && f > 0.0 => f,
        _ => default,
    }
}

impl Sizes {
    fn trebuchet(&self, t: &str, size: f64) -> f64 {
        text::width(t, size, Face::Trebuchet)
    }

    fn scaled_mmdc(&self) -> f64 {
        MMDC_FONT_SIZE * self.small_font / 12.0
    }

    fn mmdc_text_width(&self, t: &str) -> f64 {
        self.trebuchet(&display(t), self.scaled_mmdc())
    }

    fn bold_advance(&self, c: char) -> f64 {
        match name_advance(c) {
            Some(p) => p as f64 * self.name_font / 1000.0,
            None => self.trebuchet(&c.to_string(), self.name_font) * FALLBACK_RATIO,
        }
    }

    fn name_width(&self, name: &str) -> f64 {
        super::pie::fsum(display(name).chars().map(|c| self.bold_advance(c)))
    }

    fn entity_width(&self, e: &Ent) -> f64 {
        let mut header = self.name_width(&e.name);
        if let Some(st) = &e.stereotype {
            header = header.max(self.mmdc_text_width(&format!("«{st}»")));
        }
        let members: Vec<f64> = e
            .attrs
            .iter()
            .map(|a| self.mmdc_text_width(&a.display_text()))
            .chain(e.meths.iter().map(|m| self.mmdc_text_width(&m.display_text())))
            .collect();
        if members.is_empty() {
            return header + HEADER_PADDING;
        }
        let widest = members.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (header + HEADER_PADDING).max(widest + (header / 2.0) + HEADER_PADDING)
    }

    fn entity_height(&self, e: &Ent) -> f64 {
        let (a, m) = (e.attrs.len() as f64, e.meths.len() as f64);
        let header = if e.stereotype.is_some() { STEREOTYPE_HEIGHT } else { 0.0 };
        if a + m == 0.0 {
            return EMPTY_CLASS_HEIGHT + header;
        }
        let base = if a == 0.0 { METHODS_ONLY_BASE } else { ATTRIBUTES_BASE };
        base + header + (MEMBER_HEIGHT * (a + m))
    }
}

#[derive(Clone)]
struct Node {
    ent: usize,
    /// Index into `Model::notes` for a note box (`ent` is then unused).
    note: Option<usize>,
    x: f64,
    y: f64,
    /// Ruby Integer-typed `y` (the first grid row): decides the viewBox text.
    y_int: bool,
    w: f64,
    h: f64,
}

fn dir_of(d: Option<&str>) -> &'static str {
    match d {
        Some("LR") => "RIGHT",
        Some("RL") => "LEFT",
        Some("BT") => "UP",
        _ => "DOWN",
    }
}

fn max_of(it: impl Iterator<Item = f64>) -> f64 {
    it.fold(0.0, f64::max)
}

/// `ClassCellPlacement.pack`: three columns of cells, `CELL_GAP` apart.
fn pack(nodes: &mut [Node]) {
    let n = nodes.len();
    let rows = n.div_ceil(COLUMNS);
    let mut xs = vec![GRID_MARGIN];
    for col in 0..COLUMNS {
        let w = max_of((0..rows).filter_map(|r| nodes.get(r * COLUMNS + col)).map(|c| c.w));
        xs.push(xs.last().unwrap() + (w + CELL_GAP));
    }
    let mut ys = vec![GRID_MARGIN];
    for r in 0..rows {
        let h = max_of((0..COLUMNS).filter_map(|c| nodes.get(r * COLUMNS + c)).map(|c| c.h));
        ys.push(ys.last().unwrap() + (h + CELL_GAP));
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        let (r, c) = (i / COLUMNS, i % COLUMNS);
        node.x = xs[c];
        node.y = ys[r];
        node.y_int = r == 0;
    }
}

// ---- scene ----

/// A Ruby Integer-or-Float coordinate after `normalized_coordinate`.
#[derive(Clone, Copy)]
struct Co {
    v: f64,
    int: bool,
}

fn norm(v: f64) -> Co {
    Co { v, int: v.fract() == 0.0 && v.abs() < 1e15 }
}

impl Co {
    fn show(&self) -> String {
        if self.int { format!("{}", self.v as i64) } else { ruby_float(self.v) }
    }
}

#[derive(Clone, Copy)]
struct Pt {
    x: Co,
    y: Co,
}

fn connection_point(from: &Node, to: &Node) -> (f64, f64) {
    let fc = (from.x + from.w / 2.0, from.y + from.h / 2.0);
    let tc = (to.x + to.w / 2.0, to.y + to.h / 2.0);
    let (dx, dy) = (tc.0 - fc.0, tc.1 - fc.1);
    if dx.abs() < 0.001 && dy.abs() < 0.001 {
        return fc;
    }
    let project = |origin: f64, num: f64, den: f64, offset: f64| if num.abs() < 0.001 { origin } else { origin + ((num / den) * offset) };
    if dx.abs() > dy.abs() {
        let x = if dx > 0.0 { from.x + from.w } else { from.x };
        (x, project(fc.1, dy, dx, x - fc.0))
    } else {
        let y = if dy > 0.0 { from.y + from.h } else { from.y };
        (project(fc.0, dx, dy, y - fc.1), y)
    }
}

struct Marker {
    points: String,
    fill: &'static str,
}

fn marker_point(x: f64, y: f64, dist: f64, angle: f64) -> (f64, f64) {
    (x + (dist * angle.cos()), y + (dist * angle.sin()))
}

fn angle(from: Pt, to: Pt) -> f64 {
    (to.y.v - from.y.v).atan2(to.x.v - from.x.v)
}

fn fmt_pts(points: &[String]) -> String {
    points.join(" ")
}

fn fp(p: (f64, f64)) -> String {
    format!("{},{}", ruby_float(p.0), ruby_float(p.1))
}

fn triangle_marker(from: Pt, to: Pt, filled: bool) -> Marker {
    let a = angle(from, to);
    let first = marker_point(to.x.v, to.y.v, -ARROW_SIZE, a + (std::f64::consts::PI / 6.0));
    let second = marker_point(to.x.v, to.y.v, -ARROW_SIZE, a - (std::f64::consts::PI / 6.0));
    Marker {
        points: fmt_pts(&[format!("{},{}", to.x.show(), to.y.show()), fp(first), fp(second)]),
        fill: if filled { "#000000" } else { "#ffffff" },
    }
}

fn dart_marker(from: Pt, to: Pt) -> Marker {
    let a = angle(from, to);
    let tip = marker_point(from.x.v, from.y.v, DART_NEAR, a);
    let back = marker_point(from.x.v, from.y.v, DART_FAR, a);
    let first = marker_point(back.0, back.1, DART_WIDTH, a + (std::f64::consts::PI / 2.0));
    let second = marker_point(back.0, back.1, DART_WIDTH, a - (std::f64::consts::PI / 2.0));
    let mid = ((tip.0 + back.0) / 2.0, (tip.1 + back.1) / 2.0);
    Marker { points: fmt_pts(&[fp(tip), fp(first), fp(mid), fp(second)]), fill: "#000000" }
}

fn diamond_marker(from: Pt, to: Pt, filled: bool) -> Marker {
    let a = angle(from, to);
    let (x, y) = (from.x.v, from.y.v);
    let pts = [
        marker_point(x, y, DIAMOND_SIZE, a),
        marker_point(x, y, 6.0, a + (std::f64::consts::PI / 2.0)),
        marker_point(x, y, -DIAMOND_SIZE, a),
        marker_point(x, y, 6.0, a - (std::f64::consts::PI / 2.0)),
    ];
    Marker { points: fmt_pts(&pts.map(fp)), fill: if filled { "#000000" } else { "#ffffff" } }
}

fn marker_at(point: Pt, away: Pt, ty: Option<&str>) -> Option<Marker> {
    match ty? {
        "inheritance" => Some(triangle_marker(away, point, false)),
        "dependency" => Some(dart_marker(point, away)),
        "composition" => Some(diamond_marker(point, away, true)),
        "aggregation" => Some(diamond_marker(point, away, false)),
        _ => None,
    }
}

struct Style {
    node_fill: &'static str,
    node_stroke: &'static str,
    edge_stroke: &'static str,
    label_text: &'static str,
}

struct Ctx {
    st: Style,
    name_font: f64,
    small_font: f64,
}

fn fmt_size(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { ruby_float(v) }
}

fn text_el(st: &Style, t: &str, x: f64, y: f64, family: &str, size: f64, anchor: Option<&str>, weight: Option<&str>) -> Elem {
    let mut e = Elem::new("text").attr("fill", st.label_text).num("x", x).num("y", y);
    if let Some(a) = anchor {
        e = e.attr("text-anchor", a);
    }
    e = e.attr("font-family", family).attr("font-size", fmt_size(size));
    if let Some(w) = weight {
        e = e.attr("font-weight", w);
    }
    e.content(t)
}

fn line_el(stroke: &str, width: &str, x1: f64, y1: f64, x2: f64, y2: f64, dash: Option<&str>) -> Elem {
    let mut e = Elem::new("line").attr("stroke", stroke).attr("stroke-width", width).num("x1", x1).num("y1", y1).num("x2", x2).num("y2", y2);
    if let Some(d) = dash {
        e = e.attr("stroke-dasharray", d);
    }
    e
}

fn render_node(model: &Model, n: &Node, cx: &Ctx) -> Elem {
    let e = &model.ents[n.ent];
    let st = &cx.st;
    let mut g = Elem::new("g").attr("id", format!("class-{}", e.id));
    g.push(
        Elem::new("rect")
            .attr("fill", st.node_fill)
            .attr("stroke", st.node_stroke)
            .attr("stroke-width", "2")
            .num("x", n.x)
            .num("y", n.y)
            .num("width", n.w)
            .num("height", n.h)
            .num("rx", 3.0)
            .num("ry", 3.0),
    );
    let mut cursor = n.y + COMPARTMENT_PADDING;
    let cxm = n.x + n.w / 2.0;
    if let Some(s) = e.stereotype.as_ref().filter(|s| !s.is_empty()) {
        g.push(text_el(st, &format!("«{s}»"), cxm, cursor, "Arial, sans-serif", cx.small_font, Some("middle"), None));
        cursor += RENDER_LINE_HEIGHT;
    }
    g.push(text_el(st, &display(&e.name), cxm, cursor, "Arial, sans-serif", cx.name_font, Some("middle"), Some("bold")));
    cursor += RENDER_LINE_HEIGHT;
    let sep1 = cursor + 5.0;
    g.push(line_el(st.node_stroke, "1", n.x, sep1, n.x + n.w, sep1, None));
    let start = sep1 + 10.0;
    let attrs: Vec<String> = e.attrs.iter().map(Attr::display_text).collect();
    for (i, t) in attrs.iter().enumerate() {
        g.push(text_el(st, t, n.x + COMPARTMENT_PADDING, start + i as f64 * RENDER_LINE_HEIGHT, "monospace", cx.small_font, None, None));
    }
    let mut cursor = start + attrs.len() as f64 * RENDER_LINE_HEIGHT;
    if !attrs.is_empty() {
        let sep2 = cursor + 5.0;
        g.push(line_el(st.node_stroke, "1", n.x, sep2, n.x + n.w, sep2, None));
        cursor = sep2 + 10.0;
    }
    for (i, m) in e.meths.iter().enumerate() {
        g.push(text_el(st, &m.display_text(), n.x + COMPARTMENT_PADDING, cursor + i as f64 * RENDER_LINE_HEIGHT, "monospace", cx.small_font, None, None));
    }
    g
}

fn render_namespace(b: &namespace::NsBox, cx: &Ctx) -> Elem {
    let st = &cx.st;
    let mut g = Elem::new("g").attr("id", format!("namespace-{}", b.id.strip_prefix("namespace:").unwrap_or(&b.id)));
    g.push(
        Elem::new("rect")
            .attr("fill", "none")
            .attr("stroke", st.node_stroke)
            .attr("stroke-width", "1")
            .num("x", b.x)
            .num("y", b.y)
            .num("width", b.w)
            .num("height", b.h),
    );
    g.push(text_el(st, &b.title, b.title_x, b.y + 22.0, "Arial, sans-serif", 16.0, Some("middle"), None));
    g
}

fn render_note(id: usize, text: &str, n: &Node, target: Option<&Node>) -> Elem {
    let mut g = Elem::new("g").attr("id", format!("note-{id}"));
    if let Some(t) = target {
        let (a, b) = (connection_point(n, t), connection_point(t, n));
        g.push(line_el("#aaaa33", "1", a.0, a.1, b.0, b.1, Some("2,2")));
    }
    g.push(
        Elem::new("rect")
            .attr("fill", "#fff5ad")
            .attr("stroke", "#aaaa33")
            .attr("stroke-width", "1")
            .num("x", n.x)
            .num("y", n.y)
            .num("width", n.w)
            .num("height", n.h),
    );
    g.push(
        Elem::new("text")
            .attr("fill", "#000000")
            .num("x", n.x + n.w / 2.0)
            .num("y", n.y + 23.0)
            .attr("text-anchor", "middle")
            .attr("font-family", "Arial, sans-serif")
            .attr("font-size", "16")
            .content(text),
    );
    g
}

fn render_edge(rel: &Rel, from: &Node, to: &Node, cx: &Ctx) -> Elem {
    let st = &cx.st;
    let a = connection_point(from, to);
    let b = connection_point(to, from);
    let src = Pt { x: norm(a.0), y: norm(a.1) };
    let tgt = Pt { x: norm(b.0), y: norm(b.1) };
    // sections have no bend points: source_next is the end, target_previous the start
    let mixed = rel.start_marker.is_some() || rel.end_marker.is_some();
    let (markers, dashed): (Vec<Marker>, bool) = if mixed {
        let m = [marker_at(src, tgt, rel.start_marker.as_deref()), marker_at(tgt, src, rel.end_marker.as_deref())];
        (m.into_iter().flatten().collect(), rel.dashed == Some(true))
    } else {
        let m = match rel.ty.as_str() {
            "inheritance" => Some(triangle_marker(src, tgt, true)),
            "realization" => Some(triangle_marker(src, tgt, false)),
            "composition" => Some(diamond_marker(src, tgt, true)),
            "aggregation" => Some(diamond_marker(src, tgt, false)),
            _ => None,
        };
        (m.into_iter().collect(), rel.ty == "dependency")
    };
    let mut g = Elem::new("g").attr("id", format!("rel-{}_to_{}", rel.from, rel.to));
    g.push(line_el(st.edge_stroke, "2", a.0, a.1, b.0, b.1, if dashed { Some("5,5") } else { None }));
    for m in markers {
        g.push(Elem::new("polygon").attr("fill", m.fill).attr("stroke", st.edge_stroke).attr("stroke-width", "2").attr("points", m.points));
    }
    let arial = "Arial, sans-serif";
    let sf = cx.small_font;
    if let Some(l) = rel.label.as_ref().filter(|l| !l.is_empty()) {
        let half = |p: f64, q: f64, both_int: bool| if both_int { ((p as i64 + q as i64).div_euclid(2)) as f64 } else { (p + q) / 2.0 };
        let x = half(src.x.v, tgt.x.v, src.x.int && tgt.x.int);
        let y = half(src.y.v, tgt.y.v, src.y.int && tgt.y.int) - 5.0;
        g.push(text_el(st, l, x, y, arial, sf, Some("middle"), None));
    }
    if let Some(l) = rel.source_card.as_ref().filter(|l| !l.is_empty()) {
        g.push(text_el(st, l, src.x.v + 5.0, src.y.v - 5.0, arial, sf, None, None));
    }
    if let Some(l) = rel.target_card.as_ref().filter(|l| !l.is_empty()) {
        g.push(text_el(st, l, tgt.x.v - 5.0, tgt.y.v - 5.0, arial, sf, Some("end"), None));
    }
    g
}

pub fn render(source: &str) -> Result<String, Error> {
    let body = crate::parse::body(source);
    let preamble = source.rfind(body.as_str()).map_or(source, |i| &source[..i]);
    if ["layout", "theme", "init", "---", "config"].iter().any(|k| preamble.contains(k)) {
        return Err(unported());
    }
    let tree = crate::parse::parse(source, "class_diagram")?;
    let mut model = Model::new(body);
    model.apply(&tree)?;
    if !model.valid() {
        return Err(unported());
    }

    let theme = Theme::default_theme();
    let sizes = Sizes {
        name_font: valid_font(theme.number("typography", "font_size_large"), 16.0),
        small_font: valid_font(theme.number("typography", "font_size_small"), 12.0),
    };
    let cx = Ctx {
        st: Style {
            node_fill: theme.color("node_fill").unwrap_or(""),
            node_stroke: theme.color("node_stroke").unwrap_or(""),
            edge_stroke: theme.color("edge_stroke").unwrap_or(""),
            label_text: theme.color("label_text").unwrap_or(""),
        },
        name_font: sizes.name_font,
        small_font: sizes.small_font,
    };

    let mut nodes: Vec<Node> = model
        .ents
        .iter()
        .enumerate()
        .map(|(i, e)| Node { ent: i, note: None, x: 0.0, y: 0.0, y_int: false, w: sizes.entity_width(e), h: sizes.entity_height(e) })
        .collect();
    let class_count = nodes.len();
    // ClassNoteNodes: a box per note, after the classes
    let mut note_targets: Vec<Option<usize>> = vec![];
    for (i, (text, target)) in model.notes.iter().enumerate() {
        let w = text::width(text, sizes.name_font, Face::Arial) + 12.0;
        nodes.push(Node { ent: 0, note: Some(i), x: 0.0, y: 0.0, y_int: false, w, h: 24.0 + 12.0 });
        note_targets.push(target.as_ref().and_then(|t| {
            let name = t.replace('`', "");
            if let Some(i) = model.ents.iter().position(|e| e.id == name) {
                return Some(i);
            }
            let suffix = format!(".{name}");
            let m: Vec<usize> = (0..model.ents.len()).filter(|&i| model.ents[i].id.ends_with(&suffix)).collect();
            if m.len() == 1 { Some(m[0]) } else { None }
        }));
    }
    let index: HashMap<&str, usize> = model.ents.iter().enumerate().map(|(i, e)| (e.id.as_str(), i)).collect();
    let dir = dir_of(model.direction.as_deref());
    // written edges: a reversed relationship is laid out from its target
    let ends = |r: &Rel| -> (usize, usize) {
        let (f, t) = (index[r.from.as_str()], index[r.to.as_str()]);
        if r.reversed { (t, f) } else { (f, t) }
    };

    let layout_edges = || -> Vec<place::Edge> {
        model
            .rels
            .iter()
            .map(|r| {
                let (src, tgt) = ends(r);
                let labels = [r.label.as_ref(), r.source_card.as_ref(), r.target_card.as_ref()]
                    .into_iter()
                    .flatten()
                    .filter(|l| !l.is_empty())
                    .map(|l| (text::width(l, sizes.small_font, Face::Arial), sizes.small_font))
                    .collect();
                place::Edge { src, tgt, labels }
            })
            .collect()
    };
    let known: Vec<String> = model.ents.iter().map(|e| e.id.clone()).collect();
    let namespaces = namespace::collect(&model.blocks, &known);
    let mut boxes: Vec<namespace::NsBox> = vec![];
    let mut order: Vec<usize> = (0..class_count).collect();
    if !model.blocks.is_empty() {
        if !model.notes.is_empty() {
            return Err(unported());
        }
        let pairs: Vec<(&str, &str)> = model.rels.iter().map(|r| (r.from.as_str(), r.to.as_str())).collect();
        let hier = namespace::hierarchical(&namespaces, &pairs);
        let classes: Vec<(usize, String, f64, f64)> = nodes.iter().map(|n| (n.ent, model.ents[n.ent].id.clone(), n.w, n.h)).collect();
        let measure = |t: &str| {
            let m = text::measure(t, sizes.name_font, Face::Arial);
            (m.width, m.height)
        };
        let mut tree = namespace::nest(&classes, &namespaces, hier, &measure);
        namespace::place_all(&mut tree, &layout_edges(), dir)?;
        let mut plain = vec![];
        namespace::flatten(&tree, (0.0, 0.0), &mut plain, &mut boxes);
        order.clear();
        for (ent, x, y) in plain {
            nodes[ent].x = x;
            nodes[ent].y = y;
            order.push(ent);
        }
    } else if nodes.len() >= 3 {
        let edges = layout_edges();
        let sz: Vec<(f64, f64)> = nodes.iter().map(|n| (n.w, n.h)).collect();
        let pos = place::place(&sz, &edges, dir)?;
        for (n, (x, y)) in nodes.iter_mut().zip(pos) {
            n.x = x;
            n.y = y;
        }
    } else {
        let pair = if dir == "DOWN" && nodes.len() == 2 && class_count == 2 && model.rels.len() == 1 {
            let (s, t) = ends(&model.rels[0]);
            if s != t { Some((s, t)) } else { None }
        } else {
            None
        };
        match pair {
            Some((s, t)) => {
                let width = nodes[s].w.max(nodes[t].w);
                nodes[s].x = GRID_MARGIN + ((width - nodes[s].w) / 2.0);
                nodes[s].y = GRID_MARGIN;
                nodes[s].y_int = true;
                nodes[t].x = GRID_MARGIN + ((width - nodes[t].w) / 2.0);
                nodes[t].y = nodes[s].y + nodes[s].h + CELL_GAP;
            }
            None => pack(&mut nodes),
        }
    }

    // scene dimensions (Ruby Integer-vs-Float typing decides the viewBox text)
    let (vw, vh, doc_w, doc_h) = if nodes.is_empty() && boxes.is_empty() {
        ("880".to_string(), "680".to_string(), 880.0, 680.0)
    } else {
        let mut extents: Vec<(f64, f64, bool)> = nodes.iter().map(|n| (n.x + n.w, n.y + n.h, n.y_int)).collect();
        extents.extend(boxes.iter().map(|b| (b.x + b.w, b.y + b.h, false)));
        let w = extents.iter().map(|e| e.0).fold(f64::NEG_INFINITY, f64::max) + 40.0 + DIAGRAM_PADDING * 2.0;
        let mut best: Option<(f64, bool)> = None;
        for e in &extents {
            if best.is_none_or(|b| e.1 > b.0) {
                best = Some((e.1, e.2));
            }
        }
        let (h, int) = best.unwrap();
        let h = h + 40.0 + DIAGRAM_PADDING * 2.0;
        (ruby_float(w), if int { format!("{}", h as i64) } else { ruby_float(h) }, w, h)
    };
    let mut doc = Document::new(doc_w, doc_h);
    doc.view_box = Some(format!("0 0 {vw} {vh}"));
    doc.add(Elem::new("g").attr("id", "defs"));
    for b in &boxes {
        doc.add(render_namespace(b, &cx));
    }
    for r in &model.rels {
        doc.add(render_edge(r, &nodes[index[r.from.as_str()]], &nodes[index[r.to.as_str()]], &cx));
    }
    for &i in &order {
        doc.add(render_node(&model, &nodes[i], &cx));
    }
    for n in nodes.iter().filter(|n| n.note.is_some()) {
        let i = n.note.unwrap();
        doc.add(render_note(i, &model.notes[i].0, n, note_targets[i].map(|t| &nodes[t])));
    }
    Ok(doc.to_xml())
}
