//! Sequence text: `Diagram::SequenceText`, `Layout::Sequence::TextWidth` and
//! `TextWrap`.
use crate::text::{self, Face};
use regex::Regex;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("sequence text regex"))
}

fn line_break() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"(?i)<br[ \t\r\n\x0b\x0c]*/?>")
}

fn wrap_prefix() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"(?i)\A[ \t\r\n\x0b\x0c]*(?:no)?wrap:[ \t\r\n\x0b\x0c]*")
}

fn entity() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"#([A-Za-z0-9_]+);")
}

pub fn has_line_break(s: &str) -> bool {
    line_break().is_match(s)
}

/// `String#split(LINE_BREAK, -1)` (an empty string gives no pieces).
pub fn split_breaks(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![];
    }
    line_break().split(s).map(str::to_string).collect()
}

/// `TextWrap.body`: the text without its `wrap:`/`nowrap:` prefix.
pub fn body(source: &str) -> String {
    wrap_prefix().replace(source, "").into_owned()
}

/// `TextWrap.wrapped?`.
pub fn wrapped(source: &str, global: bool) -> bool {
    static ON: OnceLock<Regex> = OnceLock::new();
    static OFF: OnceLock<Regex> = OnceLock::new();
    if re(&ON, r"(?i)\A[ \t\r\n\x0b\x0c]*wrap:").is_match(source) {
        return true;
    }
    global && !re(&OFF, r"(?i)\A[ \t\r\n\x0b\x0c]*nowrap:").is_match(source)
}

/// `SequenceText.display`.
pub fn display(text: &str) -> String {
    let stripped = wrap_prefix().replace(text, "").into_owned();
    decode(&line_break().replace_all(&stripped, " "))
}

fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" | "AMP" => "&",
        "lt" | "LT" => "<",
        "gt" | "GT" => ">",
        "quot" | "QUOT" => "\"",
        "apos" => "'",
        "nbsp" => "\u{A0}",
        "infin" => "∞",
        "hearts" => "♥",
        "copy" => "©",
        "reg" => "®",
        "deg" => "°",
        "trade" => "™",
        "euro" => "€",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "larr" => "←",
        "rarr" => "→",
        "uarr" => "↑",
        "darr" => "↓",
        "times" => "×",
        "divide" => "÷",
        "plusmn" => "±",
        "laquo" => "«",
        "raquo" => "»",
        "bull" => "•",
        _ => return None,
    })
}

fn cp1252(n: u32) -> Option<char> {
    let c = match n {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => return None,
    };
    char::from_u32(c)
}

fn codepoint(digits: &str) -> String {
    let replacement = "\u{FFFD}".to_string();
    let Ok(n) = digits.parse::<u64>() else { return replacement };
    if !(1..=0x10FFFF).contains(&n) || (0xD800..=0xDFFF).contains(&n) {
        return replacement;
    }
    let n = n as u32;
    let ch = char::from_u32(n).expect("scalar value");
    if (0x80..=0x9F).contains(&n) {
        return cp1252(n).unwrap_or(ch).to_string();
    }
    ch.to_string()
}

/// `SequenceText.decode`: one pass over `#name;` references.
pub fn decode(text: &str) -> String {
    entity()
        .replace_all(text, |c: &regex::Captures| {
            let name = &c[1];
            if name.bytes().all(|b| b.is_ascii_digit()) {
                return codepoint(name);
            }
            named(name).map(str::to_string).unwrap_or_else(|| format!("&{name};"))
        })
        .into_owned()
}

const MARK: &str = "\u{FB02}°";
const END_MARK: &str = "¶ß";

fn kerning(a: char, b: char) -> f64 {
    match (a, b) {
        ('F', 'i') => -20.0,
        ('W', 'o') => -80.0,
        _ => 0.0,
    }
}

fn advance(c: char) -> f64 {
    match text::times_advance_of(c as u32) {
        Some(v) => v as f64,
        None => text::measure(&c.to_string(), 1000.0, Face::Arial).width,
    }
}

/// `TextWidth.of`: width of one line in whole pixels.
pub fn width_of(line: &str, size: f64) -> i64 {
    static NUMERIC: OnceLock<Regex> = OnceLock::new();
    let numeric = re(&NUMERIC, r"\A\+?[0-9]+\z");
    let encoded = entity().replace_all(line, |c: &regex::Captures| {
        let name = &c[1];
        format!("{MARK}{}{name}{END_MARK}", if numeric.is_match(name) { "°" } else { "" })
    });
    static WS: OnceLock<Regex> = OnceLock::new();
    let folded = re(&WS, r"[ \t\r\n\x0b\x0c]+").replace_all(&encoded, " ").into_owned();
    let folded = crate::render::pie::strip(&folded);
    let chars: Vec<char> = folded.chars().collect();
    let mut total: f64 = chars.iter().map(|c| advance(*c)).sum();
    total += chars.windows(2).map(|p| kerning(p[0], p[1])).sum::<f64>();
    ((total * size / 1000.0) + 0.5).floor() as i64
}

/// `TextWidth.widest`.
pub fn widest(lines: &[String], size: f64) -> i64 {
    lines.iter().map(|l| width_of(l, size)).max().unwrap_or(0)
}

/// `TextWrap.lines`.
pub fn wrap_lines(text: &str, limit: f64, size: f64) -> Vec<String> {
    if has_line_break(text) {
        return split_breaks(text);
    }
    let width = |s: &str| width_of(s, size) as f64;
    let mut done: Vec<String> = vec![];
    let mut line = String::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        if width(&format!("{word} ")) > limit {
            let mut pieces = vec![];
            let mut cur = String::new();
            let chars: Vec<char> = word.chars().collect();
            for (index, ch) in chars.iter().enumerate() {
                cur.push(*ch);
                if width(&cur) < limit {
                    continue;
                }
                pieces.push(if index == chars.len() - 1 { cur.clone() } else { format!("{cur}-") });
                cur.clear();
            }
            done.push(line);
            done.extend(pieces);
            line = cur;
        } else if width(&line) + width(&format!("{word} ")) >= limit {
            done.push(line);
            line = word.to_string();
        } else if line.is_empty() {
            line = word.to_string();
        } else {
            line = format!("{line} {word}");
        }
    }
    done.push(line);
    done.retain(|l| !l.is_empty());
    done
}

/// `SequenceWrap.on?`: `%%{wrap}%%` or an `init` directive with `wrap: true`.
pub fn global_wrap(source: &str) -> bool {
    static DIRECTIVE: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    static INIT: OnceLock<Regex> = OnceLock::new();
    let directive = re(&DIRECTIVE, r"(?s)%%\{(.*?)\}%%");
    let bare = re(&BARE, r"\A[ \t\r\n\x0b\x0c]*wrap[ \t\r\n\x0b\x0c]*\z");
    let init = re(
        &INIT,
        r#"(?si)\A[ \t\r\n\x0b\x0c]*init(?:ialize)?[ \t\r\n\x0b\x0c]*:.*["']?\bwrap["']?[ \t\r\n\x0b\x0c]*:[ \t\r\n\x0b\x0c]*true\b"#,
    );
    directive
        .captures_iter(source)
        .any(|c| bare.is_match(&c[1]) || init.is_match(&c[1]))
}
