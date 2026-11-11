//! Running a user command: the runner spec, trust, the venv and the Python runner.

use std::process::ExitCode;

use anyhow::Context as _;
use clap::ArgMatches;
use toolr_core::execute::{ExecutionSpec, spawn_runner, wait_with_signals, write_spec_to_tempfile};
use toolr_core::exit_status::process_exit_code;
use toolr_core::manifest::Manifest;
use toolr_core::trust::shown;
use toolr_core::venv::resolve_venv_path;

use crate::execute_build::{
    OutputOptions, PathBases, PathValueError, Verbosity, build_dispatch_spec, build_spec,
    output_options_from_matches, pack_child_args,
};
use crate::invocation::Invocation;

/// Turn a [`PathValueError`] into the one-line clap error clap itself
/// prints for a bad value, spelling the arg as the command on `path` that
/// owns it does (`--cfg <cfg>`). Other errors pass through.
fn render_path_value_error(
    err: anyhow::Error,
    root: &mut clap::Command,
    path: &[String],
) -> anyhow::Error {
    let Some(bad) = err.downcast_ref::<PathValueError>() else {
        return err;
    };
    // An arg only renders as `--cfg <cfg>` once its command is built.
    root.build();
    let root = &*root;
    let shown = path
        .iter()
        .scan(root, |cmd, name| {
            *cmd = cmd.find_subcommand(name)?;
            Some(*cmd)
        })
        .filter_map(|cmd| cmd.get_arguments().find(|a| a.get_id() == bad.arg.as_str()))
        .last()
        .expect("the failing arg belongs to a command on the parsed path")
        .to_string();
    clap::Error::raw(clap::error::ErrorKind::ValueValidation, bad.message(&shown))
        .with_cmd(root)
        .into()
}

/// The spec for the matched `cmd`. Dispatched leaves take a separate
/// spec-shape: the runner sees `dispatch: Some(...)` and routes to
/// `invoke_dispatcher` instead of calling `function` as a regular command.
/// The child is packed first, then a parent-shaped spec is built around the
/// dispatcher entry, so `build_dispatch_spec` iterates the dispatcher's OWN
/// arguments (the --cpu/--ram-style outer flags) from `parent_matches`, not
/// the leaf's (which are packed in `packed`).
fn build_execution_spec(
    manifest: &Manifest,
    cmd: &toolr_core::manifest::Command,
    cmd_matches: &ArgMatches,
    parent_matches: &ArgMatches,
    bases: &PathBases,
    output_opts: &OutputOptions,
) -> anyhow::Result<ExecutionSpec> {
    if cmd.dispatched_from.is_some() {
        let packed = pack_child_args(cmd, cmd_matches, bases)?;
        // The dispatcher's manifest entry shares `(module, function)`
        // with each of its grafted children (the argparse pipeline
        // copies those fields onto the children at graft time). Find
        // the dispatcher's own entry by matching that pair and
        // requiring `is_dispatcher = true` so we don't accidentally
        // grab a sibling child.
        let dispatcher = manifest
            .commands
            .iter()
            .find(|p| p.is_dispatcher && p.module == cmd.module && p.function == cmd.function)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "dispatcher manifest entry for `{}` (module `{}`, function `{}`) not found",
                    cmd.name,
                    cmd.module,
                    cmd.function,
                )
            })?;
        build_dispatch_spec(dispatcher, parent_matches, packed, bases, output_opts)
    } else {
        build_spec(cmd, cmd_matches, bases, output_opts)
    }
}

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

/// Run the user command below the root `matches`; `first` is its first subcommand.
pub(super) fn run_command(
    matches: &ArgMatches,
    (first_name, first_matches): (&str, &ArgMatches),
    manifest: &Manifest,
    root: &mut clap::Command,
    run: Invocation,
    grant: Option<toolr_core::trust::Trusted>,
    rebuilt: Option<toolr_core::venv::ResolvedVenv>,
) -> anyhow::Result<ExitCode> {
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
    let cmd = find_command_for_path(manifest, &path)
        .ok_or_else(|| anyhow::anyhow!("unknown command: {}", path.join(" ")))?;

    let (cwd, repo_root) = run.into_cwd_and_root();
    let repo_root = repo_root?;
    let output_opts = output_options_from_matches(matches);
    let bases = PathBases {
        cwd: &cwd,
        repo_root: &repo_root,
    };
    let spec = build_execution_spec(
        manifest,
        cmd,
        cmd_matches,
        parent_matches,
        &bases,
        &output_opts,
    )
    .map_err(|e| render_path_value_error(e, root, &path))?;
    // Before anything of the repo's runs: the venv path may `uv sync`, and the
    // host-Python path imports `tools/*.py` directly. `main` proved trust already, unless a
    // `-h` among the command's trailing values read as help there; clap only matched a user
    // command then because the run was trusted or under `TOOLR_TRUST_ALL`, so this re-check
    // is a fallback.
    let trusted = match grant {
        Some(trusted) => trusted,
        None => {
            let never_prompt = output_opts.verbosity == Verbosity::Quiet;
            let mode = toolr_core::trust::TrustMode { never_prompt };
            toolr_core::trust::require_trust(&repo_root, mode)?
        }
    };

    let tempfile = write_spec_to_tempfile(&spec)?;
    let quiet = output_opts.verbosity == Verbosity::Quiet;
    let verbose = output_opts.verbosity == Verbosity::Verbose;
    // `main` already rebuilt a foreign venv; syncing again could rebuild it twice.
    let resolved = match rebuilt {
        Some(resolved) => resolved,
        None => ready_for_dispatch(&repo_root, &trusted, resolve_venv_path(&repo_root)?, quiet)?,
    };
    let python = resolved.python.clone();

    // Touch last_used_at, backfilling `meta.json` for entries that predate it,
    // in the slot sync writes so `toolr self cache list` sees this repo.
    let touched = toolr_core::project::meta_cache_dir(&repo_root, &resolved).and_then(|dir| {
        toolr_core::cache::touch_or_backfill(
            &dir,
            &repo_root,
            env!("CARGO_PKG_VERSION"),
            &resolved.python_version,
        )
        .map_err(anyhow::Error::from)
    });
    if let Err(e) = touched {
        // `finalize_sync` fails on this write; dispatch only warns, as the command runs without it.
        // In-tree venvs, often under a read-only cache home (CI), warn only with --debug.
        if !resolved.is_in_tree() || verbose {
            eprintln!("toolr: warning: failed to touch cache meta.json: {e}");
        }
    }

    // A missing dependency surfaces as the Python runner's `ImportError`
    // hint (which points the user at `toolr project venv sync`), so there
    // is no Rust-side pre-flight import check here.

    // Pre-check that the resolved Python interpreter actually exists.
    // Without this, a missing tools venv surfaces as a bare
    // `io::Error::NotFound` from `Command::spawn`, which `main` prints
    // as `toolr: No such file or directory (os error 2)` — uninformative
    // and gives no recovery hint.
    if !python.is_file() {
        anyhow::bail!(
            "Python interpreter not found at {}.\nRun `toolr project venv sync` to materialise the tools venv.",
            shown(&python)
        );
    }

    warn_on_toolr_py_mismatch(&resolved.venv_dir);

    let mut child = spawn_runner(&python, tempfile.path(), &repo_root)
        .with_context(|| format!("spawning Python runner at {}", shown(&python)))?;
    let status = wait_with_signals(&mut child)?;

    Ok(ExitCode::from(process_exit_code(status)))
}

/// Sync the venv as dispatch needs it: after a trust prompt the user is asked first.
fn ready_for_dispatch(
    repo_root: &std::path::Path,
    trusted: &toolr_core::trust::Trusted,
    resolved: toolr_core::venv::ResolvedVenv,
    quiet: bool,
) -> anyhow::Result<toolr_core::venv::ResolvedVenv> {
    if trusted.granted_at_prompt() {
        sync_after_trust_prompt(trusted, resolved)
    } else {
        resync_if_needed(repo_root, resolved, quiet)
    }
}

/// Rebuild a foreign in-tree venv before the manifest is built: the manifest leaves out its
/// plugin commands, so clap would reject one before dispatch could rebuild it. Returns the
/// rebuilt venv for dispatch, or `None` when there was nothing foreign to rebuild.
pub(crate) fn rebuild_foreign_venv(
    repo_root: &std::path::Path,
    trusted: &toolr_core::trust::Trusted,
    quiet: bool,
) -> anyhow::Result<Option<toolr_core::venv::ResolvedVenv>> {
    let Ok(resolved) = resolve_venv_path(repo_root) else {
        return Ok(None);
    };
    if !toolr_core::venv::is_foreign(&resolved, &repo_root.join("tools")) {
        return Ok(None);
    }
    ready_for_dispatch(repo_root, trusted, resolved, quiet).map(Some)
}

/// Re-sync a venv that a `tools/` file uv reads (see `check_freshness`)
/// has moved past, so a lock bump never runs old code (#527), whose
/// interpreter was uninstalled under it (#609), or that toolr didn't build.
/// A never-synced venv is left alone, so a fresh clone installs nothing before it.
fn resync_if_needed(
    repo_root: &std::path::Path,
    resolved: toolr_core::venv::ResolvedVenv,
    quiet: bool,
) -> anyhow::Result<toolr_core::venv::ResolvedVenv> {
    use toolr_core::venv::{Freshness, SYNC_INPUTS_DESCRIPTION, check_freshness};
    let freshness = check_freshness(&resolved, &repo_root.join("tools"));
    let problem = match freshness {
        Freshness::Stale => {
            format!("the tools venv is out of date with {SYNC_INPUTS_DESCRIPTION}")
        }
        Freshness::Broken => "the tools venv's Python interpreter no longer exists".to_owned(),
        Freshness::Foreign => "toolr didn't build the in-tree tools venv".to_owned(),
        Freshness::Missing | Freshness::Fresh => return Ok(resolved),
    };
    // The sync names a foreign venv itself as it removes it.
    if !quiet && freshness != Freshness::Foreign {
        eprintln!("toolr: {problem}; syncing it first");
    }
    let (resolved, _uv) = crate::project::ready_venv(repo_root, quiet, |o| o).map_err(|e| {
        if is_untrusted_venv(&e) {
            // Already says why and how to approve.
            return e;
        }
        e.context(format!(
            "{problem} and could not be synced; run `toolr project venv sync`"
        ))
    })?;
    Ok(resolved)
}

fn is_untrusted_venv(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<toolr_core::uv::UvError>(),
        Some(toolr_core::uv::UvError::UntrustedVenv(_))
    )
}

/// The user has just trusted the repository at the prompt: a venv that needs
/// a sync gets the sync prompt, not the uv install prompt, so the command can
/// run after at most two questions. See
/// `specs/archive/2026/2026-10-31-trust-then-sync-design.md`.
fn sync_after_trust_prompt(
    trusted: &toolr_core::trust::Trusted,
    resolved: toolr_core::venv::ResolvedVenv,
) -> anyhow::Result<toolr_core::venv::ResolvedVenv> {
    use toolr_core::project::{SYNC_LATER_HINT, SyncAfterTrust, sync_after_trust_on_tty};
    match sync_after_trust_on_tty(trusted, false)? {
        SyncAfterTrust::Declined => anyhow::bail!(SYNC_LATER_HINT),
        SyncAfterTrust::Synced(synced) => Ok(synced),
        SyncAfterTrust::NotNeeded => Ok(resolved),
    }
}

/// A runner from another minor release can pass the schema check yet
/// still carry long-fixed bugs (#529), so name the skew instead of hiding it.
/// The venv already matches the lock here, so a plain sync can't be the fix.
/// Keep the remedies in step with the schema-mismatch error in `toolr/_runner.py::load_spec`.
fn warn_on_toolr_py_mismatch(venv_dir: &std::path::Path) {
    use std::cmp::Ordering;
    let binary = env!("CARGO_PKG_VERSION");
    let installed = match toolr_core::venv::installed_toolr_py_version(venv_dir) {
        Ok(Some(installed)) => installed,
        Ok(None) => return,
        Err(e) => {
            return eprintln!("toolr: warning: can't check the tools venv's toolr-py version: {e}");
        }
    };
    let remedy = match toolr_core::venv::compare_minor_versions(&installed, binary) {
        Some(Ordering::Less) => {
            "upgrade it with `toolr project venv sync -P toolr-py`, loosening the toolr-py \
             pin in tools/pyproject.toml first if it caps the version"
        }
        Some(Ordering::Greater) => "upgrade the toolr binary to match",
        Some(Ordering::Equal) | None => return,
    };
    eprintln!(
        "toolr: warning: the tools venv has toolr-py {installed} but this toolr binary \
         is {binary}; {remedy}"
    );
}

#[cfg(test)]
mod tests {
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
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }
    }

    fn manifest_with(commands: Vec<Command>) -> Manifest {
        Manifest {
            commands,
            ..Manifest::empty()
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
        let m = manifest_with(vec![cmd("build", "docker"), cmd("build", "docker.image")]);
        let c = find_command_for_path(&m, &parts(&["docker", "image", "build"])).unwrap();
        assert_eq!(c.group, "docker.image");
    }

    #[test]
    fn returns_none_when_no_command_matches() {
        let m = manifest_with(vec![cmd("migrate", "jenkins")]);
        assert!(find_command_for_path(&m, &parts(&["unknown"])).is_none());
    }
}
