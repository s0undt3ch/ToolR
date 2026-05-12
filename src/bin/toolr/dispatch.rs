use std::process::ExitCode;

use clap::ArgMatches;
use _rust_utils::manifest::Manifest;

pub fn dispatch(
    matches: &ArgMatches,
    manifest: &Manifest,
    root: &mut clap::Command,
) -> anyhow::Result<ExitCode> {
    if let Some(("__build-static-manifest", _)) = matches.subcommand() {
        return run_build_static_manifest();
    }
    let Some((group_name, group_matches)) = matches.subcommand() else {
        root.print_help()?;
        return Ok(ExitCode::SUCCESS);
    };
    let Some((cmd_name, _)) = group_matches.subcommand() else {
        // toolr <group> with no command → print group help
        return Ok(ExitCode::SUCCESS);
    };
    let cmd = manifest
        .commands
        .iter()
        .find(|c| c.group == group_name && c.name == cmd_name)
        .ok_or_else(|| anyhow::anyhow!("unknown command: {group_name} {cmd_name}"))?;

    // Plan 2 wires this up to a Python subprocess.
    eprintln!(
        "toolr: execution backend not yet implemented (would run {}/{}). \
         See specs/rust-front-end/03-plan-2-runner-execute.md.",
        cmd.group, cmd.name
    );
    Ok(ExitCode::from(64))
}

fn run_build_static_manifest() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let root = _rust_utils::discovery::discover_project_root(&cwd)?;
    let tools = root.join("tools");
    let manifest = _rust_utils::parser::build_static_manifest(&tools)?;
    let path = tools.join(".toolr-manifest.json");
    _rust_utils::manifest::write_manifest(&path, &manifest)?;
    println!(
        "toolr: wrote {} groups / {} commands to {}",
        manifest.groups.len(),
        manifest.commands.len(),
        path.display()
    );
    Ok(ExitCode::SUCCESS)
}
