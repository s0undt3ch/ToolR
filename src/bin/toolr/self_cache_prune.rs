//! `toolr self cache prune` — populated by Plan 8 Task 7. Until Task 6
//! lands the classification primitives, this is a stub that errors out.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use clap::ArgMatches;

pub fn run(_cache_root: &Path, _matches: &ArgMatches) -> Result<ExitCode> {
    anyhow::bail!("toolr self cache prune is implemented in the next task")
}
