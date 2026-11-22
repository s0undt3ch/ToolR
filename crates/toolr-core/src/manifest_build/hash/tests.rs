use super::*;
use std::fs;
use tempfile::TempDir;

/// Create a venv-shaped tree with the named packages, each shipping
/// a `toolr-manifest.json` whose contents are the supplied string.
fn venv_with_manifests(entries: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    for (pkg, content) in entries {
        let pkg_dir = site.join(pkg);
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("toolr-manifest.json"), content).unwrap();
    }
    tmp
}

#[test]
fn third_party_hash_sees_fragments_under_bracketed_venv() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("[empty]").join("venv");
    fs::create_dir_all(&empty).unwrap();
    let populated = tmp.path().join("[wip]").join("venv");
    let pkg_dir = populated.join("lib/python3.13/site-packages/foo");
    fs::create_dir_all(&pkg_dir).unwrap();
    fs::write(pkg_dir.join("toolr-manifest.json"), "{}").unwrap();
    assert_ne!(
        compute_third_party_hash(&empty).unwrap(),
        compute_third_party_hash(&populated).unwrap()
    );
}

#[test]
fn empty_venv_returns_stable_hash() {
    let tmp = TempDir::new().unwrap();
    let h = compute_third_party_hash(tmp.path()).unwrap();
    assert_eq!(h, compute_third_party_hash(tmp.path()).unwrap());
}

#[test]
fn adding_a_manifest_changes_hash() {
    let a = venv_with_manifests(&[]);
    let b = venv_with_manifests(&[("foo", "{}")]);
    assert_ne!(
        compute_third_party_hash(a.path()).unwrap(),
        compute_third_party_hash(b.path()).unwrap()
    );
}

#[test]
fn modifying_manifest_content_changes_hash() {
    let a = venv_with_manifests(&[("foo", r#"{"v":1}"#)]);
    let b = venv_with_manifests(&[("foo", r#"{"v":2}"#)]);
    assert_ne!(
        compute_third_party_hash(a.path()).unwrap(),
        compute_third_party_hash(b.path()).unwrap()
    );
}

#[test]
fn removing_a_manifest_changes_hash() {
    let a = venv_with_manifests(&[("foo", "{}"), ("bar", "{}")]);
    let b = venv_with_manifests(&[("foo", "{}")]);
    assert_ne!(
        compute_third_party_hash(a.path()).unwrap(),
        compute_third_party_hash(b.path()).unwrap()
    );
}

#[test]
fn empty_helper_matches_empty_venv_hash() {
    let tmp = TempDir::new().unwrap();
    let h1 = compute_third_party_hash(tmp.path()).unwrap();
    let h2 = empty_third_party_hash();
    assert_eq!(h1, h2);
}

#[test]
fn unrelated_dist_info_does_not_change_hash() {
    let a = venv_with_manifests(&[("foo", "{}")]);
    let before = compute_third_party_hash(a.path()).unwrap();
    let site = a
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    fs::create_dir(site.join("unrelated_pkg-1.0.0.dist-info")).unwrap();
    let after = compute_third_party_hash(a.path()).unwrap();
    assert_eq!(before, after);
}
