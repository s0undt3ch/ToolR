//! Shared test scaffolding for the binary's integration tests.
//!
//! `VenvFixture` builds a temp dir that mimics a working toolr-using project:
//!   - `tools/pyproject.toml` with `venv-location = "in-tree"` and a
//!     minimal `[project] dependencies = [...]` list.
//!   - A fake tools venv at `tools/.venv/` with `bin/python` and a
//!     `lib/python3.13/site-packages/toolr/__init__.py` so
//!     `validate_venv` passes.
//!   - A `bin/uv` stub (`#!/bin/sh`) that responds to `--version` and
//!     exits 0 for every other subcommand. Logs argv to a file so tests
//!     can inspect what uv would have been called with.
//!
//! Tests run `common::toolr().env("PATH", &fx.bin_dir)
//! .current_dir(&fx.root)` to make toolr find the stub uv only.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
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

        let fx = Self {
            _tmp: tmp,
            root,
            tools_dir,
            venv_dir,
            bin_dir,
            uv_argv_log,
        };
        fx.write_uv_stub("");
        fx
    }

    /// (Re)write the stub uv, running `sync_action` (a shell snippet) on
    /// `uv sync`. PATH holds only the stub dir, so use absolute tool paths.
    pub fn write_uv_stub(&self, sync_action: &str) {
        let stub_body = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {log}\ncase \"$1\" in\n  --version) echo 'uv 0.5.0';;\n  sync) {sync_action}\n    ;;\nesac\nexit 0\n",
            log = self.uv_argv_log.display()
        );
        write_executable(&self.bin_dir.join("uv"), &stub_body);
    }

    /// Read the captured uv argv lines (each token on a separate line).
    pub fn uv_argv(&self) -> String {
        fs::read_to_string(&self.uv_argv_log).unwrap_or_default()
    }

    /// Whether the stub uv has been called with `sync`.
    pub fn uv_ran_sync(&self) -> bool {
        self.uv_argv().lines().any(|token| token == "sync")
    }

    /// Add `rich` to `tools/pyproject.toml` without re-locking. Sleeps first
    /// so the edit is newer than any sync stamp.
    pub fn add_unlocked_dependency(&self) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let pyproject = self.tools_dir.join("pyproject.toml");
        let body = fs::read_to_string(&pyproject).unwrap();
        let edited = body.replace("\"requests\",", "\"requests\",\n    \"rich\",");
        assert_ne!(
            body, edited,
            "fixture pyproject has no \"requests\" entry to extend"
        );
        fs::write(&pyproject, edited).unwrap();
    }
}

pub const GREET: &str = "\"\"\"Greetings.\"\"\"\nfrom toolr import command_group\ngroup = command_group(\"greet\", \"Greetings\")\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n";

/// A toolr project with one local group and an in-tree venv for plugin fragments.
pub struct Project {
    pub tmp: TempDir,
    pub root: PathBuf,
}

impl Project {
    /// A project with one local group and an (initially empty) in-tree venv.
    pub fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        Self::at(tmp, root)
    }

    /// Like `new`, but rooted at `<tmp>/<subdir>`.
    pub fn under(subdir: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join(subdir);
        Self::at(tmp, root)
    }

    fn at(tmp: TempDir, root: PathBuf) -> Self {
        let tools = root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("pyproject.toml"),
            "[project]\nname=\"demo\"\nversion=\"0\"\n",
        )
        .unwrap();
        std::fs::write(tools.join("greet.py"), GREET).unwrap();
        let venv = tools.join(".venv");
        std::fs::create_dir_all(&venv).unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "home = /usr\n").unwrap();
        Self { tmp, root }
    }

    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    pub fn write_tool(&self, name: &str, body: &str) {
        std::fs::write(self.tools().join(name), body).unwrap();
    }

    pub fn add_plugin(&self, package: &str, fragment: &str) {
        let dir = self
            .tools()
            .join(".venv/lib/python3.13/site-packages")
            .join(package);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("toolr-manifest.json"), fragment).unwrap();
    }

    pub fn toolr(&self, args: &[&str]) -> Command {
        let mut cmd = toolr();
        cmd.args(args)
            .current_dir(&self.root)
            .env("TOOLR_VENV_LOCATION", "in-tree")
            .env("TOOLR_NO_CACHE_HINT", "1");
        cmd
    }

    pub fn stderr(&self, args: &[&str]) -> String {
        let out = self.toolr(args).output().unwrap();
        assert!(
            out.status.success(),
            "toolr {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stderr).unwrap()
    }

    pub fn stdout(&self, args: &[&str]) -> String {
        let out = self.toolr(args).output().unwrap();
        assert!(out.status.success(), "toolr {args:?} failed");
        String::from_utf8(out.stdout).unwrap()
    }

    pub fn manifest(&self) -> String {
        std::fs::read_to_string(self.tools().join(".toolr-manifest.json")).unwrap()
    }
}

#[cfg(unix)]
pub fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(not(unix))]
pub fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
}

/// A repository whose code drops `sentinel` the moment any of it runs:
/// `tools/hello.py` writes it at import time, and a committed in-tree
/// `tools/.venv/bin/python` writes it when executed.
#[cfg(unix)]
pub fn hostile_repo(sentinel: &Path) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    let sentinel_str = sentinel.display().to_string();
    fs::write(
        tools.join("hello.py"),
        format!(
            "\"\"\"Hi.\"\"\"\nopen({sentinel_str:?}, \"w\").write(\"pwned\")\n\
             from toolr import command_group\ngroup = command_group(\"hello\", \"Hi\")\n\
             @group.command\ndef world(ctx):\n    \"\"\"World.\"\"\"\n"
        ),
    )
    .unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname=\"evil\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    let venv_bin = tools.join(".venv").join("bin");
    fs::create_dir_all(&venv_bin).unwrap();
    write_executable(
        &venv_bin.join("python"),
        &format!("#!/bin/sh\necho pwned > {sentinel_str}\n"),
    );
    tmp
}

/// Isolated toolr state for trust tests: no `TOOLR_TRUST_ALL`, even one
/// inherited from the caller's shell, and private state/cache/data dirs, so
/// the real trust store is never read or written.
pub struct TrustEnv {
    pub state: TempDir,
    pub cache: TempDir,
    pub data: TempDir,
}

impl TrustEnv {
    pub fn new() -> Self {
        Self {
            state: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
            data: TempDir::new().unwrap(),
        }
    }

    pub fn toolr(&self, cwd: &Path) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("toolr").unwrap();
        cmd.current_dir(cwd)
            .env_remove("TOOLR_TRUST_ALL")
            .env("XDG_STATE_HOME", self.state.path())
            .env("XDG_CACHE_HOME", self.cache.path())
            .env("XDG_DATA_HOME", self.data.path())
            .env("TOOLR_NO_CACHE_HINT", "1");
        cmd
    }
}

/// The toolr binary as tests run it. Every test drives a throwaway repository,
/// so the trust check is off; trust tests use [`TrustEnv`] instead.
pub fn toolr() -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("toolr").unwrap();
    cmd.env("TOOLR_TRUST_ALL", "1");
    cmd
}

/// [`toolr`] for tests that need a `std::process::Command`.
pub fn toolr_std() -> std::process::Command {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("toolr"));
    cmd.env("TOOLR_TRUST_ALL", "1");
    cmd
}

/// `tools/pyproject.toml` for fixtures: toolr only treats a `tools/` with one
/// as a project. The in-tree venv keeps the fixture away from the real cache.
pub const TOOLS_PYPROJECT: &str = "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n";

/// Make `root` a toolr project: create `tools/` and its `pyproject.toml`.
/// Returns the `tools/` path.
pub fn write_project(root: &Path) -> PathBuf {
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), TOOLS_PYPROJECT).unwrap();
    tools
}

/// Write a hand-built manifest into `root`'s project, stamped fresh against
/// the `tools/*.py` already there and the in-tree venv, so the freshness
/// check keeps it as written instead of rebuilding it from source.
pub fn write_fresh_manifest(root: &Path, manifest_json: &str) {
    let tools = write_project(root);
    let mut manifest: serde_json::Value = serde_json::from_str(manifest_json).unwrap();
    manifest["schema_version"] = toolr_core::manifest::SCHEMA_VERSION.into();
    manifest["toolr_version"] = env!("CARGO_PKG_VERSION").into();
    manifest["static_hash"] = toolr_core::hash::hash_tools_dir(&tools).unwrap().into();
    manifest["third_party_hash"] =
        toolr_core::manifest_build::compute_third_party_hash(&tools.join(".venv"))
            .unwrap()
            .into();
    fs::write(
        tools.join(".toolr-manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}
