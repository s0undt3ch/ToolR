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
mod tests;
