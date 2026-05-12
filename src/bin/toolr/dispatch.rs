use std::path::PathBuf;
use std::process::ExitCode;

use clap::ArgMatches;

use _rust_utils::complete::{
    InstallOptions, InstallOutcome, Shell as CompletionShell, completion_script, install_script,
    resolve_manifest_at_tab, serve_completions,
};
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
    if let Some(("self", self_matches)) = matches.subcommand() {
        return run_self(self_matches);
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
    // Auto-rebuild dynamic layer when the venv has changed since the
    // manifest was last written. Tab completion never takes this path.
    ensure_dynamic_layer_fresh(&repo_root, manifest)?;
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

fn run_self(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    match matches.subcommand() {
        Some(("build-manifest", bm_matches)) => run_self_build_manifest(bm_matches),
        Some(("completion", completion_matches)) => {
            let Some((action, action_matches)) = completion_matches.subcommand() else {
                anyhow::bail!("expected a `self completion` subcommand");
            };
            match action {
                "print" => run_completion_print(action_matches),
                "install" => run_completion_install(action_matches),
                other => anyhow::bail!("unsupported self completion subcommand: {other}"),
            }
        }
        _ => anyhow::bail!("expected a `self` subcommand"),
    }
}

fn run_self_build_manifest(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    use std::process::Command;

    let package: &String = matches
        .get_one("package")
        .ok_or_else(|| anyhow::anyhow!("missing required argument: package"))?;
    let python = resolve_python_for_build(matches.get_one::<String>("python").map(String::as_str))?;
    let mut cmd = Command::new(&python);
    cmd.args(["-m", "toolr.build", package]);
    if let Some(out) = matches.get_one::<String>("output") {
        cmd.args(["--output", out]);
    }
    if let Some(ver) = matches.get_one::<String>("schema-version") {
        cmd.args(["--schema-version", ver]);
    }
    if matches.get_flag("check") {
        cmd.arg("--check");
    }
    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("failed to spawn `{}`: {e}", python.display()))?;
    let code = status.code().unwrap_or(1);
    let clamped: u8 = code.clamp(0, 255).try_into().unwrap_or(1);
    Ok(ExitCode::from(clamped))
}

fn resolve_python_for_build(override_path: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_path {
        let p = PathBuf::from(path);
        if !p.is_file() {
            anyhow::bail!("--python `{}`: not a file", p.display());
        }
        return Ok(p);
    }
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let candidate = PathBuf::from(venv).join("bin").join("python");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    for name in ["python3", "python"] {
        if let Ok(path) = which::which(name) {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "no Python interpreter found. Pass --python <path>, activate a venv, or \
         ensure `python3` is on PATH."
    )
}

fn run_completion_install(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    let shell_str = matches
        .get_one::<String>("shell")
        .ok_or_else(|| anyhow::anyhow!("missing <shell>"))?;
    let shell: CompletionShell = shell_str.parse()?;
    let force = matches.get_flag("force");

    let home = dirs_home()?;
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let opts = InstallOptions {
        shell,
        xdg_data_home,
        xdg_config_home,
        home,
        force,
        interactive: std::io::IsTerminal::is_terminal(&std::io::stdin()),
    };

    let outcome = install_script(&opts)?;
    match outcome {
        InstallOutcome::Wrote { path } => {
            println!(
                "toolr: wrote {} completion script to {}",
                shell,
                path.display()
            );
            if matches!(shell, CompletionShell::Zsh) {
                println!(
                    "toolr: ensure your ~/.zshrc includes `fpath=(~/.zfunc $fpath)` and \
                     `autoload -Uz compinit && compinit`."
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        InstallOutcome::AlreadyInstalled { path } => {
            println!(
                "toolr: {} completion already installed at {}",
                shell,
                path.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        InstallOutcome::SkippedNeedsForce { path } => {
            eprintln!(
                "toolr: refusing to overwrite {} (use --force to replace)",
                path.display()
            );
            Ok(ExitCode::from(1))
        }
    }
}

fn dirs_home() -> anyhow::Result<PathBuf> {
    // Avoid taking on a new crate dep — read $HOME directly.
    let home = std::env::var_os("HOME")
        .ok_or_else(|| anyhow::anyhow!("$HOME is not set; cannot pick install path"))?;
    Ok(PathBuf::from(home))
}

fn run_completion_print(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    let shell_str = matches
        .get_one::<String>("shell")
        .ok_or_else(|| anyhow::anyhow!("missing <shell>"))?;
    let shell: CompletionShell = shell_str.parse()?;
    print!("{}", completion_script(shell));
    Ok(ExitCode::SUCCESS)
}

fn ensure_dynamic_layer_fresh(
    project_root: &std::path::Path,
    manifest: &Manifest,
) -> anyhow::Result<()> {
    use _rust_utils::dynamic::{compute_dynamic_hash, rebuild_dynamic_only};

    // Skip projects that don't have a tools venv configured.
    if !project_root.join("tools").join("pyproject.toml").is_file() {
        return Ok(());
    }
    let resolved = match resolve_venv_path(project_root) {
        Ok(r) => r,
        // Venv not yet set up — let the normal execute path surface the
        // diagnostic. We don't try to auto-rebuild against a missing venv.
        Err(_) => return Ok(()),
    };
    if !resolved.python.is_file() {
        return Ok(());
    }
    let current = compute_dynamic_hash(&resolved.venv_dir)?;
    if manifest.dynamic_hash == current && !current.is_empty() {
        return Ok(());
    }
    eprintln!("toolr: dynamic manifest layer stale; regenerating...");
    rebuild_dynamic_only(project_root, &resolved.python, &resolved.venv_dir)?;
    Ok(())
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
