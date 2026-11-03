//! Correctly rounded sin/cos mirroring the Ruby mindmap renderer, which uses its own
//! 60-digit series rather than libm. Everything else calls the platform libm exactly
//! where Ruby calls Math.*, so Rust == Ruby on the same machine.
//! Double-double arithmetic, rounded once at the end.

#[derive(Clone, Copy)]
struct Dd(f64, f64);

fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

fn quick(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd(s, b - (s - a))
}

fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

fn dd_add(a: Dd, b: Dd) -> Dd {
    let (s1, s2) = two_sum(a.0, b.0);
    let (t1, t2) = two_sum(a.1, b.1);
    let s2 = s2 + t1;
    let Dd(s1, s2) = quick(s1, s2);
    let s2 = s2 + t2;
    quick(s1, s2)
}

fn dd_mul(a: Dd, b: Dd) -> Dd {
    let (p1, p2) = two_prod(a.0, b.0);
    quick(p1, p2 + (a.0 * b.1 + a.1 * b.0))
}

fn dd_neg(a: Dd) -> Dd {
    Dd(-a.0, -a.1)
}

fn dd_div_small(a: Dd, d: f64) -> Dd {
    let q1 = a.0 / d;
    let (p, e) = two_prod(q1, d);
    let r = dd_add(a, Dd(-p, -e));
    let q2 = r.0 / d;
    let (p, e) = two_prod(q2, d);
    let r = dd_add(r, Dd(-p, -e));
    let q3 = r.0 / d;
    let Dd(s, t) = quick(q1, q2);
    dd_add(Dd(s, t), Dd(q3, 0.0))
}

fn reduce(angle: f64) -> (i64, Dd) {
    let half_pi_1 = f64::from_bits(0x3FF921FB54442D18);
    let half_pi_2 = f64::from_bits(0x3C91A62633145C07);
    let half_pi_3 = f64::from_bits(0xB91F1976B7ED8FBC);
    let k = (angle / half_pi_1).round();
    let mut r = Dd(angle, 0.0);
    let (p, e) = two_prod(k, half_pi_1);
    r = dd_add(r, Dd(-p, -e));
    let (p, e) = two_prod(k, half_pi_2);
    r = dd_add(r, Dd(-p, -e));
    r = dd_add(r, Dd(-(k * half_pi_3), 0.0));
    (k as i64, r)
}

fn sin_series(r: Dd) -> Dd {
    let x2 = dd_mul(r, r);
    let (mut sum, mut term) = (r, r);
    for i in 1..40 {
        let d = ((2 * i) * (2 * i + 1)) as f64;
        term = dd_neg(dd_div_small(dd_mul(term, x2), d));
        if term.0 == 0.0 {
            break;
        }
        sum = dd_add(sum, term);
        if term.0.abs() < 1e-40 * sum.0.abs().max(1e-300) {
            break;
        }
    }
    sum
}

fn cos_series(r: Dd) -> Dd {
    let x2 = dd_mul(r, r);
    let (mut sum, mut term) = (Dd(1.0, 0.0), Dd(1.0, 0.0));
    for i in 1..40 {
        let d = ((2 * i - 1) * (2 * i)) as f64;
        term = dd_neg(dd_div_small(dd_mul(term, x2), d));
        if term.0 == 0.0 {
            break;
        }
        sum = dd_add(sum, term);
        if term.0.abs() < 1e-40 * sum.0.abs() {
            break;
        }
    }
    sum
}

fn finish(v: Dd) -> f64 {
    let x = v.0 + v.1;
    if x == 0.0 { 0.0 } else { x }
}

pub fn sin(radians: f64) -> f64 {
    let (k, r) = reduce(radians);
    match k.rem_euclid(4) {
        0 => finish(sin_series(r)),
        1 => finish(cos_series(r)),
        2 => finish(dd_neg(sin_series(r))),
        _ => finish(dd_neg(cos_series(r))),
    }
}

pub fn cos(radians: f64) -> f64 {
    let (k, r) = reduce(radians);
    match k.rem_euclid(4) {
        0 => finish(cos_series(r)),
        1 => finish(dd_neg(sin_series(r))),
        2 => finish(dd_neg(cos_series(r))),
        _ => finish(sin_series(r)),
    }
}
