//! Spawn `python -m toolr._introspect` in the tools venv and capture its
//! JSON payload.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

use super::payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};

#[derive(Debug, Error)]
pub enum IntrospectError {
    #[error("python interpreter not found at {0}")]
    PythonMissing(PathBuf),
    #[error("introspect helper exited with status {status:?}\nstderr:\n{stderr}")]
    SubprocessFailed { status: Option<i32>, stderr: String },
    #[error("I/O while spawning introspect helper: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON decode error in introspect payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("introspect payload schema {got}, this toolr understands {expected}")]
    UnsupportedPayloadSchema { got: u32, expected: u32 },
}

/// Run the dynamic introspection helper.
///
/// `python` is the absolute path to the Python interpreter inside the
/// tools venv (resolved by `toolr_core::venv` from Plan 3). `tools_dir`
/// is the project's `tools/` directory; the helper inserts its parent on
/// `sys.path` before importing.
pub fn run_introspect(python: &Path, tools_dir: &Path) -> Result<DynamicPayload, IntrospectError> {
    if !python.is_file() {
        return Err(IntrospectError::PythonMissing(python.to_path_buf()));
    }
    let output = Command::new(python)
        .args(["-m", "toolr._introspect", "--tools-root"])
        .arg(tools_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        return Err(IntrospectError::SubprocessFailed {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let payload: DynamicPayload = serde_json::from_slice(&output.stdout)?;
    if payload.payload_schema_version != PAYLOAD_SCHEMA_VERSION {
        return Err(IntrospectError::UnsupportedPayloadSchema {
            got: payload.payload_schema_version,
            expected: PAYLOAD_SCHEMA_VERSION,
        });
    }
    Ok(payload.retag_as_dynamic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a fake "python" shell script that prints a fixed JSON payload
    /// and exits 0. Lets us test the runner without a real venv.
    #[cfg(unix)]
    fn fake_python(tmp: &TempDir, body: &str) -> PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let path = tmp.path().join("python");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "cat <<'__EOF__'").unwrap();
        writeln!(f, "{body}").unwrap();
        writeln!(f, "__EOF__").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    #[cfg(unix)]
    fn happy_path_deserializes_and_retags_as_dynamic() {
        let tmp = TempDir::new().unwrap();
        let py = fake_python(
            &tmp,
            r#"{"payload_schema_version":1,"groups":[{"name":"x","title":"X","description":"","origin":"static"}],"commands":[],"warnings":[]}"#,
        );
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        let p = run_introspect(&py, &tools).unwrap();
        assert_eq!(p.groups.len(), 1);
        // Python said "static"; runner retagged to dynamic.
        assert_eq!(p.groups[0].origin, crate::manifest::Origin::Dynamic);
    }

    #[test]
    fn missing_python_returns_clear_error() {
        let tmp = TempDir::new().unwrap();
        let py = tmp.path().join("no-such-python");
        let err = run_introspect(&py, tmp.path()).expect_err("should fail");
        assert!(matches!(err, IntrospectError::PythonMissing(_)));
    }
}
