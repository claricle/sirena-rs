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

fn dd_div_f64(a: Dd, n: f64) -> Dd {
    let q1 = a.0 / n;
    let (p, pe) = two_prod(q1, n);
    let rem = dd_add(a, Dd(-p, -pe));
    let q2 = (rem.0 + rem.1) / n;
    quick(q1, q2)
}

/// e^a in double-double: a = k*ln2 + r, r scaled by 2^-8, Taylor, square 8 times.
fn dd_exp(a: Dd) -> Dd {
    const LN2: Dd = Dd(0.693_147_180_559_945_3, 2.319_046_813_846_299_6e-17);
    let k = ((a.0 + a.1) / LN2.0).round();
    let r = dd_add(a, dd_mul(Dd(-k, 0.0), LN2));
    let r = Dd(r.0 / 256.0, r.1 / 256.0);
    let (mut sum, mut term) = (Dd(1.0, 0.0), Dd(1.0, 0.0));
    for n in 1..=14 {
        term = dd_div_f64(dd_mul(term, r), f64::from(n));
        sum = dd_add(sum, term);
    }
    for _ in 0..8 {
        sum = dd_mul(sum, sum);
    }
    let scale = 2f64.powi(k as i32);
    Dd(sum.0 * scale, sum.1 * scale)
}

/// `powf` as Ruby's `**`/`Math.pow` computes it (platform libm, near-correctly
/// rounded). On wasm32 it is recomputed in double-double and rounded once; other
/// targets call the platform libm.
#[cfg(not(target_arch = "wasm32"))]
pub fn powf(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[cfg(target_arch = "wasm32")]
pub fn powf(x: f64, y: f64) -> f64 {
    let approx = x.powf(y);
    if !(x.is_finite() && y.is_finite() && x > 0.0 && approx.is_finite() && approx > 1e-250 && approx < 1e250) {
        return approx;
    }
    // ln x = l0 + (x*e^-l0 - 1), one Newton step in double-double.
    let l0 = x.ln();
    let e = dd_exp(Dd(-l0, 0.0));
    let (xe, xee) = two_prod(x, e.0);
    let t = dd_add(Dd(xe, xee + x * e.1), Dd(-1.0, 0.0));
    let ln_x = dd_add(Dd(l0, 0.0), t);
    let r = dd_exp(dd_mul(ln_x, Dd(y, 0.0)));
    r.0 + r.1
}
