//! Flowchart SVG rendering.
use super::*;

pub(super) const PAGE_MARGIN: f64 = 8.0;
pub(super) const EDGE_LABEL_LIFT: f64 = 5.0;
pub(super) const MIDDLE_BASELINE_EMS: f64 = 0.35;
pub(super) const ARROW_LENGTH: f64 = 8.0;
pub(super) const ARROW_HALF_WIDTH: f64 = 4.0;
pub(super) const CIRCLE_HEAD_RADIUS: f64 = 5.5;
pub(super) const CROSS_HEAD_HALF: f64 = 4.5;
pub(super) const CIRCLE_HEAD_REACH: f64 = 6.6;
pub(super) const CROSS_HEAD_REACH: f64 = 6.5;
pub(super) const COINCIDENT: f64 = 0.01;

pub(super) fn fmt1(p: (f64, f64)) -> String {
    format!("{},{}", ruby_float(rr(p.0, 1)), ruby_float(rr(p.1, 1)))
}

pub(super) fn heads(kind: &str, route: (f64, f64, f64, f64), bends: &[(f64, f64)], s: &Node, t: &Node, ink: &str) -> Vec<Elem> {
    let both = kind.ends_with("_both");
    let head_type = kind.trim_start_matches("thick_").trim_start_matches("dotted_").trim_end_matches("_both");
    let shape = match head_type {
        "arrow" | "cross" | "circle" => head_type,
        _ => return vec![],
    };
    let (sx, sy, tx, ty) = route;
    let mut out = vec![];
    if both {
        // the source end: boundary of `s` seen from the first bend, else the route's target end
        let from = bends.first().copied().unwrap_or((tx, ty));
        let tip = if s.cluster { (sx, sy) } else { node_boundary(s, from.0, from.1) };
        out.extend(head_elem(shape, tip, from, ink));
    }
    // the target end: boundary of `t` seen from the last bend, else the route's source end
    let from = bends.last().copied().unwrap_or((sx, sy));
    let tip = if t.cluster { (tx, ty) } else { node_boundary(t, from.0, from.1) };
    out.extend(head_elem(shape, tip, from, ink));
    out
}

/// An edge label's text and the backdrop behind it (`LabelBackdropGeometry`).
pub(super) fn label_elems(text: &str, dim: (f64, f64), route: (f64, f64, f64, f64), theme: &Theme) -> Vec<Elem> {
    let (sx, sy, tx, ty) = route;
    let (x, y) = ((sx + tx) / 2.0, ((sy + ty) / 2.0) - EDGE_LABEL_LIFT);
    let (width, height) = dim;
    let pad = FONT_SIZE / 8.0;
    let top = y - (height / 2.0) - (MIDDLE_BASELINE_EMS * FONT_SIZE);
    let backdrop = Elem::new("rect")
        .attr("fill", theme.color("background").unwrap_or(""))
        .attr("stroke", "none")
        .num("x", x - (width / 2.0) - pad)
        .num("y", top - pad)
        .num("width", width + pad * 2.0)
        .num("height", height + pad * 2.0);
    let label = Elem::new("text")
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .num("x", x)
        .num("y", y)
        .attr("text-anchor", "middle")
        .attr("font-family", theme.string("typography", "font_family").unwrap_or(""))
        .attr("font-size", ruby_float(FONT_SIZE))
        .content(text);
    vec![backdrop, label]
}

pub(super) fn head_elem(shape: &str, tip: (f64, f64), from: (f64, f64), ink: &str) -> Vec<Elem> {
    let (ax, ay) = unit_towards(tip.0, tip.1, from.0, from.1);
    match shape {
        "cross" => {
            let (bx, by) = (tip.0 + ax * CROSS_HEAD_REACH, tip.1 + ay * CROSS_HEAD_REACH);
            let (mut ux, uy) = unit_towards(bx, by, from.0, from.1);
            if ux == 0.0 && uy == 0.0 {
                ux = 1.0;
            }
            let arm_x = (ux - uy) * CROSS_HEAD_HALF;
            let arm_y = (uy + ux) * CROSS_HEAD_HALF;
            let mut g = vec![];
            for (lx, ly) in [(arm_x, arm_y), (arm_y, -arm_x)] {
                g.push(
                    Elem::new("line")
                        .attr("stroke", ink)
                        .attr("stroke-width", "2")
                        .num("x1", rr(bx - lx, 1))
                        .num("y1", rr(by - ly, 1))
                        .num("x2", rr(bx + lx, 1))
                        .num("y2", rr(by + ly, 1)),
                );
            }
            g
        }
        "circle" => {
            let (cx, cy) = (tip.0 + ax * CIRCLE_HEAD_REACH, tip.1 + ay * CIRCLE_HEAD_REACH);
            vec![Elem::new("circle").attr("fill", ink).attr("stroke", ink).attr("stroke-width", "1").num("cx", rr(cx, 1)).num("cy", rr(cy, 1)).num("r", CIRCLE_HEAD_RADIUS)]
        }
        _ => {
            let (bx, by) = (tip.0 + ax * ARROW_LENGTH, tip.1 + ay * ARROW_LENGTH);
            let pts = [tip, (bx - ay * ARROW_HALF_WIDTH, by + ax * ARROW_HALF_WIDTH), (bx + ay * ARROW_HALF_WIDTH, by - ax * ARROW_HALF_WIDTH)];
            let points = pts.iter().map(|p| fmt1(*p)).collect::<Vec<_>>().join(" ");
            vec![Elem::new("polygon").attr("fill", ink).attr("stroke", ink).attr("points", points)]
        }
    }
}

pub(super) fn node_elem(n: &Node, theme: &Theme) -> Elem {
    let fill = n.fill.as_deref().or(theme.color("node_fill")).unwrap_or("");
    let stroke = n.stroke.as_deref().or(theme.color("node_stroke")).unwrap_or("");
    let sw = ruby_float(theme.number("shapes", "stroke_width").unwrap_or(2.0));
    let paint = |e: Elem| e.attr("fill", fill).attr("stroke", stroke).attr("stroke-width", sw.clone());
    let (cx, cy) = centre(n);
    let shape = match n.shape {
        Shape::Rect | Shape::Rounded | Shape::Stadium | Shape::Asymmetric | Shape::Skewed => {
            let r = paint(Elem::new("rect")).num("x", n.x).num("y", n.y).num("width", n.w).num("height", n.h);
            match n.shape {
                Shape::Rounded => r.num("rx", ROUNDED_CORNER).num("ry", ROUNDED_CORNER),
                Shape::Stadium => {
                    let c = (if n.w < n.h { n.w } else { n.h }) / 2.0;
                    r.num("rx", c).num("ry", c)
                }
                _ => r,
            }
        }
        Shape::Circle | Shape::DoubleCircle => paint(Elem::new("circle")).num("cx", cx).num("cy", cy).num("r", (if n.w < n.h { n.w } else { n.h }) / 2.0),
        Shape::Rhombus => {
            let pts = [(cx, n.y), (n.x + n.w, cy), (cx, n.y + n.h), (n.x, cy)];
            paint(Elem::new("polygon")).attr("points", pts.iter().map(|p| format!("{},{}", ruby_float(p.0), ruby_float(p.1))).collect::<Vec<_>>().join(" "))
        }
        Shape::Hexagon => {
            let q = n.w / 4.0;
            let (l, il, ir, r) = (n.x, n.x + q, n.x + n.w - q, n.x + n.w);
            let pts = [(il, n.y), (ir, n.y), (r, cy), (ir, n.y + n.h), (il, n.y + n.h), (l, cy)];
            paint(Elem::new("polygon")).attr("points", pts.iter().map(|p| format!("{},{}", ruby_float(p.0), ruby_float(p.1))).collect::<Vec<_>>().join(" "))
        }
    };
    let mut label = Elem::new("text").attr("fill", theme.color("label_text").unwrap_or("")).num("x", cx);
    if n.lines.len() == 1 {
        label = label.num("y", cy).attr("text-anchor", "middle").attr("dominant-baseline", "middle");
    } else {
        let lh = FONT_SIZE * NODE_LINE_HEIGHT * n.lines.len() as f64;
        let pitch = lh / n.lines.len() as f64;
        let first = cy - (pitch * (n.lines.len() - 1) as f64 / 2.0) + MIDDLE_BASELINE_EMS * FONT_SIZE;
        label = label.num("y", first).attr("text-anchor", "middle");
    }
    label = label.attr("font-family", theme.string("typography", "font_family").unwrap_or("")).attr("font-size", ruby_float(FONT_SIZE));
    if n.lines.len() == 1 {
        label = label.content(&n.lines[0]);
    } else {
        let pitch = FONT_SIZE * NODE_LINE_HEIGHT;
        let first = cy - (pitch * (n.lines.len() - 1) as f64 / 2.0) + MIDDLE_BASELINE_EMS * FONT_SIZE;
        let last = n.lines.len() - 1;
        for (row, line) in n.lines.iter().enumerate() {
            let content = if row < last { format!("{line} ") } else { line.clone() };
            let mut t = Elem::new("tspan").num("x", cx);
            if row > 0 {
                t = t.num("y", first + row as f64 * pitch);
            }
            label = label.child(t.content(&content));
        }
    }
    Elem::new("g").attr("id", format!("node-{}", n.id)).child(shape).child(label)
}

/// A subgraph box and its title (`Renderer::Flowchart#render_cluster`).
fn cluster_elem(n: &Node, sub: &Sub, header: f64, theme: &Theme) -> Elem {
    let sw = ruby_float(theme.number("shapes", "stroke_width").unwrap_or(2.0));
    let rect = Elem::new("rect")
        .attr("fill", theme.color("surface_variant").unwrap_or(""))
        .attr("stroke", theme.color("node_stroke").unwrap_or(""))
        .attr("stroke-width", sw)
        .num("x", n.x.trunc())
        .num("y", n.y.trunc())
        .num("width", n.w.trunc())
        .num("height", n.h.trunc())
        .num("rx", 5.0)
        .num("ry", 5.0);
    let half = sub.label.1 / 2.0;
    let mut centre = 20.0;
    if centre <= half || centre + half >= header {
        centre = header / 2.0;
    }
    let x = n.x + ((n.w.trunc() as i64) / 2) as f64;
    let title = Elem::new("text")
        .attr("fill", theme.color("label_text").unwrap_or(""))
        .num("x", x)
        .num("y", n.y + centre)
        .attr("text-anchor", "middle")
        .attr("dominant-baseline", "middle")
        .attr("font-family", theme.string("typography", "font_family").unwrap_or(""))
        .attr("font-size", ruby_float(FONT_SIZE))
        .content(&n.lines[0]);
    Elem::new("g").attr("id", format!("cluster-{}", n.id)).child(rect).child(title)
}

pub fn render(source: &str) -> Result<String, Error> {
    let mut model = parse(source)?;
    if model.nodes.is_empty() {
        let mut doc = Document::new(2.0 * PAGE_MARGIN, 2.0 * PAGE_MARGIN);
        doc.overflow = Some("hidden".into());
        return Ok(doc.to_xml());
    }
    for n in &mut model.nodes {
        if !n.cluster {
            let (w, h) = size_of(&n.lines, &n.icons, n.shape);
            n.w = w;
            n.h = h;
        }
    }
    let placed = cluster::place_all(&mut model)?;
    for e in &model.edges {
        let (a, b) = (centre(&model.nodes[e.src]), centre(&model.nodes[e.tgt]));
        if crate::js_math::hypot2(a.0 - b.0, a.1 - b.1) < COINCIDENT {
            return Err(unported("coincident centres"));
        }
    }

    let theme = Theme::default_theme();
    let ink = theme.color("edge_stroke").unwrap_or("");
    let sw = theme.number("shapes", "stroke_width").unwrap_or(2.0);
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for n in &model.nodes {
        max_x = max_x.max(n.x + n.w);
        max_y = max_y.max(n.y + n.h);
    }
    let mut reach_x = f64::NEG_INFINITY;
    let mut reach_y = f64::NEG_INFINITY;
    let mut edge_elems = vec![];
    for e in &model.edges {
        let (s, t) = (&model.nodes[e.src], &model.nodes[e.tgt]);
        let (sc, tc) = (centre(s), centre(t));
        let (route, bends, mut label_route) = if s.cluster || t.cluster {
            let (points, label) = router::route(s, t);
            let bends = if points.len() > 2 { points[1..points.len() - 1].to_vec() } else { vec![] };
            let last = points[points.len() - 1];
            ((points[0].0, points[0].1, last.0, last.1), bends, label)
        } else {
            let sb = node_boundary(s, tc.0, tc.1);
            let tb = node_boundary(t, sc.0, sc.1);
            let route = (rr(sb.0, 1), rr(sb.1, 1), rr(tb.0, 1), rr(tb.1, 1));
            (route, vec![], route)
        };
        // a crossing edge's label sits between the boxes at its own level
        let id = format!("{}_to_{}", s.id, t.id);
        if let Some(Some((from, to))) = placed.level.get(&id) {
            if (*from, *to) != (e.src, e.tgt) {
                label_route = router::route(&model.nodes[*from], &model.nodes[*to]).1;
            }
        }
        for p in [(route.0, route.1), (route.2, route.3)].iter().chain(bends.iter()) {
            reach_x = reach_x.max(p.0);
            reach_y = reach_y.max(p.1);
        }
        let mut d = format!("M {} {}", ruby_float(rr(route.0, 2)), ruby_float(rr(route.1, 2)));
        for b in &bends {
            d.push_str(&format!(" L {} {}", ruby_float(rr(b.0, 1)), ruby_float(rr(b.1, 1))));
        }
        d.push_str(&format!(" L {} {}", ruby_float(rr(route.2, 2)), ruby_float(rr(route.3, 2))));
        let mut path = Elem::new("path").attr("fill", "none").attr("stroke", ink).attr("stroke-width", ruby_float(sw)).attr("d", d);
        if e.kind.starts_with("thick_") {
            let thick = theme.number("shapes", "stroke_width_thick").unwrap_or(sw * 3.5);
            path = path.attr("stroke-width", ruby_float(thick));
        }
        if e.kind.starts_with("dotted_") {
            path = path.attr("stroke-dasharray", theme.string("shapes", "dash_pattern_dotted").unwrap_or("2"));
        }
        let mut g = Elem::new("g").attr("id", format!("edge-{}", id));
        g.push(path);
        for h in heads(&e.kind, route, &bends, s, t, ink) {
            g.push(h);
        }
        if let Some(l) = &e.label {
            for el in label_elems(l, e.label_dim, label_route, &theme) {
                g.push(el);
            }
        }
        edge_elems.push(g);
    }
    let reach = |v: f64| if v.is_finite() { v + PAGE_MARGIN } else { PAGE_MARGIN };
    let width = (max_x.max(0.0) + PAGE_MARGIN).max(reach(reach_x));
    let height = (max_y.max(0.0) + PAGE_MARGIN).max(reach(reach_y));
    let mut doc = Document::new(width, height);
    doc.overflow = Some("hidden".into());
    for &i in placed.order.iter().filter(|&&i| model.nodes[i].cluster) {
        let sub = model.subs.iter().find(|s| s.node == i).expect("cluster has a sub");
        let held: Vec<usize> = model.subs.iter().filter(|s| s.parent == Some(model.subs.iter().position(|x| x.node == i).unwrap())).map(|s| s.node).chain(sub.members.iter().copied()).collect();
        let header = held.iter().map(|&k| placed.rel_y[k]).fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.min(v)))).unwrap_or(model.nodes[i].h);
        doc.add(cluster_elem(&model.nodes[i], sub, header, &theme));
    }
    for g in edge_elems {
        doc.add(g);
    }
    for &i in placed.order.iter().filter(|&&i| !model.nodes[i].cluster) {
        doc.add(node_elem(&model.nodes[i], &theme));
    }
    Ok(doc.to_xml())
}
