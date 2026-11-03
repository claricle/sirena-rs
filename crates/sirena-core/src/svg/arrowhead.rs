//! Port of `Sirena::Svg::Arrowhead`: triangular arrowheads drawn from a path.

use super::node::Elem;
use super::number::{read_num, write_num};
use super::path_geometry::{Anchor, PathGeometry};

const LENGTH: f64 = 4.0;
const HALF_WIDTH: f64 = 2.0;
const DEFAULT_STROKE_WIDTH: f64 = 1.0;

/// The path attributes an arrowhead is derived from (Ruby `Svg::Path` readers).
/// `None` is an unset attribute.
#[derive(Debug, Clone, Default)]
pub struct ArrowPath<'a> {
    pub d: &'a str,
    pub stroke: Option<&'a str>,
    pub stroke_width: Option<&'a str>,
    pub stroke_opacity: Option<&'a str>,
    pub opacity: Option<&'a str>,
    pub transform: Option<&'a str>,
    pub marker_start: Option<&'a str>,
    pub marker_end: Option<&'a str>,
}

pub struct Arrowhead;

impl Arrowhead {
    /// Ruby `Arrowhead.for`: one `<polygon>` per requested marker — the
    /// end-marker head before the start-marker head, either possibly absent.
    pub fn for_path(path: &ArrowPath) -> Vec<Elem> {
        let mut heads = vec![];
        if names_something(path.marker_end) {
            heads.push(true);
        }
        if names_something(path.marker_start) {
            heads.push(false);
        }
        if heads.is_empty() || !painted(path) {
            return vec![];
        }
        let geometry = PathGeometry::new(path.d);
        heads
            .into_iter()
            .filter_map(|is_end| {
                let anchor = if is_end {
                    geometry.terminus()
                } else {
                    geometry.origin().map(|a| Anchor { x: a.x, y: a.y, dx: -a.dx, dy: -a.dy })
                };
                triangle(path, anchor?)
            })
            .collect()
    }
}

fn names_something(value: Option<&str>) -> bool {
    match value {
        None => false,
        Some(v) => {
            let n = v.trim().to_lowercase();
            !n.is_empty() && n != "none"
        }
    }
}

fn painted(path: &ArrowPath) -> bool {
    names_something(path.stroke) && stroke_width(path) > 0.0
}

fn stroke_width(path: &ArrowPath) -> f64 {
    match path.stroke_width.and_then(read_num) {
        Some(w) if w.is_finite() && !w.is_sign_negative() => w,
        _ => DEFAULT_STROKE_WIDTH,
    }
}

fn presence(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.trim().is_empty())
}

fn triangle(path: &ArrowPath, a: Anchor) -> Option<Elem> {
    let w = stroke_width(path);
    let length = LENGTH * w;
    let half = HALF_WIDTH * w;
    let base = [a.x - a.dx * length, a.y - a.dy * length];
    let off = [a.dy * half, -a.dx * half];
    let pts = [[a.x, a.y], [base[0] - off[0], base[1] - off[1]], [base[0] + off[0], base[1] + off[1]]];
    if !pts.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let points = pts
        .iter()
        .map(|p| format!("{},{}", write_num(p[0]), write_num(p[1])))
        .collect::<Vec<_>>()
        .join(" ");
    let mut e = Elem::new("polygon").attr("points", points);
    if let Some(s) = path.stroke {
        e = e.attr("fill", s);
    }
    for (name, v) in [
        ("fill-opacity", path.stroke_opacity),
        ("opacity", path.opacity),
        ("transform", path.transform),
    ] {
        if let Some(v) = presence(v) {
            e = e.attr(name, v);
        }
    }
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::Document;

    fn xml(es: Vec<Elem>) -> Vec<String> {
        es.into_iter()
            .map(|e| {
                let mut d = Document::new(10.0, 10.0);
                d.add(e);
                d.to_xml()
            })
            .collect()
    }

    fn pts(es: &[Elem]) -> Vec<String> {
        xml(es.to_vec())
            .into_iter()
            .map(|x| x.split("points=\"").nth(1).unwrap().split('"').next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn end_head_default_width() {
        let p = ArrowPath { d: "M 0 0 L 10 0", stroke: Some("#000"), marker_end: Some("url(#a)"), ..Default::default() };
        // Ruby: points="10.0,0.0 6.0,2.0 6.0,-2.0"
        assert_eq!(pts(&Arrowhead::for_path(&p)), vec!["10.0,0.0 6.0,2.0 6.0,-2.0"]);
    }

    #[test]
    fn both_heads_end_first_and_scaled() {
        let p = ArrowPath {
            d: "M 0 0 L 10 0",
            stroke: Some("#000"),
            stroke_width: Some("2"),
            marker_end: Some("x"),
            marker_start: Some("x"),
            ..Default::default()
        };
        // Ruby: ["10.0,0.0 2.0,4.0 2.0,-4.0", "0.0,0.0 8.0,-4.0 8.0,4.0"]
        assert_eq!(pts(&Arrowhead::for_path(&p)), vec!["10.0,0.0 2.0,4.0 2.0,-4.0", "0.0,0.0 8.0,-4.0 8.0,4.0"]);
    }

    #[test]
    fn unpainted_or_unmarked_gives_nothing() {
        let base = ArrowPath { d: "M 0 0 L 10 0", stroke: Some("#000"), marker_end: Some("x"), ..Default::default() };
        assert_eq!(Arrowhead::for_path(&ArrowPath { stroke: None, ..base.clone() }).len(), 0);
        assert_eq!(Arrowhead::for_path(&ArrowPath { stroke: Some("none"), ..base.clone() }).len(), 0);
        assert_eq!(Arrowhead::for_path(&ArrowPath { stroke_width: Some("0"), ..base.clone() }).len(), 0);
        assert_eq!(Arrowhead::for_path(&ArrowPath { marker_end: Some("none"), ..base }).len(), 0);
    }
}
