//! JavaScript's `Math` transcendentals, correctly rounded: the double nearest the true
//! value, identical on every OS and in every sirena runtime (native, wasm, Python).
//! Current V8 returns the correctly rounded value for sin, cos, atan, atan2, log and
//! log10 (measured on Chrome 156), so this is also the closest match to mermaid.
//!
//! Everything is exact integer arithmetic in 256-bit fixed point. It mirrors
//! `lib/sirena/js_math.rb` in claricle/sirena step for step: keep the two in step.
//! `hypot` is V8's own algorithm (plain IEEE arithmetic). `sqrt` is IEEE-exact and stays
//! `f64::sqrt`.

use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::sync::OnceLock;

const BITS: usize = 256;
const WIDE_BITS: usize = 1400;

fn one() -> BigInt {
    BigInt::one() << BITS
}

fn big(value: i64) -> BigInt {
    BigInt::from(value)
}

/// `Math.frexp`: fraction in [0.5, 1) and the binary exponent.
fn frexp(value: f64) -> (f64, i64) {
    let (value, bias) = if value.abs() < f64::MIN_POSITIVE {
        (value * 2f64.powi(54), -54)
    } else {
        (value, 0)
    };
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i64 - 1022;
    let fraction = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (fraction, exponent + bias)
}

fn ldexp(mut value: f64, mut exponent: i64) -> f64 {
    while exponent > 1000 {
        value *= f64::from_bits((1000u64 + 1023) << 52);
        exponent -= 1000;
    }
    while exponent < -1000 {
        value *= f64::from_bits((1023u64 - 1000) << 52);
        exponent += 1000;
    }
    value * f64::from_bits(((exponent + 1023) as u64) << 52)
}

fn to_fixed(value: f64, bits: usize) -> BigInt {
    if value == 0.0 {
        return BigInt::zero();
    }
    let (fraction, exponent) = frexp(value.abs());
    let mantissa = BigInt::from((fraction * 9007199254740992.0) as u64);
    let shift = exponent - 53 + bits as i64;
    let fixed = if shift < 0 {
        mantissa >> (-shift) as usize
    } else {
        mantissa << shift as usize
    };
    if value < 0.0 {
        -fixed
    } else {
        fixed
    }
}

/// Product, truncated toward zero.
fn fixed_mul(left: &BigInt, right: &BigInt) -> BigInt {
    let product = left * right;
    if product.is_negative() {
        -((-product) >> BITS)
    } else {
        product >> BITS
    }
}

/// Truncating division (Rust's `/` on BigInt already truncates).
fn idiv(numerator: &BigInt, divisor: &BigInt) -> BigInt {
    numerator / divisor
}

fn fixed_div(left: &BigInt, right: &BigInt) -> BigInt {
    idiv(&(left << BITS), right)
}

/// The nearest double to `fixed * 2**exponent`, ties to even.
fn to_float(fixed: &BigInt, exponent: i64) -> f64 {
    if fixed.is_zero() {
        return 0.0;
    }
    if fixed.is_negative() {
        return -to_float(&-fixed, exponent);
    }
    let length = fixed.bits() as i64;
    let excess = length - 53;
    let (kept, exponent) = if excess > 0 {
        let excess = excess as usize;
        let mut kept: BigInt = fixed >> excess;
        let rest = fixed - (&kept << excess);
        let half = BigInt::one() << (excess - 1);
        if rest > half || (rest == half && kept.is_odd()) {
            kept += 1;
        }
        (kept, exponent + excess as i64)
    } else {
        (fixed.clone(), exponent)
    };
    ldexp(kept.to_f64().unwrap_or(f64::INFINITY), exponent)
}

fn sqrt_fixed(fixed: &BigInt) -> BigInt {
    let shifted = (fixed << BITS).to_biguint().unwrap_or_default();
    BigInt::from(BigUint::sqrt(&shifted))
}

fn atan_series(small: &BigInt) -> BigInt {
    let square = fixed_mul(small, small);
    let mut term = small.clone();
    let mut sum = BigInt::zero();
    let mut divisor = 1i64;
    while !term.is_zero() {
        sum += idiv(&term, &big(divisor));
        term = -fixed_mul(&term, &square);
        divisor += 2;
    }
    sum
}

/// atan of a fixed value in [0, 1], by three argument halvings.
fn atan_unit(value: &BigInt) -> BigInt {
    let mut value = value.clone();
    for _ in 0..3 {
        let root = sqrt_fixed(&(one() + fixed_mul(&value, &value)));
        value = fixed_div(&value, &(one() + root));
    }
    atan_series(&value) << 3
}

fn atan_fixed(value: &BigInt) -> BigInt {
    if value.is_negative() {
        return -atan_fixed(&-value);
    }
    if *value <= one() {
        return atan_unit(value);
    }
    (pi_fixed() >> 1) - atan_unit(&fixed_div(&one(), value))
}

fn split_double(value: f64) -> (BigInt, i64) {
    let (fraction, exponent) = frexp(value);
    (
        BigInt::from((fraction * 9007199254740992.0) as u64),
        exponent - 53,
    )
}

/// top / bottom as an exact integer pair.
fn fraction_of(top: f64, bottom: f64) -> (BigInt, BigInt) {
    let (top_num, top_exp) = split_double(top);
    let (bottom_num, bottom_exp) = split_double(bottom);
    let shift = top_exp - bottom_exp;
    if shift < 0 {
        (top_num, bottom_num << (-shift) as usize)
    } else {
        (top_num << shift as usize, bottom_num)
    }
}

/// The nearest double to numerator / denominator (both positive).
fn integer_ratio_to_float(numerator: &BigInt, denominator: &BigInt) -> f64 {
    let shift = denominator.bits() as i64 + 60 - numerator.bits() as i64;
    let (top, bottom) = if shift > 0 {
        (numerator << shift as usize, denominator.clone())
    } else {
        (numerator.clone(), denominator << (-shift) as usize)
    };
    let (scaled, rest) = top.div_rem(&bottom);
    let sticky = if rest.is_zero() { 0 } else { 1 };
    to_float(&((scaled << 1usize) | BigInt::from(sticky)), -shift - 1)
}

fn atan_tiny_fraction(num: &BigInt, den: &BigInt) -> f64 {
    let den2 = den * den;
    let num2 = num * num;
    let inner = big(15) * &den2 * &den2 - big(5) * &num2 * &den2 + big(3) * &num2 * &num2;
    integer_ratio_to_float(&(num * inner), &(big(15) * &den2 * &den2 * den))
}

fn atan_ratio_fixed(ratio: &(BigInt, BigInt)) -> BigInt {
    atan_fixed(&idiv(&(&ratio.0 << BITS), &ratio.1))
}

fn atan_ratio(ratio: &(BigInt, BigInt)) -> f64 {
    if (&ratio.0 << 27usize) < ratio.1 {
        return atan_tiny_fraction(&ratio.0, &ratio.1);
    }
    to_float(&atan_ratio_fixed(ratio), -(BITS as i64))
}

/// Machin: pi = 16 atan(1/5) - 4 atan(1/239), at +64 guard bits.
fn pi_bits(bits: usize) -> BigInt {
    let work = bits + 64;
    let value = big(16) * arctan_inverse(5, work) - big(4) * arctan_inverse(239, work);
    value >> 64usize
}

fn arctan_inverse(divisor: i64, work: usize) -> BigInt {
    let divisor = big(divisor);
    let square = &divisor * &divisor;
    let mut power: BigInt = (BigInt::one() << work) / &divisor;
    let mut sum = BigInt::zero();
    let mut k = 0i64;
    while !power.is_zero() {
        let term = &power / big(2 * k + 1);
        if k % 2 == 0 {
            sum += term
        } else {
            sum -= term
        }
        power /= &square;
        k += 1;
    }
    sum
}

fn atanh_inverse(divisor: i64) -> BigInt {
    let divisor = big(divisor);
    let square = &divisor * &divisor;
    let mut power: BigInt = (BigInt::one() << (BITS + 64)) / &divisor;
    let mut sum = BigInt::zero();
    let mut k = 0i64;
    while !power.is_zero() {
        sum += &power / big(2 * k + 1);
        power /= &square;
        k += 1;
    }
    sum
}

fn pi_fixed() -> BigInt {
    static PI: OnceLock<BigInt> = OnceLock::new();
    PI.get_or_init(|| pi_bits(BITS)).clone()
}

fn pi_float() -> f64 {
    to_float(&pi_fixed(), -(BITS as i64))
}

fn ln2_fixed() -> BigInt {
    static LN2: OnceLock<BigInt> = OnceLock::new();
    LN2.get_or_init(|| (big(2) * atanh_inverse(3)) >> 64usize)
        .clone()
}

fn ln10_fixed() -> BigInt {
    static LN10: OnceLock<BigInt> = OnceLock::new();
    LN10.get_or_init(|| (big(6) * atanh_inverse(3) + big(2) * atanh_inverse(9)) >> 64usize)
        .clone()
}

/// ln(x) for finite x > 0, as a fixed value.
fn ln_fixed(x: f64) -> BigInt {
    let (mut fraction, mut exponent) = frexp(x);
    if fraction < std::f64::consts::FRAC_1_SQRT_2 {
        fraction *= 2.0;
        exponent -= 1;
    }
    let mantissa = to_fixed(fraction, BITS);
    let ratio = fixed_div(&(&mantissa - one()), &(&mantissa + one()));
    let square = fixed_mul(&ratio, &ratio);
    let mut term = ratio;
    let mut sum = BigInt::zero();
    let mut divisor = 1i64;
    while !term.is_zero() {
        sum += idiv(&term, &big(divisor));
        term = fixed_mul(&term, &square);
        divisor += 2;
    }
    big(exponent) * ln2_fixed() + (sum << 1usize)
}

/// exp of a fixed value, as (fixed mantissa, binary exponent).
fn exp_fixed(value: &BigInt) -> (BigInt, i64) {
    let ln2 = ln2_fixed();
    let twice = (value << 1usize) + &ln2;
    let steps = twice.div_floor(&(&ln2 << 1usize));
    let reduced: BigInt = (value - &steps * &ln2) >> 8usize;
    let mut term = one();
    let mut sum = one();
    let mut index = 1i64;
    while !term.is_zero() {
        term = idiv(&fixed_mul(&term, &reduced), &big(index));
        sum += &term;
        index += 1;
    }
    for _ in 0..8 {
        sum = fixed_mul(&sum, &sum);
    }
    (sum, steps.to_i64().unwrap_or(0))
}

fn odd_integer(value: f64) -> bool {
    value.abs() < 9007199254740992.0 && value == value.trunc() && (value as i64) % 2 != 0
}

fn pow_positive(x: f64, y: f64) -> f64 {
    let power = fixed_mul(&ln_fixed(x), &to_fixed(y, BITS));
    if power > big(710) * one() {
        return f64::INFINITY;
    }
    if power < big(-746) * one() {
        return 0.0;
    }
    let (mantissa, steps) = exp_fixed(&power);
    to_float(&mantissa, steps - BITS as i64)
}

fn pow_special(x: f64, y: f64) -> f64 {
    if y.is_infinite() {
        return if (x.abs() > 1.0) == (y > 0.0) {
            f64::INFINITY
        } else {
            0.0
        };
    }
    let negative_odd = (if x == 0.0 {
        x.is_sign_negative()
    } else {
        x < 0.0
    }) && odd_integer(y);
    let huge = if x == 0.0 { y < 0.0 } else { y > 0.0 };
    let value = if huge { f64::INFINITY } else { 0.0 };
    if negative_odd {
        -value
    } else {
        value
    }
}

/// sin (kind 0) or cos (kind 1) of a double with |x| >= 2**-27.
fn trig(x: f64, kind: usize) -> f64 {
    let bits = if x.abs() < 1048576.0 { BITS } else { WIDE_BITS };
    let pio2 = pi_bits_cached(bits) >> 1usize;
    let wide = to_fixed(x, bits);
    let turns = ((&wide << 1usize) + &pio2).div_floor(&(&pio2 << 1usize));
    let remainder: BigInt = (&wide - &turns * &pio2) >> (bits - BITS);
    let quadrant = turns.mod_floor(&big(4)).to_usize().unwrap_or(0);
    // (sine?, sign) per quadrant
    let (sine, sign) = if kind == 0 {
        [(true, 1), (false, 1), (true, -1), (false, -1)][quadrant]
    } else {
        [(false, 1), (true, -1), (false, -1), (true, 1)][quadrant]
    };
    let series = if sine {
        sine_series(&remainder)
    } else {
        cosine_series(&remainder)
    };
    to_float(&(series * big(sign)), -(BITS as i64))
}

fn pi_bits_cached(bits: usize) -> BigInt {
    if bits == BITS {
        return pi_fixed();
    }
    static WIDE: OnceLock<BigInt> = OnceLock::new();
    WIDE.get_or_init(|| pi_bits(WIDE_BITS)).clone()
}

fn sine_series(small: &BigInt) -> BigInt {
    let square = fixed_mul(small, small);
    let mut term = small.clone();
    let mut sum = BigInt::zero();
    let mut index = 1i64;
    while !term.is_zero() {
        sum += &term;
        term = idiv(&-fixed_mul(&term, &square), &big((index + 1) * (index + 2)));
        index += 2;
    }
    sum
}

fn cosine_series(small: &BigInt) -> BigInt {
    let square = fixed_mul(small, small);
    let mut term = one();
    let mut sum = BigInt::zero();
    let mut index = 0i64;
    while !term.is_zero() {
        sum += &term;
        term = idiv(&-fixed_mul(&term, &square), &big((index + 1) * (index + 2)));
        index += 2;
    }
    sum
}

/// `Math.sin`.
pub fn sin(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    if x.abs() < 7.450580596923828e-9 {
        return x;
    }
    trig(x, 0)
}

/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    if x.abs() < 7.450580596923828e-9 {
        return 1.0;
    }
    trig(x, 1)
}

/// `Math.atan`.
pub fn atan(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        return x;
    }
    let magnitude = x.abs();
    let result = if magnitude < 1.4901161193847656e-8 {
        magnitude
    } else {
        to_float(&atan_fixed(&to_fixed(magnitude, BITS)), -(BITS as i64))
    };
    if x < 0.0 {
        -result
    } else {
        result
    }
}

/// `Math.atan2(y, x)`.
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 || x == 0.0 || x.is_infinite() || y.is_infinite() {
        return atan2_special(y, x);
    }
    let sign = if y < 0.0 { -1.0 } else { 1.0 };
    let ratio = fraction_of(y.abs(), x.abs());
    let angle = if x < 0.0 {
        to_float(&(pi_fixed() - atan_ratio_fixed(&ratio)), -(BITS as i64))
    } else {
        atan_ratio(&ratio)
    };
    sign * angle
}

fn atan2_special(y: f64, x: f64) -> f64 {
    if y == 0.0 {
        if x > 0.0 || (x == 0.0 && !x.is_sign_negative()) {
            return y;
        }
        return if y.is_sign_negative() {
            -pi_float()
        } else {
            pi_float()
        };
    }
    let sign = if y < 0.0 { -1.0 } else { 1.0 };
    if x == 0.0 {
        return sign * pi_float() / 2.0;
    }
    if y.is_infinite() {
        if !x.is_infinite() {
            return sign * pi_float() / 2.0;
        }
        return sign
            * if x > 0.0 {
                pi_float() / 4.0
            } else {
                3.0 * (pi_float() / 4.0)
            };
    }
    if x > 0.0 {
        sign * 0.0
    } else {
        sign * pi_float()
    }
}

/// `Math.hypot`: V8 scales by the largest magnitude and sums the squares with Kahan
/// compensation (builtins/math.tq).
pub fn hypot(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let abs: Vec<f64> = values.iter().map(|v| v.abs()).collect();
    let max = abs
        .iter()
        .copied()
        .filter(|v| !v.is_nan())
        .fold(0.0, f64::max);
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if values.iter().any(|v| v.is_nan()) {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let (mut sum, mut compensation) = (0.0f64, 0.0f64);
    for value in abs {
        let n = value / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `Math.hypot(x, y)`.
pub fn hypot2(x: f64, y: f64) -> f64 {
    hypot(&[x, y])
}

/// `Math.log`.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() || x == 1.0 {
        return if x == 1.0 { 0.0 } else { x };
    }
    to_float(&ln_fixed(x), -(BITS as i64))
}

/// `Math.log10`.
pub fn log10(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() || x == 1.0 {
        return if x == 1.0 { 0.0 } else { x };
    }
    to_float(&fixed_div(&ln_fixed(x), &ln10_fixed()), -(BITS as i64))
}

/// `Math.pow` / `**`.
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() || (y.is_infinite() && x.abs() == 1.0) {
        return f64::NAN;
    }
    if x == 0.0 || x.is_infinite() || y.is_infinite() {
        return pow_special(x, y);
    }
    if x < 0.0 && odd_integer(y) {
        return -pow(-x, y);
    }
    if x < 0.0 && y != y.trunc() {
        return f64::NAN;
    }
    pow_positive(x.abs(), y)
}

/// The binary exponent of a finite non-zero double (`frexp`'s), exact where `log2().floor() + 1`
/// is not near powers of two.
pub fn binexp(x: f64) -> i32 {
    frexp(x.abs()).1 as i32
}
