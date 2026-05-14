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

    let shell = std::env::var_os("SHELL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                std::path::PathBuf::from("cmd.exe")
            } else {
                std::path::PathBuf::from("/bin/sh")
            }
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
