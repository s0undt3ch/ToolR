//! Implementation of `toolr project <...>` subcommands.

use std::collections::HashSet;
use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::ArgMatches;

use crate::init_scaffold::{ScaffoldConflictsError, analyze_scaffold, execute_scaffold, scaffold};
use crate::init_templates::{ScaffoldOptions, parse_venv_location};

pub fn dispatch_project(matches: &ArgMatches) -> Result<ExitCode> {
    match matches.subcommand() {
        Some(("init", init_m)) => project_init(init_m),
        Some(("deps", _)) => deps_migration_hint(),
        Some(("venv", venv_m)) => match venv_m.subcommand() {
            Some(("path", _)) => venv_path(),
            Some(("shell", _)) => venv_shell(),
            Some(("sync", sync_m)) => venv_sync(sync_m),
            Some(("upgrade", upgrade_m)) => venv_upgrade(upgrade_m),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        Some(("manifest", manifest_m)) => match manifest_m.subcommand() {
            Some(("rebuild", _)) => manifest_rebuild(),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        _ => unreachable!("clap enforces subcommand_required"),
    }
}

/// Emit the migration hint for `toolr project deps <…>`. Returns exit
/// code 2 (same code we use for "your inputs were valid but you're
/// pointing at the wrong target" — see `project_init`'s scaffold-
/// conflict path).
fn deps_migration_hint() -> Result<ExitCode> {
    eprintln!("error: `project deps` was removed in 0.22");
    eprintln!("hint: use `toolr project venv` instead");
    eprintln!("       project deps sync       →  toolr project venv sync");
    eprintln!("       project deps upgrade …  →  toolr project venv upgrade …");
    eprintln!("see CHANGELOG.md (0.22 BREAKING) for the rename");
    Ok(ExitCode::from(2))
}

fn project_init(matches: &ArgMatches) -> Result<ExitCode> {
    let force = matches.get_flag("force");
    let no_sync = matches.get_flag("no-sync");
    let no_example = matches.get_flag("no-example");
    let quiet = matches.get_flag("quiet");
    let yes_all = matches.get_flag("yes");
    let venv_location_str = matches
        .get_one::<String>("venv-location")
        .map(String::as_str)
        .unwrap_or("cache");
    let venv_location = parse_venv_location(venv_location_str)?;
    let requires_python = matches
        .get_one::<String>("python")
        .cloned()
        .unwrap_or_else(detect_requires_python);

    let cwd = std::env::current_dir()?;
    let opts = ScaffoldOptions {
        requires_python,
        venv_location,
        include_example: !no_example,
    };

    // Non-interactive path: detect conflicts early and exit with code 2 so
    // agents can distinguish "conflicts" from generic failures (exit 1).
    if !force && !yes_all && !std::io::stdin().is_terminal() {
        let analysis = analyze_scaffold(&cwd, &opts)?;
        let conflicts = analysis.conflict_files();
        if !conflicts.is_empty() {
            let err = ScaffoldConflictsError {
                files: conflicts
                    .into_iter()
                    .map(|p| p.strip_prefix(&cwd).unwrap_or(p).to_path_buf())
                    .collect(),
            };
            eprintln!("toolr: error: {err}");
            return Ok(ExitCode::from(2));
        }
    }

    run_project_init(
        &cwd,
        &opts,
        force,
        no_sync,
        quiet,
        yes_all,
        &mut |dest| prompt_overwrite_file(dest, &cwd),
    )
}

/// Inner implementation, factored out so the confirm callback can be injected
/// in tests without spinning up a full process.
pub(crate) fn run_project_init(
    cwd: &Path,
    opts: &ScaffoldOptions,
    force: bool,
    no_sync: bool,
    quiet: bool,
    yes_all: bool,
    confirm: &mut dyn FnMut(&Path) -> Result<bool>,
) -> Result<ExitCode> {
    let outcome = if force {
        scaffold(cwd, opts, true)?
    } else {
        let analysis = analyze_scaffold(cwd, opts)?;
        let conflicts = analysis.conflict_files();

        let overwrite: HashSet<PathBuf> = if conflicts.is_empty() {
            HashSet::new()
        } else if yes_all {
            // --yes: approve all conflicts without prompting.
            conflicts.iter().map(|p| p.to_path_buf()).collect()
        } else {
            // Call confirm once per conflicting file; collect approved paths.
            conflicts
                .iter()
                .filter_map(|dest| match confirm(dest) {
                    Err(e) => Some(Err(e)),
                    Ok(true) => Some(Ok(dest.to_path_buf())),
                    Ok(false) => None,
                })
                .collect::<Result<_>>()?
        };

        execute_scaffold(&analysis, &overwrite)?
    };

    if !quiet {
        println!("toolr: scaffolded tools/ at {}", outcome.tools_dir.display());
        for path in &outcome.files_written {
            let rel = path.strip_prefix(cwd).unwrap_or(path).display();
            println!("toolr:   wrote {rel}");
        }
    }

    if no_sync {
        if !quiet {
            println!("toolr: skipping `uv sync` (--no-sync)");
            println!("toolr: run `toolr project venv sync` when you are ready");
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Auto-sync — same path as `toolr project venv sync --force`.
    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) =
        toolr_core::project::ensure_venv_ready(
            cwd,
            consent,
            toolr_core::project::EnsureOpts::default().with_force_sync(true),
        )?;
    if !quiet {
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0,
            uv.version.1,
            uv.version.2,
        );
        println!("toolr:");
        println!("toolr: next steps:");
        println!("toolr:   toolr example hello");
        println!("toolr:   toolr example commit");
        println!(
            "toolr:   toolr self completion install <bash|zsh|fish>   # optional, for tab completion"
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Prompt the user whether to overwrite a single conflicting file.
/// Returns `Ok(true)` if approved, `Ok(false)` if declined (including EOF).
fn prompt_overwrite_file(dest: &Path, cwd: &Path) -> Result<bool> {
    let rel = dest.strip_prefix(cwd).unwrap_or(dest);
    eprint!("toolr: overwrite {}? [y/N] ", rel.display());
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    // EOF (0 bytes read) is treated as "no" — same as pressing Enter.
    Ok(answer.trim().eq_ignore_ascii_case("y"))
}

/// Default `requires-python` value for new projects.
fn detect_requires_python() -> String {
    if let Ok(output) = std::process::Command::new("python3")
        .arg("-c")
        .arg("import sys; print(f'>={sys.version_info.major}.{sys.version_info.minor}')")
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    ">=3.11".to_string()
}

fn manifest_rebuild() -> Result<ExitCode> {
    use toolr_core::dynamic::rebuild_manifest_full;

    let cwd = std::env::current_dir()?;
    let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
    let resolved = toolr_core::venv::resolve_venv_path(&repo_root)?;
    let outcome = rebuild_manifest_full(&repo_root, &resolved.python, &resolved.venv_dir)?;
    for w in &outcome.warnings {
        eprintln!("toolr: warning: {w}");
    }
    println!(
        "toolr: wrote {} groups / {} commands to {}",
        outcome.group_count,
        outcome.command_count,
        outcome.manifest_path.display(),
    );
    Ok(ExitCode::SUCCESS)
}

fn venv_sync(matches: &ArgMatches) -> Result<ExitCode> {
    let force = matches.get_flag("force");
    let quiet = matches.get_flag("quiet");

    let cwd = std::env::current_dir()?;
    let mut consent = toolr_core::uv::install::ConsentMode::from_env();
    if quiet {
        // Unattended path: never prompt. If uv is missing and we have
        // no env-level consent, return Refuse silently and the guards
        // in venv_sync_unattended_quiet_exit convert that into a
        // benign exit 0.
        consent.silent_refuse = true;
    }

    let opts = toolr_core::project::EnsureOpts::default()
        .with_force_sync(force)
        .with_quiet(quiet);

    let result = toolr_core::project::ensure_venv_ready(&cwd, consent, opts);

    if quiet {
        if let Some(code) = venv_sync_unattended_quiet_exit(&result) {
            return Ok(code);
        }
    }

    let (resolved, uv) = result?;

    if !quiet {
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0, uv.version.1, uv.version.2,
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Unattended-mode guard table for `venv_sync --quiet`. Returns
/// `Some(ExitCode::SUCCESS)` when the failure is one of the benign rows
/// that `--quiet` swallows (not-a-toolr-repo, missing tools/, uv-consent
/// absent). Returns `None` to let the error propagate normally (lock
/// unparsable, uv sync failed, etc).
fn venv_sync_unattended_quiet_exit(
    result: &Result<(toolr_core::venv::ResolvedVenv, toolr_core::uv::UvBinary)>,
) -> Option<ExitCode> {
    let err = result.as_ref().err()?;
    let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
    let joined = chain.join(" :: ");

    // Benign markers from error contexts emitted by `ensure_venv_ready`
    // and the `uv` install path. We match substrings the error chain
    // already produces — keep this list aligned with:
    //   - discover_project_root NotFound        → "locating project root"
    //   - resolve_venv_path missing pyproject   → "resolving the tools venv path"
    //   - ensure_uv with silent_refuse=true     → UvError::UserRefusedInstall
    //     whose Display is "user declined uv install; ..."
    let benign_markers = [
        "locating project root",
        "resolving the tools venv path",
        "user declined uv install",
    ];

    if benign_markers.iter().any(|m| joined.contains(m)) {
        return Some(ExitCode::SUCCESS);
    }
    None
}

fn venv_upgrade(matches: &ArgMatches) -> Result<ExitCode> {
    let package = matches
        .get_one::<String>("package")
        .expect("clap marks this required")
        .as_str();
    // Reuse the existing flow; this function used to be named deps_upgrade.

    let cwd = std::env::current_dir()?;
    let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
    let tools_dir = repo_root.join("tools");

    // Guard: `uv lock --upgrade-package` silently no-ops if the package
    // isn't part of the dependency graph. Catch typos and "I thought I
    // had this declared" cases up front so the user sees a real error.
    let pyproject = tools_dir.join("pyproject.toml");
    if !pyproject_declares_dependency(&pyproject, package)? {
        anyhow::bail!(
            "package `{package}` is not declared in {} — add it to `[project] dependencies` first",
            pyproject.display(),
        );
    }

    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) =
        toolr_core::project::ensure_venv_ready(&cwd, consent, toolr_core::project::EnsureOpts::default())?;

    let lock_status = toolr_core::venv::run_uv_lock_upgrade(&uv, &tools_dir, &resolved, package)?;
    if !lock_status.success() {
        anyhow::bail!(
            "`uv lock --upgrade-package {package}` failed with exit code {:?}",
            lock_status.code(),
        );
    }

    let sync_status = toolr_core::venv::run_uv_sync(&uv, &tools_dir, &resolved, /*quiet=*/ false)?;
    if !sync_status.success() {
        anyhow::bail!(
            "`uv sync` after upgrade failed with exit code {:?}",
            sync_status.code(),
        );
    }

    println!(
        "toolr: upgraded `{package}` in {}",
        resolved.venv_dir.display(),
    );
    Ok(ExitCode::SUCCESS)
}

/// Light TOML inspection: does `[project].dependencies` (or any
/// `[project.optional-dependencies.*]` list) name `package`?
/// We only need to confirm presence — version pin / extras are uv's
/// problem from here on.
fn pyproject_declares_dependency(pyproject: &Path, package: &str) -> Result<bool> {
    let text = std::fs::read_to_string(pyproject)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", pyproject.display()))?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", pyproject.display()))?;

    let mut found = false;
    if let Some(deps) = parsed
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|v| v.as_array())
    {
        for dep in deps {
            if let Some(s) = dep.as_str() {
                if dep_name_matches(s, package) {
                    found = true;
                    break;
                }
            }
        }
    }
    if !found {
        if let Some(opt) = parsed
            .get("project")
            .and_then(|p| p.get("optional-dependencies"))
            .and_then(|v| v.as_table())
        {
            for (_extra, list) in opt {
                if let Some(deps) = list.as_array() {
                    for dep in deps {
                        if let Some(s) = dep.as_str() {
                            if dep_name_matches(s, package) {
                                found = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(found)
}

/// PEP 508-light: `foo`, `foo[bar]`, `foo>=1.2`, `foo ==1.2; python_version < "3.13"`
/// all parse to the name `foo`. We just peel the first identifier-ish token.
fn dep_name_matches(spec: &str, package: &str) -> bool {
    let name_end = spec
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.')
        .unwrap_or(spec.len());
    spec[..name_end].eq_ignore_ascii_case(package)
}

fn venv_path() -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
    let resolved = toolr_core::venv::resolve_venv_path(&repo_root)?;
    println!("{}", resolved.venv_dir.display());
    Ok(ExitCode::SUCCESS)
}

fn venv_shell() -> Result<ExitCode> {
    use std::process::Command;

    let cwd = std::env::current_dir()?;
    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, _) = toolr_core::project::ensure_venv_ready(
        &cwd,
        consent,
        toolr_core::project::EnsureOpts::default(),
    )?;

    let shell = default_shell_path();
    let bin_dir = venv_bin_dir(&resolved.venv_dir);
    let prepended_path = prepend_to_path(&bin_dir, std::env::var_os("PATH").as_deref())?;

    let status = Command::new(&shell)
        .env("VIRTUAL_ENV", &resolved.venv_dir)
        .env("PATH", &prepended_path)
        // Help shell prompts notice the activation.
        .env("TOOLR_VENV", &resolved.venv_dir)
        .status()?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

/// Resolve the shell binary to spawn for `toolr project venv shell`.
/// Honours `$SHELL`, falling back to a per-OS default. Extracted as a
/// pure helper so the fallback arms are unit-testable.
fn default_shell_path() -> std::path::PathBuf {
    std::env::var_os("SHELL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                std::path::PathBuf::from("cmd.exe")
            } else {
                std::path::PathBuf::from("/bin/sh")
            }
        })
}

/// Resolve the `bin` (Unix) or `Scripts` (Windows) sub-directory of a
/// venv. Extracted so `venv_shell`'s PATH-prepend logic is testable.
fn venv_bin_dir(venv_dir: &std::path::Path) -> std::path::PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts")
    } else {
        venv_dir.join("bin")
    }
}

/// Prepend `dir` to a colon-separated PATH-style value. When `existing`
/// is `None`, returns just `dir` as an `OsString`. Surfaces
/// `join_paths` errors via `?`.
fn prepend_to_path(
    dir: &std::path::Path,
    existing: Option<&std::ffi::OsStr>,
) -> Result<std::ffi::OsString> {
    match existing {
        Some(existing) => {
            let mut paths: Vec<_> = std::env::split_paths(existing).collect();
            paths.insert(0, dir.to_path_buf());
            Ok(std::env::join_paths(paths)?)
        }
        None => Ok(dir.as_os_str().to_os_string()),
    }
}

#[cfg(test)]
mod tests {
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

        let opts = init_opts();
        run_project_init(
            tmp.path(),
            &opts,
            /*force=*/ false,
            /*no_sync=*/ true,
            /*quiet=*/ true,
            /*yes_all=*/ true,
            &mut |_| unreachable!("confirm should not be called when yes_all=true"),
        )
        .unwrap();

        let content = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
        assert!(content.contains(r#"name = "tools""#));
    }

    /// run_project_init with a confirm closure that says "no" leaves conflicts intact.
    #[test]
    fn run_project_init_confirm_no_preserves_conflicts() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

        let opts = init_opts();
        run_project_init(
            tmp.path(),
            &opts,
            /*force=*/ false,
            /*no_sync=*/ true,
            /*quiet=*/ true,
            /*yes_all=*/ false,
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

        let opts = init_opts();
        run_project_init(
            tmp.path(),
            &opts,
            /*force=*/ false,
            /*no_sync=*/ true,
            /*quiet=*/ true,
            /*yes_all=*/ false,
            &mut |dest| {
                // Approve only pyproject.toml.
                Ok(dest.ends_with("pyproject.toml"))
            },
        )
        .unwrap();

        let pyproject = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
        assert!(pyproject.contains(r#"name = "tools""#), "should be overwritten");
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
        let result =
            prepend_to_path(bin, Some(std::ffi::OsStr::new(existing))).unwrap();
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
        assert!(dep_name_matches("toolr-py==0.11.2; python_version < \"3.13\"", "toolr-py"));
        assert!(!dep_name_matches("other", "toolr-py"));
    }

    #[test]
    fn dep_name_matches_is_case_insensitive() {
        assert!(dep_name_matches("Toolr-Py>=0.11", "toolr-py"));
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
    fn detect_requires_python_returns_non_empty_string() {
        // We can't pin a specific version — depends on the runner's
        // python3. Just assert the contract: returns something with the
        // `>=X.Y` prefix, falling back to ">=3.11" if python3 absent.
        let s = detect_requires_python();
        assert!(
            s.starts_with(">="),
            "expected >= prefix, got: {s}",
        );
    }
}
