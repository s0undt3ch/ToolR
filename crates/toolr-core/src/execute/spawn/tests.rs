use super::*;
use std::path::PathBuf;

#[test]
fn spawn_with_nonexistent_python_returns_io_error() {
    let bogus = PathBuf::from("/definitely/not/a/real/python-binary-xyz");
    let result = spawn_runner(&bogus, Path::new("/tmp/whatever.json"), Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn spawn_runner_passes_safe_path_flag_before_module() {
    // We can't run a real interpreter here; assert the argv we build.
    let args = runner_args();
    assert_eq!(args, ["-P", "-m", "toolr._runner"]);
}

#[test]
fn runner_command_leaves_a_user_set_io_encoding_alone() {
    let cmd = runner_command(
        Path::new("python"),
        Path::new("spec.json"),
        Path::new("."),
        true,
    );
    assert!(cmd.get_envs().all(|(k, _)| k != "PYTHONIOENCODING"));
}

/// Runs a stand-in interpreter that records `PYTHONIOENCODING` as it sees it.
#[cfg(unix)]
fn io_encoding_seen_by_child(user_value: Option<&str>) -> String {
    use crate::test_env::EnvGuard;
    use std::os::unix::fs::PermissionsExt;

    let mut env = EnvGuard::write();
    match user_value {
        Some(v) => env.set("PYTHONIOENCODING", v),
        None => env.remove("PYTHONIOENCODING"),
    }
    let dir = tempfile::TempDir::new().unwrap();
    let python = dir.path().join("fake-python");
    std::fs::write(
        &python,
        "#!/bin/sh\nprintf '%s' \"${PYTHONIOENCODING-unset}\" > \"$TOOLR_SPEC_FILE.out\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec = dir.path().join("spec.json");
    let mut child = spawn_runner(&python, &spec, dir.path()).unwrap();
    assert!(child.wait().unwrap().success());
    std::fs::read_to_string(dir.path().join("spec.json.out")).unwrap()
}

#[cfg(unix)]
#[test]
fn spawned_runner_sees_utf8_stdio_unless_the_user_chose() {
    assert_eq!(io_encoding_seen_by_child(None), "utf-8");
    assert_eq!(io_encoding_seen_by_child(Some("latin-1")), "latin-1");
    assert_eq!(io_encoding_seen_by_child(Some("")), "utf-8");
}
