//! Where an edge's line runs between two boxes: Ruby `FlowchartEdgeRouter`
//! (no layout bend points: this layout never supplies any).
use super::layout::centre;
use super::*;

pub(super) type P = (f64, f64);

const CLUSTER_CORNER: f64 = 5.0;
const TOUCHING: f64 = 1e-9;
const COINCIDENT: f64 = 0.01;
const ROUTE_EPSILON: f64 = 0.01;
const LOOP_REACH: f64 = 20.0;

fn right_of(b: &Node) -> f64 {
    b.x + b.w
}

fn bottom_of(b: &Node) -> f64 {
    b.y + b.h
}

fn near(a: P, b: P) -> bool {
    crate::js_math::hypot2(a.0 - b.0, a.1 - b.1) < COINCIDENT
}

fn r2(p: P) -> P {
    (rr(p.0, 2), rr(p.1, 2))
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// The run's points and the ends of the stretch a label sits on.
pub(super) fn route(source: &Node, target: &Node) -> (Vec<P>, (f64, f64, f64, f64)) {
    let mut points = initial_route(source, target);
    let (sn, tn) = (points[1], points[points.len() - 2]);
    let last = points.len() - 1;
    points[0] = clamp_cluster_corner(points[0], sn, source);
    points[last] = clamp_cluster_corner(points[last], tn, target);
    let label = if points.len() > 2 { &points[1..3] } else { &points[..] };
    let ends = (label[0].0, label[0].1, label[label.len() - 1].0, label[label.len() - 1].1);
    (points, ends)
}

fn initial_route(source: &Node, target: &Node) -> Vec<P> {
    if !exposed_cluster_overlap(source, target) {
        if let Some(p) = coincident_loop(source, target) {
            return p;
        }
    }
    trimmed_route(source, target)
}

fn clamp_cluster_corner(point: P, neighbour: P, b: &Node) -> P {
    if !b.cluster {
        return point;
    }
    let x_side = edge_side(point.0, [b.x, right_of(b)]);
    let y_side = edge_side(point.1, [b.y, bottom_of(b)]);
    if x_side.is_none() && y_side.is_none() {
        return point;
    }
    let (x, y, w, h) = (b.x.trunc(), b.y.trunc(), b.w.trunc(), b.h.trunc());
    let (xs, ys) = ([x, x + w], [y, y + h]);
    let (rx, ry) = (CLUSTER_CORNER.min(w / 2.0), CLUSTER_CORNER.min(h / 2.0));
    if rx == 0.0 || ry == 0.0 {
        return point;
    }
    match (x_side, y_side) {
        (Some(xi), Some(yi)) => rounded_corner(neighbour, xs, ys, rx, ry, xi, yi),
        (Some(xi), None) => (xs[xi], clamp(point.1, ys[0] + ry, ys[1] - ry)),
        (None, Some(yi)) => (clamp(point.0, xs[0] + rx, xs[1] - rx), ys[yi]),
        (None, None) => point,
    }
}

fn edge_side(coordinate: f64, edges: [f64; 2]) -> Option<usize> {
    let side = if (coordinate - edges[1]).abs() < (coordinate - edges[0]).abs() { 1 } else { 0 };
    ((coordinate - edges[side]).abs() <= ROUTE_EPSILON).then_some(side)
}

fn rounded_corner(neighbour: P, xs: [f64; 2], ys: [f64; 2], rx: f64, ry: f64, xi: usize, yi: usize) -> P {
    let corner = (xs[xi], ys[yi]);
    let centre = (if xi == 0 { xs[0] + rx } else { xs[1] - rx }, if yi == 0 { ys[0] + ry } else { ys[1] - ry });
    let direction = (corner.0 - neighbour.0, corner.1 - neighbour.1);
    let (x, y) = ((corner.0 - centre.0) / rx, (corner.1 - centre.1) / ry);
    let (dx, dy) = (direction.0 / rx, direction.1 / ry);
    let a = dx * dx + dy * dy;
    if a == 0.0 {
        return corner;
    }
    let b = 2.0 * (x * dx + y * dy);
    let disc = b * b - 4.0 * a;
    if disc < 0.0 {
        return corner;
    }
    let roots = [(-b - disc.sqrt()) / (2.0 * a), (-b + disc.sqrt()) / (2.0 * a)];
    match roots.iter().copied().filter(|v| *v >= 0.0).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))) {
        Some(step) => (rr(corner.0 + direction.0 * step, 2), rr(corner.1 + direction.1 * step, 2)),
        None => corner,
    }
}

fn coincident_loop(source: &Node, target: &Node) -> Option<Vec<P>> {
    let (sc, tc) = (centre(source), centre(target));
    if !near(sc, tc) {
        return None;
    }
    let (source_top, _) = vertical_outline(source, sc.1);
    let (_, target_bottom) = vertical_outline(target, tc.1);
    let start = (sc.0, source_top);
    let finish = (tc.0, target_bottom);
    let out = right_of(source).max(right_of(target)) + LOOP_REACH;
    let mut bend_y = finish.1;
    if near(start, finish) {
        bend_y += LOOP_REACH;
    }
    Some(vec![r2(start), r2((out, start.1)), r2((out, bend_y)), r2(finish)])
}

fn vertical_outline(b: &Node, centre_y: f64) -> (f64, f64) {
    if !matches!(b.shape, Shape::Circle | Shape::DoubleCircle) {
        return (b.y, b.y + b.h);
    }
    let radius = b.w.min(b.h) / 2.0;
    (centre_y - radius, centre_y + radius)
}

fn trimmed_route(source: &Node, target: &Node) -> Vec<P> {
    let (from, to) = (centre(source), centre(target));
    let steps = (step_out(source, from, to), step_out(target, to, from));
    if exposed_cluster_overlap(source, target) {
        return exterior_route(source, target, from, to);
    }
    if steps.0 >= 1.0 - TOUCHING || steps.1 >= 1.0 - TOUCHING {
        return enclosing_route(source, target, from, to);
    }
    if steps.0 + steps.1 >= 1.0 - TOUCHING {
        return exterior_route(source, target, from, to);
    }
    let route = vec![along(from, to, steps.0), along(to, from, steps.1)];
    if route[0] == route[1] {
        return exterior_route(source, target, from, to);
    }
    route
}

fn exposed_cluster_overlap(s: &Node, t: &Node) -> bool {
    if !(s.cluster && t.cluster) || contains(s, t) || contains(t, s) {
        return false;
    }
    s.x <= right_of(t) && right_of(s) >= t.x && s.y <= bottom_of(t) && bottom_of(s) >= t.y
}

fn contains(outer: &Node, inner: &Node) -> bool {
    outer.x <= inner.x && right_of(outer) >= right_of(inner) && outer.y <= inner.y && bottom_of(outer) >= bottom_of(inner)
}

fn enclosing_route(source: &Node, target: &Node, from: P, to: P) -> Vec<P> {
    let mirror = |a: P, b: P| (2.0 * a.0 - b.0, 2.0 * a.1 - b.1);
    let source_back = step_out(source, from, mirror(from, to));
    let target_back = step_out(target, to, mirror(to, from));
    vec![along(from, to, -source_back), along(to, from, -target_back)]
}

fn exterior_route(source: &Node, target: &Node, from: P, to: P) -> Vec<P> {
    let bottom = bottom_exterior(source, target, from, to);
    let right = right_exterior(source, target, from, to);
    let sideways = (to.0 - from.0).abs() >= (to.1 - from.1).abs();
    let candidates = if sideways { [bottom, right] } else { [right, bottom] };
    let rounded: Vec<Vec<P>> = candidates.iter().map(|c| c.iter().map(|p| r2(*p)).collect()).collect();
    let clear = rounded.iter().find(|r| clear_route(r, source, target)).cloned();
    clear.unwrap_or_else(|| rounded[0].clone())
}

fn clear_route(points: &[P], source: &Node, target: &Node) -> bool {
    points.windows(2).all(|w| ![source, target].iter().any(|b| crosses_face(b, w[0], w[1])))
}

fn crosses_face(b: &Node, from: P, to: P) -> bool {
    if from.0 == to.0 {
        let (low, high) = (from.1.min(to.1), from.1.max(to.1));
        from.0 > b.x && from.0 < right_of(b) && low < bottom_of(b) && high > b.y
    } else {
        let (low, high) = (from.0.min(to.0), from.0.max(to.0));
        from.1 > b.y && from.1 < bottom_of(b) && low < right_of(b) && high > b.x
    }
}

fn bottom_exterior(s: &Node, t: &Node, from: P, to: P) -> Vec<P> {
    let rightwards = to.0 > from.0;
    let source_x = if rightwards { s.x } else { right_of(s) };
    let target_x = if rightwards { right_of(t) } else { t.x };
    let (source_y, target_y) = (bottom_of(s), bottom_of(t));
    let outside_y = source_y.max(target_y) + LOOP_REACH;
    vec![(source_x, source_y), (source_x, outside_y), (target_x, outside_y), (target_x, target_y)]
}

fn right_exterior(s: &Node, t: &Node, from: P, to: P) -> Vec<P> {
    let downwards = to.1 > from.1;
    let (source_x, target_x) = (right_of(s), right_of(t));
    let source_y = if downwards { s.y } else { bottom_of(s) };
    let target_y = if downwards { bottom_of(t) } else { t.y };
    let outside_x = source_x.max(target_x) + LOOP_REACH;
    vec![(source_x, source_y), (outside_x, source_y), (outside_x, target_y), (target_x, target_y)]
}

/// How far along the ray to the other centre this box's border sits.
fn step_out(b: &Node, from: P, to: P) -> f64 {
    if !b.cluster {
        return 0.0;
    }
    let axis = |origin: f64, target: f64, near: f64, size: f64| -> Option<f64> {
        let delta = target - origin;
        if delta == 0.0 {
            return None;
        }
        let edge = if delta > 0.0 { near + size } else { near };
        Some((edge - origin) / delta)
    };
    let steps: Vec<f64> = [axis(from.0, to.0, b.x, b.w), axis(from.1, to.1, b.y, b.h)].into_iter().flatten().collect();
    match steps.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))) {
        None => 0.0,
        Some(m) => m.max(0.0),
    }
}

fn along(from: P, to: P, step: f64) -> P {
    if step == 0.0 {
        return from;
    }
    (rr(from.0 + (to.0 - from.0) * step, 2), rr(from.1 + (to.1 - from.1) * step, 2))
}
