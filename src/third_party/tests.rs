use super::glob::glob_manifests;
use super::model::*;
use tempfile::TempDir;

fn setup_fake_venv(packages: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    std::fs::create_dir_all(&site).unwrap();
    for (pkg, contents) in packages {
        let pkg_dir = site.join(pkg);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("toolr-manifest.json"), contents).unwrap();
    }
    tmp
}

#[test]
fn fragment_round_trips_through_json() {
    let f = ManifestFragment {
        toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
        package: "my_pkg".into(),
        groups: vec![FragmentGroup {
            name: "deploy".into(),
            title: "Deploy".into(),
            description: String::new(),
        }],
        commands: vec![],
    };
    let json = serde_json::to_string_pretty(&f).unwrap();
    let back: ManifestFragment = serde_json::from_str(&json).unwrap();
    assert_eq!(f, back);
}

#[test]
fn glob_finds_only_toolr_manifest_files() {
    let tmp = setup_fake_venv(&[
        ("a_pkg", r#"{"toolr_schema_version": 1, "package": "a_pkg"}"#),
        ("b_pkg", r#"{"toolr_schema_version": 1, "package": "b_pkg"}"#),
    ]);
    // Drop a spurious file the glob must ignore.
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    std::fs::write(site.join("a_pkg").join("README"), "ignored").unwrap();

    let hits = glob_manifests(tmp.path()).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].ends_with("a_pkg/toolr-manifest.json"));
    assert!(hits[1].ends_with("b_pkg/toolr-manifest.json"));
}

#[test]
fn glob_returns_empty_when_no_site_packages() {
    let tmp = TempDir::new().unwrap();
    let hits = glob_manifests(tmp.path()).unwrap();
    assert!(hits.is_empty());
}

use super::parse::{ThirdPartyError, parse_fragment};

fn write_fragment(tmp: &TempDir, pkg: &str, contents: &str) -> std::path::PathBuf {
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    let pkg_dir = site.join(pkg);
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let path = pkg_dir.join("toolr-manifest.json");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn parse_accepts_minimal_v1_fragment() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "my_pkg",
        r#"{
            "toolr_schema_version": 1,
            "package": "my_pkg",
            "groups": [],
            "commands": []
        }"#,
    );
    let frag = parse_fragment(&path).expect("should parse");
    assert_eq!(frag.toolr_schema_version, 1);
    assert_eq!(frag.package, "my_pkg");
}

#[test]
fn parse_rejects_missing_version_key() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "bad_pkg",
        r#"{"package": "bad_pkg", "groups": [], "commands": []}"#,
    );
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
}

#[test]
fn parse_rejects_unknown_future_version() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "future_pkg",
        r#"{"toolr_schema_version": 999, "package": "future_pkg"}"#,
    );
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(
        err,
        ThirdPartyError::UnknownVersion { version: 999, .. }
    ));
}

#[test]
fn parse_rejects_malformed_json() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(&tmp, "bad_pkg", "not valid json");
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(err, ThirdPartyError::Json { .. }));
}
