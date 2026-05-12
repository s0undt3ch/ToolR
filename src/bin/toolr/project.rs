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
    // Implemented in Task 15.
    Ok(ExitCode::from(2))
}
