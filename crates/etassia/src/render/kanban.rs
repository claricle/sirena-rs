//! Kanban renderer: port of Ruby `Parser::Native::Builders::Kanban` (board
//! rules), `Layout::Kanban`, `Layout::KanbanCardText` and `Renderer::Kanban`.
//!
//! Declined to the Ruby path (`Error::Unported`): card/column text carrying
//! emphasis markers (`*`, `_`; kramdown territory), `@{ }` metadata beyond
//! simple flow/block mappings of plain or quoted strings (the YAML schema
//! port), and every board rule that raises a parse error.
use super::pie::{items, text_of};
use crate::svg::{ruby_float, Document, Elem, Theme};
use crate::text::{self, Face};
use crate::Error;
use serde_json::Value;

const COLUMN_HORIZONTAL_SPACING: f64 = 5.0;
const CARD_VERTICAL_SPACING: f64 = 5.0;
const COLUMN_WIDTH: f64 = 200.0;
const COLUMN_HEADER_HEIGHT: f64 = 25.0;
const EMPTY_COLUMN_CONTENT_HEIGHT: f64 = 25.0;
const COLUMN_PADDING: f64 = 10.0;
const CARD_HORIZONTAL_INSET: f64 = 7.5;
const CARD_HEIGHT: f64 = 44.0;
const CARD_PADDING: f64 = 10.0;
const CARD_FONT_SIZE: f64 = 16.0;
const EXTRA_LINE_HEIGHT: f64 = 24.0;
const CANVAS_PADDING: f64 = 40.0;

fn unported() -> Error {
    Error::Unported("kanban".into())
}

// ---- model ----

#[derive(Clone, Default)]
struct Meta {
    assigned: Option<String>,
    ticket: Option<String>,
    icon: Option<String>,
    label: Option<String>,
    priority: Option<String>,
}

struct Item {
    text: String,
    indent: usize,
    meta: Meta,
}

struct Column {
    title: String,
    cards: Vec<Item>,
}

// ---- metadata (`@{ ... }`), simple YAML subset ----

fn plain_ok(v: &str) -> bool {
    let mut chars = v.chars();
    let Some(first) = chars.next() else { return false };
    first.is_ascii_alphabetic()
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-' | '/' | '(' | ')'))
        && !matches!(v, "true" | "True" | "TRUE" | "false" | "False" | "FALSE" | "null" | "Null" | "NULL")
}

/// A value at the start of `s`: quoted or plain up to `stop` (a char that
/// ends a plain scalar). Returns (text, rest).
fn scalar<'a>(s: &'a str, plain_stop: Option<char>) -> Result<(String, &'a str), Error> {
    let s = s.trim_start_matches(' ');
    if let Some(rest) = s.strip_prefix('\'') {
        let end = rest.find('\'').ok_or_else(unported)?;
        let (v, after) = (&rest[..end], &rest[end + 1..]);
        if after.starts_with('\'') || v.contains('\n') {
            return Err(unported());
        }
        return Ok((v.to_string(), after));
    }
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"').ok_or_else(unported)?;
        let v = &rest[..end];
        if v.contains('\\') || v.contains('\n') {
            return Err(unported());
        }
        return Ok((v.to_string(), &rest[end + 1..]));
    }
    let end = match plain_stop {
        Some(c) => s.find(c).unwrap_or(s.len()),
        None => s.len(),
    };
    let v = s[..end].trim_end_matches(' ');
    if !plain_ok(v) {
        return Err(unported());
    }
    Ok((v.to_string(), &s[end..]))
}

fn key_ok(k: &str) -> bool {
    let mut c = k.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn metadata_pairs(body: &str) -> Result<Vec<(String, String)>, Error> {
    if body.chars().any(|c| matches!(c, '\t' | '#' | '%' | '\u{feff}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')) {
        return Err(unported());
    }
    let mut pairs: Vec<(String, String)> = vec![];
    if body.contains('\n') {
        let lines: Vec<&str> = body.split('\n').filter(|l| !l.trim_matches(' ').is_empty()).collect();
        let indent = lines.first().map(|l| l.len() - l.trim_start_matches(' ').len()).ok_or_else(unported)?;
        for line in lines {
            if line.len() - line.trim_start_matches(' ').len() != indent {
                return Err(unported());
            }
            let rest = line.trim_start_matches(' ');
            let (k, v) = rest.split_once(':').ok_or_else(unported)?;
            if !v.starts_with(' ') || !key_ok(k) {
                return Err(unported());
            }
            let (val, after) = scalar(v, None)?;
            if !after.trim_matches(' ').is_empty() {
                return Err(unported());
            }
            pairs.push((k.to_string(), val));
        }
    } else {
        let mut rest = body.trim_matches(' ');
        if rest.is_empty() {
            return Err(unported());
        }
        loop {
            let (k, v) = rest.split_once(':').ok_or_else(unported)?;
            let k = k.trim_matches(' ');
            if !v.starts_with(' ') || !key_ok(k) {
                return Err(unported());
            }
            let (val, after) = scalar(v, Some(','))?;
            pairs.push((k.to_string(), val));
            let after = after.trim_start_matches(' ');
            match after.strip_prefix(',') {
                Some(next) => rest = next.trim_start_matches(' '),
                None if after.is_empty() => break,
                None => return Err(unported()),
            }
        }
    }
    for (i, (k, _)) in pairs.iter().enumerate() {
        if k == "shape" || pairs[..i].iter().any(|(o, _)| o == k) {
            return Err(unported());
        }
    }
    Ok(pairs)
}

fn apply_metadata(meta: &mut Meta, pairs: Vec<(String, String)>) {
    for (k, v) in pairs {
        if v.is_empty() {
            continue; // dropped by mermaid
        }
        match k.as_str() {
            "assigned" => meta.assigned = Some(v),
            "ticket" => meta.ticket = Some(v),
            "icon" => meta.icon = Some(v),
            "label" => meta.label = Some(v),
            "priority" => meta.priority = Some(v),
            _ => {}
        }
    }
}

// ---- builder (BoardBuilder) ----

fn build(tree: &Value) -> Result<Vec<Column>, Error> {
    let lines = tree.get("lines").map(items).unwrap_or_default();
    let mut anonymous = 0usize;
    let mut list: Vec<Item> = vec![];
    for line in lines {
        let icon = line.get("icon");
        let classes = line.get("classes");
        if icon.is_some() || classes.is_some() {
            let Some(last) = list.last_mut() else { return Err(unported()) };
            if let Some(i) = icon {
                if i.is_array() {
                    return Err(unported());
                }
                last.meta.icon = Some(text_of(i));
            }
            continue;
        }
        let id = match line.get("id") {
            Some(v) if !v.is_array() => text_of(v),
            Some(_) => return Err(unported()),
            None => {
                anonymous += 1;
                format!("kanban-{anonymous}")
            }
        };
        let text = match line.get("text") {
            Some(Value::Array(a)) if a.is_empty() => "[]".to_string(),
            Some(v) => text_of(v),
            None => id.clone(),
        };
        let indent = line.get("indent").map(|v| text_of(v).chars().count()).unwrap_or(0);
        let mut meta = Meta::default();
        if let Some(m) = line.get("metadata") {
            apply_metadata(&mut meta, metadata_pairs(&text_of(&m["body"]))?);
        }
        list.push(Item { text, indent, meta });
    }
    let Some(column_indent) = list.first().map(|i| i.indent) else { return Ok(vec![]) };
    let mut columns: Vec<Column> = vec![];
    let mut shallower = false;
    for item in list {
        if shallower {
            return Err(unported());
        }
        if item.indent < column_indent {
            shallower = true;
        }
        if item.indent == column_indent {
            let title = item.meta.label.clone().unwrap_or_else(|| item.text.clone());
            columns.push(Column { title, cards: vec![] });
        } else {
            columns.last_mut().ok_or_else(unported)?.cards.push(item);
        }
    }
    Ok(columns)
}

// ---- markdown text (literal path) ----

/// `MarkdownText.parse_lines` for text without emphasis markers: one
/// `Option<text>` per line (`None` = empty line).
fn parse_lines(text: &str) -> Result<Vec<Option<String>>, Error> {
    if text.is_empty() {
        return Ok(vec![None]);
    }
    if text.contains('*') || text.contains('_') {
        return Err(unported());
    }
    // split(/\n{2,}/, -1)
    let mut paragraphs: Vec<&str> = vec![];
    let b = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        if b[i] == b'\n' {
            let mut j = i;
            while j < b.len() && b[j] == b'\n' {
                j += 1;
            }
            if j - i >= 2 {
                paragraphs.push(&text[start..i]);
                start = j;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    paragraphs.push(&text[start..]);
    let mut out = vec![];
    for p in paragraphs {
        if p.is_empty() {
            continue;
        }
        for line in p.split('\n') {
            out.push(if line.is_empty() { None } else { Some(line.to_string()) });
        }
    }
    Ok(out)
}

fn is_rb_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

fn measure(text_: &str, size: f64) -> f64 {
    text::width(text_, size, Face::Arial)
}

/// `KanbanCardText.lines`: wrapped lines, each `None` (empty) or its text.
fn card_lines(text_: &str, width: f64, size: f64) -> Result<Vec<Option<String>>, Error> {
    let mut out = vec![];
    for line in parse_lines(text_)? {
        let Some(line) = line else {
            out.push(None);
            continue;
        };
        // pieces
        let mut pieces: Vec<String> = vec![];
        let mut chars = line.chars().peekable();
        while let Some(&c) = chars.peek() {
            let space = is_rb_space(c);
            let mut token = String::new();
            while let Some(&d) = chars.peek() {
                if is_rb_space(d) == space {
                    token.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if measure(&token, size) <= width {
                pieces.push(token);
            } else {
                let mut chunks = vec![String::new()];
                for ch in token.chars() {
                    let last = chunks.last().unwrap();
                    if !last.is_empty() && measure(&format!("{last}{ch}"), size) > width {
                        chunks.push(String::new());
                    }
                    chunks.last_mut().unwrap().push(ch);
                }
                pieces.extend(chunks);
            }
        }
        let mut lines: Vec<Vec<String>> = vec![vec![]];
        for piece in pieces {
            let used = lines.last().unwrap().iter().fold(0.0, |u, p| u + measure(p, size));
            if !lines.last().unwrap().is_empty() && used + measure(&piece, size) > width {
                lines.push(vec![]);
            }
            if piece.chars().all(is_rb_space) && lines.last().unwrap().is_empty() && lines.len() > 1 {
                continue;
            }
            lines.last_mut().unwrap().push(piece);
        }
        for l in lines {
            if l.is_empty() {
                out.push(None);
            } else {
                let joined = l.concat();
                out.push(Some(joined.trim_end_matches(|c: char| is_rb_space(c) || c == '\0').to_string()));
            }
        }
    }
    Ok(out)
}

/// `Renderer::MarkdownText.assign_markdown_text` for non-emphasis runs.
fn markdown_text(mut e: Elem, lines: &[Option<String>], x: f64, bold: bool) -> Elem {
    if lines.len() == 1 {
        return e.content(lines[0].as_deref().unwrap_or(""));
    }
    let mut pending = 0;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            pending += 1;
        }
        if let Some(t) = line {
            let mut span = Elem::new("tspan");
            if pending > 0 {
                span = span.num("x", x).attr("line-shift", pending.to_string());
            }
            if bold {
                span = span.attr("font-weight", "bold");
            }
            e.push(span.content(t));
            pending = 0;
        }
    }
    e
}

// ---- layout + render ----

fn size_str(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { ruby_float(v) }
}

fn font_size(theme: Theme, name: &str, fallback: f64) -> f64 {
    theme.number("typography", name).filter(|v| *v > 0.0).unwrap_or(fallback)
}

struct CardL {
    x: f64,
    y: f64,
    height: f64,
    text: String,
    lines: usize,
    meta: Vec<(&'static str, String)>,
    ticket: Option<String>,
    assigned: Option<String>,
    priority: Option<String>,
}

struct ColL {
    x: f64,
    height: f64,
    height_float: bool,
    header_h: f64,
    title: String,
    cards: Vec<CardL>,
}

enum Style {
    Header,
    Badge,
    MetaLabel,
    Other,
}

fn text_elem(theme: Theme, x: f64, y: f64, anchor: Option<&str>, size: f64, weight: Option<&str>, style: Style) -> Elem {
    let color = match style {
        Style::Header | Style::Badge => theme.color("background"),
        Style::MetaLabel => theme.color("secondary"),
        Style::Other => theme.color("label_text"),
    };
    let mut e = Elem::new("text").num("x", x).num("y", y);
    if let Some(a) = anchor {
        e = e.attr("text-anchor", a);
    }
    e = e
        .attr("fill", color.unwrap_or(""))
        .attr("font-size", size_str(size))
        .attr("font-family", theme.string("typography", "font_family").unwrap_or("Arial, sans-serif"));
    if let Some(w) = weight {
        e = e.attr("font-weight", w);
    }
    e
}

fn box_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Elem {
    Elem::new("rect").num("x", x).num("y", y).num("width", w).num("height", h).num("rx", r).num("ry", r)
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "kanban")?;
    let columns = build(&tree)?;
    let theme = Theme::default_theme();
    let card_size = CARD_FONT_SIZE;
    let line_h = font_size(theme, "font_size_small", 12.0);
    let text_w = COLUMN_WIDTH - CARD_HORIZONTAL_INSET * 2.0 - CARD_PADDING * 2.0;

    // layout
    let mut cols: Vec<ColL> = vec![];
    for (index, col) in columns.iter().enumerate() {
        let header_h = COLUMN_HEADER_HEIGHT + col.title.matches('\n').count() as f64 * EXTRA_LINE_HEIGHT;
        let x = index as f64 * (COLUMN_WIDTH + COLUMN_HORIZONTAL_SPACING);
        let mut cards = vec![];
        let mut y = header_h;
        let mut sum = 0.0;
        let mut any_meta = false;
        for item in &col.cards {
            let lines = card_lines(&item.text, text_w, card_size)?.len();
            let mut meta: Vec<(&'static str, String)> = vec![];
            for (k, v) in [("assigned", &item.meta.assigned), ("ticket", &item.meta.ticket), ("icon", &item.meta.icon), ("label", &item.meta.label)] {
                if let Some(v) = v {
                    meta.push((k, v.clone()));
                }
            }
            let mut height = CARD_HEIGHT;
            if !meta.is_empty() {
                height += line_h;
                any_meta = true;
            }
            height += (lines as f64 - 1.0) * EXTRA_LINE_HEIGHT;
            cards.push(CardL {
                x: x + CARD_HORIZONTAL_INSET,
                y,
                height,
                text: item.text.clone(),
                lines,
                ticket: item.meta.ticket.clone(),
                assigned: item.meta.assigned.clone(),
                priority: item.meta.priority.clone(),
                meta,
            });
            y += height + CARD_VERTICAL_SPACING;
            sum += height;
        }
        let height = if cards.is_empty() {
            header_h + EMPTY_COLUMN_CONTENT_HEIGHT
        } else {
            header_h + sum + (cards.len() as f64 - 1.0) * CARD_VERTICAL_SPACING + COLUMN_PADDING
        };
        cols.push(ColL { x, height, height_float: any_meta, header_h, title: col.title.clone(), cards });
    }
    let (width, height, height_float) = if cols.is_empty() {
        (0.0, 0.0, false)
    } else {
        let w = cols.iter().map(|c| c.x + COLUMN_WIDTH).fold(f64::MIN, f64::max);
        let mut best = &cols[0];
        for c in &cols[1..] {
            if c.height > best.height {
                best = c;
            }
        }
        (w, best.height, best.height_float)
    };
    let (w, h) = (width + CANVAS_PADDING * 2.0, height + CANVAS_PADDING * 2.0);
    let mut doc = Document::new(w, h);
    let h_text = if height_float { ruby_float(h) } else { format!("{}", h as i64) };
    doc.view_box = Some(format!("0 0 {} {}", w as i64, h_text));

    let normal = font_size(theme, "font_size_normal", 14.0);
    let small_badge = font_size(theme, "font_size_small", 11.0);
    let small_meta = font_size(theme, "font_size_small", 10.0);
    let fg = theme.color("foreground").unwrap_or("");

    for col in &cols {
        let (px, py) = (col.x + CANVAS_PADDING, CANVAS_PADDING);
        doc.add(
            box_rect(px, py, COLUMN_WIDTH, col.height, 8.0)
                .attr("fill", theme.color("background").unwrap_or(""))
                .attr("stroke", theme.color("border").unwrap_or(""))
                .attr("stroke-width", "1"),
        );
        doc.add(box_rect(px, py, COLUMN_WIDTH, col.header_h, 8.0).attr("fill", theme.color("primary").unwrap_or("")));
        let title_lines = parse_lines(&col.title)?;
        let rendered = normal * 1.2;
        let ty = py + col.header_h / 2.0 + 5.0 - ((title_lines.len() as f64 - 1.0) * rendered / 2.0);
        let title = text_elem(theme, px + COLUMN_WIDTH / 2.0, ty, Some("middle"), normal, Some("bold"), Style::Header);
        doc.add(markdown_text(title, &title_lines, px + COLUMN_WIDTH / 2.0, true));
        if !col.cards.is_empty() {
            doc.add(
                box_rect(px + COLUMN_WIDTH - 25.0, py + 2.5, 20.0, 20.0, 10.0)
                    .attr("fill", fg)
                    .attr("opacity", "0.3"),
            );
            doc.add(
                text_elem(theme, px + COLUMN_WIDTH - 15.0, py + 16.5, Some("middle"), small_badge, Some("bold"), Style::Badge)
                    .content(&col.cards.len().to_string()),
            );
        }
    }
    for col in &cols {
        for card in &col.cards {
            let (px, py) = (card.x + CANVAS_PADDING, card.y + CANVAS_PADDING);
            let w = COLUMN_WIDTH - CARD_HORIZONTAL_INSET * 2.0;
            doc.add(
                box_rect(px, py, w, card.height, 6.0)
                    .attr("fill", theme.color("node_fill").unwrap_or(""))
                    .attr("stroke", theme.color("border").unwrap_or(""))
                    .attr("stroke-width", "1"),
            );
            let accent = match card.priority.as_deref() {
                Some("Very High") => Some("red"),
                Some("High") => Some("orange"),
                Some("Low") => Some("blue"),
                Some("Very Low") => Some("lightblue"),
                _ => None,
            };
            if let Some(color) = accent {
                doc.add(
                    Elem::new("line")
                        .attr("stroke", color)
                        .attr("stroke-width", "4")
                        .num("x1", px + 2.0)
                        .num("y1", py + 2.0)
                        .num("x2", px + 2.0)
                        .num("y2", py + card.height - 2.0),
                );
            }
            let lines = card_lines(&card.text, text_w, card_size)?;
            let label = text_elem(theme, px + 10.0, py + 25.0, None, card_size, None, Style::Other);
            doc.add(markdown_text(label, &lines, px + 10.0, false));
            if card.meta.is_empty() {
                continue;
            }
            let y_pos = py + card.height - 10.0;
            if let Some(t) = &card.ticket {
                doc.add(text_elem(theme, px + 10.0, y_pos, None, small_meta, None, Style::Other).content(t));
            }
            if let Some(a) = &card.assigned {
                doc.add(text_elem(theme, px + w - 10.0, y_pos, Some("end"), small_meta, None, Style::Other).content(a));
            }
            let start_y = py + 50.0 + (card.lines as f64 - 1.0).max(0.0) * EXTRA_LINE_HEIGHT;
            let rest = card.meta.iter().filter(|(k, _)| *k != "ticket" && *k != "assigned");
            for (index, (key, value)) in rest.enumerate() {
                let y = start_y + index as f64 * line_h;
                let mut cap = key.to_string();
                cap[..1].make_ascii_uppercase();
                doc.add(text_elem(theme, px + 10.0, y, None, small_meta, None, Style::MetaLabel).content(&format!("{cap}:")));
                doc.add(text_elem(theme, px + 70.0, y, None, small_meta, Some("bold"), Style::Other).content(value));
            }
        }
    }
    Ok(doc.to_xml())
}
