use std::collections::BTreeMap;
fn main() {
    let ty = std::env::args().nth(1).unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden");
    let m: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let mut h: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in m["cases"].as_array().unwrap() {
        if c["type"].as_str().unwrap() != ty { continue; }
        let case = c["case"].as_str().unwrap();
        let src = std::fs::read_to_string(dir.join(case)).unwrap();
        if let Err(e) = sirena_core::render(&src) { h.entry(e.to_string()).or_default().push(case.to_string()); }
    }
    let mut v: Vec<_> = h.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(c.len()));
    for (k, c) in v { println!("{:3} {}  e.g. {}", c.len(), k, c[0]); }
}
