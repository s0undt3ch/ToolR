//! Drive `uv add` / `uv remove` against the tools venv. These commands
//! edit `tools/pyproject.toml` and internally run `uv lock` + `uv sync`,
//! so on success the venv reflects the new state.

use std::path::Path;
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result};

use crate::uv::UvBinary;

use super::resolve::ResolvedVenv;
use super::sync::touch_marker_after_success;

/// Run `uv add <specs...> --project <tools>` synchronously. uv mutates
/// `tools/pyproject.toml`, refreshes `tools/uv.lock`, and re-syncs the
/// environment in one call.
pub fn run_uv_add(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    specs: &[String],
    quiet: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new(&uv.path); // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
    cmd.arg("add")
        .args(specs)
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    let status = cmd
        .status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
    if status.success() {
        touch_marker_after_success(&resolved.venv_dir)?;
    }
    Ok(status)
}

/// Run `uv remove <packages...> --project <tools>` synchronously. Same
/// shape as [`run_uv_add`]; uv drops the listed entries from pyproject
/// and re-syncs.
pub fn run_uv_remove(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    packages: &[String],
    quiet: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new(&uv.path); // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
    cmd.arg("remove")
        .args(packages)
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    let status = cmd
        .status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
    if status.success() {
        touch_marker_after_success(&resolved.venv_dir)?;
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uv::UvSource;
    use crate::venv::config::ToolrConfig;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn dummy_resolved(venv_dir: PathBuf) -> ResolvedVenv {
        ResolvedVenv {
            venv_dir: venv_dir.clone(),
            python: venv_dir.join("bin").join("python"),
            repo_key: "x".into(),
            python_version: "3.13".into(),
            config: ToolrConfig::default(),
        }
    }

    #[cfg(unix)]
    fn stub_uv(tmp: &Path, argdump: &Path) -> UvBinary {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let stub = tmp.join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0", argdump.display()).unwrap();
        drop(f);
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();
        UvBinary { path: stub, version: (0, 4, 0), source: UvSource::Path }
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_add_passes_specs_and_project() {
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let uv = stub_uv(tmp.path(), &argdump);
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        let specs = vec!["httpx".to_string(), "rich@13.7".to_string()];
        run_uv_add(&uv, tmp.path(), &resolved, &specs, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "add"), "args: {dump}");
        assert!(dump.contains("httpx"), "args: {dump}");
        assert!(dump.contains("rich@13.7"), "args: {dump}");
        assert!(dump.contains("--project"), "args: {dump}");
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_remove_passes_packages_and_project() {
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let uv = stub_uv(tmp.path(), &argdump);
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        let pkgs = vec!["httpx".to_string()];
        run_uv_remove(&uv, tmp.path(), &resolved, &pkgs, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "remove"), "args: {dump}");
        assert!(dump.contains("httpx"), "args: {dump}");
        assert!(dump.contains("--project"), "args: {dump}");
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_add_propagates_quiet() {
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let uv = stub_uv(tmp.path(), &argdump);
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        run_uv_add(&uv, tmp.path(), &resolved, &["foo".to_string()], /*quiet=*/ true)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "--quiet"), "args: {dump}");
    }
}
