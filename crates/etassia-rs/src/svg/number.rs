//! Number formatting that matches the Ruby runtime byte for byte.

/// Shortest round-trip digits of a finite, non-zero `|x|` and the decimal
/// point position (`0.d1d2.. * 10^point`), trailing zeros removed.
fn decompose(x: f64) -> (String, i32) {
    let s = format!("{:e}", x.abs());
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    (digits.to_string(), exp.parse::<i32>().unwrap_or(0) + 1)
}

fn non_finite(x: f64) -> Option<String> {
    if x.is_nan() {
        Some("NaN".into())
    } else if x.is_infinite() {
        Some(if x < 0.0 { "-Infinity" } else { "Infinity" }.into())
    } else {
        None
    }
}

/// ECMA-262 `Number::toString` (Ruby `Sirena::JsNumber.stringify`):
/// `1e21` -> `"1e+21"`, `1e-5` -> `"0.00001"`, `-0.0` -> `"0"`.
pub fn fmt_num(x: f64) -> String {
    if let Some(s) = non_finite(x) {
        return s;
    }
    if x == 0.0 {
        return "0".into();
    }
    let (digits, point) = decompose(x);
    let n = digits.len() as i32;
    let body = if !(point > -6 && point <= 21) {
        let power = point - 1;
        let head = if n == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        format!("{}e{}{}", head, if power < 0 { '-' } else { '+' }, power.abs())
    } else if n <= point {
        format!("{}{}", digits, "0".repeat((point - n) as usize))
    } else if point > 0 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else {
        format!("0.{}{}", "0".repeat((-point) as usize), digits)
    };
    if x < 0.0 { format!("-{body}") } else { body }
}

/// Ruby `Float#to_s`: `10.0` -> `"10.0"`, `1e16` -> `"1.0e+16"`, `1e-5` -> `"1.0e-05"`.
pub fn ruby_float(x: f64) -> String {
    if let Some(s) = non_finite(x) {
        return s;
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if x == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, point) = decompose(x);
    let n = digits.len() as i32;
    let body = if point > 0 && point <= 15 {
        if n <= point {
            format!("{}{}.0", digits, "0".repeat((point - n) as usize))
        } else {
            format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
        }
    } else if point <= 0 && point > -4 {
        format!("0.{}{}", "0".repeat((-point) as usize), digits)
    } else {
        let rest = if n == 1 { "0" } else { &digits[1..] };
        let e = point - 1;
        format!("{}.{}e{}{:02}", &digits[..1], rest, if e < 0 { '-' } else { '+' }, e.abs())
    };
    format!("{sign}{body}")
}

/// Ruby `Float#round(4)` (round-half-up with Ruby's correction step).
pub fn round4(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    // Ruby leaves numbers with too many integer digits unchanged.
    let binexp = crate::js_math::binexp(x);
    let limit = if binexp > 0 { binexp / 4 } else { binexp / 3 - 1 };
    if 4 >= 15 - limit {
        return x;
    }
    // ...and flushes numbers far below the precision to a positive zero.
    let under = if binexp > 0 { binexp / 3 + 1 } else { binexp / 4 };
    if 4 < -under {
        return 0.0;
    }
    let s = 10_000.0_f64;
    let mut f = (x * s).round();
    if x > 0.0 {
        if (f + 0.5) / s <= x {
            f += 1.0;
        }
    } else if (f - 0.5) / s >= x {
        f -= 1.0;
    }
    f / s
}

/// Ruby `Sirena::Svg::Numbers.write`: `value.round(4).to_s`.
pub fn write_num(x: f64) -> String {
    ruby_float(round4(x))
}

/// Ruby `Sirena::Svg::Numbers.read`: the leading SVG number of `s`
/// (`"14px"` -> `14.0`), or `None`.
pub fn read_num(s: &str) -> Option<f64> {
    let b = s.trim_start().as_bytes();
    let t = std::str::from_utf8(b).ok()?;
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - ds;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        frac_digits = j - i - 1;
        // `\d+\.?\d*` accepts "1."; `\.\d+` needs a digit.
        if int_digits > 0 || frac_digits > 0 {
            i = j;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    let mant_end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
            j += 1;
        }
        let es = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > es {
            i = j;
        }
    }
    let _ = mant_end;
    t[..i].parse::<f64>().ok().or_else(|| t[..i].trim_end_matches('.').parse().ok())
}
