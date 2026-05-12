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
