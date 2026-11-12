//! Text measurement, ported from `lib/sirena/text_measurement.rb` and its
//! helpers. Pure Rust, no font file read at runtime: width is the sum of
//! per-glyph advances (per-mille em) from tables generated out of the Ruby
//! ones by `scripts/generate_text_tables.rb` (see `tables.rs`).
//!
//! Results are bit-identical `f64`s to the Ruby implementation; the
//! `text_parity` test checks this against a Ruby-produced fixture.
//!
//! # Public API
//!
//! * [`measure`]`(text, font_size, face)` -> [`Measure`]: mirrors
//!   `TextMeasurement.measure` (`Face::Arial`) and `.measure_in`
//!   (`Face::Trebuchet`, `Face::Arial`).
//! * [`measure_monospace`] - `TextMeasurement.measure(.., monospace: true)`.
//! * [`measure_with`] - `measure` with optional width/height overrides.
//! * [`width`] - shorthand for `measure(..).width`.
//! * [`reserved_text_width`] - `Renderer::Base#reserved_text_width`
//!   (measured width x [`SUBSTITUTE_FONT_HEADROOM`]).
//! * [`trebuchet_kerning_adjustment`] - `TrebuchetKerning.adjustment`.
//! * [`advance_of`]: per-mille advance of one codepoint in a table face
//!   (`TrebuchetAdvances::TABLE` / `TimesAdvances::TABLE` / Arial lookups).
//! * [`timeline`]: `Layout::TimelineText` (wrap, width, heights).
//!
//! Rules (same as Ruby): the text is one line (tab/LF/CR count as a space);
//! zero-width categories (Mn, Me, Cf, Cc) are 0; CJK/unknown 1.0 em; emoji
//! (and pictograph + U+FE0F) 1.5 em; four Arabic ligature OUTLIERS; a
//! codepoint missing from a face falls back to the Arial table.

pub mod tables;
pub mod timeline;

/// Text height as a ratio of font size.
pub const HEIGHT_RATIO: f64 = 1.0;
/// Headroom `reserved_text_width` adds for a substituted font.
pub const SUBSTITUTE_FONT_HEADROOM: f64 = 1.2;

const DEFAULT_ADVANCE: i64 = 1000;
const EMOJI_ADVANCE: i64 = 1500;
const MONOSPACE_ADVANCE: i64 = 600;
const UNITS_PER_EM: f64 = 2048.0;

/// The faces a caller can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Arial,
    Trebuchet,
}

/// Measured text box, in the unit of `font_size`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub width: f64,
    pub height: f64,
}

fn lookup<V: Copy>(table: &[(u32, V)], code: u32) -> Option<V> {
    table
        .binary_search_by_key(&code, |&(c, _)| c)
        .ok()
        .map(|i| table[i].1)
}

fn in_ranges(ranges: &[(u32, u32)], code: u32) -> bool {
    ranges
        .binary_search_by(|&(lo, hi)| {
            if code < lo {
                std::cmp::Ordering::Greater
            } else if code > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn is_emoji(c: char) -> bool {
    in_ranges(tables::EMOJI, c as u32)
}

fn space_advance() -> i64 {
    i64::from(lookup(tables::ARIAL, 0x20).expect("arial has space"))
}

fn face_advance(face: Face, code: u32) -> Option<i64> {
    let own = match face {
        Face::Arial => lookup(tables::ARIAL, code),
        Face::Trebuchet => lookup(tables::TREBUCHET, code),
    };
    own.or_else(|| lookup(tables::ARIAL, code)).map(i64::from)
}

/// Per-mille advance of `code` in `face` (Arial fallback), `None` when no
/// table has it. Mirrors `FACE_TABLES[face][code] || ArialAdvances::TABLE[code]`.
pub fn advance_of(face: Face, code: u32) -> Option<i64> {
    face_advance(face, code)
}

/// Per-mille advance of `code` in Times New Roman, `None` when absent.
pub fn times_advance_of(code: u32) -> Option<i64> {
    lookup(tables::TIMES, code).map(i64::from)
}

/// Trebuchet table lookup only (no Arial fallback):
/// `TrebuchetAdvances::TABLE[code]`.
pub fn trebuchet_advance_of(code: u32) -> Option<i64> {
    lookup(tables::TREBUCHET, code).map(i64::from)
}

fn table_advance(table: i64, monospace: bool) -> i64 {
    if monospace && table > 0 {
        MONOSPACE_ADVANCE
    } else {
        table
    }
}

fn whitespace_advance(monospace: bool) -> i64 {
    if monospace {
        MONOSPACE_ADVANCE
    } else {
        space_advance()
    }
}

fn advance(c: char, monospace: bool, face: Face) -> i64 {
    let code = c as u32;
    if matches!(c, '\t' | '\n' | '\r') {
        return whitespace_advance(monospace);
    }
    if in_ranges(tables::ZERO_WIDTH, code) {
        return 0;
    }
    if let Some(table) = face_advance(face, code) {
        return table_advance(table, monospace);
    }
    if let Some(o) = lookup(tables::OUTLIERS, code) {
        return i64::from(o);
    }
    if is_emoji(c) {
        return EMOJI_ADVANCE;
    }
    if monospace {
        MONOSPACE_ADVANCE
    } else {
        DEFAULT_ADVANCE
    }
}

fn ascii_advance(code: u8, monospace: bool, face: Face) -> i64 {
    if matches!(code, 9 | 10 | 13) {
        return whitespace_advance(monospace);
    }
    if code < 32 || code == 127 {
        return 0;
    }
    table_advance(
        face_advance(face, u32::from(code)).expect("ascii glyph in table"),
        monospace,
    )
}

fn total_advance(text: &str, monospace: bool, face: Face) -> i64 {
    if text.is_ascii() {
        return text
            .bytes()
            .map(|b| ascii_advance(b, monospace, face))
            .sum();
    }
    let mut total = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        // pictograph + U+FE0F is one 2-char cluster at emoji width.
        if is_emoji(c) && chars.peek() == Some(&'\u{FE0F}') {
            chars.next();
            total += EMOJI_ADVANCE;
        } else {
            total += advance(c, monospace, face);
        }
    }
    total
}

fn calculate_width(text: &str, font_size: f64, monospace: bool, face: Face) -> f64 {
    total_advance(text, monospace, face) as f64 * font_size / 1000.0
}

/// Measures `text` (one line) at `font_size` in `face`.
pub fn measure(text: &str, font_size: f64, face: Face) -> Measure {
    Measure {
        width: calculate_width(text, font_size, false, face),
        height: font_size * HEIGHT_RATIO,
    }
}

/// `TextMeasurement.measure(.., monospace: true)` (Arial table, monospace
/// advances).
pub fn measure_monospace(text: &str, font_size: f64) -> Measure {
    Measure {
        width: calculate_width(text, font_size, true, Face::Arial),
        height: font_size * HEIGHT_RATIO,
    }
}

/// `TextMeasurement.measure` with optional overrides and monospace flag.
pub fn measure_with(
    text: &str,
    font_size: f64,
    width: Option<f64>,
    height: Option<f64>,
    monospace: bool,
) -> Measure {
    Measure {
        width: width.unwrap_or_else(|| calculate_width(text, font_size, monospace, Face::Arial)),
        height: height.unwrap_or(font_size * HEIGHT_RATIO),
    }
}

/// Width of `text` in `face` (`measure(..).width`).
pub fn width(text: &str, font_size: f64, face: Face) -> f64 {
    calculate_width(text, font_size, false, face)
}

/// `Renderer::Base#reserved_text_width`: Arial width with headroom for a
/// substituted font. Not a bound.
pub fn reserved_text_width(text: &str, font_size: f64) -> f64 {
    width(text, font_size, Face::Arial) * SUBSTITUTE_FONT_HEADROOM
}

/// `TextMeasurement::TrebuchetKerning.adjustment`.
pub fn trebuchet_kerning_adjustment(text: &str, font_size: f64) -> f64 {
    let chars: Vec<char> = text.chars().collect();
    let mut units: i64 = 0;
    let mut pair = String::new();
    for w in chars.windows(2) {
        pair.clear();
        pair.push(w[0]);
        pair.push(w[1]);
        if let Some(&(_, v)) = tables::TREBUCHET_KERNING.iter().find(|&&(k, _)| k == pair) {
            units += i64::from(v);
        }
    }
    units as f64 * font_size / UNITS_PER_EM
}
