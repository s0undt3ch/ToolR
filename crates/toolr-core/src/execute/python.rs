//! Host Python version probing. `toolr project init` reads the host
//! `python3`'s minor to pick the default `requires-python`; commands always
//! run with the tools venv's interpreter (`toolr_core::venv::resolve_venv_path`).

use std::path::Path;
use std::process::Command;

/// Oldest Python 3 minor toolr supports. Keep in step with `requires-python`
/// in `crates/toolr-py/pyproject.toml` (enforced by `tests/python_floor_lockstep.rs`);
/// the runner also spawns with `-P` (3.11+).
pub const MIN_PYTHON_MINOR: u32 = 11;

/// The Python 3 minor `<exe> --version` reports, or `None` if it doesn't run
/// or isn't Python 3.
pub fn python3_minor(exe: &Path) -> Option<u32> {
    parse_minor(&version_text(exe).ok()?)
}

/// `<exe> --version` output, or why it couldn't be read.
fn version_text(exe: &Path) -> Result<String, String> {
    let output = Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("it does not run ({e})"))?;
    if !output.status.success() {
        return Err("`--version` failed".to_string());
    }
    // Python 2 printed its version to stderr.
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn parse_minor(version_output: &str) -> Option<u32> {
    let rest = version_output.trim().strip_prefix("Python 3.")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
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
}
