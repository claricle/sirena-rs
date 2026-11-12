//! Bit-equality of the Rust text measurement against Ruby. The fixture is
//! written by `scripts/text_parity_dump.rb`; floats are stored as u64 bits.

use serde_json::Value;
use etassia_rs::text::{self, timeline, Face};

fn fixture() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/text/parity.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn bits(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

fn f(v: &Value) -> f64 {
    f64::from_bits(bits(v))
}

fn check_row(text: &str, sizes: &[f64], expected: &[Value], bad: &mut Vec<String>) {
    assert_eq!(expected.len(), sizes.len() * 6);
    for (size, e) in sizes.iter().zip(expected.chunks(6)) {
        let size = *size;
        let got = [
            text::measure(text, size, Face::Arial).width,
            text::measure(text, size, Face::Trebuchet).width,
            text::measure_monospace(text, size).width,
            text::reserved_text_width(text, size),
            text::trebuchet_kerning_adjustment(text, size),
            timeline::width_of(text, size),
        ];
        let names = ["arial", "trebuchet", "mono", "reserved", "kern", "timeline"];
        for i in 0..6 {
            if got[i].to_bits() != bits(&e[i]) {
                bad.push(format!(
                    "{} {:?} @{}: rust {} ({:#x}) ruby {} ({:#x})",
                    names[i],
                    text,
                    size,
                    got[i],
                    got[i].to_bits(),
                    f(&e[i]),
                    bits(&e[i])
                ));
            }
        }
    }
}

#[test]
fn widths_match_ruby_bit_for_bit() {
    let fx = fixture();
    let sizes: Vec<f64> = fx["sizes"].as_array().unwrap().iter().map(f).collect();
    let mut bad = Vec::new();
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() > 1000);
    for c in cases {
        let t = c["t"].as_str().unwrap();
        check_row(t, &sizes, c["w"].as_array().unwrap(), &mut bad);
    }
    let sweep = fx["sweep"].as_array().unwrap();
    assert!(sweep.len() > 1000);
    for c in sweep {
        let t = c["t"].as_str().unwrap();
        check_row(t, &sizes[2..3], c["w"].as_array().unwrap(), &mut bad);
    }
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:#?}",
        bad.len(),
        &bad[..bad.len().min(10)]
    );
}

#[test]
fn timeline_wrap_matches_ruby() {
    let fx = fixture();
    let width = f(&fx["wrap_width"]);
    let mut bad = Vec::new();
    for c in fx["wraps"].as_array().unwrap() {
        let t = c["t"].as_str().unwrap();
        let lines = timeline::wrap(t, width, timeline::FONT_SIZE);
        let want: Vec<&str> = c["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap())
            .collect();
        let h = timeline::text_height(&lines, timeline::FONT_SIZE).to_bits();
        let ch = timeline::card_height(&lines, timeline::FONT_SIZE).to_bits();
        if lines != want || h != bits(&c["h"]) || ch != bits(&c["c"]) {
            bad.push(format!("{:?}: rust {:?} ruby {:?}", t, lines, want));
        }
    }
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:#?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

#[test]
fn overrides_and_height() {
    let m = text::measure_with("Text", 12.0, Some(100.0), Some(20.0), false);
    assert_eq!((m.width, m.height), (100.0, 20.0));
    assert_eq!(text::measure("x", 14.0, Face::Arial).height, 14.0);
}
