//! Flowchart source parser.
use super::*;

pub(super) const RESERVED: &[&str] = &[
    "end", "graph", "flowchart", "subgraph", "style", "class", "classDef", "click", "linkStyle", "direction", "default", "interpolate", "call", "href", "accTitle", "accDescr", "swimlane",
];


/// Split on `<br>` (any case, optional `/` and spaces) and newlines.
fn split_breaks(s: &str) -> Vec<String> {
    let lower = s.to_ascii_lowercase();
    let b = lower.as_bytes();
    let (mut out, mut start, mut i) = (vec![], 0usize, 0usize);
    while i < b.len() {
        if b[i] == b'\n' {
            out.push(s[start..i].to_string());
            i += 1;
            start = i;
        } else if b[i..].starts_with(b"<br") {
            let mut j = i + 3;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'/' {
                j += 1;
            }
            if j < b.len() && b[j] == b'>' {
                out.push(s[start..i].to_string());
                i = j + 1;
                start = i;
                continue;
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    out.push(s[start..].to_string());
    out
}

/// `\bfa[bklrs]?:fa-[\w-]+\s*` occurrences: (count, text without them).
fn strip_icons(line: &str) -> (usize, String) {
    let cs: Vec<char> = line.chars().collect();
    let (mut out, mut n, mut i) = (String::new(), 0, 0);
    let word = |c: char| c.is_alphanumeric() || c == '_';
    while i < cs.len() {
        let boundary = i == 0 || !word(cs[i - 1]);
        if boundary && cs[i] == 'f' && cs.get(i + 1) == Some(&'a') {
            let mut j = i + 2;
            if matches!(cs.get(j), Some('b' | 'k' | 'l' | 'r' | 's')) {
                j += 1;
            }
            if cs.get(j) == Some(&':') && cs.get(j + 1) == Some(&'f') && cs.get(j + 2) == Some(&'a') && cs.get(j + 3) == Some(&'-') {
                let mut k = j + 4;
                while k < cs.len() && (word(cs[k]) || cs[k] == '-') {
                    k += 1;
                }
                if k > j + 4 {
                    while k < cs.len() && cs[k].is_whitespace() {
                        k += 1;
                    }
                    n += 1;
                    i = k;
                    continue;
                }
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    (n, out)
}

/// Remove HTML tags (`</?[a-zA-Z][^>]*>`) as mermaid's label text does.
fn strip_tags(s: &str) -> String {
    let (mut out, mut rest) = (String::new(), s);
    while let Some(i) = rest.find('<') {
        let after = &rest[i + 1..];
        let body = after.strip_prefix('/').unwrap_or(after);
        match (body.chars().next().map_or(false, |c| c.is_ascii_alphabetic()), after.find('>')) {
            (true, Some(e)) if !after[..e].to_ascii_lowercase().starts_with("br") => {
                out.push_str(&rest[..i]);
                rest = &after[e + 1..];
            }
            _ => {
                out.push_str(&rest[..i + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A node label's lines and icon counts (`FlowchartLabelText.lines`), or None
/// for text outside the supported subset. `skewed` allows edge slashes.
pub(super) fn label_lines(raw: &str, skewed: bool) -> Option<(Vec<String>, Vec<usize>)> {
    label_lines_in(raw, skewed, &[])
}

fn label_lines_in(raw: &str, skewed: bool, extra: &[char]) -> Option<(Vec<String>, Vec<usize>)> {
    let raw = raw.trim();
    let quoted = raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"');
    let raw = if quoted { &raw[1..raw.len() - 1] } else { raw };
    let stripped;
    let raw = if quoted { stripped = strip_tags(raw); stripped.as_str() } else { raw };
    let (mut lines, mut icons) = (vec![], vec![]);
    for part in split_breaks(raw) {
        let (n, t) = strip_icons(&part);
        let t = t.replace("\\\\", "\\");
        if !t.chars().all(|c| c.is_alphanumeric() || extra.contains(&c) || matches!(c, ' ' | '_' | '-' | '.' | ',' | '?' | '!' | '+' | '*' | ':' | '/' | '\\')) {
            return None;
        }
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if !skewed && (t.starts_with(['/', '\\']) || t.ends_with(['/', '\\'])) {
            return None;
        }
        lines.push(t);
        icons.push(n);
    }
    if lines.iter().all(|l| l.is_empty()) && lines.len() == 1 && icons[0] == 0 {
        return None;
    }
    Some((lines, icons))
}

pub(super) fn read_id(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let mut end = 0;
    while end < b.len() {
        let c = b[end];
        let ok = c.is_ascii_alphanumeric()
            || c == b'$'
            || c == b'_'
            || c == b'/'
            || c == b'\\'
            || (c == b'.' && !matches!(b.get(end + 1), Some(b'-' | b'=')) && !(end > 0 && b[end - 1] == b'-'))
            || (c == b'-' && (!matches!(b.get(end + 1), Some(b'-' | b'.' | b'>' | b'=')) || (b.get(end + 1) == Some(&b'>') && end > 0 && b[end - 1] != b'-')));
        if !ok {
            break;
        }
        end += 1;
    }
    if end == 0 {
        return None;
    }
    let id = &s[..end];
    // a dotted-link opener or `x`/`o` head glued to the id is not an id char
    let lead = id.trim_start_matches(|c: char| !c.is_ascii_alphabetic());
    if RESERVED.iter().any(|r| lead.starts_with(r)) || id.starts_with('.') || id.starts_with('-') {
        return None;
    }
    if id.ends_with(['x', 'o']) && ["--", "==", "-."].iter().any(|p| s[end..].starts_with(p)) {
        return None;
    }
    Some((id, &s[end..]))
}

/// A node reference: `id` or `id` + a plain-text shape. Returns the rest.
pub(super) fn read_node<'a>(s: &'a str, model: &mut Model, index: &mut HashMap<String, usize>) -> Result<(usize, &'a str), Error> {
    let (id, rest) = read_id(s).ok_or_else(|| unported("node id"))?;
    let shapes: [(&str, &[&str], Shape); 12] = [
        ("(((", &[")))"], Shape::DoubleCircle),
        ("((", &["))"], Shape::Circle),
        ("([", &["])"], Shape::Stadium),
        ("[[", &["]]"], Shape::Rect),
        ("[(", &[")]"], Shape::Rect),
        ("{{", &["}}"], Shape::Hexagon),
        ("[/", &["/]", "\\]"], Shape::Skewed),
        ("[\\", &["\\]", "/]"], Shape::Skewed),
        ("[", &["]"], Shape::Rect),
        ("(", &[")"], Shape::Rounded),
        ("{", &["}"], Shape::Rhombus),
        (">", &["]"], Shape::Asymmetric),
    ];
    let mut explicit: Option<(Option<Label>, Shape)> = None;
    let mut rest = rest;
    for (open, closes, shape) in shapes {
        if rest.starts_with("@{") {
            break;
        }
        if let Some(inner) = rest.strip_prefix(open) {
            let (end, close) = closes.iter().filter_map(|c| inner.find(c).map(|i| (i, *c))).min_by_key(|(i, _)| *i).ok_or_else(|| unported("shape close"))?;
            let skewed = shape == Shape::Skewed;
            let (lines, icons) = label_lines(&inner[..end], skewed).ok_or_else(|| unported("label text"))?;
            explicit = Some((Some((lines, icons)), shape));
            rest = &inner[end + close.len()..];
            break;
        }
    }
    let mut data_only = false;
    if let Some(r) = rest.strip_prefix("@{") {
        let end = data_end(r).ok_or_else(|| unported("node data"))?;
        let (dlabel, dshape) = node_data(&r[..end])?;
        rest = &r[end + 1..];
        if rest.starts_with(['@', '{']) {
            return Err(unported("node data"));
        }
        explicit = match explicit {
            None => {
                data_only = true;
                Some((dlabel, dshape.unwrap_or(Shape::Rect)))
            }
            Some((label, shape)) => {
                if dlabel.is_some() {
                    return Err(unported("node data"));
                }
                Some((label, dshape.unwrap_or(shape)))
            }
        };
    }
    if rest.starts_with(['(', '[', '{', '>']) {
        return Err(unported("shape"));
    }
    // `:::name` assigns a class; classes paint nothing
    if let Some(r) = rest.strip_prefix(":::") {
        let end = r.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(r.len());
        if end == 0 || !r.as_bytes()[0].is_ascii_alphabetic() {
            return Err(unported("inline class"));
        }
        rest = &r[end..];
    }
    let i = match index.get(id) {
        Some(&i) => {
            match &explicit {
                None => {}
                // `A@{ shape: rect }` on a rect `A` changes nothing
                Some((None, shape)) if data_only && *shape == model.nodes[i].shape => {}
                Some(_) => return Err(unported("redeclared node")),
            }
            i
        }
        None => {
            let ((lines, icons), shape) = explicit.map(|(l, sh): (Option<Label>, Shape)| (l.unwrap_or_else(|| (vec![id.to_string()], vec![0])), sh)).unwrap_or(((vec![id.to_string()], vec![0]), Shape::Rect));
            model.nodes.push(Node { id: id.to_string(), lines, icons, shape, fill: None, stroke: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0, cluster: false, collision: false });
            index.insert(id.to_string(), model.nodes.len() - 1);
            model.nodes.len() - 1
        }
    };
    Ok((i, rest))
}

/// `Builders::Flowchart#link_type`: weight, then the head(s) the token carries.
pub(super) fn link_type(token: &str) -> String {
    let marker = |c: char| match c {
        '>' | '<' => Some("arrow"),
        'x' => Some("cross"),
        'o' => Some("circle"),
        _ => None,
    };
    let head = marker(token.chars().last().unwrap());
    let tail = marker(token.chars().next().unwrap());
    let matched = head.is_some() && head == tail;
    let weight = if token.contains('.') {
        "dotted_"
    } else if token.contains('=') && (matched || tail.is_none()) {
        "thick_"
    } else {
        ""
    };
    let ends = if matched { format!("{}_both", head.unwrap()) } else { head.unwrap_or("line").to_string() };
    format!("{weight}{ends}")
}

/// `minimum_rank_span`: a link longer than the ordinary one needs rank
/// spacing the layout does not model yet.
fn rank_span(close: &str) -> usize {
    let line = &close[..close.len() - 1];
    let marker = |c: char| matches!(c, '>' | '<' | 'x' | 'o');
    let line = if close.chars().next().map_or(false, marker) && close.chars().next().map(|c| if c == '<' { '>' } else { c }) == close.chars().last() { &line[1..] } else { line };
    let dots = line.matches('.').count();
    let span = if dots > 0 { dots } else { line.len().saturating_sub(1) };
    if span > 1 { span } else { 0 }
}

fn edge_text(t: &str) -> Result<String, Error> {
    let t = t.trim_matches([' ', '\t']);
    let ok = !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | ',' | '/' | '?' | '!' | '+' | '*' | ':' | '=') || false);
    if !ok {
        return Err(unported("edge label text"));
    }
    Ok(t.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A link token after a node: returns (arrow type, label, rest).
pub(super) fn parse_link(tail: &str) -> Result<(String, Option<String>, usize, &str), Error> {
    let (start, t) = match tail.chars().next() {
        Some(c @ ('x' | 'o' | '<')) => (Some(c), &tail[1..]),
        _ => (None, tail),
    };
    let sc = start.map(String::from).unwrap_or_default();
    let ch = t.chars().next().ok_or_else(|| unported("link"))?;
    let finish = |close: &str, rest: &'_ str| -> Result<(String, Option<String>, String), Error> {
        let _ = rest;
        Ok((close.to_string(), None, String::new()))
    };
    let _ = finish;
    // dotted
    if t.starts_with("-.") {
        let after = &t[2..];
        if let Some(r) = after.strip_prefix('-') {
            // `-.-` / `-.->`
            let (head, r) = match r.chars().next() {
                Some(h @ ('>' | 'x' | 'o')) => (Some(h), &r[1..]),
                _ => (None, r),
            };
            let token = format!("{sc}-.-{}", head.map(String::from).unwrap_or_default());
            if start.is_some() && head.is_none() {
                return Err(unported("link"));
            }
            return finish_link(&token, r, start);
        }
        // inline text `-. text .->`
        let dot = after.find('.').ok_or_else(|| unported("link"))?;
        let text = edge_text(&after[..dot])?;
        let r = after[dot + 1..].strip_prefix('-').ok_or_else(|| unported("link"))?;
        let (head, r) = match r.chars().next() {
            Some(h @ ('>' | 'x' | 'o')) => (Some(h), &r[1..]),
            _ => (None, r),
        };
        let close = format!(".-{}", head.map(String::from).unwrap_or_default());
        let token = format!("{sc}-.{close}");
        return inline_done(&token, &close, text, r, start);
    }
    if ch == '-' || ch == '=' {
        let n = t.chars().take_while(|c| *c == ch).count();
        if n >= 2 {
            let after = &t[n..];
            let head = after.chars().next().filter(|c| matches!(c, '>' | 'x' | 'o'));
            if n == 2 && head.is_none() && after.starts_with(|c: char| c == ' ' || c == '\t' || c.is_ascii_alphanumeric()) {
                // inline text `-- text -->`
                let pat = format!("{ch}{ch}");
                let at = after.find(&pat).ok_or_else(|| unported("link"))?;
                let text = edge_text(&after[..at])?;
                let r = &after[at + 2..];
                let k = r.chars().take_while(|c| *c == ch).count();
                let (head, r2) = match r[k..].chars().next() {
                    Some(h @ ('>' | 'x' | 'o')) => (Some(h), &r[k + 1..]),
                    _ => (None, &r[k..]),
                };
                if k == 0 && head.is_none() {
                    return Err(unported("link"));
                }
                let close = format!("{pat}{}{}", ch.to_string().repeat(k), head.map(String::from).unwrap_or_default());
                let token = format!("{sc}{pat}{close}");
                return inline_done(&token, &close, text, r2, start);
            }
            if head.is_none() && n == 2 {
                return Err(unported("link"));
            }
            if head.is_none() && start.is_some() {
                return Err(unported("link"));
            }
            let used = n + head.map_or(0, |_| 1);
            let token = format!("{sc}{}{}", ch.to_string().repeat(n), head.map(String::from).unwrap_or_default());
            return finish_link(&token, &t[used..], start);
        }
    }
    Err(unported("link"))
}

fn start_ok(token: &str, start: Option<char>) -> bool {
    match start {
        None => true,
        Some(c) => {
            let want = if c == '<' { '>' } else { c };
            token.ends_with(want) && token.len() > 1
        }
    }
}

fn inline_done<'a>(token: &str, close: &str, text: String, rest: &'a str, start: Option<char>) -> Result<(String, Option<String>, usize, &'a str), Error> {
    if !start_ok(token, start) {
        return Err(unported("link"));
    }
    let rest_ok = !matches!(rest.chars().next(), Some('x' | 'o' | '>' | '-' | '.' | '=' | '|'));
    if !rest_ok {
        return Err(unported("link tail"));
    }
    Ok((link_type(token), Some(text), rank_span(close), rest))
}

fn finish_link<'a>(token: &str, rest: &'a str, start: Option<char>) -> Result<(String, Option<String>, usize, &'a str), Error> {
    if !start_ok(token, start) {
        return Err(unported("link"));
    }
    let span = rank_span(token);
    // a pipe label may follow the token
    if let Some(r) = rest.strip_prefix('|') {
        let end = r.find('|').ok_or_else(|| unported("pipe label"))?;
        let text = edge_text(&r[..end])?;
        return Ok((link_type(token), Some(text), span, &r[end + 1..]));
    }
    let ends_head = token.ends_with(['x', 'o']);
    let letter_ok = token.ends_with('>') && matches!(rest.chars().next(), Some('x' | 'o'));
    if matches!(rest.chars().next(), Some('x' | 'o' | '>' | '-' | '.' | '=')) && !ends_head && !letter_ok {
        return Err(unported("link tail"));
    }
    Ok((link_type(token), None, span, rest))
}

/// `click ID ...` draws nothing; accepted only for a known node and a plain
/// argument list (callback / call f(x) / "url" / "tooltip" / target).
pub(super) fn click_ok(args: &str, index: &HashMap<String, usize>) -> Result<(), Error> {
    let id = args.split([' ', '\t']).next().unwrap_or("");
    let plain = args.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '\t' | '"' | ':' | '/' | '.' | '(' | ')' | '<' | '>' | '_' | '-'));
    let template = id.starts_with("${") && id.ends_with('}');
    let plain = plain || (template && args.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '\t' | '$' | '{' | '}')));
    if !(index.contains_key(id) || template) || !plain || args.matches('"').count() % 2 != 0 {
        return Err(unported("click"));
    }
    Ok(())
}

pub(super) fn parse(source: &str) -> Result<Model, Error> {
    if source.contains('\r') {
        return Err(unported("carriage return"));
    }
    let text = source.trim_start_matches([' ', '\t', '\n']);
    let (word, mut rest) = if let Some(r) = text.strip_prefix("flowchart") {
        ("flowchart", r)
    } else if let Some(r) = text.strip_prefix("graph") {
        ("graph", r)
    } else {
        return Err(unported("header"));
    };
    let _ = word;
    let mut dir = "DOWN";
    let trimmed = rest.trim_start_matches([' ', '\t']);
    if trimmed.len() < rest.len() {
        for (name, d) in [("TD", "DOWN"), ("TB", "DOWN"), ("BT", "UP"), ("LR", "RIGHT"), ("RL", "LEFT")] {
            if let Some(r) = trimmed.strip_prefix(name) {
                dir = d;
                rest = r;
                break;
            }
        }
    }
    let rest_t = rest.trim_start_matches([' ', '\t']);
    if !(rest_t.is_empty() || rest_t.starts_with([';', '\n'])) {
        return Err(unported("header tail"));
    }
    let mut model = Model { dir, nodes: vec![], edges: vec![], subs: vec![] };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edge_ids: Vec<String> = vec![];
    let stmts = split_statements(rest_t)?;
    let generated = subgraph::generated_ids(&stmts)?;
    let mut boxes = subgraph::Boxes::default();
    for (at, stmt) in stmts.into_iter().enumerate() {
        let stmt = stmt.trim_matches([' ', '\t']);
        if stmt.is_empty() {
            continue;
        }
        if let Some(args) = keyword_args(stmt, "subgraph") {
            boxes.open(subgraph::header(args)?, generated.get(&at).cloned())?;
            continue;
        }
        if stmt == "end" {
            boxes.close();
            continue;
        }
        if let Some(args) = keyword_args(stmt, "direction") {
            let dir = match args {
                "TB" | "TD" => "DOWN",
                "BT" => "UP",
                "LR" => "RIGHT",
                "RL" => "LEFT",
                _ => return Err(unported("direction")),
            };
            boxes.set_direction(dir);
            continue;
        }
        if let Some(args) = stmt.strip_prefix("click ") {
            click_ok(args, &index)?;
            continue;
        }
        if let Some(args) = keyword_args(stmt, "classDef") {
            class_def_ok(args)?;
            continue;
        }
        if let Some(args) = keyword_args(stmt, "class") {
            class_ok(args)?;
            continue;
        }
        if let Some(args) = keyword_args(stmt, "style") {
            style_statement(args, &mut model, &mut index)?;
            continue;
        }
        if let Some(args) = keyword_args(stmt, "linkStyle") {
            link_style_ok(args, model.edges.len())?;
            continue;
        }
        if let Some((name, r)) = stmt.split_once("@{") {
            if edge_ids.iter().any(|e| e == name) {
                let end = data_end(r).ok_or_else(|| unported("node data"))?;
                if !r[end + 1..].trim().is_empty() {
                    return Err(unported("node data"));
                }
                edge_data_ok(&r[..end])?;
                continue;
            }
        }
        let (mut prev, mut tail) = read_group(stmt, &mut model, &mut index)?;
        for &n in &prev {
            boxes.claim_member(&model.nodes[n].id);
        }
        loop {
            tail = tail.trim_start_matches([' ', '\t']);
            if tail.is_empty() {
                break;
            }
            tail = skip_edge_id(tail, &mut edge_ids)?;
            let (kind, label, span, rest) = parse_link(tail)?;
            tail = rest;
            tail = tail.trim_start_matches([' ', '\t']);
            let (next, after) = read_group(tail, &mut model, &mut index)?;
            for &n in &next {
                boxes.claim_member(&model.nodes[n].id);
            }
            for &a in &prev {
                for &b in &next {
                    if a == b {
                        return Err(unported("self loop"));
                    }
                    model.edges.push(Edge { src: a, tgt: b, kind: kind.clone(), span, label: label.clone(), label_dim: label.as_ref().map(|l| edge_label_dim(l)).unwrap_or((0.0, 0.0)) });
                }
            }
            prev = next;
            tail = after;
        }
    }
    if model.nodes.iter().any(|n| edge_ids.contains(&n.id)) {
        return Err(unported("edge id shares a node id"));
    }
    if !edge_ids.is_empty() && boxes.any() {
        return Err(unported("edge ids beside subgraphs"));
    }
    boxes.finish(&mut model)?;
    Ok(model)
}

/// `A & B & C`: nodes joined by ampersands.
fn read_group<'a>(s: &'a str, model: &mut Model, index: &mut HashMap<String, usize>) -> Result<(Vec<usize>, &'a str), Error> {
    let (first, mut rest) = read_node(s, model, index)?;
    let mut out = vec![first];
    loop {
        let t = rest.trim_start_matches([' ', '\t']);
        match t.strip_prefix('&') {
            Some(r) => {
                let (n, after) = read_node(r.trim_start_matches([' ', '\t']), model, index)?;
                if out.contains(&n) {
                    return Err(unported("repeated group member"));
                }
                out.push(n);
                rest = after;
            }
            None => return Ok((out, rest)),
        }
    }
}

/// `e1@` in front of a link names the edge; names draw nothing.
fn skip_edge_id<'a>(tail: &'a str, ids: &mut Vec<String>) -> Result<&'a str, Error> {
    let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(0);
    if end == 0 || !tail[end..].starts_with('@') || tail[end + 1..].starts_with('{') {
        return Ok(tail);
    }
    ids.push(tail[..end].to_string());
    Ok(&tail[end + 1..])
}

const STATEMENT_KEYWORDS: [&str; 5] = ["click ", "style ", "classDef ", "class ", "linkStyle "];

/// Split a source tail into statements: `;` and newlines separate them, except
/// that a style-like statement owns the rest of its line (`;` is value text).
fn split_statements(rest: &str) -> Result<Vec<&str>, Error> {
    let mut out = vec![];
    let mut lines = merge_data_lines(rest).into_iter();
    while let Some(line) = lines.next() {
        let mut cur = line;
        loop {
            let t = cur.trim_start_matches([' ', '\t']);
            if let Some(after) = t.strip_prefix("accTitle").or_else(|| t.strip_prefix("accDescr")) {
                let after = after.trim_start_matches([' ', '\t']);
                if let Some(text) = after.strip_prefix(':') {
                    if text.trim().is_empty() || text.contains("%%") {
                        return Err(unported("accessibility text"));
                    }
                    break;
                }
                if t.starts_with("accDescr") && after.starts_with('{') {
                    // block form: the brace ends its line, a lone `}` closes it
                    if !after[1..].trim().is_empty() {
                        return Err(unported("accDescr block"));
                    }
                    loop {
                        let body = lines.next().ok_or_else(|| unported("accDescr block"))?;
                        if body.contains("%%") {
                            return Err(unported("accDescr block"));
                        }
                        if body.trim() == "}" {
                            break;
                        }
                        if body.contains('}') {
                            return Err(unported("accDescr block"));
                        }
                    }
                    break;
                }
            }
            if STATEMENT_KEYWORDS.iter().any(|k| t.starts_with(k)) {
                out.push(t);
                break;
            }
            let semi = {
                let mut q = false;
                cur.char_indices().find(|&(_, c)| {
                    if c == '"' {
                        q = !q;
                    }
                    c == ';' && !q
                }).map(|(i, _)| i)
            };
            match semi {
                Some(i) => {
                    out.push(&cur[..i]);
                    cur = &cur[i + 1..];
                }
                None => {
                    out.push(cur);
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// The arguments after `word` + a gap, with one trailing `;` removed.
pub(super) fn keyword_args<'a>(stmt: &'a str, word: &str) -> Option<&'a str> {
    let r = stmt.strip_prefix(word)?;
    if !r.starts_with([' ', '\t']) {
        return None;
    }
    let r = r.trim_matches([' ', '\t']);
    Some(r.strip_suffix(';').unwrap_or(r).trim_end_matches([' ', '\t']))
}

/// Comma-separated `name:value` declarations (commas inside parentheses stay).
fn declarations(props: &str) -> Result<Vec<(&str, &str)>, Error> {
    if props.is_empty() || props.contains([';', '"', '\'', '\\', '<', '>', '{', '}', '[', ']', '|', '@', '&']) {
        return Err(unported("style properties"));
    }
    let mut out = vec![];
    let (mut depth, mut start) = (0i32, 0usize);
    let bytes = props.as_bytes();
    for i in 0..=props.len() {
        let at_end = i == props.len();
        if !at_end {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
        }
        if at_end || (bytes[i] == b',' && depth == 0) {
            let part = &props[start..i];
            let (k, v) = part.split_once(':').ok_or_else(|| unported("style declaration"))?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || v.trim().is_empty() || depth != 0 {
                return Err(unported("style declaration"));
            }
            out.push((k, v.trim()));
            start = i + 1;
        }
    }
    Ok(out)
}

fn class_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && read_id(s).map_or(false, |(id, r)| id == s && r.is_empty())
}

/// `classDef a,b props` paints nothing in the Ruby renderer.
fn class_def_ok(args: &str) -> Result<(), Error> {
    let (names, props) = args.split_once([' ', '\t']).ok_or_else(|| unported("classDef"))?;
    if !names.split(',').all(class_name) {
        return Err(unported("classDef name"));
    }
    declarations(props.trim())?;
    Ok(())
}

/// `class ID name` paints nothing either, and declares no node.
fn class_ok(args: &str) -> Result<(), Error> {
    let (ids, name) = args.split_once([' ', '\t']).ok_or_else(|| unported("class"))?;
    if !ids.split(',').all(class_name) || !class_name(name.trim()) {
        return Err(unported("class"));
    }
    Ok(())
}

/// `linkStyle 0,1 props` / `linkStyle default props`: validated, never drawn.
fn link_style_ok(args: &str, edges: usize) -> Result<(), Error> {
    let (targets, props) = args.split_once([' ', '\t']).ok_or_else(|| unported("linkStyle"))?;
    if targets != "default" {
        for t in targets.split(',') {
            let n: usize = t.parse().map_err(|_| unported("linkStyle index"))?;
            if n.to_string() != t || n >= edges {
                return Err(unported("linkStyle index"));
            }
        }
    }
    let props = props.trim();
    if props.contains(['#', ';']) || props.contains("default") || props.contains("interpolate") {
        return Err(unported("linkStyle props"));
    }
    declarations(props)?;
    Ok(())
}

const PAINT_KEYWORDS: &str = "none currentcolor transparent aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgrey darkgreen darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray grey green greenyellow honeydew hotpink indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray lightgrey lightgreen lightpink lightsalmon lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow yellowgreen";

/// `FlowchartNode.paint_value`: a CSS keyword, `#rgb`/`#rrggbb` or `rgb(n,n,n)`.
fn paint_value(v: &str) -> Option<String> {
    let v = v.trim();
    let lower = v.to_ascii_lowercase();
    if PAINT_KEYWORDS.split(' ').any(|k| k == lower) {
        return Some(v.to_string());
    }
    if let Some(h) = v.strip_prefix('#') {
        if matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(v.to_string());
        }
    }
    if lower.starts_with("rgb(") && lower.ends_with(')') {
        let inner = &lower[4..lower.len() - 1];
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let comp = |p: &str| {
            let p = p.strip_suffix('%').unwrap_or(p);
            let p = p.strip_prefix(['+', '-']).unwrap_or(p);
            !p.is_empty() && p.chars().all(|c| c.is_ascii_digit() || c == '.') && p.chars().filter(|c| *c == '.').count() <= 1 && p != "."
        };
        if parts.len() == 3 && parts.iter().all(|p| comp(p)) {
            return Some(v.to_string());
        }
    }
    None
}

/// `style ID props`: declares the node when unseen and keeps fill/stroke.
fn style_statement(args: &str, model: &mut Model, index: &mut HashMap<String, usize>) -> Result<(), Error> {
    let (target, props) = args.split_once([' ', '\t']).ok_or_else(|| unported("style"))?;
    let (id, rest) = read_id(target).ok_or_else(|| unported("style target"))?;
    if !rest.is_empty() {
        return Err(unported("style target"));
    }
    let decls = declarations(props.trim())?;
    let i = match index.get(id) {
        Some(&i) => i,
        None => {
            model.nodes.push(Node { id: id.to_string(), lines: vec![id.to_string()], icons: vec![0], shape: Shape::Rect, fill: None, stroke: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0, cluster: false, collision: false });
            index.insert(id.to_string(), model.nodes.len() - 1);
            model.nodes.len() - 1
        }
    };
    for (k, v) in decls {
        let paint = paint_value(v);
        match (k, paint) {
            ("fill", Some(p)) => model.nodes[i].fill = Some(p),
            ("stroke", Some(p)) => model.nodes[i].stroke = Some(p),
            _ => {}
        }
    }
    Ok(())
}

type Label = (Vec<String>, Vec<usize>);

/// The `}` closing a node-data body, skipping any inside a double-quoted string.
fn data_end(r: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in r.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

enum Scalar {
    Plain(String),
    Quoted(String),
}

/// The `key: value` entries of a flow (one line) or block (several) YAML mapping.
fn data_entries(body: &str) -> Result<Vec<(String, Scalar)>, Error> {
    let bad = || unported("node data");
    if body.contains(['%', '\\', '\t', '\r']) {
        return Err(bad());
    }
    let mut out = vec![];
    if !body.contains('\n') {
        let (mut parts, mut cur, mut quoted) = (vec![], String::new(), false);
        for c in body.chars() {
            match c {
                '"' => {
                    quoted = !quoted;
                    cur.push(c);
                }
                ',' if !quoted => parts.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            }
        }
        if quoted {
            return Err(bad());
        }
        parts.push(cur);
        for part in parts {
            let (k, v) = part.split_once(':').ok_or_else(bad)?;
            out.push((k.trim().to_string(), scalar(v.trim())?));
        }
        return Ok(out);
    }
    let lines: Vec<&str> = body.split('\n').collect();
    let indent = |l: &str| l.len() - l.trim_start_matches(' ').len();
    let mut i = 0;
    let mut base = None;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim().is_empty() {
            continue;
        }
        let ind = indent(line);
        if *base.get_or_insert(ind) != ind {
            return Err(bad());
        }
        let (k, v) = line.trim().split_once(':').ok_or_else(bad)?;
        let v = v.trim();
        if v == "|" {
            let mut text = vec![];
            while i < lines.len() && (lines[i].trim().is_empty() || indent(lines[i]) > ind) {
                text.push(lines[i].trim());
                i += 1;
            }
            out.push((k.trim().to_string(), Scalar::Quoted(text.join("\n"))));
        } else if v.starts_with('"') && (v.len() < 2 || !v[1..].contains('"')) {
            let mut text = v[1..].to_string();
            loop {
                let next = lines.get(i).ok_or_else(bad)?;
                i += 1;
                text.push_str("<br/>");
                if let Some(e) = next.trim().find('"') {
                    if e + 1 != next.trim().len() {
                        return Err(bad());
                    }
                    text.push_str(&next.trim()[..e]);
                    break;
                }
                text.push_str(next.trim());
            }
            out.push((k.trim().to_string(), Scalar::Quoted(text)));
        } else {
            out.push((k.trim().to_string(), scalar(v)?));
        }
    }
    Ok(out)
}

fn scalar(v: &str) -> Result<Scalar, Error> {
    let bad = || unported("node data");
    let wrapped = |q: char| v.len() >= 2 && v.starts_with(q) && v.ends_with(q) && !v[1..v.len() - 1].contains(q);
    if wrapped('"') || wrapped('\'') {
        return Ok(Scalar::Quoted(v[1..v.len() - 1].to_string()));
    }
    if v.is_empty() || v.contains(['"', '\'', ':', '#', '|', '{', '}', '[', ']', '&', '*', '!', '`', '@', '>']) {
        return Err(bad());
    }
    Ok(Scalar::Plain(v.to_string()))
}

/// `@{ shape: x, label: "text" }`: the label and shape it sets; keys sirena does not read are ignored.
fn node_data(body: &str) -> Result<(Option<Label>, Option<Shape>), Error> {
    let (mut label, mut shape) = (None, None);
    for (k, v) in data_entries(body)? {
        match k.as_str() {
            "shape" => {
                let (Scalar::Plain(b) | Scalar::Quoted(b)) = &v;
                shape = Some(match b.as_str() {
                    "rect" => Shape::Rect,
                    "rounded" | "event" => Shape::Rounded,
                    "circ" | "circle" => Shape::Circle,
                    "diam" | "diamond" | "decision" => Shape::Rhombus,
                    "hex" | "hexagon" => Shape::Hexagon,
                    _ => return Err(unported("node data shape")),
                });
            }
            "label" => match v {
                Scalar::Quoted(b) => label = Some(label_lines_in(&b, false, &['@', '}']).ok_or_else(|| unported("node data label"))?),
                Scalar::Plain(_) => return Err(unported("node data label")),
            },
            "icon" | "img" => return Err(unported("node data key")),
            _ => {}
        }
    }
    Ok((label, shape))
}

/// An edge-id statement `e1@{ animate: true }`: validated, draws nothing.
fn edge_data_ok(body: &str) -> Result<(), Error> {
    for (k, v) in data_entries(body)? {
        let ok = match (k.as_str(), &v) {
            ("animate", Scalar::Plain(b)) => matches!(b.as_str(), "true" | "false"),
            ("animation", Scalar::Plain(b)) => matches!(b.as_str(), "fast" | "slow"),
            _ => false,
        };
        if !ok {
            return Err(unported("node data key"));
        }
    }
    Ok(())
}

/// How many lines mmdc breaks an edge label into at 200px (a space stays on its line).
fn wrapped_lines(text: &str) -> usize {
    let mut lines: Vec<String> = vec![String::new()];
    let mut words: Vec<String> = vec![];
    let mut cur = String::new();
    for ch in text.chars() {
        if ch == ' ' {
            if !cur.is_empty() {
                cur.push(' ');
                words.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    for w in words {
        let last = lines.last().unwrap();
        if !last.is_empty() && label_width(&format!("{last}{w}")) > 200.0 {
            lines.push(String::new());
        }
        lines.last_mut().unwrap().push_str(&w);
    }
    lines.len()
}

pub(super) fn edge_label_dim(text: &str) -> (f64, f64) {
    let n = wrapped_lines(text);
    if n > 1 { (200.0, FONT_SIZE * n as f64) } else { (label_width(text), FONT_SIZE) }
}


/// Source lines, with a `@{` body left open at a line end joined to the lines that close it.
fn merge_data_lines(rest: &str) -> Vec<&str> {
    let raw: Vec<&str> = rest.split('\n').collect();
    let at = |l: &str| l.as_ptr() as usize - rest.as_ptr() as usize;
    let (mut out, mut i) = (vec![], 0);
    while i < raw.len() {
        let mut j = i;
        loop {
            let span = &rest[at(raw[i])..at(raw[j]) + raw[j].len()];
            let open = span.rfind("@{").map_or(false, |k| data_end(&span[k + 2..]).is_none());
            if !open || j + 1 >= raw.len() {
                break;
            }
            j += 1;
        }
        out.push(&rest[at(raw[i])..at(raw[j]) + raw[j].len()]);
        i = j + 1;
    }
    out
}
