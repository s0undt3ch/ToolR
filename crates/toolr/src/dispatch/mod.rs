//! Routes the parsed command line: help, then toolr's own commands, then the user's command.

mod complete;
mod run;
mod self_cmd;

use std::process::ExitCode;

use clap::ArgMatches;
use toolr_core::manifest::Manifest;

use crate::help::{self, HelpMode};
use crate::invocation::{Invocation, RootArgv};

/// The deepest matched command with the help flag set, its `bin_path` and `mode`, or `None`
/// when no help was asked for. `-h` and `--help` are one global arg, so `mode` comes from argv.
fn resolve_help_target(
    root_cmd: &clap::Command,
    matches: &ArgMatches,
    mode: HelpMode,
) -> Option<(clap::Command, String, HelpMode)> {
    let mut cur_cmd = root_cmd.clone();
    let mut cur_matches = matches;
    let mut path: Vec<String> = vec![root_cmd.get_name().to_string()];

    while let Some((sub_name, sub_matches)) = cur_matches.subcommand() {
        let Some(child) = cur_cmd.find_subcommand(sub_name) else {
            break;
        };
        cur_cmd = child.clone();
        cur_matches = sub_matches;
        path.push(sub_name.to_string());
    }

    if !cur_matches.get_flag("help") {
        return None;
    }
    Some((cur_cmd, path.join(" "), mode))
}

/// Help for a request clap rejected first: `subcommand_required` commands (`self`, `project`,
/// `self cache`) fail validation before the global help flag is seen. The target is the
/// deepest command argv names.
pub fn print_help_from_argv(argv: &RootArgv, root: &clap::Command, mode: HelpMode) {
    let (cmd, names) = argv.command_path(root);
    let path: Vec<&str> = std::iter::once(root.get_name()).chain(names).collect();
    help::print(cmd, &path.join(" "), mode);
}

pub fn dispatch(
    matches: &ArgMatches,
    manifest: &Manifest,
    root: &mut clap::Command,
    run: Invocation,
) -> anyhow::Result<ExitCode> {
    // Intercept --help / -h before any other routing. Tab-completion
    // invocations (`__complete`) never set help flags, so they pass through
    // immediately.
    let mode = run.argv.help_mode().unwrap_or(HelpMode::Long);
    if !matches!(matches.subcommand(), Some(("__complete", _)))
        && let Some((resolved_cmd, bin_path, mode)) = resolve_help_target(root, matches, mode)
    {
        help::print(&resolved_cmd, &bin_path, mode);
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(("__complete", sub)) = matches.subcommand() {
        return complete::run_complete(sub, &run.argv);
    }
    if let Some(("self", self_matches)) = matches.subcommand() {
        return self_cmd::run_self(self_matches, matches.get_flag("quiet"));
    }
    if let Some(("__install-uv-now", _)) = matches.subcommand() {
        return self_cmd::run_install_uv_now();
    }
    if let Some(("project", project_m)) = matches.subcommand() {
        return crate::project::dispatch_project(project_m, matches.get_flag("quiet"));
    }
    let Some((first_name, first_matches)) = matches.subcommand() else {
        // No subcommand and no --help flag: show root help as a courtesy,
        // using the same styled renderer so we never mix renderers.
        help::print(root, "toolr", HelpMode::Long);
        return Ok(ExitCode::SUCCESS);
    };
    run::run_command(matches, (first_name, first_matches), manifest, root, run)
}
