use std::path::PathBuf;
use std::process::ExitCode;

use clap::ArgMatches;

use _rust_utils::complete::{resolve_manifest_at_tab, serve_completions};
use _rust_utils::discovery::discover_project_root;
use _rust_utils::execute::{
    build_spec, resolve_python, spawn_runner, wait_with_signals, write_spec_to_tempfile,
};
use _rust_utils::manifest::Manifest;
use _rust_utils::venv::resolve_venv_path;

pub fn dispatch(
    matches: &ArgMatches,
    manifest: &Manifest,
    root: &mut clap::Command,
) -> anyhow::Result<ExitCode> {
    if let Some(("__complete", sub)) = matches.subcommand() {
        return run_complete(sub);
    }
    if let Some(("__build-static-manifest", _)) = matches.subcommand() {
        return run_build_static_manifest();
    }
    if let Some(("__install-uv-now", _)) = matches.subcommand() {
        return run_install_uv_now();
    }
    if let Some(("project", project_m)) = matches.subcommand() {
        return crate::project::dispatch_project(project_m);
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
    // Prefer the resolved tools-venv python (Plan 3). Fall back to the
    // PATH/TOOLR_PYTHON lookup only when there is no `tools/pyproject.toml`
    // — i.e. legacy projects that never opted into the venv layer.
    let python = if repo_root.join("tools").join("pyproject.toml").is_file() {
        resolve_venv_path(&repo_root)?.python
    } else {
        resolve_python()?
    };
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

fn run_complete(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    // Tab completion must be quiet: any error produces a silent exit
    // code 1 so the shell falls back to its default completion. We do
    // not write to stderr here — that would clobber the user's prompt.
    let Some(cwd) = matches.get_one::<String>("cwd").map(PathBuf::from) else {
        return Ok(ExitCode::from(1));
    };
    let tokens: Vec<String> = matches
        .get_many::<String>("args")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let Ok(resolved) = resolve_manifest_at_tab(&cwd) else {
        return Ok(ExitCode::from(1));
    };
    for candidate in serve_completions(&resolved.manifest, &tokens) {
        println!("{candidate}");
    }
    Ok(ExitCode::SUCCESS)
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

fn run_install_uv_now() -> anyhow::Result<std::process::ExitCode> {
    let consent = _rust_utils::uv::install::ConsentMode {
        yes_flag: true,
        auto_install_env: true,
    };
    let uv = _rust_utils::uv::ensure_uv(consent)?;
    println!(
        "toolr: uv {}.{}.{} ready at {} (source: {:?})",
        uv.version.0,
        uv.version.1,
        uv.version.2,
        uv.path.display(),
        uv.source,
    );
    Ok(std::process::ExitCode::SUCCESS)
}

#[allow(dead_code)]
pub(crate) fn report_uv_error(err: &_rust_utils::uv::UvError) -> String {
    use _rust_utils::uv::UvError;
    match err {
        UvError::UserRefusedInstall => {
            "toolr: uv is required for this command. Install from \
             https://docs.astral.sh/uv/getting-started/installation/ \
             and rerun, or set TOOLR_AUTO_INSTALL_UV=1."
                .into()
        }
        UvError::VersionTooOld { found, required } => format!(
            "toolr: uv on PATH is {}.{}.{}, but toolr requires \
             >= {}.{}.{}. Upgrade uv and try again.",
            found.0, found.1, found.2, required.0, required.1, required.2,
        ),
        other => format!("toolr: {other}"),
    }
}
