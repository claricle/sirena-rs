//! Flowchart node sizing and boundary geometry.
use super::*;

pub(super) const FONT_SIZE: f64 = 16.0;
pub(super) const NODE_LINE_HEIGHT: f64 = 1.5;
pub(super) const ROUNDED_CORNER: f64 = 5.0;

pub(super) const ICON_ADVANCE_EMS: f64 = 1.3;

/// The widest line and the stacked line height of a label.
pub(super) fn label_dims(lines: &[String], icons: &[usize]) -> (f64, f64) {
    let lw = lines.iter().zip(icons).map(|(l, i)| text::width(l, FONT_SIZE, Face::Trebuchet) + *i as f64 * FONT_SIZE * ICON_ADVANCE_EMS).fold(f64::NEG_INFINITY, f64::max);
    (lw, FONT_SIZE * NODE_LINE_HEIGHT * lines.len() as f64)
}

pub(super) fn size_of(lines: &[String], icons: &[usize], shape: Shape) -> (f64, f64) {
    let (lw, lh) = label_dims(lines, icons);
    match shape {
        Shape::Rhombus => {
            let s = lw + lh + 30.0;
            (s, s)
        }
        Shape::Circle => {
            let s = lw + 15.0;
            (s, s)
        }
        Shape::DoubleCircle => {
            let s = lw + 25.0;
            (s, s)
        }
        Shape::Rounded => (lw + 30.0, lh + 30.0),
        Shape::Asymmetric => (lw + 24.75, lh + 15.0),
        Shape::Skewed => (lw + 60.0, lh + 15.0),
        Shape::Hexagon => (lw + (45.0 + 0.118 * (lh - FONT_SIZE * NODE_LINE_HEIGHT)), lh + 15.0),
        Shape::Rect | Shape::Stadium => (lw + 60.0, lh + 30.0),
    }
}

pub(super) fn centre(n: &Node) -> (f64, f64) {
    (n.x + n.w / 2.0, n.y + n.h / 2.0)
}

pub(super) fn axis_ratio(delta: f64, half: f64) -> f64 {
    if delta == 0.0 { 0.0 } else { delta.abs() / half }
}

pub(super) fn box_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    let a = if dx == 0.0 { f64::INFINITY } else { hw / dx.abs() };
    let b = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
    if b < a { b } else { a }
}

pub(super) fn cap_intersection(cx: f64, radius: f64, dx: f64, dy: f64, cy: f64) -> f64 {
    let a = dx * dx + dy * dy;
    let b = dx * cx + dy * cy;
    let c = (cx * cx + cy * cy) - radius * radius;
    let disc = b * b - a * c;
    (b + (if disc > 0.0 { disc } else { 0.0 }).sqrt()) / a
}

pub(super) fn ellipse_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    let xr = axis_ratio(dx, hw);
    let yr = axis_ratio(dy, hh);
    1.0 / ((xr.powi(2)) + (yr.powi(2))).sqrt()
}

pub(super) fn stadium_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    if hw == hh {
        return ellipse_scale(hw, hh, dx, dy);
    }
    if hw < hh {
        return stadium_scale(hh, hw, dy, dx);
    }
    let flat = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
    let straight = hw - hh;
    if (flat * dx).abs() <= straight {
        return flat;
    }
    cap_intersection(straight * if dx < 0.0 { -1.0 } else { 1.0 }, hh, dx, dy, 0.0)
}

pub(super) fn rounded_scale(hw: f64, hh: f64, dx: f64, dy: f64) -> f64 {
    let scale = box_scale(hw, hh, dx, dy);
    let inner_x = hw - ROUNDED_CORNER;
    let inner_y = hh - ROUNDED_CORNER;
    if (dx * scale).abs() <= inner_x || (dy * scale).abs() <= inner_y {
        return scale;
    }
    cap_intersection(inner_x * if dx < 0.0 { -1.0 } else { 1.0 }, ROUNDED_CORNER, dx, dy, inner_y * if dy < 0.0 { -1.0 } else { 1.0 })
}

pub(super) fn node_boundary(n: &Node, fx: f64, fy: f64) -> (f64, f64) {
    let (cx, cy) = centre(n);
    let (dx, dy) = (fx - cx, fy - cy);
    if dx == 0.0 && dy == 0.0 {
        return (cx, cy);
    }
    let (hw, hh) = (n.w / 2.0, n.h / 2.0);
    let scale = match n.shape {
        Shape::Rhombus => 1.0 / (axis_ratio(dx, hw) + axis_ratio(dy, hh)),
        Shape::Circle | Shape::DoubleCircle => (if hw < hh { hw } else { hh }) / dx.hypot(dy),
        Shape::Hexagon => {
            let slope = 1.0 / (axis_ratio(dx, hw) + axis_ratio(dy, 2.0 * hh));
            let side = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
            if side < slope { side } else { slope }
        }
        Shape::Rounded => rounded_scale(hw, hh, dx, dy),
        Shape::Stadium => stadium_scale(hw, hh, dx, dy),
        Shape::Rect | Shape::Asymmetric | Shape::Skewed => box_scale(hw, hh, dx, dy),
    };
    (cx + dx * scale, cy + dy * scale)
}

pub(super) fn unit_towards(tx: f64, ty: f64, fx: f64, fy: f64) -> (f64, f64) {
    let span = (fx - tx).hypot(fy - ty);
    if span == 0.0 { (0.0, 0.0) } else { ((fx - tx) / span, (fy - ty) / span) }
}


/// An edge label's measured width (Trebuchet, one line; wider ones wrap in mmdc).
pub(super) fn label_width(text: &str) -> f64 {
    text::width(text, FONT_SIZE, Face::Trebuchet)
}
