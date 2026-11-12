//! Ruby `Integer`/`Float` arithmetic: the sequence layout interpolates some
//! numbers straight into strings (`points`, `d`), where `125` and `125.0`
//! differ, so every layout value keeps its Ruby type.
use crate::svg::ruby_float;
use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug)]
pub enum N {
    I(i64),
    F(f64),
}

pub fn i(v: i64) -> N {
    N::I(v)
}

pub fn fl(v: f64) -> N {
    N::F(v)
}

impl N {
    pub fn f(self) -> f64 {
        match self {
            N::I(v) => v as f64,
            N::F(v) => v,
        }
    }

    /// Ruby `to_s`.
    pub fn show(self) -> String {
        match self {
            N::I(v) => v.to_string(),
            N::F(v) => ruby_float(v),
        }
    }

    /// `self / 2` (floor division for an Integer).
    pub fn half(self) -> N {
        match self {
            N::I(v) => N::I(v.div_euclid(2)),
            N::F(v) => N::F(v / 2.0),
        }
    }

    /// `[self, other].max` (the first wins a tie).
    pub fn max(self, o: N) -> N {
        if o.f() > self.f() { o } else { self }
    }


    pub fn abs(self) -> N {
        match self {
            N::I(v) => N::I(v.abs()),
            N::F(v) => N::F(v.abs()),
        }
    }

    /// `self <=> other` as -1, 0 or 1.
    pub fn cmp(self, o: N) -> i64 {
        let (a, b) = (self.f(), o.f());
        if a > b { 1 } else if a < b { -1 } else { 0 }
    }

    pub fn eq(self, o: N) -> bool {
        self.f() == o.f()
    }

    pub fn is_zero(self) -> bool {
        self.f() == 0.0
    }
}

macro_rules! op {
    ($tr:ident, $m:ident, $o:tt) => {
        impl $tr<N> for N {
            type Output = N;
            fn $m(self, r: N) -> N {
                match (self, r) {
                    (N::I(a), N::I(b)) => N::I(a $o b),
                    (a, b) => N::F(a.f() $o b.f()),
                }
            }
        }
        impl $tr<i64> for N {
            type Output = N;
            fn $m(self, r: i64) -> N {
                $tr::$m(self, N::I(r))
            }
        }
    };
}
op!(Add, add, +);
op!(Sub, sub, -);
op!(Mul, mul, *);

impl Neg for N {
    type Output = N;
    fn neg(self) -> N {
        match self {
            N::I(v) => N::I(-v),
            N::F(v) => N::F(-v),
        }
    }
}

/// Ruby `Array#sum` (integers exact, Kahan-Babuska once a Float appears).
pub fn nsum(items: impl IntoIterator<Item = N>) -> N {
    let mut total: i64 = 0;
    let mut it = items.into_iter();
    let mut first_float = None;
    for x in it.by_ref() {
        match x {
            N::I(v) => total += v,
            f => {
                first_float = Some(f);
                break;
            }
        }
    }
    let Some(first) = first_float else { return N::I(total) };
    let (mut f, mut c) = (total as f64, 0.0f64);
    for x in std::iter::once(first).chain(it) {
        let x = x.f();
        let t = f + x;
        if !t.is_finite() {
            f = t;
            c = 0.0;
            continue;
        }
        if f.abs() >= x.abs() { c += (f - t) + x } else { c += (x - t) + f }
        f = t;
    }
    N::F(f + c)
}
