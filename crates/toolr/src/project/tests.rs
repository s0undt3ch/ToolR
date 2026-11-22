use super::*;
use crate::init_templates::{ScaffoldOptions, VenvLocation};
use std::fs;
use tempfile::TempDir;

/// Mutating SHELL / PATH and observing their effects requires
/// process-wide env coordination — serialise so cargo's parallel
/// runner doesn't race.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_env_var<R>(key: &str, value: Option<&str>, f: impl FnOnce() -> R) -> R {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::env::var_os(key);
    // SAFETY: ENV_LOCK serialises every test in this module that
    // touches env vars.
    unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    let r = f();
    unsafe {
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    r
}

/// A trust store outside the scaffolded repo, so tests never touch the
/// real one.
struct TrustDir(TempDir);

impl TrustDir {
    fn new() -> Self {
        Self(TempDir::new().unwrap())
    }
    fn store(&self) -> TrustStore {
        TrustStore::at(self.0.path().join("trust"))
    }
}

fn init_opts() -> ScaffoldOptions {
    ScaffoldOptions {
        requires_python: ">=3.11".into(),
        venv_location: VenvLocation::Cache,
        include_example: true,
    }
}

/// run_project_init with --yes auto-approves all conflicts.
#[test]
fn run_project_init_yes_all_overwrites_conflicts() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale").unwrap();
    let store = TrustDir::new();

    let opts = init_opts();
    run_project_init(
        tmp.path(),
        &opts,
        /*force=*/ false,
        /*no_sync=*/ true,
        /*quiet=*/ true,
        /*yes_all=*/ true,
        Some(&store.store()),
        &mut |_| unreachable!("confirm should not be called when yes_all=true"),
    )
    .unwrap();

    let content = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
    assert!(content.contains(r#"name = "tools""#));
    assert!(store.store().is_trusted(tmp.path()).unwrap());
}

/// run_project_init with a confirm closure that says "no" leaves conflicts intact.
#[test]
fn run_project_init_confirm_no_preserves_conflicts() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale").unwrap();
    let store = TrustDir::new();

    let opts = init_opts();
    run_project_init(
        tmp.path(),
        &opts,
        /*force=*/ false,
        /*no_sync=*/ true,
        /*quiet=*/ true,
        /*yes_all=*/ false,
        Some(&store.store()),
        &mut |_| Ok(false), // always decline
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(tools.join("pyproject.toml")).unwrap(),
        "# stale"
    );
}

/// run_project_init with a confirm closure records which files were approved.
#[test]
fn run_project_init_confirm_selectively_overwrites() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale-pyproject").unwrap();
    fs::write(tools.join(".gitignore"), "# stale-gitignore").unwrap();
    let store = TrustDir::new();

    let opts = init_opts();
    run_project_init(
        tmp.path(),
        &opts,
        /*force=*/ false,
        /*no_sync=*/ true,
        /*quiet=*/ true,
        /*yes_all=*/ false,
        Some(&store.store()),
        &mut |dest| {
            // Approve only pyproject.toml.
            Ok(dest.ends_with("pyproject.toml"))
        },
    )
    .unwrap();

    let pyproject = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
    assert!(
        pyproject.contains(r#"name = "tools""#),
        "should be overwritten"
    );
    assert_eq!(
        fs::read_to_string(tools.join(".gitignore")).unwrap(),
        "# stale-gitignore",
        ".gitignore should be preserved"
    );
}

#[test]
fn default_shell_path_uses_shell_env_when_set() {
    let p = with_env_var("SHELL", Some("/custom/sh"), default_shell_path);
    assert_eq!(p, std::path::PathBuf::from("/custom/sh"));
}

#[test]
fn default_shell_path_falls_back_to_per_os_default() {
    let p = with_env_var("SHELL", None, default_shell_path);
    if cfg!(windows) {
        assert_eq!(p, std::path::PathBuf::from("cmd.exe"));
    } else {
        assert_eq!(p, std::path::PathBuf::from("/bin/sh"));
    }
}

#[test]
fn venv_bin_dir_picks_per_os_subdirectory() {
    let venv = std::path::Path::new("/tmp/venv");
    let bin = venv_bin_dir(venv);
    if cfg!(windows) {
        assert_eq!(bin, venv.join("Scripts"));
    } else {
        assert_eq!(bin, venv.join("bin"));
    }
}

#[test]
fn prepend_to_path_with_existing_inserts_at_front() {
    let bin = std::path::Path::new("/venv/bin");
    let existing = if cfg!(windows) {
        "C:\\Windows;C:\\Windows\\System32"
    } else {
        "/usr/bin:/bin"
    };
    let result = prepend_to_path(bin, Some(std::ffi::OsStr::new(existing))).unwrap();
    let paths: Vec<_> = std::env::split_paths(&result).collect();
    assert_eq!(paths[0], bin);
    assert!(paths.len() >= 2);
}

#[test]
fn prepend_to_path_with_no_existing_returns_bin_alone() {
    let bin = std::path::Path::new("/venv/bin");
    let result = prepend_to_path(bin, None).unwrap();
    assert_eq!(result, bin.as_os_str());
}

#[test]
fn dep_name_matches_strips_version_extras_and_markers() {
    assert!(dep_name_matches("toolr-py", "toolr-py"));
    assert!(dep_name_matches("toolr-py>=0.11", "toolr-py"));
    assert!(dep_name_matches("toolr-py[extra]", "toolr-py"));
    assert!(dep_name_matches(
        "toolr-py==0.11.2; python_version < \"3.13\"",
        "toolr-py"
    ));
    assert!(!dep_name_matches("other", "toolr-py"));
}

/// Expected values come from `packaging.utils.canonicalize_name` (26.3).
#[test]
fn normalize_dist_name_matches_packaging_canonicalize_name() {
    for (name, expected) in [
        ("toolr_py", "toolr-py"),
        ("Toolr.Py", "toolr-py"),
        ("toolr--py", "toolr-py"),
        ("toolr-_.py", "toolr-py"),
        ("TOOLR-PY", "toolr-py"),
        // Full Unicode lowercasing like Python's `str.lower()`: `package`
        // comes from the CLI unfiltered, so ASCII-only would diverge.
        ("Ünicode", "ünicode"),
        ("İstanbul", "i\u{307}stanbul"),
        ("_toolr_", "-toolr-"),
        ("toolrpy", "toolrpy"),
    ] {
        assert_eq!(normalize_dist_name(name), expected, "{name:?}");
    }
}

#[test]
fn dep_name_matches_normalises_separators_per_pep_503() {
    assert!(dep_name_matches("Toolr-Py>=0.11", "toolr-py"));
    assert!(dep_name_matches("toolr_py", "toolr-py"));
    assert!(dep_name_matches("Toolr.Py>=1", "toolr-py"));
    assert!(dep_name_matches("toolr--py", "toolr_py"));
    assert!(!dep_name_matches("toolrpy", "toolr-py"));
}

#[test]
fn pyproject_declares_dependency_finds_direct_dep() {
    let tmp = TempDir::new().unwrap();
    let pyproject = tmp.path().join("pyproject.toml");
    fs::write(
        &pyproject,
        r#"[project]
name = "tools"
dependencies = [
    "toolr-py>=0.11",
    "requests",
]
"#,
    )
    .unwrap();
    assert!(pyproject_declares_dependency(&pyproject, "toolr-py").unwrap());
    assert!(pyproject_declares_dependency(&pyproject, "requests").unwrap());
    assert!(!pyproject_declares_dependency(&pyproject, "absent").unwrap());
}

#[test]
fn pyproject_declares_dependency_finds_optional_dep() {
    let tmp = TempDir::new().unwrap();
    let pyproject = tmp.path().join("pyproject.toml");
    fs::write(
        &pyproject,
        r#"[project]
name = "tools"
dependencies = []

[project.optional-dependencies]
dev = ["pytest", "ruff"]
"#,
    )
    .unwrap();
    assert!(pyproject_declares_dependency(&pyproject, "pytest").unwrap());
    assert!(pyproject_declares_dependency(&pyproject, "ruff").unwrap());
    assert!(!pyproject_declares_dependency(&pyproject, "absent").unwrap());
}

#[test]
fn detect_requires_python_reads_the_real_python3() {
    // Probed separately so a broken probe script can't hide behind the
    // floor fallback.
    let floor = toolr_core::execute::python::MIN_PYTHON_MINOR;
    let detected = detect_requires_python();
    let minor: u32 = detected
        .strip_prefix(">=3.")
        .and_then(|m| m.parse().ok())
        .unwrap_or_else(|| panic!("not a >=3.N floor: {detected}"));
    assert!(minor >= floor, "{detected} is below the floor");
    let real = std::process::Command::new("python3")
        .args(["-c", "import sys; print(sys.version_info.minor)"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        });
    let real = real.filter(|&m| m >= floor).unwrap_or(minor);
    assert_eq!(minor, real, "{detected} doesn't match python3 3.{real}");
}

#[test]
fn requires_python_falls_back_to_the_supported_floor() {
    let s = requires_python_from(Some(Path::new("definitely-not-a-real-python-toolr-test")));
    assert_eq!(
        s,
        format!(">=3.{}", toolr_core::execute::python::MIN_PYTHON_MINOR)
    );
}

#[test]
#[cfg(unix)]
fn requires_python_raises_a_too_old_interpreter_to_the_supported_floor() {
    let bin = tempfile::TempDir::new().unwrap();
    let python = stub_python(bin.path(), "3.9.18");
    assert_eq!(
        requires_python_from(Some(&python)),
        format!(">=3.{}", toolr_core::execute::python::MIN_PYTHON_MINOR)
    );
}

#[test]
#[cfg(unix)]
fn requires_python_keeps_a_newer_interpreter_version() {
    let bin = tempfile::TempDir::new().unwrap();
    let python = stub_python(bin.path(), "3.99.0");
    assert_eq!(requires_python_from(Some(&python)), ">=3.99");
}

/// A `python` that reports `Python {version}` for whatever it is asked to run.
#[cfg(unix)]
fn stub_python(dir: &std::path::Path, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("python");
    std::fs::write(&path, format!("#!/bin/sh\necho 'Python {version}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn build_upgrade_mode_none_when_nothing_set() {
    use toolr_core::venv::UpgradeMode;
    let m = build_upgrade_mode(false, vec![]);
    assert!(matches!(m, UpgradeMode::None));
}

#[test]
fn build_upgrade_mode_all_when_upgrade_flag_set() {
    use toolr_core::venv::UpgradeMode;
    let m = build_upgrade_mode(true, vec![]);
    assert!(matches!(m, UpgradeMode::All));
}

#[test]
fn build_upgrade_mode_all_wins_over_packages_when_both_set() {
    use toolr_core::venv::UpgradeMode;
    let m = build_upgrade_mode(true, vec!["foo".into()]);
    assert!(matches!(m, UpgradeMode::All));
}

#[test]
fn build_upgrade_mode_packages_when_only_dash_p_set() {
    use toolr_core::venv::UpgradeMode;
    let m = build_upgrade_mode(false, vec!["foo".into(), "bar".into()]);
    match m {
        UpgradeMode::Packages(p) => assert_eq!(p, vec!["foo".to_string(), "bar".to_string()]),
        other => panic!("expected Packages, got {other:?}"),
    }
}
