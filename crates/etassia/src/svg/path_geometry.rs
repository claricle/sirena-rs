//! Port of `Etassia::Svg::PathGeometry`: where a path starts and ends and its
//! heading there. Arcs yield no heading (their chord is not a tangent).

/// A point on the path and the unit direction of travel through it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
}

type P = [f64; 2];

#[derive(Clone, Copy)]
enum Origin {
    Unset,
    Blocked,
    Set(P, P),
}

#[derive(Debug)]
pub struct PathGeometry {
    point: P,
    subpath_start: P,
    origin: Origin,
    terminus: Option<(P, P)>,
    origin_direction: Option<(P, P)>,
    recent_direction: Option<(P, P)>,
    quadratic_control: Option<P>,
    cubic_control: Option<P>,
}

impl std::fmt::Debug for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Unset => write!(f, "Unset"),
            Origin::Blocked => write!(f, "Blocked"),
            Origin::Set(a, b) => write!(f, "Set({a:?}, {b:?})"),
        }
    }
}

fn arity(lower: u8) -> usize {
    match lower {
        b'm' | b'l' | b't' => 2,
        b'h' | b'v' => 1,
        b'c' => 6,
        b's' | b'q' => 4,
        b'a' => 7,
        _ => 0, // z
    }
}

fn is_command(b: u8) -> bool {
    matches!(b.to_ascii_lowercase(), b'm' | b'l' | b'h' | b'v' | b'c' | b's' | b'q' | b't' | b'a' | b'z')
}

/// End index of `[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?` starting at `i`.
fn match_number(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
        j += 1;
    }
    let digits = |mut k: usize| {
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        k
    };
    let int_end = digits(j);
    if int_end > j {
        j = int_end;
        if j < b.len() && b[j] == b'.' {
            j += 1;
            j = digits(j);
        }
    } else if j < b.len() && b[j] == b'.' {
        let f = digits(j + 1);
        if f == j + 1 {
            return None;
        }
        j = f;
    } else {
        return None;
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        let mut k = j + 1;
        if k < b.len() && (b[k] == b'-' || b[k] == b'+') {
            k += 1;
        }
        let e = digits(k);
        if e > k {
            j = e;
        }
    }
    Some(j)
}

enum Tok {
    Cmd(u8),
    Num(f64),
}

fn scan_token(b: &[u8], pos: &mut usize, flag: bool) -> Option<Tok> {
    let mut i = *pos;
    while i < b.len() {
        if is_command(b[i]) {
            *pos = i + 1;
            return Some(Tok::Cmd(b[i]));
        }
        if flag {
            if b[i] == b'0' || b[i] == b'1' {
                *pos = i + 1;
                return Some(Tok::Num((b[i] - b'0') as f64));
            }
        } else if let Some(end) = match_number(b, i) {
            *pos = end;
            let s = std::str::from_utf8(&b[i..end]).unwrap();
            return Some(Tok::Num(s.parse::<f64>().unwrap_or(0.0)));
        }
        i += 1;
    }
    None
}

fn flush(letter: Option<u8>, numbers: &[f64], out: &mut Vec<(u8, Vec<f64>)>) {
    let Some(letter) = letter else { return };
    let size = arity(letter.to_ascii_lowercase());
    if size == 0 {
        out.push((letter, vec![]));
        return;
    }
    for (index, group) in numbers.chunks(size).enumerate() {
        if group.len() < size {
            break;
        }
        let l = if index == 0 {
            letter
        } else {
            match letter {
                b'M' => b'L',
                b'm' => b'l',
                other => other,
            }
        };
        out.push((l, group.to_vec()));
    }
}

fn commands(data: &str) -> Vec<(u8, Vec<f64>)> {
    let b = data.as_bytes();
    let mut out = vec![];
    let mut letter: Option<u8> = None;
    let mut numbers: Vec<f64> = vec![];
    let mut pos = 0;
    loop {
        let flag = letter.map(|l| l.to_ascii_lowercase()) == Some(b'a') && (3..=4).contains(&(numbers.len() % 7));
        match scan_token(b, &mut pos, flag) {
            None => break,
            Some(Tok::Num(n)) => numbers.push(n),
            Some(Tok::Cmd(c)) => {
                flush(letter, &numbers, &mut out);
                letter = Some(c);
                numbers = vec![];
            }
        }
    }
    flush(letter, &numbers, &mut out);
    out
}

fn direction(from: P, to: P) -> P {
    let d = [to[0] - from[0], to[1] - from[1]];
    if d.iter().all(|v| v.is_finite()) {
        return d;
    }
    [to[0] / 2.0 - from[0] / 2.0, to[1] / 2.0 - from[1] / 2.0]
}

fn heading(at: P, from: P, to: P) -> Option<Anchor> {
    let [dx, dy] = direction(from, to);
    let length = crate::js_math::hypot2(dx, dy);
    if !(at[0].is_finite() && at[1].is_finite() && length.is_finite() && length > 0.0) {
        return None;
    }
    Some(Anchor { x: at[0], y: at[1], dx: dx / length, dy: dy / length })
}

impl PathGeometry {
    /// Walks the `d` attribute (lenient: junk bytes skipped, short groups dropped).
    pub fn new(data: &str) -> Self {
        let mut g = PathGeometry {
            point: [0.0, 0.0],
            subpath_start: [0.0, 0.0],
            origin: Origin::Unset,
            terminus: None,
            origin_direction: None,
            recent_direction: None,
            quadratic_control: None,
            cubic_control: None,
        };
        for (letter, args) in commands(data) {
            g.apply(letter, args);
        }
        g
    }

    /// The first point, pointing along the first segment.
    pub fn origin(&self) -> Option<Anchor> {
        let Origin::Set(start, reference) = self.origin else { return None };
        heading(start, start, reference)
            .or_else(|| self.origin_direction.and_then(|(f, t)| heading(start, f, t)))
    }

    /// The last point, pointing along the last segment.
    pub fn terminus(&self) -> Option<Anchor> {
        let (reference, finish) = self.terminus?;
        heading(finish, reference, finish)
            .or_else(|| self.recent_direction.and_then(|(f, t)| heading(finish, f, t)))
    }

    fn apply(&mut self, letter: u8, args: Vec<f64>) {
        let lower = letter.to_ascii_lowercase();
        let args = self.relative(letter, lower, args);
        match lower {
            b'q' | b't' => self.cubic_control = None,
            b'c' | b's' => self.quadratic_control = None,
            _ => {
                self.quadratic_control = None;
                self.cubic_control = None;
            }
        }
        match lower {
            b'm' => self.move_to(&args),
            b'z' => self.segment(self.subpath_start, self.point, self.subpath_start),
            b'a' => self.arc(&args),
            _ => {
                let (o, i, e) = self.references(lower, &args);
                self.segment(o, i, e);
            }
        }
    }

    fn relative(&self, letter: u8, lower: u8, args: Vec<f64>) -> Vec<f64> {
        if letter.is_ascii_uppercase() {
            return args;
        }
        let pts = |a: &[f64]| -> Vec<f64> {
            a.chunks(2).flat_map(|c| [c[0] + self.point[0], c[1] + self.point[1]]).collect()
        };
        match lower {
            b'h' => vec![args[0] + self.point[0]],
            b'v' => vec![args[0] + self.point[1]],
            b'a' => {
                let mut v = args[..5].to_vec();
                v.extend(pts(&args[5..]));
                v
            }
            _ => pts(&args),
        }
    }

    fn arc(&mut self, a: &[f64]) {
        let end = [a[5], a[6]];
        if end == self.point {
            return;
        }
        if a[0] == 0.0 || a[1] == 0.0 {
            let (o, i, e) = self.straight(end);
            return self.segment(o, i, e);
        }
        if self.origin_direction.is_none() {
            self.origin = Origin::Blocked;
        }
        self.terminus = None;
        self.recent_direction = None;
        self.point = end;
    }

    fn straight(&self, dest: P) -> (P, P, P) {
        (dest, self.point, dest)
    }

    fn reflection(&self, control: Option<P>) -> P {
        let s = control.unwrap_or(self.point);
        [self.point[0] + (self.point[0] - s[0]), self.point[1] + (self.point[1] - s[1])]
    }

    fn references(&mut self, lower: u8, a: &[f64]) -> (P, P, P) {
        match lower {
            b'l' => self.straight([a[0], a[1]]),
            b'h' => self.straight([a[0], self.point[1]]),
            b'v' => self.straight([self.point[0], a[0]]),
            b'c' => {
                self.cubic_control = Some([a[2], a[3]]);
                ([a[0], a[1]], [a[2], a[3]], [a[4], a[5]])
            }
            b's' => {
                let out = self.reflection(self.cubic_control);
                self.cubic_control = Some([a[0], a[1]]);
                (out, [a[0], a[1]], [a[2], a[3]])
            }
            b'q' => {
                self.quadratic_control = Some([a[0], a[1]]);
                ([a[0], a[1]], [a[0], a[1]], [a[2], a[3]])
            }
            _ => {
                // t
                let c = self.reflection(self.quadratic_control);
                self.quadratic_control = Some(c);
                (c, c, [a[0], a[1]])
            }
        }
    }

    fn move_to(&mut self, a: &[f64]) {
        if matches!(self.origin, Origin::Set(..)) && self.origin_direction.is_none() {
            self.origin = Origin::Blocked;
        }
        self.point = [a[0], a[1]];
        self.subpath_start = self.point;
        self.terminus = None;
        self.recent_direction = None;
    }

    fn segment(&mut self, out_ref: P, in_ref: P, end: P) {
        if matches!(self.origin, Origin::Unset) {
            self.origin = Origin::Set(self.point, out_ref);
        }
        let pts = [self.point, out_ref, in_ref, end];
        let dirs: Vec<(P, P)> =
            pts.windows(2).map(|w| (w[0], w[1])).filter(|&(f, t)| heading(t, f, t).is_some()).collect();
        if let (Some(first), Some(last)) = (dirs.first(), dirs.last()) {
            if self.origin_direction.is_none() {
                self.origin_direction = Some(*first);
            }
            self.recent_direction = Some(*last);
        }
        self.terminus = Some((in_ref, end));
        self.point = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(x: Option<Anchor>) -> Option<[f64; 4]> {
        x.map(|a| [a.x, a.y, a.dx, a.dy])
    }

    #[test]
    fn line() {
        let g = PathGeometry::new("M 0 0 L 10 0");
        assert_eq!(a(g.origin()), Some([0.0, 0.0, 1.0, 0.0]));
        assert_eq!(a(g.terminus()), Some([10.0, 0.0, 1.0, 0.0]));
    }

    #[test]
    fn cubic_and_relative() {
        // Ruby: origin [0,0,0,1]; terminus [20,0,0.7071067811865475,-0.7071067811865475]
        let g = PathGeometry::new("M0,0 c 0,10 10,10 20,0");
        assert_eq!(a(g.origin()), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(a(g.terminus()), Some([20.0, 0.0, 0.7071067811865475, -0.7071067811865475]));
    }

    #[test]
    fn arc_has_no_heading_and_empty_is_none() {
        let g = PathGeometry::new("M 0 0 A 5 5 0 0 1 10 0");
        assert_eq!(g.origin(), None);
        assert_eq!(g.terminus(), None);
        assert_eq!(PathGeometry::new("").terminus(), None);
    }

    #[test]
    fn degenerate_arc_is_ignored() {
        let g = PathGeometry::new("M 0 0 L 10 0 A 5 5 0 0 1 10 0");
        assert_eq!(a(g.terminus()), Some([10.0, 0.0, 1.0, 0.0]));
    }

    #[test]
    fn implicit_lineto_and_flags_without_separators() {
        let g = PathGeometry::new("M 0 0 10 0 0 10");
        assert_eq!(a(g.terminus()), Some([0.0, 10.0, -0.7071067811865475, 0.7071067811865475]));
        let g = PathGeometry::new("M 0 0 L 5 0 a5 5 0 1110 0 L 30 0");
        assert_eq!(a(g.terminus()), Some([30.0, 0.0, 1.0, 0.0]));
    }
}
