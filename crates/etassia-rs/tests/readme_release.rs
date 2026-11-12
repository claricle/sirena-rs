//! The RELEASING section of the README must describe what release.yml and the
//! repository actually do, or following it never starts a release.
use std::fs;
use std::path::PathBuf;

fn root_file(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn readme_tags_match_release_workflow_triggers() {
    let readme = root_file("README.md");
    let workflow = root_file(".github/workflows/release.yml");
    let mut tags = 0;
    for line in readme.lines().filter(|l| l.starts_with("| `") && l.contains("-vX.Y.Z`")) {
        let tag = line.trim_start_matches("| `").split("X.Y.Z").next().unwrap();
        let trigger = format!("\"{tag}*\"");
        assert!(workflow.contains(&trigger), "README tag {tag}X.Y.Z has no trigger {trigger} in release.yml");
        tags += 1;
    }
    assert_eq!(tags, 3, "expected three tag rows in the README release table");
}

#[test]
fn readme_trusted_publisher_owner_is_the_repo_owner() {
    let readme = root_file("README.md");
    assert!(!readme.contains("claricle"), "README still names the old owner claricle");
    let flat = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(flat.matches("owner `etassia`").count(), 2, "RubyGems and PyPI must use owner `etassia`");
    assert!(flat.contains("org `etassia`"), "npm must use org `etassia`");
}
