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
        Some(("manifest", manifest_m)) => match manifest_m.subcommand() {
            Some(("rebuild", _)) => manifest_rebuild(),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        _ => unreachable!("clap enforces subcommand_required"),
    }
}

fn manifest_rebuild() -> Result<ExitCode> {
    use _rust_utils::dynamic::rebuild_manifest_full;

    let cwd = std::env::current_dir()?;
    let repo_root = _rust_utils::discovery::discover_project_root(&cwd)?;
    let resolved = _rust_utils::venv::resolve_venv_path(&repo_root)?;
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

fn venv_path() -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let repo_root = _rust_utils::discovery::discover_project_root(&cwd)?;
    let resolved = _rust_utils::venv::resolve_venv_path(&repo_root)?;
    println!("{}", resolved.venv_dir.display());
    Ok(ExitCode::SUCCESS)
}

fn venv_shell() -> Result<ExitCode> {
    use std::process::Command;

    let cwd = std::env::current_dir()?;
    let consent = _rust_utils::uv::install::ConsentMode::from_env();
    let (resolved, _) = _rust_utils::project::ensure_venv_ready(
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
