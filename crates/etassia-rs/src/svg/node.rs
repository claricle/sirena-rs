//! SVG node tree and serializer (Ruby `Sirena::Svg::{Element,Group,Text,Document}`).

use super::escape::{escape_attr, escape_text, valid_name};
use super::number::{read_num, round4, ruby_float, write_num};

/// Attributes every element emits first, in this order. `opacity` is never
/// emitted: it is folded into `fill-opacity` / `stroke-opacity`.
const BASE: [&str; 8] =
    ["id", "class", "transform", "fill", "fill-opacity", "stroke", "stroke-width", "stroke-opacity"];
const TEXT_ATTRS: [&str; 7] =
    ["x", "y", "text-anchor", "font-family", "font-size", "font-weight", "font-style"];
const TSPAN_ATTRS: [&str; 4] = ["x", "y", "font-weight", "font-style"];

/// One SVG element (`rect`, `g`, `text`, ...). Build with [`Elem::new`] and
/// chain [`attr`](Elem::attr) / [`num`](Elem::num) / [`child`](Elem::child).
///
/// Attribute order in the output is fixed by the serializer (common
/// attributes first, then the rest in insertion order), so callers need not
/// care about the order they set them in.
#[derive(Debug, Clone, PartialEq)]
pub struct Elem {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Elem>,
}

impl Elem {
    /// A new element with the given tag name and no attributes.
    pub fn new(tag: &str) -> Self {
        Elem { tag: tag.into(), attrs: vec![], text: String::new(), children: vec![] }
    }

    /// Sets (or replaces) a string attribute. The value is escaped on output.
    pub fn attr(mut self, name: &str, value: impl Into<String>) -> Self {
        self.set(name, value.into());
        self
    }

    /// Sets a numeric attribute rendered as Ruby `Float#to_s` (`10.0`).
    pub fn num(self, name: &str, value: f64) -> Self {
        self.attr(name, ruby_float(value))
    }

    /// Sets a numeric attribute rendered as `Numbers.write` (rounded to 4
    /// decimals, then `Float#to_s`).
    pub fn num4(self, name: &str, value: f64) -> Self {
        self.attr(name, write_num(value))
    }

    /// Appends literal text content (escaped on output). Used by `text`/`tspan`.
    pub fn content(mut self, text: &str) -> Self {
        self.text.push_str(text);
        self
    }

    /// Appends a child element (a `g` child, or a `tspan` of a `text`).
    pub fn child(mut self, e: Elem) -> Self {
        self.children.push(e);
        self
    }

    /// Appends a child element in place.
    pub fn push(&mut self, e: Elem) {
        self.children.push(e);
    }

    /// In-place [`attr`](Elem::attr).
    pub fn set(&mut self, name: &str, value: String) {
        match self.attrs.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((name.into(), value)),
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn composed(&self, name: &str) -> Option<String> {
        let comp = self.get(name).map(str::to_string);
        let Some(whole) = self.get("opacity").and_then(read_num) else { return comp };
        let part = match &comp {
            None => Some(1.0),
            Some(c) => read_num(c),
        };
        let Some(part) = part else { return comp };
        if !(whole.is_finite() && part.is_finite()) {
            return comp;
        }
        Some(write_num(part.clamp(0.0, 1.0) * whole.clamp(0.0, 1.0)))
    }

    fn pairs(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = vec![];
        for name in BASE {
            let v = match name {
                "fill-opacity" | "stroke-opacity" => self.composed(name),
                _ => self.get(name).map(str::to_string),
            };
            if let Some(v) = v {
                out.push((name.into(), v));
            }
        }
        match self.tag.as_str() {
            "text" => {
                for (n, v) in [("x", self.offset_x()), ("y", self.baseline_y())] {
                    if let Some(v) = v {
                        out.push((n.into(), v));
                    }
                }
                for n in &TEXT_ATTRS[2..] {
                    if let Some(v) = self.get(n) {
                        out.push(((*n).into(), v.into()));
                    }
                }
            }
            "tspan" => {
                for n in TSPAN_ATTRS {
                    if let Some(v) = self.get(n) {
                        out.push((n.into(), v.into()));
                    }
                }
            }
            _ => {
                for (n, v) in &self.attrs {
                    if !BASE.contains(&n.as_str()) && n != "opacity" {
                        out.push((n.clone(), v.clone()));
                    }
                }
            }
        }
        out
    }

    fn offset_x(&self) -> Option<String> {
        let Some(offset) = self.get("dx").and_then(read_num) else {
            return self.get("x").map(str::to_string);
        };
        let computed = self.get("x").and_then(read_num).unwrap_or(0.0) + offset;
        if computed.is_finite() { Some(write_num(computed)) } else { self.get("x").map(str::to_string) }
    }

    fn baseline_shift(&self) -> f64 {
        let ems = match self.get("dominant-baseline").map(|s| s.trim().to_lowercase()).as_deref() {
            Some("middle") | Some("central") => 0.35,
            Some("hanging") | Some("text-before-edge") => 0.8,
            Some("text-after-edge") | Some("ideographic") => -0.2,
            _ => return 0.0,
        };
        ems * self.get("font-size").and_then(read_num).unwrap_or(16.0)
    }

    fn baseline_y(&self) -> Option<String> {
        let shift = self.baseline_shift();
        let offset = self.get("dy").and_then(read_num);
        if shift == 0.0 && offset.is_none() {
            return self.get("y").map(str::to_string);
        }
        let computed = self.get("y").and_then(read_num).unwrap_or(0.0) + offset.unwrap_or(0.0) + shift;
        if computed.is_finite() { Some(write_num(computed)) } else { self.get("y").map(str::to_string) }
    }

    fn attrs_text(&self) -> String {
        let mut s = String::new();
        for (n, v) in self.pairs() {
            assert!(valid_name(&n), "not a valid attribute name: {n:?}");
            s.push_str(&format!(" {n}=\"{}\"", escape_attr(&v)));
        }
        s
    }

    /// One entry per structural unit (a multi-line `text` is a single entry).
    fn entries(&self) -> Vec<String> {
        let attrs = self.attrs_text();
        match self.tag.as_str() {
            "g" if self.children.is_empty() => vec![format!("<g{attrs}/>")],
            "g" => {
                let mut v = vec![format!("<g{attrs}>")];
                for c in &self.children {
                    v.extend(c.entries().into_iter().map(|l| format!("  {l}")));
                }
                v.push("</g>".into());
                v
            }
            "text" => {
                let mut body = escape_text(&self.text);
                let (mut lines, size) = (0i64, self.get("font-size").and_then(read_num).unwrap_or(16.0));
                let base = read_num(self.baseline_y().as_deref().unwrap_or("")).unwrap_or(0.0);
                for run in &self.children {
                    let mut run = run.clone();
                    if let Some(shift) = run.get("line-shift").and_then(|s| s.parse::<i64>().ok()) {
                        lines += shift;
                        let placed = base + (lines as f64 * 1.2 * size);
                        match placed.is_finite() {
                            true => run.set("y", ruby_float(round4(placed))),
                            false => run.attrs.retain(|(n, _)| n != "y"),
                        }
                    }
                    body.push_str(&run.entries().join("\n"));
                }
                vec![format!("<text{attrs}>{body}</text>")]
            }
            "tspan" => vec![format!("<tspan{attrs}>{}</tspan>", escape_text(&self.text))],
            tag => vec![format!("<{tag}{attrs}/>")],
        }
    }

    /// Serializes this element as a fragment (newline-joined entries).
    pub fn to_xml(&self) -> String {
        self.entries().join("\n")
    }
}

/// The `<svg>` root: dimensions, optional `overflow`, and child elements.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Document width (`None` omits the attribute).
    pub width: Option<f64>,
    /// Document height (`None` omits the attribute).
    pub height: Option<f64>,
    /// Explicit `viewBox`; derived as `"0 0 {width} {height}"` (Ruby float text) when `None`.
    pub view_box: Option<String>,
    /// `overflow` presentation attribute, omitted when `None`.
    pub overflow: Option<String>,
    /// Top-level children, in order.
    pub children: Vec<Elem>,
}

impl Document {
    /// A document with a viewBox derived from the given size.
    pub fn new(width: f64, height: f64) -> Self {
        Document { width: Some(width), height: Some(height), view_box: None, overflow: None, children: vec![] }
    }

    /// Appends a top-level element.
    pub fn add(&mut self, e: Elem) {
        self.children.push(e);
    }

    /// Serializes to the exact Ruby `Svg::Document#to_xml` output (no trailing newline).
    pub fn to_xml(&self) -> String {
        let vb = self.view_box.clone().or_else(|| match (self.width, self.height) {
            (Some(w), Some(h)) => Some(format!("0 0 {} {}", ruby_float(w), ruby_float(h))),
            _ => None,
        });
        let mut parts = vec!["<svg".to_string()];
        let attrs = [
            ("width", self.width.map(ruby_float)),
            ("height", self.height.map(ruby_float)),
            ("viewBox", vb),
            ("version", Some("1.2".into())),
            ("baseProfile", Some("tiny".into())),
            ("xmlns", Some("http://www.w3.org/2000/svg".into())),
            ("overflow", self.overflow.clone()),
        ];
        for (n, v) in attrs {
            if let Some(v) = v {
                parts.push(format!(" {n}=\"{}\"", escape_attr(&v)));
            }
        }
        parts.push(">".into());
        parts.extend(self.children.iter().map(Elem::to_xml));
        parts.push("</svg>".into());
        parts.join("\n")
    }
}

/// Serializes a document (alias of [`Document::to_xml`]).
pub fn serialize(doc: &Document) -> String {
    doc.to_xml()
}
