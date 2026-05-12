use std::process::ExitCode;

use clap::ArgMatches;

use _rust_utils::discovery::discover_project_root;
use _rust_utils::execute::{
    build_spec, resolve_python, spawn_runner, wait_with_signals, write_spec_to_tempfile,
};
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
    let Some((cmd_name, cmd_matches)) = group_matches.subcommand() else {
        // toolr <group> with no command → print group help
        return Ok(ExitCode::SUCCESS);
    };
    let cmd = manifest
        .commands
        .iter()
        .find(|c| c.group == group_name && c.name == cmd_name)
        .ok_or_else(|| anyhow::anyhow!("unknown command: {group_name} {cmd_name}"))?;

    let cwd = std::env::current_dir()?;
    let repo_root = discover_project_root(&cwd)?;
    let verbosity = if matches.get_flag("quiet") {
        "quiet"
    } else if matches.get_flag("debug") {
        "verbose"
    } else {
        "normal"
    };
    let log_level = if matches.get_flag("debug") {
        "DEBUG"
    } else {
        "INFO"
    };
    let spec = build_spec(cmd, cmd_matches, &repo_root, verbosity, false, log_level);

    let tempfile = write_spec_to_tempfile(&spec)?;
    let python = resolve_python()?;
    let mut child = spawn_runner(&python, tempfile.path())?;
    let status = wait_with_signals(&mut child)?;

    // Map child status to a process exit code.
    let code = status.code().unwrap_or_else(|| {
        // Signal-terminated child on Unix: report 128 + signal.
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = status.signal() {
                return 128 + sig;
            }
        }
        1
    });
    // ExitCode only carries u8 — clamp anything outside 0..=255.
    let clamped: u8 = code.clamp(0, 255).try_into().unwrap_or(1);
    Ok(ExitCode::from(clamped))
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
