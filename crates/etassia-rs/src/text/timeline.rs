//! Port of `Layout::TimelineText`: wraps and sizes timeline card text the
//! way mmdc does, with Trebuchet MS design-unit advances.

use super::{measure, tables, trebuchet_advance_of, Face};

pub const FONT_SIZE: f64 = 16.0;
pub const FIRST_LINE_HEIGHT: f64 = 19.0;
pub const NEXT_LINE_HEIGHT: f64 = 17.6;
/// Space under the text that every card keeps, besides its padding.
pub const TEXT_MARGIN: f64 = FONT_SIZE * 1.1 * 0.5;
pub const PADDING: f64 = 20.0;
const BREAK: &str = "<br>";
const DESIGN_UNITS_PER_EM: f64 = 2048.0;

// Ruby `\s` is ASCII-only; `strip` also removes NUL.
fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

fn is_strip(c: char) -> bool {
    is_ws(c) || c == '\0'
}

/// `words.join(" ").gsub(/\s+/, " ").strip`
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
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
    out.trim_matches(is_strip).to_string()
}

fn join(words: &[&str]) -> String {
    collapse(&words.join(" "))
}

/// Ruby `text.split(/(\s+|<br>)/)`: separators kept, trailing empties dropped.
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().unwrap();
        if is_ws(c) {
            let mut j = i;
            while j < text.len() && is_ws(text[j..].chars().next().unwrap()) {
                j += 1;
            }
            out.push(&text[start..i]);
            out.push(&text[i..j]);
            start = j;
            i = j;
        } else if bytes[i] == b'<' && rest.starts_with(BREAK) {
            out.push(&text[start..i]);
            out.push(BREAK);
            i += BREAK.len();
            start = i;
        } else {
            i += c.len_utf8();
        }
    }
    out.push(&text[start..]);
    while out.last() == Some(&"") {
        out.pop();
    }
    out
}

fn design_units(c: char) -> i64 {
    let index = c as i64 - 0x20;
    if (0..tables::TIMELINE_BASIC_LATIN_UNITS.len() as i64).contains(&index) {
        return i64::from(tables::TIMELINE_BASIC_LATIN_UNITS[index as usize]);
    }
    let adv = match trebuchet_advance_of(c as u32) {
        Some(a) => a as f64,
        None => {
            let mut buf = [0u8; 4];
            measure(c.encode_utf8(&mut buf), 1000.0, Face::Arial).width
        }
    };
    (adv * DESIGN_UNITS_PER_EM / 1000.0).round() as i64
}

/// Width of one line in user units.
pub fn width_of(line: &str, font_size: f64) -> f64 {
    let units: i64 = collapse(line).chars().map(design_units).sum();
    units as f64 * font_size / DESIGN_UNITS_PER_EM
}

/// Lines of `text` wrapped at `width`; whitespace collapsed.
pub fn wrap(text: &str, width: f64, font_size: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line: Vec<&str> = Vec::new();
    for word in tokens(text) {
        line.push(word);
        if word == BREAK || width_of(&join(&line), font_size) > width {
            line.pop();
            lines.push(join(&line));
            line = if word == BREAK { vec![""] } else { vec![word] };
        }
    }
    lines.push(join(&line));
    lines
}

fn scaled(value: f64, font_size: f64) -> f64 {
    value * font_size / FONT_SIZE
}

/// Height of the drawn text, blank lines not counted.
pub fn text_height(lines: &[String], font_size: f64) -> f64 {
    let drawn = lines.iter().filter(|l| !l.is_empty()).count();
    if drawn == 0 {
        return 0.0;
    }
    scaled(FIRST_LINE_HEIGHT, font_size) + (drawn - 1) as f64 * scaled(NEXT_LINE_HEIGHT, font_size)
}

/// Card height the text needs before any minimum.
pub fn card_height(lines: &[String], font_size: f64) -> f64 {
    let margin = scaled(TEXT_MARGIN, font_size);
    text_height(lines, font_size) + margin + PADDING
}
