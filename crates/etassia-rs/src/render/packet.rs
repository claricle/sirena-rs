//! Packet renderer: port of Ruby `Native::Builders::Packet`, `Layout::Packet`
//! and `Renderer::Packet`.
use super::pie::{is_slice, items, text_of};
use crate::svg::{Document, Elem, Theme};
use crate::Error;
use serde_json::Value;

const BITS_PER_ROW: i64 = 32;
const BIT_WIDTH: i64 = 32;
const ROW_HEIGHT: i64 = 32;
const PADDING_X: i64 = 5;
const PADDING_Y: i64 = 15;
const ROW_PITCH: i64 = ROW_HEIGHT + PADDING_Y;

struct Field {
    first: i64,
    last: i64,
    label: String,
}

fn parse_err(message: String) -> Error {
    Error::Parse { class: "ParseError".into(), message, line: 0, col: 0 }
}

/// Ruby `String#to_i` for the digit strings the grammar yields.
fn to_i(s: &str) -> i64 {
    let s = s.trim_start();
    let end = s.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')))).map_or(s.len(), |x| x.0);
    s[..end].parse().unwrap_or(0)
}

/// `gsub(/\\(.)/m, '\1')`: drops the backslash of each escape pair.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some(n) => out.push(n),
                None => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn build(tree: &Value) -> Result<(Option<String>, Vec<Field>), Error> {
    let mut title = None;
    let mut fields: Vec<Field> = vec![];
    let mut next_bit = 0i64;
    if tree.is_object() && !is_slice(tree) {
        for item in items(&tree["statements"]) {
            if let Some(t) = item.get("title") {
                title = Some(text_of(t));
                continue;
            }
            let first = to_i(&text_of(&item["bit_start"]));
            let last = if item.get("bit_end").is_some() { to_i(&text_of(&item["bit_end"])) } else { first };
            if last < first {
                return Err(parse_err("Packet field end must be greater than or equal to start.".into()));
            }
            if first != next_bit {
                return Err(parse_err(format!("Packet fields must start from {next_bit}.")));
            }
            next_bit = last + 1;
            fields.push(Field { first, last, label: unescape(&text_of(&item["label"])) });
        }
    }
    Ok((title, fields))
}

fn size_str(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { crate::svg::ruby_float(v) }
}

pub fn render(source: &str) -> Result<String, Error> {
    let tree = crate::parse::parse(source, "packet")?;
    let (title, fields) = build(&tree)?;
    let theme = Theme::default_theme();
    let normal = theme.number("typography", "font_size_normal").unwrap_or(14.0);
    let small = theme.number("typography", "font_size_small").unwrap_or(12.0);
    let bit_size = small - (normal - small);

    let row_count = match fields.iter().map(|f| f.last).max() {
        None => 0,
        Some(max) => (max + 1 + BITS_PER_ROW - 1) / BITS_PER_ROW,
    };
    let width = BITS_PER_ROW * BIT_WIDTH + 2;
    let height = ROW_PITCH * (row_count + 1) - if title.is_some() { 0 } else { ROW_HEIGHT };

    let mut doc = Document::new(width as f64, height as f64);
    doc.view_box = Some(format!("0 0 {width} {height}"));
    let font = theme.string("typography", "font_family").unwrap_or("Arial, sans-serif");
    let label = |x: f64, y: f64, anchor: &str, baseline: &str, color: &str, size: f64, text: &str| {
        Elem::new("text")
            .attr("fill", color)
            .num4("x", x)
            .num4("y", y)
            .attr("text-anchor", anchor)
            .attr("dominant-baseline", baseline)
            .attr("font-family", font)
            .attr("font-size", size_str(size))
            .content(text)
    };
    let label_text = theme.color("label_text").unwrap_or("");
    let field_text = theme.color("field_text").unwrap_or("");

    for f in &fields {
        // A field crossing a 32-bit row is drawn as one segment per row.
        let mut cur = f.first;
        while cur <= f.last {
            let row = cur / BITS_PER_ROW;
            let seg_end = f.last.min((row + 1) * BITS_PER_ROW - 1);
            let (first, last) = (cur, seg_end);
            let bx = ((first % BITS_PER_ROW) * BIT_WIDTH + 1) as f64;
            let by = (row * ROW_PITCH + PADDING_Y) as f64;
            let bw = ((last - first + 1) * BIT_WIDTH - PADDING_X) as f64;
            doc.add(
                Elem::new("rect")
                    .attr("fill", theme.color("field_background").unwrap_or(""))
                    .attr("stroke", theme.color("field_border").unwrap_or(""))
                    .attr("stroke-width", "1")
                    .num4("x", bx)
                    .num4("y", by)
                    .num4("width", bw)
                    .num4("height", ROW_HEIGHT as f64),
            );
            doc.add(label(bx + bw / 2.0, by + ROW_HEIGHT as f64 / 2.0, "middle", "middle", field_text, small, &f.label));
            let single = first == last;
            doc.add(label(
                bx + if single { bw / 2.0 } else { 0.0 },
                by - 2.0,
                if single { "middle" } else { "start" },
                "auto",
                label_text,
                bit_size,
                &first.to_string(),
            ));
            if !single {
                doc.add(label(bx + bw, by - 2.0, "end", "auto", label_text, bit_size, &last.to_string()));
            }
            cur = seg_end + 1;
        }
    }
    if let Some(t) = &title {
        doc.add(label(
            width as f64 / 2.0,
            height as f64 - ROW_PITCH as f64 / 2.0,
            "middle",
            "middle",
            theme.color("title_text").unwrap_or(""),
            normal,
            t,
        ));
    }
    Ok(doc.to_xml())
}
