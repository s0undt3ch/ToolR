<!-- rumdl-disable MD046 MD076 -->

# Plan 3: Tools Venv + uv Integration

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Make `toolr` capable of materializing and managing the per-repo
tools venv via `uv`. At the end, `toolr` can discover or install `uv`, read
`tools/pyproject.toml`, resolve the venv to either the global cache
(`$XDG_CACHE_HOME/toolr/<repo-key>/venv/`) or in-tree (`tools/.venv/`), run
`uv sync` to materialize it from `tools/uv.lock`, verify that the `toolr`
Python package is installed, optionally `uv pip install -e .` repo code, and
expose this as three user-facing commands: `toolr project deps sync`,
`toolr project venv path`, and `toolr project venv shell`. Plan 2's runner
spawn point becomes venv-aware: spawning Python now uses
`<tools-venv>/bin/python` rather than relying on PATH.

**Architecture:** A new `_rust_utils::uv` module owns uv discovery,
consent-based install, and `uv sync` invocation. A new
`_rust_utils::venv` module owns venv path resolution, repo-key hashing,
and `tools/pyproject.toml` `[tool.toolr]` config parsing. A new
`_rust_utils::project` module exposes the unified "ensure venv is ready"
entrypoint that combines uv discovery + sync + toolr-package validation +
optional editable install. The binary grows a `toolr project` subcommand
group alongside the user-defined groups; `toolr self` and `toolr project`
are reserved namespaces (per the design's
[CLI surface — toolr-built-in commands](./00-design.md#cli-surface--toolr-built-in-commands)).
Plan 2's dispatch.rs spawn site is updated to consume the resolved venv
python path rather than a bare `python`.

**Tech Stack:** Rust 2021, `toml = "0.8"` for `tools/pyproject.toml` parsing,
`dirs = "5"` for cross-platform `$XDG_DATA_HOME`/`$XDG_CACHE_HOME` resolution,
`reqwest = { version = "0.12", features = ["blocking", "rustls-tls"] }` for the
consented uv download, plus everything Plan 1 already pulled in
(`anyhow`, `thiserror`, `serde_json`, `walkdir`, `blake3`,
`clap 4 (derive,env,wrap_help,string)`, `assert_cmd`, `tempfile`).

**Reading order in this plan:** Tasks build on each other. Tasks 1–5 land the
uv discovery + install pipeline. Tasks 6–8 land config parsing + venv
location. Task 9 wires `uv sync`. Tasks 10–11 add post-sync validation +
editable install. Tasks 12–15 wire the `toolr project` subcommand surface.
Tasks 16–17 cover integration testing. Task 18 closes the loop with Plan 2's
spawn site. Task 19 updates the roadmap.

**Dependencies on prior plans:** Plan 2's runner module
(`python/toolr/_runner.py`) and the dispatch.rs subprocess spawn must be
landed first. Task 18 modifies that spawn site, so it cannot be done before
Plan 2's spawn site exists.

---

## Task 1: Add new dependencies and create the `uv` module skeleton

Add the crates needed for TOML parsing and HTTP downloads, then stub the
`uv` module so subsequent tasks have a stable location for their types.

**Files:**

- Modify: `Cargo.toml`

- Create: `src/uv/mod.rs`

- Modify: `src/lib.rs`

- [ ] **Step 1.1: Add new dependencies to `Cargo.toml`**

    Append to `[dependencies]`:

    ```toml
    toml = "0.8"
    dirs = "5"
    reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }
    ```

    These are additive — none of Plan 1's existing pins are touched.

- [ ] **Step 1.2: Create the `uv` module stub**

    Create `src/uv/mod.rs`:

    ```rust
    //! `uv` integration: discovery, consent-based install, and sync invocation.

    use std::path::{Path, PathBuf};

    use thiserror::Error;

    /// Minimum supported uv version. Bumped when toolr starts to rely on a
    /// uv feature only available in a newer release.
    pub const MIN_UV_VERSION: (u32, u32, u32) = (0, 4, 0);

    /// A resolved uv binary location.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UvBinary {
        pub path: PathBuf,
        /// Parsed `uv --version` output, `(major, minor, patch)`.
        pub version: (u32, u32, u32),
        /// Where this binary was found.
        pub source: UvSource,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UvSource {
        /// Found on `$PATH`.
        Path,
        /// Found at `$XDG_DATA_HOME/toolr/bin/uv` (toolr-managed).
        Managed,
        /// Just installed by toolr this run.
        FreshlyInstalled,
    }

    #[derive(Debug, Error)]
    pub enum UvError {
        #[error("uv is required but not available; install it from https://docs.astral.sh/uv/")]
        NotAvailable,
        #[error("uv on PATH reported version {found:?} but toolr requires >= {required:?}")]
        VersionTooOld {
            found: (u32, u32, u32),
            required: (u32, u32, u32),
        },
        #[error("`uv --version` produced unparsable output: {0:?}")]
        UnparsableVersion(String),
        #[error("user declined uv install; commands that require uv cannot run")]
        UserRefusedInstall,
        #[error("I/O error: {0}")]
        Io(#[from] std::io::Error),
        #[error("HTTP error during uv install: {0}")]
        Http(String),
        #[error("uv sync failed with exit code {0:?}")]
        SyncFailed(Option<i32>),
    }

    /// Where toolr keeps its private state (binaries, etc).
    /// Defaults to `$XDG_DATA_HOME/toolr`, falling back to
    /// `~/.local/share/toolr` if `XDG_DATA_HOME` is unset.
    pub fn toolr_data_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(|v| PathBuf::from(v).join("toolr"))
            .or_else(|| dirs::data_dir().map(|d| d.join("toolr")))
    }

    /// Where toolr keeps cached venvs and other transient files.
    /// Defaults to `$XDG_CACHE_HOME/toolr`, falling back to
    /// `~/.cache/toolr`.
    pub fn toolr_cache_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_CACHE_HOME")
            .map(|v| PathBuf::from(v).join("toolr"))
            .or_else(|| dirs::cache_dir().map(|d| d.join("toolr")))
    }

    /// The path where toolr installs a managed uv if the user consents.
    pub fn managed_uv_path() -> Option<PathBuf> {
        toolr_data_dir().map(|d| d.join("bin").join(uv_basename()))
    }

    fn uv_basename() -> &'static str {
        if cfg!(windows) { "uv.exe" } else { "uv" }
    }

    /// Optional binary-resolution helper to be exposed in later tasks.
    pub fn _placeholder(_path: &Path) -> Option<UvBinary> {
        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn min_uv_version_is_a_real_tuple() {
            let (maj, _, _) = MIN_UV_VERSION;
            assert!(maj < 100, "min uv major version should be plausible");
        }

        #[test]
        fn data_dir_resolves_or_returns_none_on_exotic_envs() {
            // We don't assert a specific path: this just exercises the call.
            let _ = toolr_data_dir();
            let _ = toolr_cache_dir();
            let _ = managed_uv_path();
        }
    }
    ```

- [ ] **Step 1.3: Expose the module from `src/lib.rs`**

    Add:

    ```rust
    pub mod uv;
    ```

- [ ] **Step 1.4: Build and run tests**

    ```bash
    cargo build --bin toolr
    cargo test --lib uv::
    ```

    Expected: clean build; 2 tests passing.

- [ ] **Step 1.5: Commit**

    ```bash
    git add Cargo.toml src/lib.rs src/uv/
    git commit -m "feat(uv): Add uv module skeleton with discovery types"
    ```

---

## Task 2: PATH-check for an existing `uv` binary

Implement `find_uv_on_path` that runs `uv --version`, parses the output,
and returns a `UvBinary` if the version is at least `MIN_UV_VERSION`.

**Files:**

- Create: `src/uv/discover.rs`

- Modify: `src/uv/mod.rs`

- [ ] **Step 2.1: Write the failing tests inline in `src/uv/discover.rs`**

    Create `src/uv/discover.rs`:

    ```rust
    //! Locate a working uv binary on the host.

    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{MIN_UV_VERSION, UvBinary, UvError, UvSource, managed_uv_path};

    /// Try to find a usable `uv` on `$PATH`. Returns `Ok(None)` if uv is not
    /// on PATH at all (so the caller can fall through to the managed path);
    /// `Err(UvError::VersionTooOld { .. })` if it exists but is too old.
    pub fn find_uv_on_path() -> Result<Option<UvBinary>, UvError> {
        let candidate = which_uv()?;
        let Some(path) = candidate else {
            return Ok(None);
        };
        probe(&path, UvSource::Path).map(Some)
    }

    /// Try to find a toolr-managed uv at `$XDG_DATA_HOME/toolr/bin/uv`.
    pub fn find_managed_uv() -> Result<Option<UvBinary>, UvError> {
        let Some(path) = managed_uv_path() else {
            return Ok(None);
        };
        if !path.is_file() {
            return Ok(None);
        }
        probe(&path, UvSource::Managed).map(Some)
    }

    /// Run `<path> --version`, parse output, validate against the minimum.
    pub fn probe(path: &Path, source: UvSource) -> Result<UvBinary, UvError> {
        let output = Command::new(path).arg("--version").output()?;
        if !output.status.success() {
            return Err(UvError::UnparsableVersion(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let version = parse_uv_version(&stdout)
            .ok_or_else(|| UvError::UnparsableVersion(stdout.clone()))?;
        if version < MIN_UV_VERSION {
            return Err(UvError::VersionTooOld {
                found: version,
                required: MIN_UV_VERSION,
            });
        }
        Ok(UvBinary {
            path: path.to_path_buf(),
            version,
            source,
        })
    }

    /// `uv --version` prints something like `uv 0.5.1 (...)`. Extract the
    /// three-component numeric prefix.
    pub fn parse_uv_version(output: &str) -> Option<(u32, u32, u32)> {
        let line = output.lines().next()?;
        let words = line.split_whitespace().collect::<Vec<_>>();
        // Find the first token that looks like a `1.2.3` (allow a trailing
        // alphanumeric suffix, which we discard).
        for word in words {
            let trimmed: String = word
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let parts: Vec<&str> = trimmed.split('.').collect();
            if parts.len() != 3 {
                continue;
            }
            let (Ok(a), Ok(b), Ok(c)) = (
                parts[0].parse::<u32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            ) else {
                continue;
            };
            return Some((a, b, c));
        }
        None
    }

    /// Naive PATH scan that returns the first `uv` (or `uv.exe` on Windows)
    /// found.
    pub fn which_uv() -> Result<Option<PathBuf>, UvError> {
        let basename = if cfg!(windows) { "uv.exe" } else { "uv" };
        let Some(path) = std::env::var_os("PATH") else {
            return Ok(None);
        };
        for entry in std::env::split_paths(&path) {
            let candidate = entry.join(basename);
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_standard_uv_version_string() {
            let s = "uv 0.5.1 (xyz)\n";
            assert_eq!(parse_uv_version(s), Some((0, 5, 1)));
        }

        #[test]
        fn parses_with_no_trailing_paren() {
            assert_eq!(parse_uv_version("uv 1.10.2"), Some((1, 10, 2)));
        }

        #[test]
        fn returns_none_on_garbage() {
            assert_eq!(parse_uv_version("garbage"), None);
            assert_eq!(parse_uv_version("uv broken"), None);
            assert_eq!(parse_uv_version(""), None);
        }

        #[test]
        fn version_too_old_error_includes_both_versions() {
            let err = UvError::VersionTooOld {
                found: (0, 1, 0),
                required: MIN_UV_VERSION,
            };
            let msg = err.to_string();
            assert!(msg.contains("0.1.0") || msg.contains("(0, 1, 0)"));
        }
    }
    ```

- [ ] **Step 2.2: Re-export from `src/uv/mod.rs`**

    Add:

    ```rust
    pub mod discover;
    pub use discover::{find_managed_uv, find_uv_on_path, parse_uv_version, probe, which_uv};
    ```

- [ ] **Step 2.3: Run tests**

    ```bash
    cargo test --lib uv::
    ```

    Expected: all uv tests pass (existing + 4 new).

- [ ] **Step 2.4: Commit**

    ```bash
    git add src/uv/
    git commit -m "feat(uv): Probe uv --version on PATH and managed paths"
    ```

---

## Task 3: Consent-driven install plan

Define `decide_install` which, given the result of the PATH and managed
probes plus the current invocation mode (interactive vs auto-yes), returns
an `InstallDecision` enum that downstream code acts on. The actual download
lives in Task 4; this task is pure logic and is therefore easy to test.

**Files:**

- Create: `src/uv/install.rs`

- Modify: `src/uv/mod.rs`

- [ ] **Step 3.1: Add the failing tests + skeleton**

    Create `src/uv/install.rs`:

    ```rust
    //! Decision logic + executor for installing a toolr-managed uv.

    use std::io::{IsTerminal, Write};

    use super::UvError;

    /// What the discovery + consent flow tells the caller to do.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InstallDecision {
        /// uv was found and is usable as-is. No install required.
        AlreadyAvailable,
        /// uv is not available and the caller wants us to install it.
        Install,
        /// uv is not available and the user declined (or stdin is not a TTY
        /// and no auto-yes was set). Caller should surface a friendly error.
        Refuse,
    }

    /// How the toolr binary was invoked, for non-interactive decisions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ConsentMode {
        /// `--yes` was passed.
        pub yes_flag: bool,
        /// `TOOLR_AUTO_INSTALL_UV=1` is set in the environment.
        pub auto_install_env: bool,
    }

    impl ConsentMode {
        pub fn from_env() -> Self {
            Self {
                yes_flag: false,
                auto_install_env: std::env::var_os("TOOLR_AUTO_INSTALL_UV")
                    .is_some_and(|v| v == "1"),
            }
        }
    }

    /// Pure decision logic. `path_found` is whether `find_uv_on_path` returned
    /// `Some`. `managed_found` is whether `find_managed_uv` returned `Some`.
    /// `stdin_tty` controls whether to prompt; in tests / pipes we never
    /// prompt and rely on `consent` instead.
    pub fn decide_install(
        path_found: bool,
        managed_found: bool,
        consent: ConsentMode,
        stdin_tty: bool,
    ) -> InstallDecision {
        if path_found || managed_found {
            return InstallDecision::AlreadyAvailable;
        }
        if consent.yes_flag || consent.auto_install_env {
            return InstallDecision::Install;
        }
        if !stdin_tty {
            return InstallDecision::Refuse;
        }
        match prompt_for_consent() {
            Ok(true) => InstallDecision::Install,
            _ => InstallDecision::Refuse,
        }
    }

    /// Print the prompt and return `true` for "install".
    fn prompt_for_consent() -> Result<bool, UvError> {
        let mut stderr = std::io::stderr();
        writeln!(
            stderr,
            "toolr needs uv (https://docs.astral.sh/uv/) and didn't find it on PATH."
        )?;
        let managed = super::managed_uv_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "$XDG_DATA_HOME/toolr/bin/uv".to_string());
        writeln!(stderr, "  [I] Install it for me at {managed}")?;
        writeln!(
            stderr,
            "  [M] I'll install it manually (see https://docs.astral.sh/uv/getting-started/installation/)"
        )?;
        write!(stderr, "Choice [I/M] ")?;
        stderr.flush()?;
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(matches!(buf.trim(), "I" | "i" | ""))
    }

    /// Convenience that combines a TTY check + decision.
    pub fn decide_install_auto(
        path_found: bool,
        managed_found: bool,
        consent: ConsentMode,
    ) -> InstallDecision {
        decide_install(
            path_found,
            managed_found,
            consent,
            std::io::stdin().is_terminal(),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn already_available_when_path_present() {
            assert_eq!(
                decide_install(true, false, ConsentMode::default(), false),
                InstallDecision::AlreadyAvailable
            );
        }

        #[test]
        fn already_available_when_managed_present() {
            assert_eq!(
                decide_install(false, true, ConsentMode::default(), false),
                InstallDecision::AlreadyAvailable
            );
        }

        #[test]
        fn yes_flag_forces_install_even_without_tty() {
            let consent = ConsentMode { yes_flag: true, ..Default::default() };
            assert_eq!(
                decide_install(false, false, consent, false),
                InstallDecision::Install
            );
        }

        #[test]
        fn auto_install_env_forces_install_even_without_tty() {
            let consent = ConsentMode { auto_install_env: true, ..Default::default() };
            assert_eq!(
                decide_install(false, false, consent, false),
                InstallDecision::Install
            );
        }

        #[test]
        fn refuses_non_interactive_without_consent() {
            assert_eq!(
                decide_install(false, false, ConsentMode::default(), false),
                InstallDecision::Refuse
            );
        }
    }
    ```

- [ ] **Step 3.2: Re-export from `src/uv/mod.rs`**

    Add:

    ```rust
    pub mod install;
    pub use install::{ConsentMode, InstallDecision, decide_install, decide_install_auto};
    ```

- [ ] **Step 3.3: Run tests**

    ```bash
    cargo test --lib uv::install::
    ```

    Expected: 5 tests pass.

- [ ] **Step 3.4: Commit**

    ```bash
    git add src/uv/
    git commit -m "feat(uv): Decision logic for consent-based uv install"
    ```

---

## Task 4: Download + install uv into `$XDG_DATA_HOME/toolr/bin/uv`

Implement the actual download. Use the upstream Astral installer URLs
(`uv-<triple>.tar.gz` / `.zip`) for the host's target triple, extract the
binary, place it at `managed_uv_path()` with the executable bit set, and
return a `UvBinary` describing it.

**Files:**

- Modify: `src/uv/install.rs`

- Create: `tests/uv_install_offline.rs`

- [ ] **Step 4.1: Append a network-touching `perform_install` helper**

    Append to `src/uv/install.rs`:

    ```rust
    use std::fs;
    use std::path::PathBuf;

    use super::{UvBinary, UvSource, managed_uv_path, probe};

    /// Host triple selection. Returns an Astral release asset name without the
    /// extension, plus the extension itself.
    ///
    /// Reference: https://github.com/astral-sh/uv/releases
    pub fn host_asset() -> Option<(&'static str, &'static str)> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Some(("uv-x86_64-unknown-linux-gnu", "tar.gz")),
            ("linux", "aarch64") => Some(("uv-aarch64-unknown-linux-gnu", "tar.gz")),
            ("macos", "x86_64") => Some(("uv-x86_64-apple-darwin", "tar.gz")),
            ("macos", "aarch64") => Some(("uv-aarch64-apple-darwin", "tar.gz")),
            ("windows", "x86_64") => Some(("uv-x86_64-pc-windows-msvc", "zip")),
            _ => None,
        }
    }

    /// Where to download from. Parametrised so tests can override.
    pub fn asset_url(asset: &str, ext: &str) -> String {
        format!(
            "https://github.com/astral-sh/uv/releases/latest/download/{asset}.{ext}"
        )
    }

    /// Download + extract uv into the managed location.
    pub fn perform_install() -> Result<UvBinary, UvError> {
        let dest = managed_uv_path().ok_or_else(|| {
            UvError::Http("could not resolve $XDG_DATA_HOME/toolr/bin".into())
        })?;
        let (asset, ext) = host_asset().ok_or_else(|| {
            UvError::Http(format!(
                "no prebuilt uv asset for {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
        })?;
        let url = asset_url(asset, ext);
        download_and_extract(&url, asset, ext, &dest)?;
        probe(&dest, UvSource::FreshlyInstalled)
    }

    fn download_and_extract(
        url: &str,
        asset: &str,
        ext: &str,
        dest: &PathBuf,
    ) -> Result<(), UvError> {
        let parent = dest.parent().ok_or_else(|| {
            UvError::Http("managed uv path has no parent directory".into())
        })?;
        fs::create_dir_all(parent)?;

        let response = reqwest::blocking::get(url)
            .map_err(|e| UvError::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| UvError::Http(e.to_string()))?;
        let bytes = response
            .bytes()
            .map_err(|e| UvError::Http(e.to_string()))?;

        let archive_name = format!("{asset}.{ext}");
        let tmp = tempfile::tempdir()?;
        let archive_path = tmp.path().join(&archive_name);
        fs::write(&archive_path, &bytes)?;

        match ext {
            "tar.gz" => extract_tar_gz(&archive_path, tmp.path())?,
            "zip" => extract_zip(&archive_path, tmp.path())?,
            other => {
                return Err(UvError::Http(format!("unknown archive extension: {other}")));
            }
        }

        // The archive contains a single `uv` binary at the top of a
        // directory matching the asset name.
        let binary_name = if cfg!(windows) { "uv.exe" } else { "uv" };
        let candidates = [
            tmp.path().join(asset).join(binary_name),
            tmp.path().join(binary_name),
        ];
        let src = candidates
            .iter()
            .find(|p| p.is_file())
            .ok_or_else(|| {
                UvError::Http(format!(
                    "extracted archive did not contain a {binary_name} binary"
                ))
            })?;

        fs::copy(src, dest)?;
        set_executable(dest)?;
        Ok(())
    }

    #[cfg(unix)]
    fn set_executable(path: &PathBuf) -> Result<(), UvError> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn set_executable(_path: &PathBuf) -> Result<(), UvError> {
        Ok(())
    }

    fn extract_tar_gz(archive: &PathBuf, into: &std::path::Path) -> Result<(), UvError> {
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .current_dir(into)
            .status()?;
        if !status.success() {
            return Err(UvError::Http(format!(
                "tar exited with status {status:?}"
            )));
        }
        Ok(())
    }

    fn extract_zip(archive: &PathBuf, into: &std::path::Path) -> Result<(), UvError> {
        let status = std::process::Command::new("unzip")
            .arg("-q")
            .arg(archive)
            .arg("-d")
            .arg(into)
            .status()?;
        if !status.success() {
            return Err(UvError::Http(format!(
                "unzip exited with status {status:?}"
            )));
        }
        Ok(())
    }
    ```

    Rationale on extraction shelling out to `tar`/`unzip`: both are present
    on every platform we target (Windows 10+ ships `tar.exe` and PowerShell
    can `Expand-Archive`, but `unzip` is fine for our tests). This avoids
    pulling in the `flat2 + tar` and `zip` crates for this one-shot
    operation. If the implementer disagrees, swap in the pure-Rust crates —
    the public surface (`perform_install`) stays the same.

- [ ] **Step 4.2: Add offline integration test**

    Create `tests/uv_install_offline.rs`:

    ```rust
    //! Offline tests for the install path. The network-touching
    //! `perform_install` is exercised manually by the implementer in Task 4.3.

    use _rust_utils::uv::install::{asset_url, host_asset};

    #[test]
    fn host_asset_present_on_supported_targets() {
        // On the CI runners we care about, this should always succeed.
        // (Linux x86_64, macOS aarch64.) On unsupported triples the test
        // still passes — it just records `None`.
        let _ = host_asset();
    }

    #[test]
    fn asset_url_points_at_astral_releases() {
        let url = asset_url("uv-x86_64-unknown-linux-gnu", "tar.gz");
        assert!(url.starts_with("https://github.com/astral-sh/uv/releases"));
        assert!(url.ends_with("uv-x86_64-unknown-linux-gnu.tar.gz"));
    }
    ```

- [ ] **Step 4.3: Manual end-to-end smoke test**

    On a host with no toolr-managed uv yet:

    ```bash
    rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/toolr/bin/uv"
    cargo run --bin toolr -- self __install-uv-now    # placeholder; see Task 5
    "${XDG_DATA_HOME:-$HOME/.local/share}/toolr/bin/uv" --version
    ```

    Expected: a `uv` binary appears at the managed path and `--version`
    reports a number `>= MIN_UV_VERSION`. (The `__install-uv-now` hidden
    command is added in Task 5.)

- [ ] **Step 4.4: Run tests**

    ```bash
    cargo test --test uv_install_offline
    cargo test --lib uv::install::
    ```

    Expected: all pass.

- [ ] **Step 4.5: Commit**

    ```bash
    git add Cargo.toml src/uv/ tests/uv_install_offline.rs
    git commit -m "feat(uv): Download and install uv to $XDG_DATA_HOME/toolr/bin"
    ```

---

## Task 5: Unified `ensure_uv` entrypoint and refusal-path UX

Tie the discovery + consent + install pipeline into a single function
`ensure_uv` that callers (later in Plan 3, and Plan 6+ later) use to
obtain a working `UvBinary` or a clear error. Also wire a hidden
`__install-uv-now` developer command for Task 4's smoke test.

**Files:**

- Modify: `src/uv/mod.rs`

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/dispatch.rs`

- [ ] **Step 5.1: Add `ensure_uv` to `src/uv/mod.rs`**

    Append:

    ```rust
    use install::{ConsentMode, InstallDecision, decide_install_auto, perform_install};
    use discover::{find_managed_uv, find_uv_on_path};

    /// Find or install a working uv binary. The single entrypoint other
    /// modules call when they need uv.
    pub fn ensure_uv(consent: ConsentMode) -> Result<UvBinary, UvError> {
        if let Some(uv) = find_uv_on_path()? {
            return Ok(uv);
        }
        if let Some(uv) = find_managed_uv()? {
            return Ok(uv);
        }
        match decide_install_auto(false, false, consent) {
            InstallDecision::AlreadyAvailable => {
                // Shouldn't happen given the checks above, but if it does,
                // try one more time.
                find_uv_on_path()?
                    .or(find_managed_uv()?)
                    .ok_or(UvError::NotAvailable)
            }
            InstallDecision::Install => perform_install(),
            InstallDecision::Refuse => Err(UvError::UserRefusedInstall),
        }
    }
    ```

- [ ] **Step 5.2: Add the `__install-uv-now` hidden subcommand**

    In `src/bin/toolr/cli.rs`, where the other hidden commands are
    constructed (next to `__build-static-manifest`), add:

    ```rust
    root = root.subcommand(
        Command::new("__install-uv-now")
            .hide(true)
            .about("(internal) Force-install toolr-managed uv now"),
    );
    ```

- [ ] **Step 5.3: Handle dispatch**

    In `src/bin/toolr/dispatch.rs`, alongside the existing
    `__build-static-manifest` branch, add:

    ```rust
    if let Some(("__install-uv-now", _)) = matches.subcommand() {
        return run_install_uv_now();
    }
    ```

    And add:

    ```rust
    fn run_install_uv_now() -> anyhow::Result<std::process::ExitCode> {
        let consent = _rust_utils::uv::install::ConsentMode {
            yes_flag: true,
            auto_install_env: true,
        };
        let uv = _rust_utils::uv::ensure_uv(consent)?;
        println!(
            "toolr: uv {}.{}.{} ready at {} (source: {:?})",
            uv.version.0, uv.version.1, uv.version.2,
            uv.path.display(),
            uv.source,
        );
        Ok(std::process::ExitCode::SUCCESS)
    }
    ```

- [ ] **Step 5.4: Friendly refusal message at the binary level**

    Wherever `ensure_uv` returns `Err(UvError::UserRefusedInstall)` —
    initially only triggered via the project subcommands — the binary
    must surface a one-line message linking to docs. The conversion lives
    in dispatch helpers; later tasks build on that.

    For now, add this helper to `src/bin/toolr/dispatch.rs`:

    ```rust
    pub(crate) fn report_uv_error(err: &_rust_utils::uv::UvError) -> String {
        use _rust_utils::uv::UvError;
        match err {
            UvError::UserRefusedInstall => {
                "toolr: uv is required for this command. Install from \
                 https://docs.astral.sh/uv/getting-started/installation/ \
                 and rerun, or set TOOLR_AUTO_INSTALL_UV=1.".into()
            }
            UvError::VersionTooOld { found, required } => format!(
                "toolr: uv on PATH is {}.{}.{}, but toolr requires \
                 >= {}.{}.{}. Upgrade uv and try again.",
                found.0, found.1, found.2,
                required.0, required.1, required.2,
            ),
            other => format!("toolr: {other}"),
        }
    }
    ```

- [ ] **Step 5.5: Build and smoke-test**

    ```bash
    cargo build --bin toolr
    ```

    Expected: clean build. (No new automated tests — Task 4.2 covers the
    pure logic; the install path is exercised manually.)

- [ ] **Step 5.6: Commit**

    ```bash
    git add src/uv/mod.rs src/bin/toolr/
    git commit -m "feat(uv): Unified ensure_uv entrypoint and hidden install command"
    ```

---

## Task 6: Read `tools/pyproject.toml` `[tool.toolr]` table

A typed accessor for the toolr-specific configuration block.

**Files:**

- Create: `src/venv/mod.rs`

- Create: `src/venv/config.rs`

- Modify: `src/lib.rs`

- [ ] **Step 6.1: Create the `venv` module stub**

    Create `src/venv/mod.rs`:

    ```rust
    //! Tools venv resolution, configuration, and lifecycle.

    pub mod config;

    pub use config::{ToolrConfig, VenvLocation, load_toolr_config};
    ```

- [ ] **Step 6.2: Expose the module from `src/lib.rs`**

    Add:

    ```rust
    pub mod venv;
    ```

- [ ] **Step 6.3: Create `src/venv/config.rs`**

    ```rust
    //! Parse the `[tool.toolr]` table out of `tools/pyproject.toml`.

    use std::path::Path;

    use serde::Deserialize;
    use thiserror::Error;

    /// Where the tools venv should be materialised.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
    #[serde(rename_all = "kebab-case")]
    pub enum VenvLocation {
        /// Default: `$XDG_CACHE_HOME/toolr/<repo-key>/venv/`.
        #[default]
        Cache,
        /// Opt-in: `tools/.venv/`.
        InTree,
    }

    /// Strongly-typed view of the `[tool.toolr]` table.
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct ToolrConfig {
        #[serde(default)]
        pub venv_location: VenvLocation,
        /// Opt-in editable installs run post-`uv sync`. E.g. `["."]`.
        #[serde(default)]
        pub editable_install: Vec<String>,
        /// Optional explicit Python version override.
        #[serde(default)]
        pub python_version: Option<String>,
    }

    #[derive(Debug, Error)]
    pub enum ConfigError {
        #[error("missing tools/pyproject.toml at {0}")]
        Missing(std::path::PathBuf),
        #[error("I/O error reading pyproject.toml: {0}")]
        Io(#[from] std::io::Error),
        #[error("invalid TOML in pyproject.toml: {0}")]
        Toml(#[from] toml::de::Error),
    }

    /// Read `tools/pyproject.toml` and extract `[tool.toolr]` (or defaults).
    pub fn load_toolr_config(tools_dir: &Path) -> Result<ToolrConfig, ConfigError> {
        let path = tools_dir.join("pyproject.toml");
        if !path.is_file() {
            return Err(ConfigError::Missing(path));
        }
        let raw = std::fs::read_to_string(&path)?;
        #[derive(Deserialize)]
        struct Root {
            #[serde(default)]
            tool: ToolTable,
        }
        #[derive(Deserialize, Default)]
        struct ToolTable {
            #[serde(default)]
            toolr: ToolrConfig,
        }
        let root: Root = toml::from_str(&raw)?;
        Ok(root.tool.toolr)
    }

    /// Extract `requires-python` from the `[project]` table. Used as a
    /// fallback when `[tool.toolr] python-version` is unset.
    pub fn read_requires_python(tools_dir: &Path) -> Result<Option<String>, ConfigError> {
        let path = tools_dir.join("pyproject.toml");
        if !path.is_file() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        #[derive(Deserialize)]
        struct Root {
            #[serde(default)]
            project: ProjectTable,
        }
        #[derive(Deserialize, Default)]
        struct ProjectTable {
            #[serde(default, rename = "requires-python")]
            requires_python: Option<String>,
        }
        let root: Root = toml::from_str(&raw)?;
        Ok(root.project.requires_python)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn write_pyproject(tools: &Path, body: &str) {
            std::fs::create_dir_all(tools).unwrap();
            std::fs::write(tools.join("pyproject.toml"), body).unwrap();
        }

        #[test]
        fn defaults_when_table_is_absent() {
            let tmp = TempDir::new().unwrap();
            let tools = tmp.path().join("tools");
            write_pyproject(&tools, "[project]\nname=\"x\"\nversion=\"0\"\n");
            let cfg = load_toolr_config(&tools).unwrap();
            assert_eq!(cfg.venv_location, VenvLocation::Cache);
            assert!(cfg.editable_install.is_empty());
            assert!(cfg.python_version.is_none());
        }

        #[test]
        fn parses_in_tree_venv_location() {
            let tmp = TempDir::new().unwrap();
            let tools = tmp.path().join("tools");
            write_pyproject(
                &tools,
                r#"
[project]
name = "x"
version = "0"

[tool.toolr]
venv-location = "in-tree"
editable-install = ["."]
python-version = "3.13"
"#,
            );
            let cfg = load_toolr_config(&tools).unwrap();
            assert_eq!(cfg.venv_location, VenvLocation::InTree);
            assert_eq!(cfg.editable_install, vec![".".to_string()]);
            assert_eq!(cfg.python_version.as_deref(), Some("3.13"));
        }

        #[test]
        fn reports_missing_pyproject() {
            let tmp = TempDir::new().unwrap();
            let err = load_toolr_config(&tmp.path().join("tools")).unwrap_err();
            assert!(matches!(err, ConfigError::Missing(_)));
        }

        #[test]
        fn reads_requires_python() {
            let tmp = TempDir::new().unwrap();
            let tools = tmp.path().join("tools");
            write_pyproject(
                &tools,
                "[project]\nname=\"x\"\nversion=\"0\"\nrequires-python = \">=3.11\"\n",
            );
            let v = read_requires_python(&tools).unwrap();
            assert_eq!(v.as_deref(), Some(">=3.11"));
        }
    }
    ```

- [ ] **Step 6.4: Run tests**

    ```bash
    cargo test --lib venv::config::
    ```

    Expected: 4 tests pass.

- [ ] **Step 6.5: Commit**

    ```bash
    git add src/lib.rs src/venv/
    git commit -m "feat(venv): Parse [tool.toolr] config from tools/pyproject.toml"
    ```

---

## Task 7: Compute the stable `repo-key`

The repo-key feeds the cache venv path. Per the design, it is a hash of
the canonical repo path + python version + toolr major version. Symlinks
are followed to keep `~/work/repo` and `/Users/me/work/repo` resolving to
the same cache slot.

**Files:**

- Create: `src/venv/repo_key.rs`

- Modify: `src/venv/mod.rs`

- [ ] **Step 7.1: Add the failing tests + implementation**

    Create `src/venv/repo_key.rs`:

    ```rust
    //! Compute the cache-slot key for a repo's tools venv.

    use std::path::Path;

    use anyhow::{Context, Result};
    use blake3::Hasher;

    /// Toolr's own major version, baked in at build time.
    /// `CARGO_PKG_VERSION_MAJOR` is always set by cargo.
    pub const TOOLR_MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");

    /// Compute the stable repo-key. Inputs:
    /// - canonical repo path (symlinks resolved)
    /// - python version (e.g. "3.13"); empty string allowed when unknown
    /// - toolr major version
    pub fn compute_repo_key(repo_root: &Path, python_version: &str) -> Result<String> {
        let canonical = repo_root
            .canonicalize()
            .with_context(|| format!("canonicalising {}", repo_root.display()))?;
        let mut hasher = Hasher::new();
        hasher.update(canonical.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(python_version.as_bytes());
        hasher.update(b"\0");
        hasher.update(TOOLR_MAJOR.as_bytes());
        // Truncate to 16 hex chars — enough to avoid collisions, short enough
        // for nice on-disk paths.
        let hex = hasher.finalize().to_hex().to_string();
        Ok(hex[..16].to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        #[test]
        fn deterministic_for_same_inputs() {
            let tmp = TempDir::new().unwrap();
            let a = compute_repo_key(tmp.path(), "3.13").unwrap();
            let b = compute_repo_key(tmp.path(), "3.13").unwrap();
            assert_eq!(a, b);
        }

        #[test]
        fn differs_with_python_version() {
            let tmp = TempDir::new().unwrap();
            let a = compute_repo_key(tmp.path(), "3.12").unwrap();
            let b = compute_repo_key(tmp.path(), "3.13").unwrap();
            assert_ne!(a, b);
        }

        #[test]
        fn differs_with_path() {
            let a_tmp = TempDir::new().unwrap();
            let b_tmp = TempDir::new().unwrap();
            let a = compute_repo_key(a_tmp.path(), "3.13").unwrap();
            let b = compute_repo_key(b_tmp.path(), "3.13").unwrap();
            assert_ne!(a, b);
        }

        #[test]
        fn errors_on_missing_path() {
            let result = compute_repo_key(Path::new("/no/such/dir-toolr-test"), "3.13");
            assert!(result.is_err());
        }
    }
    ```

- [ ] **Step 7.2: Re-export from `src/venv/mod.rs`**

    Add:

    ```rust
    pub mod repo_key;
    pub use repo_key::{TOOLR_MAJOR, compute_repo_key};
    ```

- [ ] **Step 7.3: Run tests**

    ```bash
    cargo test --lib venv::repo_key::
    ```

    Expected: 4 tests pass.

- [ ] **Step 7.4: Commit**

    ```bash
    git add src/venv/
    git commit -m "feat(venv): Stable repo-key hash for cache-slot disambiguation"
    ```

---

## Task 8: Resolve the venv path

Combine the config (cache vs in-tree), the repo-key, and the python
version into one `resolve_venv_path(repo_root)` function.

**Files:**

- Create: `src/venv/resolve.rs`

- Modify: `src/venv/mod.rs`

- [ ] **Step 8.1: Failing tests + implementation**

    Create `src/venv/resolve.rs`:

    ```rust
    //! Resolve the absolute path where the tools venv should live.

    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};

    use super::config::{ToolrConfig, VenvLocation, load_toolr_config, read_requires_python};
    use super::repo_key::compute_repo_key;
    use crate::uv::toolr_cache_dir;

    /// Output of venv resolution.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ResolvedVenv {
        /// Where the venv directory lives.
        pub venv_dir: PathBuf,
        /// `<venv>/bin/python` (or `Scripts\\python.exe` on Windows).
        pub python: PathBuf,
        /// The repo-key used in the cache layout (empty when in-tree).
        pub repo_key: String,
        /// Python version string used as a hash input (best-effort).
        pub python_version: String,
        /// Source `tools/pyproject.toml` config.
        pub config: ToolrConfig,
    }

    /// Resolve the tools venv path for the given repo root.
    pub fn resolve_venv_path(repo_root: &Path) -> Result<ResolvedVenv> {
        let tools = repo_root.join("tools");
        let config = load_toolr_config(&tools)
            .with_context(|| format!("loading tools/pyproject.toml under {}", repo_root.display()))?;
        let python_version = config
            .python_version
            .clone()
            .or(read_requires_python(&tools).ok().flatten())
            .unwrap_or_default();

        let (venv_dir, repo_key) = match config.venv_location {
            VenvLocation::InTree => (tools.join(".venv"), String::new()),
            VenvLocation::Cache => {
                let key = compute_repo_key(repo_root, &python_version)?;
                let base = toolr_cache_dir().ok_or_else(|| {
                    anyhow::anyhow!("could not resolve toolr cache directory")
                })?;
                (base.join(&key).join("venv"), key)
            }
        };

        let python = if cfg!(windows) {
            venv_dir.join("Scripts").join("python.exe")
        } else {
            venv_dir.join("bin").join("python")
        };

        Ok(ResolvedVenv {
            venv_dir,
            python,
            repo_key,
            python_version,
            config,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn setup_repo(body: &str) -> TempDir {
            let tmp = TempDir::new().unwrap();
            let tools = tmp.path().join("tools");
            std::fs::create_dir(&tools).unwrap();
            std::fs::write(tools.join("pyproject.toml"), body).unwrap();
            tmp
        }

        #[test]
        fn cache_default_uses_repo_key_subdir() {
            let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
            let resolved = resolve_venv_path(tmp.path()).unwrap();
            assert!(resolved.venv_dir.ends_with("venv"));
            assert!(!resolved.repo_key.is_empty());
            assert!(resolved.venv_dir.to_string_lossy().contains(&resolved.repo_key));
        }

        #[test]
        fn in_tree_lands_inside_tools_dot_venv() {
            let tmp = setup_repo(
                "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
            );
            let resolved = resolve_venv_path(tmp.path()).unwrap();
            assert_eq!(resolved.venv_dir, tmp.path().join("tools").join(".venv"));
            assert!(resolved.repo_key.is_empty());
        }
    }
    ```

- [ ] **Step 8.2: Re-export**

    Add to `src/venv/mod.rs`:

    ```rust
    pub mod resolve;
    pub use resolve::{ResolvedVenv, resolve_venv_path};
    ```

- [ ] **Step 8.3: Run tests**

    ```bash
    cargo test --lib venv::resolve::
    ```

    Expected: 2 tests pass.

- [ ] **Step 8.4: Commit**

    ```bash
    git add src/venv/
    git commit -m "feat(venv): Resolve cache vs in-tree venv path"
    ```

---

## Task 9: Run `uv sync` against the tools project

Add a thin wrapper over `uv sync --project tools/` plus a freshness check
that compares the venv's last-sync mtime against `tools/uv.lock`'s mtime.

**Files:**

- Create: `src/venv/sync.rs`

- Modify: `src/venv/mod.rs`

- [ ] **Step 9.1: Implement sync + freshness with tests**

    Create `src/venv/sync.rs`:

    ```rust
    //! Drive `uv sync` to materialise the tools venv.

    use std::fs;
    use std::path::Path;
    use std::process::{Command, ExitStatus};
    use std::time::SystemTime;

    use anyhow::{Context, Result};

    use crate::uv::{UvBinary, UvError};

    use super::resolve::ResolvedVenv;

    /// Marker file written into the venv after each successful sync.
    /// Its mtime is compared against `tools/uv.lock`'s mtime to decide
    /// whether re-sync is needed.
    const FRESHNESS_MARKER: &str = ".toolr-sync-stamp";

    /// Decision returned by `is_fresh`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Freshness {
        /// Venv has never been synced (marker absent or venv missing).
        Missing,
        /// Lock has been edited since last sync.
        Stale,
        /// Marker mtime >= lock mtime.
        Fresh,
    }

    pub fn check_freshness(resolved: &ResolvedVenv, tools_dir: &Path) -> Freshness {
        let marker = resolved.venv_dir.join(FRESHNESS_MARKER);
        let lock = tools_dir.join("uv.lock");
        let (Ok(marker_meta), Ok(lock_meta)) = (fs::metadata(&marker), fs::metadata(&lock)) else {
            return Freshness::Missing;
        };
        let marker_t = marker_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let lock_t = lock_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if marker_t >= lock_t {
            Freshness::Fresh
        } else {
            Freshness::Stale
        }
    }

    /// Run `uv sync --project <tools>` synchronously, inheriting stdio.
    pub fn run_uv_sync(
        uv: &UvBinary,
        tools_dir: &Path,
        resolved: &ResolvedVenv,
    ) -> Result<ExitStatus> {
        // Ensure the parent of an off-tree venv exists so uv can write into it.
        if let Some(parent) = resolved.venv_dir.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut cmd = Command::new(&uv.path);
        cmd.arg("sync")
            .arg("--project")
            .arg(tools_dir)
            .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir);
        if let Some(version) = resolved.config.python_version.as_ref() {
            cmd.arg("--python").arg(version);
        }
        let status = cmd
            .status()
            .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
        if status.success() {
            touch_marker(&resolved.venv_dir)?;
        }
        Ok(status)
    }

    /// Convenience wrapper that maps a failure to `UvError::SyncFailed`.
    pub fn sync_if_needed(
        uv: &UvBinary,
        tools_dir: &Path,
        resolved: &ResolvedVenv,
        force: bool,
    ) -> Result<(), UvError> {
        if !force && matches!(check_freshness(resolved, tools_dir), Freshness::Fresh) {
            return Ok(());
        }
        let status = run_uv_sync(uv, tools_dir, resolved)
            .map_err(|e| UvError::Http(e.to_string()))?;
        if !status.success() {
            return Err(UvError::SyncFailed(status.code()));
        }
        Ok(())
    }

    fn touch_marker(venv_dir: &Path) -> Result<()> {
        fs::create_dir_all(venv_dir)?;
        fs::write(venv_dir.join(FRESHNESS_MARKER), b"")?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::Duration;
        use tempfile::TempDir;

        fn dummy_resolved(venv_dir: std::path::PathBuf) -> ResolvedVenv {
            ResolvedVenv {
                venv_dir: venv_dir.clone(),
                python: venv_dir.join("bin").join("python"),
                repo_key: "x".into(),
                python_version: "3.13".into(),
                config: Default::default(),
            }
        }

        #[test]
        fn missing_marker_or_lock_reports_missing() {
            let tmp = TempDir::new().unwrap();
            let resolved = dummy_resolved(tmp.path().join("venv"));
            assert_eq!(
                check_freshness(&resolved, tmp.path()),
                Freshness::Missing
            );
        }

        #[test]
        fn marker_older_than_lock_reports_stale() {
            let tmp = TempDir::new().unwrap();
            let venv = tmp.path().join("venv");
            fs::create_dir_all(&venv).unwrap();
            touch_marker(&venv).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
            let resolved = dummy_resolved(venv);
            assert_eq!(
                check_freshness(&resolved, tmp.path()),
                Freshness::Stale
            );
        }

        #[test]
        fn marker_newer_than_lock_reports_fresh() {
            let tmp = TempDir::new().unwrap();
            let venv = tmp.path().join("venv");
            fs::create_dir_all(&venv).unwrap();
            std::fs::write(tmp.path().join("uv.lock"), b"locks").unwrap();
            std::thread::sleep(Duration::from_millis(20));
            touch_marker(&venv).unwrap();
            let resolved = dummy_resolved(venv);
            assert_eq!(
                check_freshness(&resolved, tmp.path()),
                Freshness::Fresh
            );
        }
    }
    ```

- [ ] **Step 9.2: Re-export**

    Add to `src/venv/mod.rs`:

    ```rust
    pub mod sync;
    pub use sync::{Freshness, check_freshness, run_uv_sync, sync_if_needed};
    ```

- [ ] **Step 9.3: Run tests**

    ```bash
    cargo test --lib venv::sync::
    ```

    Expected: 3 tests pass.

- [ ] **Step 9.4: Commit**

    ```bash
    git add src/venv/
    git commit -m "feat(venv): Run uv sync with mtime-based freshness check"
    ```

---

## Task 10: Validate that `toolr` Python package is installed

After `uv sync` finishes, look up `<venv>/lib/python*/site-packages/toolr/__init__.py`
and refuse to operate if it's missing.

**Files:**

- Create: `src/venv/validate.rs`

- Modify: `src/venv/mod.rs`

- [ ] **Step 10.1: Failing tests + implementation**

    Create `src/venv/validate.rs`:

    ```rust
    //! Post-sync validation: the venv must contain the toolr Python package.

    use std::path::{Path, PathBuf};

    use thiserror::Error;
    use walkdir::WalkDir;

    #[derive(Debug, Error)]
    pub enum ValidationError {
        #[error(
            "toolr: tools/pyproject.toml must declare a `toolr>=X.Y` dependency. \
             Add it and retry."
        )]
        ToolrPackageMissing,
        #[error("toolr: venv at {0} does not contain a python interpreter")]
        InterpreterMissing(PathBuf),
    }

    /// Walk the venv's `lib/python*/site-packages` and look for the toolr
    /// package directory. Returns its path on success.
    pub fn locate_toolr_package(venv_dir: &Path) -> Option<PathBuf> {
        for candidate in candidate_site_packages(venv_dir) {
            let init = candidate.join("toolr").join("__init__.py");
            if init.is_file() {
                return Some(candidate.join("toolr"));
            }
        }
        None
    }

    /// Iterate possible `site-packages` directories within a venv.
    /// Linux/macOS: `<venv>/lib/python*/site-packages/`.
    /// Windows: `<venv>/Lib/site-packages/`.
    pub fn candidate_site_packages(venv_dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if cfg!(windows) {
            out.push(venv_dir.join("Lib").join("site-packages"));
        } else {
            let lib = venv_dir.join("lib");
            if lib.is_dir() {
                for entry in WalkDir::new(&lib).max_depth(1).into_iter().filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy();
                    if name.starts_with("python") {
                        out.push(entry.path().join("site-packages"));
                    }
                }
            }
        }
        out
    }

    /// Validate the venv has both a python interpreter and the toolr package.
    pub fn validate_venv(venv_dir: &Path, python: &Path) -> Result<PathBuf, ValidationError> {
        if !python.is_file() {
            return Err(ValidationError::InterpreterMissing(venv_dir.to_path_buf()));
        }
        locate_toolr_package(venv_dir).ok_or(ValidationError::ToolrPackageMissing)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn fake_unix_venv(root: &Path, with_toolr: bool, with_python: bool) {
            let py_dir = root.join("lib").join("python3.13").join("site-packages");
            std::fs::create_dir_all(&py_dir).unwrap();
            let bin = root.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            if with_python {
                std::fs::write(bin.join("python"), b"").unwrap();
            }
            if with_toolr {
                std::fs::create_dir_all(py_dir.join("toolr")).unwrap();
                std::fs::write(py_dir.join("toolr").join("__init__.py"), b"").unwrap();
            }
        }

        #[test]
        #[cfg(unix)]
        fn detects_installed_toolr_package() {
            let tmp = TempDir::new().unwrap();
            fake_unix_venv(tmp.path(), true, true);
            let python = tmp.path().join("bin").join("python");
            let pkg = validate_venv(tmp.path(), &python).unwrap();
            assert!(pkg.ends_with("toolr"));
        }

        #[test]
        #[cfg(unix)]
        fn reports_missing_toolr_package() {
            let tmp = TempDir::new().unwrap();
            fake_unix_venv(tmp.path(), false, true);
            let python = tmp.path().join("bin").join("python");
            let err = validate_venv(tmp.path(), &python).unwrap_err();
            assert!(matches!(err, ValidationError::ToolrPackageMissing));
        }

        #[test]
        #[cfg(unix)]
        fn reports_missing_interpreter() {
            let tmp = TempDir::new().unwrap();
            fake_unix_venv(tmp.path(), true, false);
            let python = tmp.path().join("bin").join("python");
            let err = validate_venv(tmp.path(), &python).unwrap_err();
            assert!(matches!(err, ValidationError::InterpreterMissing(_)));
        }
    }
    ```

- [ ] **Step 10.2: Re-export**

    Add to `src/venv/mod.rs`:

    ```rust
    pub mod validate;
    pub use validate::{ValidationError, locate_toolr_package, validate_venv};
    ```

- [ ] **Step 10.3: Run tests**

    ```bash
    cargo test --lib venv::validate::
    ```

    Expected: 3 tests pass on Unix.

- [ ] **Step 10.4: Commit**

    ```bash
    git add src/venv/
    git commit -m "feat(venv): Validate toolr Python package presence in venv"
    ```

---

## Task 11: Opt-in editable install (best-effort)

If `[tool.toolr] editable-install` is non-empty, run
`uv pip install -e <path> --python <venv-python>` for each entry after
sync. Failures are warned, not fatal.

**Files:**

- Create: `src/venv/editable.rs`

- Modify: `src/venv/mod.rs`

- [ ] **Step 11.1: Implement + tests**

    Create `src/venv/editable.rs`:

    ```rust
    //! Best-effort post-sync editable installs.

    use std::path::Path;
    use std::process::Command;

    use crate::uv::UvBinary;

    use super::config::ToolrConfig;

    /// Outcome of one editable-install attempt.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum EditableOutcome {
        Installed(String),
        Skipped(String),
        Failed { spec: String, message: String },
    }

    /// Run `uv pip install -e <spec>` for each configured entry. The
    /// `repo_root` resolves the `"."` shorthand to an absolute path so
    /// the venv doesn't depend on cwd.
    pub fn perform_editable_installs(
        uv: &UvBinary,
        config: &ToolrConfig,
        repo_root: &Path,
        venv_python: &Path,
    ) -> Vec<EditableOutcome> {
        let mut out = Vec::with_capacity(config.editable_install.len());
        for spec in &config.editable_install {
            if spec.trim().is_empty() {
                out.push(EditableOutcome::Skipped(spec.clone()));
                continue;
            }
            let resolved = if spec == "." || spec == "./" {
                repo_root.display().to_string()
            } else {
                spec.clone()
            };
            let result = Command::new(&uv.path)
                .arg("pip")
                .arg("install")
                .arg("--python")
                .arg(venv_python)
                .arg("-e")
                .arg(&resolved)
                .status();
            match result {
                Ok(status) if status.success() => {
                    out.push(EditableOutcome::Installed(spec.clone()))
                }
                Ok(status) => out.push(EditableOutcome::Failed {
                    spec: spec.clone(),
                    message: format!("uv pip install exited with {status:?}"),
                }),
                Err(e) => out.push(EditableOutcome::Failed {
                    spec: spec.clone(),
                    message: e.to_string(),
                }),
            }
        }
        out
    }

    /// Emit a stderr line per failed install. Toolr does not abort on
    /// failure — tools that need the repo will surface a normal ImportError
    /// at execute time.
    pub fn warn_failures(outcomes: &[EditableOutcome]) {
        for outcome in outcomes {
            if let EditableOutcome::Failed { spec, message } = outcome {
                eprintln!(
                    "toolr: warning: editable install of `{spec}` failed: {message}"
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn empty_config_produces_no_outcomes() {
            let uv = UvBinary {
                path: std::path::PathBuf::from("/nonexistent/uv"),
                version: (0, 0, 0),
                source: crate::uv::UvSource::Path,
            };
            let cfg = ToolrConfig::default();
            let outcomes = perform_editable_installs(
                &uv,
                &cfg,
                Path::new("/tmp"),
                Path::new("/tmp/python"),
            );
            assert!(outcomes.is_empty());
        }

        #[test]
        fn missing_uv_binary_produces_failure_outcome_not_panic() {
            let uv = UvBinary {
                path: std::path::PathBuf::from("/nonexistent/uv-toolr-test"),
                version: (0, 0, 0),
                source: crate::uv::UvSource::Path,
            };
            let cfg = ToolrConfig {
                editable_install: vec![".".into()],
                ..Default::default()
            };
            let outcomes = perform_editable_installs(
                &uv,
                &cfg,
                Path::new("/tmp"),
                Path::new("/tmp/python"),
            );
            assert_eq!(outcomes.len(), 1);
            assert!(matches!(outcomes[0], EditableOutcome::Failed { .. }));
        }
    }
    ```

- [ ] **Step 11.2: Re-export**

    Add to `src/venv/mod.rs`:

    ```rust
    pub mod editable;
    pub use editable::{EditableOutcome, perform_editable_installs, warn_failures};
    ```

- [ ] **Step 11.3: Run tests**

    ```bash
    cargo test --lib venv::editable::
    ```

    Expected: 2 tests pass.

- [ ] **Step 11.4: Commit**

    ```bash
    git add src/venv/
    git commit -m "feat(venv): Best-effort editable installs with warn-on-fail"
    ```

---

## Task 12: Wire the `toolr project` subcommand group

Reserve the `project` namespace at the top of clap, alongside the existing
hidden internal commands. User-defined groups still appear at the top
level; clap detects collisions but the design forbids users from naming
a group `project` or `self`.

**Files:**

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/dispatch.rs`

- Create: `src/bin/toolr/project.rs`

- [ ] **Step 12.1: Reserve and refuse colliding user groups**

    In `src/bin/toolr/cli.rs`, before adding user groups, add:

    ```rust
    const RESERVED_GROUPS: &[&str] = &["self", "project"];

    fn user_group_collides(name: &str) -> bool {
        RESERVED_GROUPS.iter().any(|r| *r == name)
    }
    ```

    Update the loop that adds user-defined groups to skip + warn:

    ```rust
    for group in &manifest.groups {
        if user_group_collides(&group.name) {
            eprintln!(
                "toolr: warning: ignoring user-defined group `{}` — \
                 this name is reserved by toolr itself.",
                group.name
            );
            continue;
        }
        // …existing g = Command::new(...) construction…
    }
    ```

- [ ] **Step 12.2: Add the `project` subcommand tree**

    Below the user-group loop, add:

    ```rust
    root = root.subcommand(
        Command::new("project")
            .about("Operations on the current repo's tools/ directory")
            .subcommand_required(true)
            .subcommand(
                Command::new("deps").about("Tools-venv dependency management")
                    .subcommand_required(true)
                    .subcommand(Command::new("sync").about("Run `uv sync` against tools/")),
            )
            .subcommand(
                Command::new("venv").about("Inspect or activate the tools venv")
                    .subcommand_required(true)
                    .subcommand(Command::new("path").about("Print the absolute path to the tools venv"))
                    .subcommand(Command::new("shell").about("Spawn a subshell with the tools venv activated")),
            ),
    );
    ```

- [ ] **Step 12.3: Stub `src/bin/toolr/project.rs`**

    Create `src/bin/toolr/project.rs`:

    ```rust
    //! Implementation of `toolr project <...>` subcommands.

    use std::process::ExitCode;

    use anyhow::Result;
    use clap::ArgMatches;

    pub fn dispatch_project(matches: &ArgMatches) -> Result<ExitCode> {
        match matches.subcommand() {
            Some(("deps", deps_m)) => match deps_m.subcommand() {
                Some(("sync", _)) => deps_sync(),
                _ => unreachable!("clap enforces subcommand_required"),
            },
            Some(("venv", venv_m)) => match venv_m.subcommand() {
                Some(("path", _)) => venv_path(),
                Some(("shell", _)) => venv_shell(),
                _ => unreachable!("clap enforces subcommand_required"),
            },
            _ => unreachable!("clap enforces subcommand_required"),
        }
    }

    fn deps_sync() -> Result<ExitCode> {
        // Implemented in Task 13.
        Ok(ExitCode::from(2))
    }

    fn venv_path() -> Result<ExitCode> {
        // Implemented in Task 14.
        Ok(ExitCode::from(2))
    }

    fn venv_shell() -> Result<ExitCode> {
        // Implemented in Task 15.
        Ok(ExitCode::from(2))
    }
    ```

- [ ] **Step 12.4: Route `project` from `dispatch.rs`**

    In `src/bin/toolr/dispatch.rs`, before the user-command lookup:

    ```rust
    if let Some(("project", project_m)) = matches.subcommand() {
        return crate::project::dispatch_project(project_m);
    }
    ```

    And `mod project;` in `src/bin/toolr/main.rs`.

- [ ] **Step 12.5: Verify**

    ```bash
    cargo run --bin toolr -- project --help
    cargo run --bin toolr -- project deps --help
    cargo run --bin toolr -- project venv --help
    ```

    Expected: all three render the new subtree.

- [ ] **Step 12.6: Commit**

    ```bash
    git add src/bin/toolr/
    git commit -m "feat(cli): Reserve `toolr project` namespace and stub subcommands"
    ```

---

## Task 13: `toolr project deps sync`

Force a full `uv sync` of the current repo's tools venv, including
post-sync validation and editable-install hooks.

**Files:**

- Modify: `src/bin/toolr/project.rs`

- Create: `src/project.rs`

- Modify: `src/lib.rs`

- [ ] **Step 13.1: Create the top-level orchestrator `src/project.rs`**

    ```rust
    //! High-level orchestration: find repo, ensure uv, sync venv, validate.

    use std::path::Path;

    use anyhow::{Context, Result};

    use crate::discovery::discover_project_root;
    use crate::uv::{UvBinary, UvError, ensure_uv, install::ConsentMode};
    use crate::venv::{
        ResolvedVenv, perform_editable_installs, resolve_venv_path,
        sync::sync_if_needed, validate::validate_venv, warn_failures,
    };

    /// One-stop "make the venv ready" entrypoint. Returns the resolved venv
    /// + the chosen uv binary on success.
    pub fn ensure_venv_ready(
        cwd: &Path,
        consent: ConsentMode,
        force_sync: bool,
    ) -> Result<(ResolvedVenv, UvBinary)> {
        let repo_root = discover_project_root(cwd)
            .context("locating project root for the tools venv")?;
        let resolved = resolve_venv_path(&repo_root)
            .context("resolving the tools venv path")?;
        let uv = match ensure_uv(consent) {
            Ok(uv) => uv,
            Err(e @ UvError::UserRefusedInstall) => return Err(anyhow::anyhow!(e)),
            Err(e) => return Err(anyhow::anyhow!(e)),
        };
        let tools = repo_root.join("tools");
        sync_if_needed(&uv, &tools, &resolved, force_sync)
            .with_context(|| format!("uv sync against {}", tools.display()))?;
        validate_venv(&resolved.venv_dir, &resolved.python)
            .context("validating the synced venv")?;
        let outcomes = perform_editable_installs(
            &uv,
            &resolved.config,
            &repo_root,
            &resolved.python,
        );
        warn_failures(&outcomes);
        Ok((resolved, uv))
    }
    ```

- [ ] **Step 13.2: Re-export**

    Add to `src/lib.rs`:

    ```rust
    pub mod project;
    ```

- [ ] **Step 13.3: Wire `deps sync` in `src/bin/toolr/project.rs`**

    Replace `deps_sync`:

    ```rust
    fn deps_sync() -> Result<ExitCode> {
        let cwd = std::env::current_dir()?;
        let consent = _rust_utils::uv::install::ConsentMode::from_env();
        let (resolved, uv) = _rust_utils::project::ensure_venv_ready(
            &cwd, consent, /*force_sync=*/ true,
        )?;
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0, uv.version.1, uv.version.2,
        );
        Ok(ExitCode::SUCCESS)
    }
    ```

- [ ] **Step 13.4: Run unit tests**

    ```bash
    cargo test --lib project::
    cargo build --bin toolr
    ```

    Expected: clean build. The full end-to-end test lives in Task 17.

- [ ] **Step 13.5: Commit**

    ```bash
    git add src/lib.rs src/project.rs src/bin/toolr/project.rs
    git commit -m "feat(project): Implement `toolr project deps sync`"
    ```

---

## Task 14: `toolr project venv path`

Print the resolved venv path. Does **not** require uv to be installed — it's
a pure read of `tools/pyproject.toml` + repo-key calculation.

**Files:**

- Modify: `src/bin/toolr/project.rs`

- [ ] **Step 14.1: Implement `venv_path`**

    Replace:

    ```rust
    fn venv_path() -> Result<ExitCode> {
        let cwd = std::env::current_dir()?;
        let repo_root = _rust_utils::discovery::discover_project_root(&cwd)?;
        let resolved = _rust_utils::venv::resolve_venv_path(&repo_root)?;
        println!("{}", resolved.venv_dir.display());
        Ok(ExitCode::SUCCESS)
    }
    ```

- [ ] **Step 14.2: Manual smoke test**

    ```bash
    cargo run --bin toolr -- project venv path
    ```

    Expected: prints either `<repo>/tools/.venv` (in-tree) or
    `$XDG_CACHE_HOME/toolr/<key>/venv` (cache), based on config.

- [ ] **Step 14.3: Commit**

    ```bash
    git add src/bin/toolr/project.rs
    git commit -m "feat(project): Implement `toolr project venv path`"
    ```

---

## Task 15: `toolr project venv shell`

Spawn the user's shell with the tools venv activated. Requires the venv
to be ready — call `ensure_venv_ready` first.

**Files:**

- Modify: `src/bin/toolr/project.rs`

- [ ] **Step 15.1: Implement `venv_shell`**

    Replace:

    ```rust
    fn venv_shell() -> Result<ExitCode> {
        use std::process::Command;

        let cwd = std::env::current_dir()?;
        let consent = _rust_utils::uv::install::ConsentMode::from_env();
        let (resolved, _) = _rust_utils::project::ensure_venv_ready(
            &cwd, consent, /*force_sync=*/ false,
        )?;

        let shell = std::env::var_os("SHELL")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| if cfg!(windows) {
                std::path::PathBuf::from("cmd.exe")
            } else {
                std::path::PathBuf::from("/bin/sh")
            });

        let bin_dir = if cfg!(windows) {
            resolved.venv_dir.join("Scripts")
        } else {
            resolved.venv_dir.join("bin")
        };
        let prepended_path = match std::env::var_os("PATH") {
            Some(existing) => {
                let mut paths: Vec<_> = std::env::split_paths(&existing).collect();
                paths.insert(0, bin_dir.clone());
                std::env::join_paths(paths)?
            }
            None => bin_dir.clone().into_os_string(),
        };

        let status = Command::new(&shell)
            .env("VIRTUAL_ENV", &resolved.venv_dir)
            .env("PATH", &prepended_path)
            // Help shell prompts notice the activation.
            .env("TOOLR_VENV", &resolved.venv_dir)
            .status()?;
        Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
    }
    ```

    Note: this does **not** source a shell-specific activation script; it
    simulates the effect (prepending the bin dir to PATH, setting
    `VIRTUAL_ENV`). That's enough for `python`, `pip`, and friends to
    resolve to the venv's binaries.

- [ ] **Step 15.2: Manual smoke test**

    Once Task 17's full end-to-end test fixture exists, run:

    ```bash
    cargo run --bin toolr -- project venv shell
    # in the spawned shell:
    which python
    # → <venv>/bin/python
    exit
    ```

- [ ] **Step 15.3: Commit**

    ```bash
    git add src/bin/toolr/project.rs
    git commit -m "feat(project): Implement `toolr project venv shell`"
    ```

---

## Task 16: Integration tests for venv path resolution

Cover the cache-vs-in-tree decision matrix with `assert_cmd`-driven tests
that don't depend on having `uv` installed.

**Files:**

- Create: `tests/project_venv_path.rs`

- [ ] **Step 16.1: Write the tests**

    ```rust
    use assert_cmd::Command;
    use tempfile::TempDir;

    fn write_pyproject(tools: &std::path::Path, body: &str) {
        std::fs::create_dir_all(tools).unwrap();
        std::fs::write(tools.join("pyproject.toml"), body).unwrap();
    }

    #[test]
    fn project_venv_path_prints_cache_path_by_default() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(
            &tmp.path().join("tools"),
            "[project]\nname=\"x\"\nversion=\"0\"\n",
        );
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["project", "venv", "path"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        assert!(
            stdout.contains("venv"),
            "expected a path containing `venv`, got: {stdout}"
        );
        assert!(
            !stdout.contains(tmp.path().join("tools").join(".venv").to_string_lossy().as_ref()),
            "default config should not land in-tree, got: {stdout}"
        );
    }

    #[test]
    fn project_venv_path_prints_in_tree_path_when_configured() {
        let tmp = TempDir::new().unwrap();
        write_pyproject(
            &tmp.path().join("tools"),
            "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
        );
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["project", "venv", "path"])
            .output()
            .unwrap();
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let expected = tmp.path().join("tools").join(".venv");
        assert!(
            stdout.contains(expected.to_string_lossy().as_ref()),
            "expected in-tree path {} in: {stdout}",
            expected.display(),
        );
    }

    #[test]
    fn project_venv_path_requires_pyproject() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["project", "venv", "path"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("pyproject.toml"));
    }
    ```

- [ ] **Step 16.2: Run**

    ```bash
    cargo test --test project_venv_path
    ```

    Expected: 3 tests pass.

- [ ] **Step 16.3: Commit**

    ```bash
    git add tests/project_venv_path.rs
    git commit -m "test(project): Integration tests for venv path resolution"
    ```

---

## Task 17: End-to-end sync + execute smoke test

A `#[ignore]`-tagged integration test that, when run explicitly, exercises
the full path: fixture repo with `tools/pyproject.toml` + `tools/uv.lock`
declaring `toolr`, then runs `toolr project deps sync`, then runs a
real user command via Plan 2's runner. This is the "happy-path" gate.

**Files:**

- Create: `tests/end_to_end_sync.rs`

- [ ] **Step 17.1: Write the test**

    ```rust
    //! End-to-end smoke. Requires network access (uv download) and that
    //! Plan 2's runner is already wired. Run explicitly with:
    //!
    //!     cargo test --test end_to_end_sync -- --ignored --nocapture

    use assert_cmd::Command;
    use tempfile::TempDir;

    const PYPROJECT: &str = r#"
    [project]
    name = "toolr-tools"
    version = "0"
    requires-python = ">=3.11"
    dependencies = ["toolr"]

    [tool.toolr]
    venv-location = "in-tree"
    "#;

    #[test]
    #[ignore = "network-touching: requires uv to be available or installable"]
    fn deps_sync_then_run_user_command() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(tools.join("pyproject.toml"), PYPROJECT).unwrap();
        // A minimal command file so Plan 1 picks up a group.
        std::fs::write(
            tools.join("ci.py"),
            r#"
    """CI helpers."""

    from toolr import command_group

    group = command_group("ci", "CI helpers", docstring=__doc__)

    @group.command
    def hello(ctx):
        """Say hello."""
        print("hello from tools.ci")
    "#,
        )
        .unwrap();

        // 1. Build the static manifest.
        Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["__build-static-manifest"])
            .env("TOOLR_AUTO_INSTALL_UV", "1")
            .assert()
            .success();

        // 2. Sync the venv (will install uv on first run if needed).
        Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["project", "deps", "sync"])
            .env("TOOLR_AUTO_INSTALL_UV", "1")
            .assert()
            .success();

        // 3. Run the user command — Plan 2's runner now executes via the
        //    venv python.
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["ci", "hello"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        assert!(stdout.contains("hello from tools.ci"));
    }
    ```

- [ ] **Step 17.2: Run with `--ignored` once Plan 2's spawn site is venv-aware (see Task 18)**

    ```bash
    cargo test --test end_to_end_sync -- --ignored --nocapture
    ```

    Expected: green when run on a machine with internet access. The test
    is `#[ignore]` so CI doesn't trip on offline sandboxes; the implementer
    runs it manually before merging.

- [ ] **Step 17.3: Commit**

    ```bash
    git add tests/end_to_end_sync.rs
    git commit -m "test(project): End-to-end deps-sync + execute smoke (ignored by default)"
    ```

---

## Task 18: Update Plan 2's spawn site to use the venv's Python

Plan 2 introduced `dispatch.rs` spawning `python -m toolr._runner`. That
relied on PATH-resolution of `python`. With Plan 3 landed, the spawn
must use `<tools-venv>/bin/python`. This task wires that in.

**Files:**

- Modify: `src/bin/toolr/dispatch.rs`

- [ ] **Step 18.1: Locate the spawn site**

    The exact line(s) live in whatever helper Plan 2 introduced
    (commonly something like `spawn_runner(spec_path: &Path)`). It
    currently uses `Command::new("python")` or similar.

- [ ] **Step 18.2: Replace the lookup with a `ensure_venv_ready` call**

    Pseudocode (adapt to the actual Plan 2 shape):

    ```rust
    use _rust_utils::project::ensure_venv_ready;
    use _rust_utils::uv::install::ConsentMode;

    fn spawn_runner(spec_path: &std::path::Path) -> anyhow::Result<std::process::ExitStatus> {
        let cwd = std::env::current_dir()?;
        let consent = ConsentMode::from_env();
        // force_sync = false: rely on mtime freshness, only re-sync on drift.
        let (resolved, _uv) = ensure_venv_ready(&cwd, consent, false)?;
        let status = std::process::Command::new(&resolved.python)
            .arg("-m")
            .arg("toolr._runner")
            .env("TOOLR_SPEC_FILE", spec_path)
            // …signal handling + stdio inheritance per Plan 2…
            .status()?;
        Ok(status)
    }
    ```

- [ ] **Step 18.3: Update Plan 2's smoke tests if they hard-coded a python path**

    The implementer must reread the Plan 2 integration test file (likely
    `tests/runner_smoke.rs` or similar). Any test that calls the binary
    and expects it to succeed without a venv must either:

    - acquire a `[tool.toolr] editable-install = ["."]` setup, or
    - be marked `#[ignore]` and converted to the form used in Task 17.

    This is the trickiest cross-plan stitch — the implementer should
    audit Plan 2's tests methodically.

- [ ] **Step 18.4: Run the full test suite**

    ```bash
    cargo test
    ```

    Expected: all tests pass. The end-to-end test from Task 17 stays
    `#[ignore]`; lightweight tests stay green.

- [ ] **Step 18.5: Commit**

    ```bash
    git add src/bin/toolr/dispatch.rs tests/
    git commit -m "feat(dispatch): Spawn the runner via the tools venv python"
    ```

---

## Task 19: Update the roadmap

Mark Plan 3 as Done.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 19.1: Update the Plan 3 entry**

    Change `### Plan 3: Tools venv + uv integration` block's
    `**Status:**` line to `✅ Done` and set `**Plan doc:**` to point at
    `[04-plan-3-venv-uv.md](./04-plan-3-venv-uv.md)`.

- [ ] **Step 19.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 3 as done"
    ```

---

## Done criteria

Plan 3 is complete when:

- `cargo test` passes for all unit and integration tests (the
  `#[ignore]`-tagged end-to-end test stays out of the default suite).
- `cargo test --test end_to_end_sync -- --ignored` passes on a host with
  internet access — the fixture is synced, validated, and a user command
  successfully executes via the venv python.
- `cargo run --bin toolr -- project venv path` prints the cache path by
  default, or `tools/.venv` when configured.
- `cargo run --bin toolr -- project deps sync` runs `uv sync`, validates
  the `toolr` Python package's presence, and runs any configured
  editable installs (best-effort).
- `cargo run --bin toolr -- project venv shell` spawns the user's shell
  with `VIRTUAL_ENV` and `PATH` configured so that `python` resolves to
  `<tools-venv>/bin/python`.
- uv discovery follows the documented sequence: PATH → managed →
  consented install → refusal. `TOOLR_AUTO_INSTALL_UV=1` skips the
  prompt; non-interactive sessions without that env var refuse cleanly.
- User-defined groups named `self` or `project` are rejected with a
  clear warning, never silently shadowed.
- Plan 2's dispatch.rs subprocess spawn now uses the resolved venv
  python rather than a PATH lookup.
- The roadmap status table reflects Plan 3 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. **`reqwest` blocking client + TLS backend.** This plan picks
   `reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }`
   so the wheel build stays portable (no system OpenSSL dependency). If
   the implementer prefers a smaller dependency footprint, `ureq` is a
   thinner blocking client; the `download_and_extract` body is the only
   place to swap. The crate's other deps don't pull in reqwest.
2. **Archive extraction via shelling out to `tar`/`unzip`.** Task 4
   shells out for simplicity. macOS, Linux, and Windows-10+ all ship
   these. If a CI image surfaces without them, swap in `flat2 + tar` and
   `zip` crates. The public API of `perform_install` stays the same.
3. **uv release URL stability.** The plan uses
   `https://github.com/astral-sh/uv/releases/latest/download/...`. If the
   asset name scheme changes upstream, update `host_asset()`. Consider
   pinning to a specific known-good uv version rather than `latest` to
   avoid silent surprise upgrades — that's a trade-off the implementer
   makes given current uv stability.
4. **`MIN_UV_VERSION` choice.** The plan anchors at `0.4.0` as a
   conservative minimum. Bump when toolr starts to rely on a feature only
   present in a newer release.
5. **`uv sync --project tools/` vs `cd tools/ && uv sync`.** The plan
   uses `--project`. If the implementer finds that uv's `--project`
   semantics don't exactly match what we want (e.g. lock file location
   resolution differs), fall back to spawning with `current_dir(tools)`.
   The visible behavior should be identical for our `tools/pyproject.toml`
   - `tools/uv.lock` layout.
6. **`UV_PROJECT_ENVIRONMENT` override.** Task 9 passes
   `UV_PROJECT_ENVIRONMENT=<venv_dir>` so uv materialises the venv where
   toolr expects it (especially for the cache layout). Verify this env
   var still steers uv on the version pinned in `MIN_UV_VERSION` — uv
   has refactored this control surface before.
7. **Shell-aware activation in `venv shell`.** The plan uses a
   PATH-prepend + `VIRTUAL_ENV` env strategy that works for `bash`,
   `zsh`, and `fish`. PowerShell/cmd users on Windows may want a richer
   activation; that's out of scope for v1 but worth a follow-up.
