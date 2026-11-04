//! Compares `elk::layout` with golden/elk/<type>/<case>.json (recorded by
//! golden/elk_dump.rb from elkrb 2.0.1). Floats compared by exact bits.
//! Prints matched/unported/mismatch per type; a mismatch fails the test.
use serde_json::Value;
use sirena_core::{elk, Error};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn same(a: &Value, b: &Value, path: &str) -> Result<(), String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if x.to_bits() == y.to_bits() { Ok(()) } else { Err(format!("{path}: want {x:?} got {y:?}")) }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() { return Err(format!("{path}: want len {} got {}", x.len(), y.len())); }
            x.iter().zip(y).enumerate().try_for_each(|(i, (p, q))| same(p, q, &format!("{path}[{i}]")))
        }
        (Value::Object(x), Value::Object(y)) => {
            if x.len() != y.len() { return Err(format!("{path}: want keys {:?} got {:?}", x.keys().collect::<Vec<_>>(), y.keys().collect::<Vec<_>>())); }
            x.iter().try_for_each(|(k, p)| match y.get(k) {
                Some(q) => same(p, q, &format!("{path}.{k}")),
                None => Err(format!("{path}.{k}: missing")),
            })
        }
        _ => if a == b { Ok(()) } else { Err(format!("{path}: want {a} got {b}")) },
    }
}

#[test]
fn elk_golden() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/elk");
    let mut counts: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    for ty in std::fs::read_dir(&dir).unwrap() {
        let ty = ty.unwrap().path();
        if !ty.is_dir() { continue; }
        let tn = ty.file_name().unwrap().to_string_lossy().into_owned();
        let mut files: Vec<_> = std::fs::read_dir(&ty).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        for f in files {
            let rec: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
            let c = counts.entry(tn.clone()).or_default();
            let mut verdict: Result<(), String> = Ok(());
            let mut unported = false;
            for call in rec["calls"].as_array().unwrap() {
                match elk::layout(&call["input"]) {
                    Err(Error::Unported(_)) => { unported = true; break; }
                    Err(e) => { verdict = Err(e.to_string()); break; }
                    Ok(got) => if let Err(d) = same(&call["output"], &got, "$") { verdict = Err(d); break; },
                }
            }
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            if unported { c.2 += 1; }
            else if let Err(d) = verdict { c.1 += 1; first.entry(tn.clone()).or_insert(format!("{name}: {d}")); }
            else { c.0 += 1; }
        }
    }
    let mut bad = false;
    for (t, (m, x, u)) in &counts {
        println!("elk {t}: match={m} mismatch={x} unported={u} total={}", m + x + u);
        if let Some(d) = first.get(t) { println!("  first mismatch: {d}"); }
        bad |= *x > 0;
    }
    assert!(!bad, "elk layout mismatch");
}
