use super::*;

#[test]
#[cfg(unix)]
fn python3_minor_reads_the_interpreter_version() {
    let bin = tempfile::TempDir::new().unwrap();
    assert_eq!(
        python3_minor(&stub_python(bin.path(), "python3", Some("3.13.2"))),
        Some(13)
    );
    assert_eq!(
        python3_minor(&stub_python(bin.path(), "python2", Some("2.7.18"))),
        None
    );
    assert_eq!(
        python3_minor(&stub_python(bin.path(), "broken", None)),
        None
    );
}

#[test]
fn python3_minor_of_a_missing_binary_is_none() {
    assert_eq!(
        python3_minor(Path::new("definitely-not-a-real-binary-toolr-test")),
        None
    );
}

#[test]
fn parse_minor_reads_python_3_versions_only() {
    assert_eq!(parse_minor("Python 3.11.4\n"), Some(11));
    assert_eq!(parse_minor("Python 3.13.0rc1"), Some(13));
    assert_eq!(parse_minor("Python 2.7.18"), None);
    assert_eq!(parse_minor("garbage"), None);
}

#[cfg(unix)]
/// `version: None` makes a stub whose `--version` fails.
fn stub_python(dir: &Path, name: &str, version: Option<&str>) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    let body = match version {
        Some(v) => format!("#!/bin/sh\necho 'Python {v}'\n"),
        None => "#!/bin/sh\nexit 1\n".to_string(),
    };
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
