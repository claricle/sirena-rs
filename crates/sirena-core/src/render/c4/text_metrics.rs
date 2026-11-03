//! `Layout::C4Text`: line splitting, widths and heights of C4 labels.
use crate::text::{self, Face};
use regex::Regex;
use std::sync::OnceLock;

pub struct Sizes {
    pub large: i64,
    pub normal: i64,
    pub small: i64,
}

fn line_break() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)<br[ \t\r\n\x0b\x0c]*/?>").unwrap())
}

fn strip(s: &str) -> &str {
    super::strip(s)
}

/// `C4Text.lines`: split on `<br>`, keeping trailing empties, stripped.
pub fn lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let parts: Vec<String> = line_break().split(text).map(|p| strip(p).to_string()).collect();
    parts
}

fn kerning(line: &str, size: i64) -> f64 {
    let chars: Vec<char> = line.chars().collect();
    let hits = chars.windows(2).filter(|w| w[0] == 'A' && w[1] == ' ').count() as i64;
    (-55 * hits * size) as f64 / 1000.0
}

fn line_width(line: &str, size: i64) -> i64 {
    let measured = text::measure(line, size as f64, Face::Arial).width;
    (measured + kerning(line, size)).round() as i64
}

fn line_height(line: &str, size: i64) -> i64 {
    if line.is_empty() {
        return 0;
    }
    match size {
        12 => 14,
        14 => 16,
        16 => 17,
        s => (s as f64 * 1.15).round() as i64,
    }
}

pub fn width(text: &str, size: i64) -> i64 {
    lines(text).iter().map(|l| line_width(l, size)).max().unwrap_or(0)
}

pub fn height(text: &str, size: i64) -> i64 {
    lines(text).iter().map(|l| line_height(l, size)).sum()
}
