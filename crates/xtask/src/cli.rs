//! Command-line surface for `cargo xtask`.
//!
//! Each subcommand maps to a maintainer-only operation against the
//! toolr workspace. The crate is not published and is invoked exclusively
//! via the `cargo xtask` alias defined in `.cargo/config.toml`.

use clap::{Parser, Subcommand};

/// Top-level CLI for `cargo xtask`.
#[derive(Parser)]
#[command(
    name = "xtask",
    about = "Toolr maintainer tooling",
    long_about = "Maintainer-only commands for the toolr workspace. Not shipped \
                  in any released binary; invoked via the `cargo xtask` alias."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Regenerate `skills/*/references/*.md` from toolr's own source.
    ///
    /// Each registered skill has a generator that walks toolr-py /
    /// toolr-core for the surface it documents and emits a canonical
    /// markdown reference. `--check` exits non-zero if the on-disk
    /// files differ from what the generator produces, without writing
    /// any changes.
    BuildSkillRefs {
        /// Verify the on-disk references match regeneration. Exits 1
        /// (with a diff in stderr) on drift instead of overwriting.
        #[arg(long)]
        check: bool,
    },
}
