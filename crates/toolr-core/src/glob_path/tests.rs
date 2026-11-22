use super::*;

#[cfg(unix)]
#[test]
fn escapes_metacharacters_in_components() {
    let escaped = escape_path(Path::new("/src/[wip]/x*y?/repo")).unwrap();
    assert_eq!(escaped, "/src/[[]wip[]]/x[*]y[?]/repo");
}

#[test]
fn relative_path_keeps_its_shape() {
    let escaped = escape_path(Path::new("a/[b]")).unwrap();
    assert_eq!(escaped, format!("a{MAIN_SEPARATOR}[[]b[]]"));
}

#[test]
fn dot_components_stay_unescaped() {
    let escaped = escape_path(Path::new("./[a]/../b")).unwrap();
    let sep = MAIN_SEPARATOR;
    assert_eq!(escaped, format!(".{sep}[[]a[]]{sep}..{sep}b"));
}

#[cfg(windows)]
#[test]
fn verbatim_prefix_stays_literal() {
    let escaped = escape_path(Path::new(r"\\?\C:\src\[wip]")).unwrap();
    assert_eq!(escaped, r"\\?\C:\src\[[]wip[]]");
}

#[test]
fn canonical_path_globs_to_itself() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("[wip]");
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    let canonical = dir.canonicalize().unwrap();
    let pattern = format!("{}/*", escape_path(&canonical).unwrap());
    let found: Vec<_> = glob::glob(&pattern).unwrap().flatten().collect();
    assert_eq!(found, [canonical.join("inner")]);
}
