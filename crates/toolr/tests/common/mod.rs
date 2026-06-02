//! Shared test scaffolding for the `project_venv_*` integration tests.
//!
//! Builds a temp dir that mimics a working toolr-using project:
//!   - `tools/pyproject.toml` with `venv-location = "in-tree"` and a
//!     minimal `[project] dependencies = [...]` list.
//!   - A fake tools venv at `tools/.venv/` with `bin/python` and a
//!     `lib/python3.13/site-packages/toolr/__init__.py` so
//!     `validate_venv` passes.
//!   - A `bin/uv` stub (`#!/bin/sh`) that responds to `--version` and
//!     exits 0 for every other subcommand. Logs argv to a file so tests
//!     can inspect what uv would have been called with.
//!
//! Tests run `Command::cargo_bin("toolr").env("PATH", &fx.bin_dir)
//! .current_dir(&fx.root)` to make toolr find the stub uv only.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub struct VenvFixture {
    /// Backing tempdir keeping the files alive.
    pub _tmp: TempDir,
    pub root: PathBuf,
    pub tools_dir: PathBuf,
    pub venv_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub uv_argv_log: PathBuf,
}

impl VenvFixture {
    pub fn new() -> Self {
        Self::with_dependencies(&["requests", "toolr-py>=0.21"])
    }

    pub fn with_dependencies(deps: &[&str]) -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let tools_dir = root.join("tools");
        fs::create_dir(&tools_dir).unwrap();

        let dep_lines = deps
            .iter()
            .map(|d| format!("    \"{d}\","))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            tools_dir.join("pyproject.toml"),
            format!(
                "[project]\nname = \"tools\"\nversion = \"0.0.0\"\nrequires-python = \">=3.11\"\ndependencies = [\n{dep_lines}\n]\n\n[tool.toolr]\nvenv-location = \"in-tree\"\npython-version = \"3.13\"\n"
            ),
        )
        .unwrap();

        // Minimal uv.lock so freshness check finds it (marker will be missing
        // → Freshness::Missing → uv runs → stub exits 0 → marker gets written).
        fs::write(tools_dir.join("uv.lock"), "version = 1\n").unwrap();

        // Fake venv that validate_venv will accept.
        let venv_dir = tools_dir.join(".venv");
        let site_packages = venv_dir.join("lib/python3.13/site-packages/toolr");
        fs::create_dir_all(&site_packages).unwrap();
        fs::write(site_packages.join("__init__.py"), "").unwrap();
        let venv_bin = venv_dir.join("bin");
        fs::create_dir_all(&venv_bin).unwrap();
        write_executable(&venv_bin.join("python"), "#!/bin/sh\nexit 0\n");

        // Stub uv on PATH — responds to --version and exits 0 for everything
        // else while logging each argv token to uv_argv_log (one per line).
        let bin_dir = root.join("test-bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let uv_argv_log = root.join("uv-argv.log");
        let stub_body = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {log}\ncase \"$1\" in\n  --version) echo 'uv 0.5.0'; exit 0;;\n  *) exit 0;;\nesac\n",
            log = uv_argv_log.display()
        );
        write_executable(&bin_dir.join("uv"), &stub_body);

        Self {
            _tmp: tmp,
            root,
            tools_dir,
            venv_dir,
            bin_dir,
            uv_argv_log,
        }
    }

    /// Read the captured uv argv lines (each token on a separate line).
    pub fn uv_argv(&self) -> String {
        fs::read_to_string(&self.uv_argv_log).unwrap_or_default()
    }
}

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(not(unix))]
fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
}
