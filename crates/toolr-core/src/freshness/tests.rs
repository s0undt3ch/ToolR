use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::freshness::{FreshnessVerdict, compare};
use crate::manifest::Manifest;

const PY_VERSION_DIR: &str = "python3.13";

fn make_tools(tmp: &Path, files: &[(&str, &str)]) {
    let tools = tmp.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for (rel, content) in files {
        let path = tools.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
}

fn make_venv(tmp: &Path, plugins: &[(&str, &str)]) {
    let site = tmp
        .join("venv")
        .join("lib")
        .join(PY_VERSION_DIR)
        .join("site-packages");
    fs::create_dir_all(&site).unwrap();
    for (pkg, content) in plugins {
        let pkg_dir = site.join(pkg);
        fs::create_dir(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("toolr-manifest.json"), content).unwrap();
    }
}

fn manifest_for(tmp: &Path) -> Manifest {
    use crate::hash::hash_tools_dir;
    use crate::manifest_build::compute_third_party_hash;
    Manifest {
        schema_version: crate::manifest::SCHEMA_VERSION,
        static_hash: hash_tools_dir(&tmp.join("tools")).unwrap(),
        third_party_hash: compute_third_party_hash(&tmp.join("venv")).unwrap(),
        toolr_version: env!("CARGO_PKG_VERSION").to_string(),
        groups: vec![],
        commands: vec![],
    }
}

#[test]
fn returns_fresh_when_both_axes_match() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::Fresh));
}

#[test]
fn returns_static_drift_when_py_file_changed() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    fs::write(tmp.path().join("tools").join("a.py"), "x = 2\n").unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::StaticDrift));
}

#[test]
fn returns_third_party_drift_when_plugin_manifest_added() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[]);
    let cached = manifest_for(tmp.path());
    let site = tmp.path().join("venv").join("lib").join(PY_VERSION_DIR).join("site-packages");
    let pkg = site.join("foo");
    fs::create_dir(&pkg).unwrap();
    fs::write(pkg.join("toolr-manifest.json"), "{}").unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn collapses_both_axes_to_third_party_drift() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    fs::write(tmp.path().join("tools").join("a.py"), "x = 2\n").unwrap();
    let pkg = tmp.path().join("venv").join("lib").join(PY_VERSION_DIR).join("site-packages").join("foo");
    fs::write(pkg.join("toolr-manifest.json"), r#"{"v":2}"#).unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn unrelated_dist_info_returns_fresh() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let cached = manifest_for(tmp.path());
    let site = tmp.path().join("venv").join("lib").join(PY_VERSION_DIR).join("site-packages");
    fs::create_dir(site.join("unrelated-1.0.0.dist-info")).unwrap();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::Fresh));
}

#[test]
fn no_cache_forces_rebuild_via_third_party_drift() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[]);
    let venv = tmp.path().join("venv");
    let verdict = compare(None, &tmp.path().join("tools"), Some(&venv)).unwrap();
    // No cache means we have nothing to compare to; force the strongest
    // rebuild so the caller produces a fresh manifest including third-party.
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn missing_venv_skips_third_party_axis() {
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    // Cached manifest with an arbitrary non-empty third_party_hash —
    // under the new semantics this is ignored when venv_dir is None.
    let cached = Manifest {
        schema_version: crate::manifest::SCHEMA_VERSION,
        static_hash: crate::hash::hash_tools_dir(&tmp.path().join("tools")).unwrap(),
        third_party_hash: "arbitrary-non-empty-hash".to_string(),
        toolr_version: env!("CARGO_PKG_VERSION").to_string(),
        groups: vec![],
        commands: vec![],
    };
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), None).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::Fresh));
}

#[test]
fn toolr_version_mismatch_with_venv_forces_third_party_drift_even_when_both_axes_match() {
    // #420: a toolr upgrade alone must invalidate the cache. With a venv
    // available it's safe to escalate to the strongest verdict.
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    make_venv(tmp.path(), &[("foo", "{}")]);
    let mut cached = manifest_for(tmp.path());
    cached.toolr_version = "0.0.0-old".to_string();
    let venv = tmp.path().join("venv");
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), Some(&venv)).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::ThirdPartyDrift));
}

#[test]
fn toolr_version_mismatch_without_venv_only_forces_static_drift() {
    // Without a venv, escalating to `ThirdPartyDrift` would make the
    // dispatch-side rebuild discard cached third-party entries with
    // nothing to replace them. Must cap at `StaticDrift`.
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    let cached = Manifest {
        schema_version: crate::manifest::SCHEMA_VERSION,
        static_hash: crate::hash::hash_tools_dir(&tmp.path().join("tools")).unwrap(),
        third_party_hash: "arbitrary-non-empty-hash".to_string(),
        toolr_version: "0.0.0-old".to_string(),
        groups: vec![],
        commands: vec![],
    };
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), None).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::StaticDrift));
}

#[test]
fn empty_toolr_version_from_a_pre_field_manifest_is_treated_as_a_mismatch() {
    // Manifests written before this field existed deserialize it as ""
    // (`#[serde(default)]`), which must never accidentally equal a real
    // running version and pass as Fresh.
    let tmp = TempDir::new().unwrap();
    make_tools(tmp.path(), &[("a.py", "x = 1\n")]);
    let mut cached = manifest_for(tmp.path());
    cached.toolr_version = String::new();
    let verdict = compare(Some(&cached), &tmp.path().join("tools"), None).unwrap();
    assert!(matches!(verdict, FreshnessVerdict::StaticDrift));
}
