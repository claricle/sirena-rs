//! The RELEASING section of the README must describe what the repository
//! actually does, or following it never starts a release.
use std::fs;
use std::path::PathBuf;

fn root_file(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn readme_release_flows_reference_existing_workflows() {
    let readme = root_file("README.md");
    for workflow in ["release-plz.yml", "release-bindings.yml"] {
        assert!(
            readme.contains(workflow),
            "README names {workflow} but the RELEASING section does not"
        );
        root_file(&format!(".github/workflows/{workflow}"));
    }
}

#[test]
fn readme_does_not_reference_removed_bindings_or_workflows() {
    let readme = root_file("README.md");
    for gone in [
        "bindings/ts",
        "bindings/python",
        "release.yml",
        "python-wheels.yml",
        "npm-package.yml",
        "smoke_installed_npm.mjs",
    ] {
        assert!(!readme.contains(gone), "README still references removed {gone}");
    }
}

#[test]
fn readme_owner_is_the_repo_owner() {
    let readme = root_file("README.md");
    assert!(!readme.contains("claricle"), "README still names the old owner claricle");
    assert!(readme.contains("etassia/etassia-py"), "README must point Python at etassia/etassia-py");
    assert!(readme.contains("etassia/etassia-ts"), "README must point TypeScript at etassia/etassia-ts");
}
