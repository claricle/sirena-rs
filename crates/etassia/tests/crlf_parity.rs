//! CRLF parity: every golden source, CRLF-converted, renders byte-identically to
//! Ruby's render of the same CRLF source. Ruby is not available in CI, so the test
//! runs only when ETASSIA_RUBY_DIR points at a etassia checkout (with its bundle).
use std::path::{Path, PathBuf};
use std::process::Command;

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "mmd") {
            out.push(p);
        }
    }
}

#[test]
fn crlf_sources_match_ruby() {
    let Some(ruby_dir) = std::env::var_os("ETASSIA_RUBY_DIR") else {
        eprintln!("SKIPPED: set ETASSIA_RUBY_DIR to a etassia checkout to run the CRLF parity test");
        return;
    };
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden");
    let tmp = std::env::temp_dir().join(format!("etassia-crlf-{}", std::process::id()));
    let (corpus, out) = (tmp.join("corpus"), tmp.join("out"));
    let mut files = vec![];
    collect(&golden, &mut files);
    for f in &files {
        let rel = f.strip_prefix(&golden).unwrap();
        let src = std::fs::read_to_string(f).unwrap().replace("\r\n", "\n").replace('\n', "\r\n");
        let dest = corpus.join(rel);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, src).unwrap();
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/golden_generate.rb");
    // `ruby -S bundle` finds bundler on every OS; on Windows `bundle` is a
    // .bat script that Command::new cannot resolve.
    let status = Command::new("ruby")
        .args(["-S", "bundle", "exec", "ruby"])
        .arg(script)
        .current_dir(&ruby_dir)
        .env("ETASSIA_CORPUS", &corpus)
        .env("GOLDEN_OUT", &out)
        .status()
        .expect("bundle runs");
    assert!(status.success(), "golden_generate.rb failed");

    let (mut ok, mut declined, mut bad) = (0, 0, vec![]);
    for f in &files {
        let rel = f.strip_prefix(&golden).unwrap();
        let Ok(want) = std::fs::read(out.join(rel.with_extension("svg"))) else { continue };
        let src = std::fs::read_to_string(corpus.join(rel)).unwrap();
        match etassia::render::render(&src) {
            Ok(got) if got.as_bytes() == want.as_slice() => ok += 1,
            Ok(_) => bad.push(format!("{} differs", rel.display())),
            // Unported is fine only when the LF original is declined too.
            Err(etassia::Error::Unported(_))
                if etassia::render::render(&std::fs::read_to_string(f).unwrap()).is_err() =>
            {
                declined += 1
            }
            Err(e) => bad.push(format!("{}: {e}", rel.display())),
        }
    }
    eprintln!("crlf parity: {ok} identical, {declined} declined as on LF, {} bad", bad.len());
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(bad.is_empty(), "{} CRLF cases diverge from Ruby:\n{}", bad.len(), bad.join("\n"));
}
