use super::*;
use tempfile::TempDir;

#[test]
fn find_in_venv_picks_first_lexicographic_match() {
    let tmp = TempDir::new().unwrap();
    let venv = tmp.path();
    std::fs::create_dir_all(venv.join("lib/python3.12/site-packages/pkg")).unwrap();
    std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/pkg")).unwrap();
    let found = find_in_venv(venv, "pkg").unwrap();
    assert!(
        found.ends_with("lib/python3.12/site-packages/pkg"),
        "got: {}",
        found.display()
    );
}

#[test]
fn find_in_venv_under_bracketed_venv() {
    let tmp = TempDir::new().unwrap();
    let venv = tmp.path().join("[wip]").join("venv");
    std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/pkg")).unwrap();
    let found = find_in_venv(&venv, "pkg").unwrap();
    assert_eq!(found, venv.join("lib/python3.13/site-packages/pkg"));
}

#[test]
fn find_in_venv_errors_when_missing() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("lib/python3.13/site-packages/other")).unwrap();
    let err = find_in_venv(tmp.path(), "nope").unwrap_err().to_string();
    assert!(err.contains("no site-packages directory in"), "{err}");
    assert!(err.contains("contains `nope`"), "{err}");
}

/// The reported shape: on Windows the package lives under `Lib\site-packages`.
#[test]
fn find_in_venv_finds_windows_layout() {
    let tmp = TempDir::new().unwrap();
    let venv = tmp.path();
    std::fs::create_dir_all(venv.join("Lib/site-packages/pkg")).unwrap();
    let found = find_in_venv(venv, "pkg").unwrap();
    assert_eq!(found, venv.join("Lib").join("site-packages").join("pkg"));
}

#[test]
fn find_in_venv_finds_free_threaded_layout() {
    let tmp = TempDir::new().unwrap();
    let venv = tmp.path();
    std::fs::create_dir_all(venv.join("lib/python3.14t/site-packages/pkg")).unwrap();
    let found = find_in_venv(venv, "pkg").unwrap();
    assert_eq!(found, venv.join("lib/python3.14t/site-packages/pkg"));
}

#[test]
fn find_in_venv_ignores_a_file_named_like_the_package() {
    let tmp = TempDir::new().unwrap();
    let sp = tmp.path().join("lib/python3.13/site-packages");
    std::fs::create_dir_all(&sp).unwrap();
    std::fs::write(sp.join("pkg"), "").unwrap();
    assert!(find_in_venv(tmp.path(), "pkg").is_err());
}

#[test]
fn leaf_dir_name_extracts_basename() {
    let dir = PathBuf::from("/a/b/mypkg");
    assert_eq!(leaf_dir_name(&dir).as_deref(), Some("mypkg"));
}
