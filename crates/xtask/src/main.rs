//! Maintainer-only tooling for the toolr workspace.
//!
//! This crate is invoked via the `cargo xtask` alias defined in
//! `.cargo/config.toml`. It is **not** published and is **not** built
//! into the release `toolr` binary. End users have no need to run it
//! — they consume what it generates (e.g. `skills/*/references/*.md`)
//! via `skillshare`.

mod build_skill_refs;
mod cli;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::BuildSkillRefs { check } => build_skill_refs::run(check),
    }
}
