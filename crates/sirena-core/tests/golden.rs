//! Golden runner: renders every golden/<type>/<case>.mmd and compares bytes to
//! the Ruby-generated .svg. Prints per-type counts. Types in `REQUIRED` must be
//! byte-identical; others are reported (unported -> Ruby fallback).
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Types that must match byte-for-byte (grows as stages land).
const REQUIRED: &[&str] = &["info", "pie", "packet", "timeline", "treemap", "quadrant", "radar", "xychart", "user_journey", "kanban", "mindmap", "gantt", "gitgraph", "er_diagram", "requirement", "c4", "block", "architecture", "sequence", "state_diagram", "flowchart", "class_diagram"];

fn golden_dir() -> PathBuf {
    match std::env::var_os("SIRENA_GOLDEN_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden"),
    }
}

fn first_diff(a: &[u8], b: &[u8]) -> String {
    let n = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let ctx = |s: &[u8]| String::from_utf8_lossy(&s[n.saturating_sub(20)..(n + 60).min(s.len())]).into_owned();
    format!("offset {n}: want …{:?}… got …{:?}…", ctx(a), ctx(b))
}

#[test]
fn manifest_matches_files() {
    let dir = golden_dir();
    let m: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    for c in m["cases"].as_array().unwrap() {
        let case = c["case"].as_str().unwrap();
        let src = std::fs::read(dir.join(case)).unwrap();
        let svg = std::fs::read(dir.join(case.replace(".mmd", ".svg"))).unwrap();
        assert_eq!(hex(&Sha256::digest(&src)), c["source_sha256"].as_str().unwrap(), "{case} source");
        assert_eq!(hex(&Sha256::digest(&svg)), c["svg_sha256"].as_str().unwrap(), "{case} svg");
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn golden_byte_match() {
    let dir = golden_dir();
    let m: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    // type -> (match, mismatch, unported)
    let mut counts: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    for c in m["cases"].as_array().unwrap() {
        let case = c["case"].as_str().unwrap();
        let ty = c["type"].as_str().unwrap().to_string();
        let src = String::from_utf8(std::fs::read(dir.join(case)).unwrap()).unwrap();
        let want = std::fs::read(dir.join(case.replace(".mmd", ".svg"))).unwrap();
        let e = counts.entry(ty.clone()).or_default();
        match sirena_core::render(&src) {
            Ok(got) if got.as_bytes() == want.as_slice() => e.0 += 1,
            Ok(got) => {
                e.1 += 1;
                first.entry(ty).or_insert_with(|| format!("{case}: {}", first_diff(&want, got.as_bytes())));
            }
            Err(_) => e.2 += 1,
        }
    }
    let mut failed = vec![];
    for (ty, (ok, bad, unp)) in &counts {
        println!("{ty:16} match={ok:4} mismatch={bad:4} unported={unp:4}");
        if let Some(d) = first.get(ty) {
            println!("    first diff: {d}");
        }
        if REQUIRED.contains(&ty.as_str()) && (*bad > 0 || *unp > 0) {
            failed.push(ty.clone());
        }
    }
    assert!(failed.is_empty(), "required types not byte-identical: {failed:?}");
}
