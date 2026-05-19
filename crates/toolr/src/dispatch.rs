use std::path::PathBuf;
use std::process::ExitCode;

use clap::ArgMatches;

use toolr_core::complete::{
    InstallOptions, InstallOutcome, Shell as CompletionShell, completion_script, install_script,
    resolve_manifest_at_tab, serve_completions,
};
use toolr_core::discovery::discover_project_root;
use toolr_core::execute::{
    resolve_python, spawn_runner_capturing_stderr, wait_with_signals, write_spec_to_tempfile,
};
use toolr_core::manifest::Manifest;
use toolr_core::venv::resolve_venv_path;

use crate::execute_build::{OutputOptions, build_dispatch_spec, build_spec, pack_child_args};

/// Resolve a parsed subcommand `path` (e.g. `["jenkins", "job", "migrate"]`)
/// to its manifest entry. Tries the most-specific candidate group first
/// (`path[..len-1]`), then falls back one level up (`path[..len-2]`) so
/// that grafted children dispatched under a parent group still resolve
/// when the user typed an extra hop (the dispatcher segment) on the
/// command line. Returns `None` when no candidate group hosts a command
/// with the leaf name.
fn find_command_for_path<'a>(
    manifest: &'a Manifest,
    path: &[String],
) -> Option<&'a toolr_core::manifest::Command> {
    let leaf_name = path.last()?;
    let candidates: Vec<String> = if path.len() >= 2 {
        vec![
            path[..path.len() - 1].join("."),
            path[..path.len() - 2].join("."),
        ]
    } else {
        vec![String::new()]
    };
    // Try the most-specific group first; first match wins.
    candidates.iter().find_map(|group| {
        manifest
            .commands
            .iter()
            .find(|c| &c.group == group && &c.name == leaf_name)
    })
}

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
    let Some((first_name, first_matches)) = matches.subcommand() else {
        root.print_help()?;
        return Ok(ExitCode::SUCCESS);
    };
    // Walk down the subcommand chain so nested groups (`docker image
    // build`) reach their leaf command. `path` collects every
    // intermediate name; the last entry is the leaf, the prefix is
    // the dotted full_path of the owning group. `parent_matches`
    // tracks the matches one level above the leaf — needed when the
    // leaf is a dispatched grafted command so we can extract the
    // parent dispatcher's own kwargs.
    let mut path: Vec<String> = vec![first_name.to_string()];
    let mut current = first_matches;
    let mut parent_matches: &ArgMatches = matches;
    while let Some((next_name, next_matches)) = current.subcommand() {
        path.push(next_name.to_string());
        parent_matches = current;
        current = next_matches;
    }
    let cmd_matches = current;
    if path.len() < 2 {
        // toolr <group> with no command → print group help
        return Ok(ExitCode::SUCCESS);
    }
    let cmd = find_command_for_path(manifest, &path).ok_or_else(|| {
        anyhow::anyhow!("unknown command: {}", path.join(" "))
    })?;

    let cwd = std::env::current_dir()?;
    let repo_root = discover_project_root(&cwd)?;
    // Auto-rebuild dynamic layer when the venv has changed since the
    // manifest was last written. Tab completion never takes this path.
    ensure_dynamic_layer_fresh(&repo_root, manifest)?;
    let output_opts = output_options_from_matches(matches);
    // Dispatched leaves take a separate spec-shape: the runner sees
    // `dispatch: Some(...)` and routes to `invoke_dispatcher` instead
    // of calling `function` as a regular command. Pack the child first,
    // then build a parent-shaped spec around the dispatcher entry.
    //
    // The dispatcher's own manifest entry lives next to the leaf in the
    // same group; its `name` matches the group's final segment (the
    // "@group.command def <group_leaf>(...)" pattern produced by
    // `graft_children`). We look it up so `build_dispatch_spec` can
    // iterate the dispatcher's OWN arguments (the --cpu/--ram-style
    // outer flags) and extract them from `parent_matches`, instead of
    // iterating the leaf's arguments (which are packed in `packed`).
    let spec = if cmd.dispatched_from.is_some() {
        let packed = pack_child_args(cmd, cmd_matches);
        let group_leaf = cmd.group.rsplit('.').next().unwrap_or(cmd.group.as_str());
        let dispatcher = manifest
            .commands
            .iter()
            .find(|p| p.group == cmd.group && p.name == group_leaf)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "dispatcher manifest entry for `{}` (group `{}`, name `{}`) not found",
                    cmd.name,
                    cmd.group,
                    group_leaf,
                )
            })?;
        build_dispatch_spec(dispatcher, parent_matches, packed, &repo_root, &output_opts)
    } else {
        build_spec(cmd, cmd_matches, &repo_root, &output_opts)
    };

    let tempfile = write_spec_to_tempfile(&spec)?;
    // Prefer the resolved tools-venv python (Plan 3). Fall back to the
    // PATH/TOOLR_PYTHON lookup only when there is no `tools/pyproject.toml`
    // — i.e. legacy projects that never opted into the venv layer.
    let (python, venv_dir) = if repo_root.join("tools").join("pyproject.toml").is_file() {
        let resolved = resolve_venv_path(&repo_root)?;
        (resolved.python, Some(resolved.venv_dir))
    } else {
        (resolve_python()?, None)
    };

    // Plan 8: touch last_used_at on every invocation against a cached venv.
    if let Some(venv) = &venv_dir {
        if let Some(cache_dir) = venv.parent() {
            if let Err(e) = toolr_core::cache::touch_last_used(cache_dir) {
                eprintln!("toolr: warning: failed to touch cache meta.json: {e}");
            }
        }
    }

    // Pre-flight missing-dependency check (Plan 7). Skip when the user
    // sets `TOOLR_NO_PREFLIGHT_DEPS` to a non-empty, non-`0` value —
    // post-mortem interception still catches inline imports.
    let skip_preflight = std::env::var_os("TOOLR_NO_PREFLIGHT_DEPS")
        .is_some_and(|v| !v.is_empty() && v != "0");
    if !skip_preflight {
        if let Some(venv) = &venv_dir {
            if let Some(sp) = toolr_core::deps_check::site_packages_dir(venv) {
                if let Err(err) = toolr_core::deps_check::check_imports(&sp, &cmd.imports) {
                    eprintln!("toolr: {err}");
                    return Ok(ExitCode::from(78));
                }
            }
        }
    }

    let (mut child, stderr_capture) = spawn_runner_capturing_stderr(&python, tempfile.path())?;
    let status = wait_with_signals(&mut child)?;
    let stderr_bytes = stderr_capture.take();
    let stderr_str = String::from_utf8_lossy(&stderr_bytes);
    use std::io::Write;
    if !status.success() {
        if let Some(report) = toolr_core::deps_check::intercept_import_error(&stderr_str) {
            std::io::stderr().write_all(report.render().as_bytes())?;
        } else {
            std::io::stderr().write_all(&stderr_bytes)?;
        }
    } else {
        std::io::stderr().write_all(&stderr_bytes)?;
    }

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
        Some(("cache", cache_matches)) => crate::self_cache::dispatch(cache_matches),
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
    use toolr_core::dynamic::{compute_dynamic_hash, rebuild_dynamic_only};

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
    let root = toolr_core::discovery::discover_project_root(&cwd)?;
    let tools = root.join("tools");
    let manifest = toolr_core::parser::build_static_manifest(&tools)?;
    let path = tools.join(".toolr-manifest.json");
    toolr_core::manifest::write_manifest(&path, &manifest)?;
    println!(
        "toolr: wrote {} groups / {} commands to {}",
        manifest.groups.len(),
        manifest.commands.len(),
        path.display()
    );
    Ok(ExitCode::SUCCESS)
}

fn run_install_uv_now() -> anyhow::Result<std::process::ExitCode> {
    let consent = toolr_core::uv::install::ConsentMode {
        yes_flag: true,
        auto_install_env: true,
    };
    let uv = toolr_core::uv::ensure_uv(consent)?;
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

/// Read the root-level "Output Options" flags from the parsed matches
/// into an `OutputOptions`. Centralised so every command path produces
/// the same shape; missing flags fall back to `Default`.
fn output_options_from_matches(matches: &ArgMatches) -> OutputOptions {
    let mut opts = OutputOptions::default();
    if matches.get_flag("quiet") {
        opts.verbosity = "quiet".into();
        opts.log_level = "INFO".into();
    } else if matches.get_flag("debug") {
        opts.verbosity = "verbose".into();
        opts.log_level = "DEBUG".into();
    }
    // `--no-timestamps` wins over `--timestamps`; clap enforces the
    // mutex via `conflicts_with`, so at most one is set here.
    if matches.get_flag("timestamps") && !matches.get_flag("no-timestamps") {
        opts.timestamps = true;
    }
    if let Some(secs) = matches.get_one::<f64>("timeout-secs").copied() {
        opts.default_timeout_secs = Some(secs);
    }
    if let Some(secs) = matches
        .get_one::<f64>("no-output-timeout-secs")
        .copied()
    {
        opts.default_no_output_timeout_secs = Some(secs);
    }
    opts
}

#[allow(dead_code)]
pub(crate) fn report_uv_error(err: &toolr_core::uv::UvError) -> String {
    use toolr_core::uv::UvError;
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

#[cfg(test)]
mod tests {
    //! In-process unit tests for the pure helpers in this module.
    //!
    //! Integration tests under `crates/toolr/tests/` spawn the `toolr`
    //! binary via `assert_cmd`, so they exercise the dispatcher's
    //! observable behaviour but don't contribute to tarpaulin's
    //! line-coverage of this file (tarpaulin doesn't aggregate
    //! subprocess profraws by default). These unit tests run inside the
    //! test process so the coverage counter actually moves.
    use super::*;
    use toolr_core::uv::UvError;

    // ----------------------------------------------------------------
    // report_uv_error: one assertion per variant.
    // ----------------------------------------------------------------

    #[test]
    fn report_uv_error_renders_user_refused_install() {
        let s = report_uv_error(&UvError::UserRefusedInstall);
        assert!(s.contains("uv is required"));
        assert!(s.contains("TOOLR_AUTO_INSTALL_UV"));
    }

    #[test]
    fn report_uv_error_renders_version_too_old() {
        let s = report_uv_error(&UvError::VersionTooOld {
            found: (0, 1, 2),
            required: (3, 4, 5),
        });
        assert!(s.contains("0.1.2"), "actual: {s}");
        assert!(s.contains("3.4.5"), "actual: {s}");
        assert!(s.contains("Upgrade uv"), "actual: {s}");
    }

    #[test]
    fn report_uv_error_renders_other_variant() {
        // Any non-(UserRefused|VersionTooOld) variant falls into the
        // catch-all `format!("toolr: {other}")` arm.
        let s = report_uv_error(&UvError::NotAvailable);
        assert!(s.starts_with("toolr:"), "actual: {s}");
    }

    // ----------------------------------------------------------------
    // resolve_python_for_build: override / VIRTUAL_ENV / PATH fallback.
    // ----------------------------------------------------------------

    #[test]
    fn resolve_python_for_build_accepts_existing_override() {
        let tmp = tempfile::TempDir::new().unwrap();
        let p = tmp.path().join("python");
        std::fs::write(&p, "").unwrap();
        let resolved = resolve_python_for_build(Some(p.to_str().unwrap())).unwrap();
        assert_eq!(resolved, p);
    }

    #[test]
    fn resolve_python_for_build_rejects_nonexistent_override() {
        let err = resolve_python_for_build(Some("/definitely/not/a/python")).unwrap_err();
        assert!(err.to_string().contains("not a file"), "actual: {err}");
    }

    // Note: the VIRTUAL_ENV + PATH fallback branches mutate process-wide
    // env state, which races against any other test in the same binary
    // that also reads `$VIRTUAL_ENV`. We rely on the integration tests
    // in `cli_smoke.rs` (which spawn fresh subprocesses) to exercise
    // those branches. Coverage on lines 196-210 is therefore expected
    // to remain partial under tarpaulin's in-process metric.

    // ----------------------------------------------------------------
    // dirs_home: $HOME present / absent.
    // ----------------------------------------------------------------

    // Same env-mutation reasoning as above — `$HOME` is set on every
    // sane CI runner, so the happy path is implicitly covered by the
    // completion-install integration tests. We don't exercise the
    // "no HOME" bail branch here for the same reason.

    // ----------------------------------------------------------------
    // output_options_from_matches: every flag knob.
    // ----------------------------------------------------------------

    /// Build a one-group manifest so `cli::build_command` can produce a
    /// root command we can call `try_get_matches_from` on. The manifest
    /// content doesn't matter for output-option tests — only the root
    /// flags do.
    fn empty_manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: Vec::new(),
            commands: Vec::new(),
        }
    }

    fn parse(args: &[&str]) -> ArgMatches {
        let manifest = empty_manifest();
        let root = crate::cli::build_command(&manifest);
        let mut full: Vec<String> = vec!["toolr".into()];
        full.extend(args.iter().map(|s| (*s).to_string()));
        root.try_get_matches_from(full).unwrap()
    }

    #[test]
    fn output_options_defaults_when_no_flags_set() {
        let m = parse(&[]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.verbosity, OutputOptions::default().verbosity);
        assert_eq!(opts.log_level, OutputOptions::default().log_level);
        assert!(!opts.timestamps);
        assert!(opts.default_timeout_secs.is_none());
        assert!(opts.default_no_output_timeout_secs.is_none());
    }

    #[test]
    fn output_options_quiet_overrides_verbosity_and_log_level() {
        let m = parse(&["--quiet"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.verbosity, "quiet");
        assert_eq!(opts.log_level, "INFO");
    }

    #[test]
    fn output_options_debug_overrides_verbosity_and_log_level() {
        let m = parse(&["--debug"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.verbosity, "verbose");
        assert_eq!(opts.log_level, "DEBUG");
    }

    #[test]
    fn output_options_timestamps_flag_is_propagated() {
        let m = parse(&["--timestamps"]);
        let opts = output_options_from_matches(&m);
        assert!(opts.timestamps);
    }

    #[test]
    fn output_options_no_timestamps_wins_over_timestamps() {
        // The cli defines `--no-timestamps` as the override; setting both
        // (which clap rejects via `conflicts_with`) isn't reachable.
        // The code path explicitly checks `!no-timestamps`, so the
        // default-no-flag case + an explicit `--no-timestamps` both
        // result in `timestamps = false`.
        let m = parse(&["--no-timestamps"]);
        let opts = output_options_from_matches(&m);
        assert!(!opts.timestamps);
    }

    #[test]
    fn output_options_propagates_timeout_secs() {
        let m = parse(&["--timeout-secs", "12.5"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.default_timeout_secs, Some(12.5));
    }

    #[test]
    fn output_options_propagates_no_output_timeout_secs() {
        let m = parse(&["--no-output-timeout-secs", "3"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.default_no_output_timeout_secs, Some(3.0));
    }
}

#[cfg(test)]
mod path_lookup_tests {
    //! Unit tests for `find_command_for_path`, the pure helper that
    //! resolves a parsed subcommand path to its manifest entry. After
    //! Task 6 grafts children under dispatchers, a 3-segment path like
    //! `toolr jenkins job migrate` must still find `migrate` at
    //! `group == "jenkins"` (the dispatcher hop is invisible to the
    //! manifest). These tests pin the most-specific-first preference
    //! and the one-level fallback.
    use super::*;
    use toolr_core::manifest::{Command, Manifest, Origin};

    fn cmd(name: &str, group: &str) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{name}"),
            function: name.into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }
    }

    fn manifest_with(commands: Vec<Command>) -> Manifest {
        Manifest {
            schema_version: 1,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: vec![],
            commands,
        }
    }

    fn parts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_two_segment_path_in_group() {
        let m = manifest_with(vec![cmd("migrate", "jenkins")]);
        let c = find_command_for_path(&m, &parts(&["jenkins", "migrate"])).unwrap();
        assert_eq!(c.name, "migrate");
    }

    #[test]
    fn finds_three_segment_path_under_dispatcher() {
        // `migrate` lives at group=jenkins (the group), name=migrate;
        // the user typed `toolr jenkins job migrate` (3 segments).
        let m = manifest_with(vec![cmd("migrate", "jenkins")]);
        let c = find_command_for_path(&m, &parts(&["jenkins", "job", "migrate"])).unwrap();
        assert_eq!(c.name, "migrate");
    }

    #[test]
    fn prefers_more_specific_group_when_both_exist() {
        // If both `docker.image.build` and `docker.build` exist, the
        // 3-segment path `docker image build` must resolve to the
        // nested one, not the top-level one.
        let m = manifest_with(vec![
            cmd("build", "docker"),
            cmd("build", "docker.image"),
        ]);
        let c = find_command_for_path(&m, &parts(&["docker", "image", "build"])).unwrap();
        assert_eq!(c.group, "docker.image");
    }

    #[test]
    fn returns_none_when_no_command_matches() {
        let m = manifest_with(vec![cmd("migrate", "jenkins")]);
        assert!(find_command_for_path(&m, &parts(&["unknown"])).is_none());
    }
}
