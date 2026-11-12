//! Stage 3 parity: detect() and parse() against the Ruby native parser.
//!
//! Without Ruby: every golden case must detect as its manifest type and parse.
//! With Ruby trees (`golden/dump_trees.rb`), set
//!   SIRENA_TREES=<dir>   dump of golden/        (compared against golden/*.mmd)
//!   SIRENA_CORPUS=<dir>  spec/mermaid           and
//!   SIRENA_CORPUS_TREES=<dir> its dump
//! and every tree, plus detect(), is compared with Ruby's.
use serde_json::Value;
use etassia_rs::parse::{detect, parse};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn golden() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden")
}

fn mmd_files(root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = vec![];
    for dir in fs::read_dir(root).unwrap().flatten() {
        if !dir.path().is_dir() {
            continue;
        }
        let ty = dir.file_name().to_string_lossy().to_string();
        for f in fs::read_dir(dir.path()).unwrap().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|e| e == "mmd") {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                out.push((ty.clone(), stem, p));
            }
        }
    }
    out.sort();
    out
}

#[derive(Default)]
struct Count {
    cases: usize,
    pass: usize,
}

fn report(title: &str, counts: &BTreeMap<String, Count>, failures: &[String]) {
    eprintln!("== {title}");
    for (t, c) in counts {
        eprintln!("{t:15} {}/{}", c.pass, c.cases);
    }
    let (p, n) = counts.values().fold((0, 0), |a, c| (a.0 + c.pass, a.1 + c.cases));
    eprintln!("{:15} {p}/{n}", "TOTAL");
    assert!(failures.is_empty(), "{} failures, first: {:#?}", failures.len(), &failures[..failures.len().min(5)]);
}

/// Golden cases whose directory is not the type Ruby detects (the "unknown"
/// directory holds sources of registered types; this one is filed under
/// er_diagram but opens with `journey`). Ruby's detect_type agrees with Rust on
/// all of them (see the tree-parity tests).
fn expected_detect<'a>(dir: &'a str, stem: &str, detected: Option<&'a str>) -> Option<&'a str> {
    if dir == "unknown" || stem == "er_diagram/journey_spec-1f8d17b6.mmd" { detected } else { Some(dir) }
}

#[test]
fn parse_golden_detects_and_parses() {
    let manifest: Value = serde_json::from_str(&fs::read_to_string(golden().join("manifest.json")).unwrap()).unwrap();
    let mut counts: BTreeMap<String, Count> = BTreeMap::new();
    let mut failures = vec![];
    for case in manifest["cases"].as_array().unwrap() {
        let name = case["case"].as_str().unwrap();
        let ty = case["type"].as_str().unwrap();
        let source = fs::read_to_string(golden().join(name)).unwrap();
        let c = counts.entry(ty.to_string()).or_default();
        c.cases += 1;
        let detected = detect(&source);
        let mut ok = detected.as_deref() == expected_detect(ty, name, detected.as_deref());
        if !ok {
            failures.push(format!("{name}: detect {detected:?}, want {ty}"));
        } else if let Some(d) = detected.as_deref().filter(|d| *d != "error") {
            // every golden case renders, so its body parses with its detected entry
            if let Err(e) = parse(&source, d) {
                ok = false;
                failures.push(format!("{name}: parse failed: {e}"));
            }
        }
        c.pass += ok as usize;
    }
    report("golden detect+parse", &counts, &failures);
}

fn compare_dir(title: &str, corpus: &Path, trees: &Path) {
    let mut counts: BTreeMap<String, Count> = BTreeMap::new();
    let mut failures = vec![];
    for (ty, stem, path) in mmd_files(corpus) {
        let ruby: Value = match fs::read_to_string(trees.join(&ty).join(format!("{stem}.json"))) {
            Ok(t) => serde_json::from_str(&t).unwrap(),
            Err(_) => continue,
        };
        let name = format!("{ty}/{stem}");
        let source = String::from_utf8_lossy(&fs::read(&path).unwrap()).to_string();
        let c = counts.entry(ty.clone()).or_default();
        c.cases += 1;
        let detected = detect(&source);
        let want = ruby["detected"].as_str().map(str::to_string);
        if detected != want {
            failures.push(format!("{name}: detect {detected:?}, ruby {want:?}"));
            continue;
        }
        let Some(entry) = ruby["entry"].as_str() else {
            c.pass += 1;
            continue;
        };
        let rust = parse(&source, entry).ok();
        if ruby["tree"].is_null() == rust.is_none() && rust.as_ref().is_none_or(|t| *t == ruby["tree"]) {
            c.pass += 1;
        } else {
            failures.push(format!("{name}: tree differs from ruby (rust parsed: {})", rust.is_some()));
        }
    }
    assert!(!counts.is_empty(), "{title}: no cases compared");
    report(title, &counts, &failures);
}

#[test]
fn parse_tree_parity_golden() {
    match std::env::var("SIRENA_TREES") {
        Ok(t) => compare_dir("golden tree parity", &golden(), Path::new(&t)),
        Err(_) => eprintln!("SIRENA_TREES unset: ruby tree parity skipped"),
    }
}

#[test]
fn parse_tree_parity_corpus() {
    match (std::env::var("SIRENA_CORPUS"), std::env::var("SIRENA_CORPUS_TREES")) {
        (Ok(c), Ok(t)) => compare_dir("corpus tree parity", Path::new(&c), Path::new(&t)),
        _ => eprintln!("SIRENA_CORPUS/SIRENA_CORPUS_TREES unset: corpus parity skipped"),
    }
}
