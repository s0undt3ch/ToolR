use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::ArgMatches;

use toolr_core::complete::{
    InstallOptions, InstallOutcome, PriorState, Shell as CompletionShell, completion_script,
    install_script,
    resolve_manifest_at_tab, serve_completions,
};
use toolr_core::discovery::discover_project_root;
use toolr_core::execute::{
    resolve_python, spawn_runner, wait_with_signals, write_spec_to_tempfile,
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
        // The dispatcher's manifest entry shares `(module, function)`
        // with each of its grafted children (the argparse pipeline
        // copies those fields onto the children at graft time). Find
        // the dispatcher's own entry by matching that pair and
        // requiring `is_dispatcher = true` so we don't accidentally
        // grab a sibling child.
        let dispatcher = manifest
            .commands
            .iter()
            .find(|p| {
                p.is_dispatcher
                    && p.module == cmd.module
                    && p.function == cmd.function
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "dispatcher manifest entry for `{}` (module `{}`, function `{}`) not found",
                    cmd.name,
                    cmd.module,
                    cmd.function,
                )
            })?;
        build_dispatch_spec(dispatcher, parent_matches, packed, &repo_root, &output_opts)
    } else {
        build_spec(cmd, cmd_matches, &repo_root, &output_opts)
    };

    let tempfile = write_spec_to_tempfile(&spec)?;
    // Prefer the resolved tools-venv python. Fall back to the
    // PATH/TOOLR_PYTHON lookup only when there is no
    // `tools/pyproject.toml` — i.e. projects that never opted into
    // the per-repo venv layer.
    let (python, venv_dir, python_version) =
        if repo_root.join("tools").join("pyproject.toml").is_file() {
            let resolved = resolve_venv_path(&repo_root)?;
            (
                resolved.python,
                Some(resolved.venv_dir),
                Some(resolved.python_version),
            )
        } else {
            (resolve_python()?, None, None)
        };

    // Touch last_used_at on every invocation against a cached venv.
    // Backfill a fresh `meta.json` for cache entries that predate the
    // sidecar — without this, `toolr self cache list` would silently
    // hide venvs created by older binaries.
    if let Some(venv) = &venv_dir {
        if let Some(cache_dir) = venv.parent() {
            let py_ver = python_version.as_deref().unwrap_or("");
            if let Err(e) = toolr_core::cache::touch_or_backfill(
                cache_dir,
                &repo_root,
                env!("CARGO_PKG_VERSION"),
                py_ver,
            ) {
                eprintln!("toolr: warning: failed to touch cache meta.json: {e}");
            }
        }
    }

    // Pre-flight missing-dependency check against the command's
    // declared `imports` list. Skip when the user sets
    // `TOOLR_NO_PREFLIGHT_DEPS` to a non-empty, non-`0` value — at that
    // point a missing dep surfaces as a raw Python traceback from the
    // child, the same way it would when running python directly.
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

    // Pre-check that the resolved Python interpreter actually exists.
    // Without this, a missing tools venv surfaces as a bare
    // `io::Error::NotFound` from `Command::spawn`, which `main` prints
    // as `toolr: No such file or directory (os error 2)` — uninformative
    // and gives no recovery hint. Mirror the same check `run_introspect`
    // performs (`crates/toolr-core/src/dynamic/runner.rs`).
    if !python.is_file() {
        anyhow::bail!(
            "Python interpreter not found at {}.\n\
             Run `toolr project deps sync` to materialise the tools venv.",
            python.display()
        );
    }
    let mut child = spawn_runner(&python, tempfile.path())
        .with_context(|| format!("spawning Python runner at {}", python.display()))?;
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
    let resolved = crate::build_manifest_resolve::resolve_source_and_package(matches)?;

    let schema_version: u32 = matches
        .get_one::<u32>("schema-version")
        .copied()
        .unwrap_or(toolr_core::third_party::FRAGMENT_SCHEMA_VERSION);

    let output_path = resolve_output_path(matches, &resolved.source_dir);

    let fragment = toolr_core::build_fragment::build_third_party_fragment(
        &resolved.source_dir,
        &resolved.package_name,
        schema_version,
    )?;
    let serialised = toolr_core::build_fragment::serialise_fragment(&fragment)?;

    if matches.get_flag("check") {
        return check_against_disk(&output_path, &serialised);
    }

    write_atomically(&output_path, &serialised)?;
    eprintln!(
        "toolr build-manifest: wrote {} group(s) / {} command(s) to {}",
        fragment.groups.len(),
        fragment.commands.len(),
        output_path.display(),
    );
    Ok(ExitCode::SUCCESS)
}

fn resolve_output_path(matches: &clap::ArgMatches, source_dir: &std::path::Path) -> PathBuf {
    matches
        .get_one::<String>("output")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_dir.join("toolr-manifest.json"))
}

fn write_atomically(path: &std::path::Path, contents: &str) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(
        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
    )?;
    tmp.write_all(contents.as_bytes())?;
    tmp.persist(path).map_err(|e| anyhow::anyhow!("persist: {e}"))?;
    Ok(())
}

fn check_against_disk(path: &std::path::Path, serialised: &str) -> anyhow::Result<ExitCode> {
    let existing = if path.is_file() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };
    if existing == serialised {
        Ok(ExitCode::SUCCESS)
    } else {
        let diff = similar::TextDiff::from_lines(existing.as_str(), serialised);
        eprintln!(
            "toolr build-manifest: {} is out of date - regenerate with `toolr self build-manifest <pkg>`",
            path.display(),
        );
        eprintln!("{}", diff.unified_diff().header("on-disk", "regenerated"));
        Ok(ExitCode::from(2))
    }
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
        InstallOutcome::Wrote { path, prior } => {
            let msg = match prior {
                PriorState::None => format!(
                    "toolr: wrote {} completion script to {}",
                    shell,
                    path.display()
                ),
                PriorState::Identical => format!(
                    "toolr: rewrote {} completion script at {} (--force; content unchanged)",
                    shell,
                    path.display()
                ),
                PriorState::Differed => format!(
                    "toolr: replaced existing {} completion script at {}",
                    shell,
                    path.display()
                ),
            };
            println!("{msg}");
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
                "toolr: {} completion already up to date at {}",
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
    // The engine is purely manifest-driven, but the binary owns its own
    // built-in `self` / `project` subtree. Inject synthetic entries that
    // mirror `cli::build_command` so those subcommands also complete.
    let mut manifest = resolved.manifest;
    let (extra_groups, extra_commands) =
        crate::builtin_completions::built_in_completion_entries();
    manifest.groups.extend(extra_groups);
    manifest.commands.extend(extra_commands);
    for candidate in serve_completions(&manifest, &tokens) {
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
        silent_refuse: false,
    };
    let uv = toolr_core::uv::ensure_uv(consent)
        .map_err(toolr_core::uv::UvError::into_anyhow)?;
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
            third_party_hash: String::new(),
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
    //! resolves a parsed subcommand path to its manifest entry.
    //! Grafted children of a dispatcher live under a 3-segment path
    //! like `toolr jenkins job migrate`, but `migrate` is stored at
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
            third_party_hash: String::new(),
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
