//! Node placement against golden/elk (recorded from elkrb 2.0.1): compares
//! only graph and node x/y/width/height (edge routing is a later phase).
//! Prints matched/unported/mismatch per type; a mismatch fails the test.
use serde_json::Value;
use etassia_rs::{elk, Error};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn geometry(v: &Value) -> Value {
    let kids: Vec<Value> = v["children"].as_array().unwrap().iter().map(geometry).collect();
    serde_json::json!({"id": v["id"], "x": v["x"], "y": v["y"], "width": v["width"], "height": v["height"], "children": kids})
}

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
        (Value::Object(x), Value::Object(y)) => x.iter().try_for_each(|(k, p)| match y.get(k) {
            Some(q) => same(p, q, &format!("{path}.{k}")),
            None => Err(format!("{path}.{k}: missing")),
        }),
        _ => if a == b { Ok(()) } else { Err(format!("{path}: want {a} got {b}")) },
    }
}

#[test]
fn elk_place_golden() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/elk");
    let mut counts: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut first: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ty in std::fs::read_dir(&dir).unwrap() {
        let ty = ty.unwrap().path();
        if !ty.is_dir() { continue; }
        let tn = ty.file_name().unwrap().to_string_lossy().into_owned();
        let mut files: Vec<_> = std::fs::read_dir(&ty).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        for f in files {
            let rec: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
            let c = counts.entry(tn.clone()).or_default();
            let (mut verdict, mut unported) = (Ok(()), false);
            for call in rec["calls"].as_array().unwrap() {
                match elk::layout_placement(&call["input"]) {
                    Err(Error::Unported(_)) => { unported = true; break; }
                    Err(e) => { verdict = Err(e.to_string()); break; }
                    Ok(got) => if let Err(d) = same(&geometry(&call["output"]), &geometry(&got), "$") { verdict = Err(d); break; },
                }
            }
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            if unported { c.2 += 1; }
            else if let Err(d) = verdict { c.1 += 1; first.entry(tn.clone()).or_default().push(format!("{name}: {d}")); }
            else { c.0 += 1; }
        }
    }
    let mut bad = false;
    for (t, (m, x, u)) in &counts {
        println!("elk place {t}: match={m} mismatch={x} unported={u} total={}", m + x + u);
        for d in first.get(t).into_iter().flatten().take(3) { println!("  mismatch: {d}"); }
        bad |= *x > 0;
    }
    assert!(!bad, "elk placement mismatch");
}
