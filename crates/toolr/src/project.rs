//! Implementation of `toolr project <...>` subcommands.

use std::process::ExitCode;

use anyhow::Result;
use clap::ArgMatches;

use crate::init_scaffold::scaffold;
use crate::init_templates::{ScaffoldOptions, parse_venv_location};

pub fn dispatch_project(matches: &ArgMatches) -> Result<ExitCode> {
    match matches.subcommand() {
        Some(("init", init_m)) => project_init(init_m),
        Some(("deps", deps_m)) => match deps_m.subcommand() {
            Some(("sync", _)) => deps_sync(),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        Some(("venv", venv_m)) => match venv_m.subcommand() {
            Some(("path", _)) => venv_path(),
            Some(("shell", _)) => venv_shell(),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        Some(("manifest", manifest_m)) => match manifest_m.subcommand() {
            Some(("rebuild", _)) => manifest_rebuild(),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        _ => unreachable!("clap enforces subcommand_required"),
    }
}

fn project_init(matches: &ArgMatches) -> Result<ExitCode> {
    let force = matches.get_flag("force");
    let no_sync = matches.get_flag("no-sync");
    let no_example = matches.get_flag("no-example");
    let quiet = matches.get_flag("quiet");
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
    let outcome = scaffold(&cwd, &opts, force)?;

    if !quiet {
        println!("toolr: scaffolded tools/ at {}", outcome.tools_dir.display());
        for path in &outcome.files_written {
            let rel = path.strip_prefix(&cwd).unwrap_or(path).display();
            println!("toolr:   wrote {rel}");
        }
    }

    if no_sync {
        if !quiet {
            println!("toolr: skipping `uv sync` (--no-sync)");
            println!("toolr: run `toolr project deps sync` when you are ready");
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Auto-sync — same path as `toolr project deps sync`.
    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) =
        toolr_core::project::ensure_venv_ready(&cwd, consent, /*force_sync=*/ true)?;
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

fn deps_sync() -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) = toolr_core::project::ensure_venv_ready(
        &cwd, consent, /*force_sync=*/ true,
    )?;
    println!(
        "toolr: synced venv at {} using uv {}.{}.{}",
        resolved.venv_dir.display(),
        uv.version.0, uv.version.1, uv.version.2,
    );
    Ok(ExitCode::SUCCESS)
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
        &cwd, consent, /*force_sync=*/ false,
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
