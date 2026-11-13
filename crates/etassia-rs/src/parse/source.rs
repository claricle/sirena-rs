//! Port of `Etassia::Source.split`: strips the preamble (frontmatter, `%%{...}%%`
//! directives, `%%` comments) and returns the diagram body. Hand-written
//! because the Ruby patterns use backreferences and lookahead.

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_blank(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    i
}

fn skip_while(s: &[u8], mut i: usize, f: fn(u8) -> bool) -> usize {
    while i < s.len() && f(s[i]) {
        i += 1;
    }
    i
}

fn find(s: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    s.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Mermaid's frontmatter block at `start`; returns the end of its closing fence.
fn frontmatter_end(s: &[u8], start: usize) -> Option<usize> {
    let fence_start = skip_blank(s, start);
    let indent = &s[start..fence_start];
    if !s[fence_start..].starts_with(b"---") {
        return None;
    }
    let after = skip_blank(s, fence_start + 3);
    if s.get(after) != Some(&b'\n') {
        return None;
    }
    let content = after + 1;
    // Lazy `.+?`: at least one byte, then the first line start that closes.
    let mut p = content + 1;
    while p <= s.len() {
        if s[p - 1] == b'\n' && s[p..].starts_with(indent) {
            let f = p + indent.len();
            if s[f..].starts_with(b"---") {
                let e = skip_blank(s, f + 3);
                if e == s.len() {
                    return Some(e);
                }
                if s[e] == b'\n' {
                    return Some(e + 1);
                }
            }
        }
        p += 1;
    }
    None
}

/// `DIRECTIVE`: `%%{`, a header word (colon optional), then a bare word or
/// everything up to the first `}%%`; the terminator is optional.
fn directive_end(s: &[u8], start: usize) -> Option<usize> {
    let i = skip_blank(s, start);
    if !s[i..].starts_with(b"%%{") {
        return None;
    }
    let mut i = skip_while(s, i + 3, is_space);
    let w = skip_while(s, i, is_word);
    if w == i {
        return None;
    }
    i = w;
    let j = skip_while(s, i, is_space);
    i = if s.get(j) == Some(&b':') { j + 1 } else { i };
    i = skip_while(s, i, is_space);
    let word_end = skip_while(s, i, is_word);
    if word_end > i {
        i = word_end;
    } else {
        i = find(s, i, b"}%%").unwrap_or(s.len());
    }
    i = skip_while(s, i, is_space);
    if s[i..].starts_with(b"}%%") {
        i += 3;
    }
    Some(i)
}

/// `[ \t]*%%` + not-`{` or `{` + rest of line; the line's end incl. newline.
fn comment_end(s: &[u8], start: usize, directive: bool) -> Option<usize> {
    let i = skip_blank(s, start);
    if !s[i..].starts_with(b"%%") {
        return None;
    }
    let brace = s.get(i + 2) == Some(&b'{');
    if brace != directive {
        return None;
    }
    let e = s[i..].iter().position(|&b| b == b'\n').map_or(s.len(), |p| i + p + 1);
    Some(e)
}

fn blank_line_end(s: &[u8], start: usize) -> Option<usize> {
    let i = skip_blank(s, start);
    (s.get(i) == Some(&b'\n')).then_some(i + 1)
}

fn bare_comment(s: &[u8], start: usize) -> bool {
    let i = skip_blank(s, start);
    s[i..].starts_with(b"%%") && matches!(s.get(i + 2), None | Some(b'\n'))
}

/// The diagram body of `source` (CR/CRLF normalised to LF).
pub fn body(source: &str) -> String {
    let text = source.replace("\r\n", "\n").replace('\r', "\n");
    let s = text.as_bytes();
    let mut pos = frontmatter_end(s, 0).unwrap_or(0);
    if s[pos..].starts_with("\u{feff}".as_bytes()) {
        pos += 3;
    }
    let mut erased = false;
    loop {
        if let Some(e) = blank_line_end(s, pos) {
            pos = e;
        } else if let Some(e) = directive_end(s, pos) {
            pos = e;
        } else if let Some(e) = comment_end(s, pos, true) {
            pos = e;
            erased = true;
        } else if let Some(e) = comment_end(s, pos, false) {
            erased = erased || bare_comment(s, pos);
            pos = e;
        } else if !erased {
            match frontmatter_end(s, pos) {
                Some(e) => {
                    pos = e;
                    erased = false;
                }
                None => break,
            }
        } else {
            break;
        }
    }
    text[pos..].to_string()
}
