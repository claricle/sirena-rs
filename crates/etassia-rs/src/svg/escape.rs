//! XML escaping, identical to Ruby `Sirena::Svg::Escaping`.

fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().filter(|c| allowed(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\'' if attr => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapes element text: `& < >`; drops characters XML cannot represent.
pub fn escape_text(s: &str) -> String {
    escape(s, false)
}

/// Escapes a double-quoted attribute value: text rules plus `"` and `'`.
pub fn escape_attr(s: &str) -> String {
    escape(s, true)
}

/// True for `[A-Za-z_][\w.:-]*`, the only attribute names the serializer accepts.
pub fn valid_name(n: &str) -> bool {
    let mut it = n.chars();
    matches!(it.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && it.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}
