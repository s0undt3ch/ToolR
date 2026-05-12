use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "toolr",
    version,
    about = "In-project CLI tooling support",
    long_about = None,
    disable_help_subcommand = true,
)]
pub struct Cli {
    /// Increase verbosity.
    #[arg(short = 'd', long = "debug", global = true)]
    pub debug: bool,

    /// Suppress non-error output.
    #[arg(short = 'q', long = "quiet", global = true, conflicts_with = "debug")]
    pub quiet: bool,
}

impl Cli {
    pub fn parse_args() -> Self {
        <Self as Parser>::parse()
    }
}
