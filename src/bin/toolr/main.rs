mod cli;

use cli::Cli;

fn main() {
    let _args = Cli::parse_args();
    // Subcommand dispatch lands in a later task.
    eprintln!("toolr: no user commands registered yet (manifest support comes in Task 15).");
    std::process::exit(0);
}
