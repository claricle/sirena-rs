//! Compares js_math against `<fn> <file>` lines of hex bit patterns (inputs..., expected).
//! Usage: cargo run --release --example js_math_cmp -- <fn> <file> [limit]
use etassia::js_math as m;
use std::io::{BufRead, BufReader};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (name, path) = (args[1].as_str(), &args[2]);
    let limit: usize = args
        .get(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let (mut n, mut diffs) = (0usize, 0usize);
    for line in BufReader::new(std::fs::File::open(path).unwrap()).lines() {
        if n >= limit {
            break;
        }
        let v: Vec<f64> = line
            .unwrap()
            .split_whitespace()
            .map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap()))
            .collect();
        let (expected, inputs) = v.split_last().unwrap();
        let got = match name {
            "sin" => m::sin(inputs[0]),
            "cos" => m::cos(inputs[0]),
            "atan" => m::atan(inputs[0]),
            "log" => m::ln(inputs[0]),
            "log10" => m::log10(inputs[0]),
            "atan2" => m::atan2(inputs[0], inputs[1]),
            "hypot" => m::hypot2(inputs[0], inputs[1]),
            "pow" => m::pow(inputs[0], inputs[1]),
            _ => panic!("fn"),
        };
        n += 1;
        if std::env::var("DUMP").is_ok() {
            println!("{:016x}", got.to_bits());
            continue;
        }
        let same = if got.is_nan() {
            expected.is_nan()
        } else {
            got.to_bits() == expected.to_bits()
        };
        if !same {
            diffs += 1;
            if diffs <= 3 {
                println!("DIFF {inputs:?} got={got:e} expected={expected:e}");
            }
        }
    }
    if std::env::var("DUMP").is_err() {
        println!("{name}: n={n} diffs={diffs}");
    }
}
