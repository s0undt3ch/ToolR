mod bootstrap;
mod build_manifest_resolve;
mod builtin_completions;
mod cli;
mod dispatch;
mod execute_build;
mod help;
mod init_scaffold;
mod init_templates;
mod invocation;
mod project;
mod self_cache;
mod self_cache_prune;
mod value_parsers;

use std::process::ExitCode;

use toolr_core::discovery::DiscoveryError;

use crate::invocation::{Invocation, RootArgv};
use toolr_core::manifest::{Manifest, ManifestError, load_manifest};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        // A clap error raised after parsing prints the way clap prints it.
        Err(e) => match e.downcast_ref::<clap::Error>() {
            Some(clap_err) => {
                let _ = clap_err.print();
                ExitCode::from(clap_err.exit_code() as u8)
            }
            None => {
                eprintln!("toolr: {e:#}");
                ExitCode::from(2)
            }
        },
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let run = Invocation::new(RootArgv::new(std::env::args().collect()), cwd);
    let mut built = match bootstrap::ensure_manifest(&run)? {
        Some(built) => built,
        // `__complete` resolves the manifest itself; loading it here too
        // would parse it twice per TAB and warn about a corrupt one.
        // `--version` is answered by clap before the manifest is read, and neither should walk
        // the tree.
        None if run.argv.is_tab_completion() || run.argv.wants_version() => {
            Manifest::empty().into()
        }
        None => load_or_empty(&run)?.into(),
    };
    // Covers fragments and a hand-edited committed manifest alike. Tab
    // completion drops the same entries in `resolve_manifest_at_tab`.
    built.manifest.drop_unsafe_names();
    if !run.argv.skips_manifest_build() {
        // Printed under `-q` too (the rule is on the `-q` arg in `cli::root_flag_args`).
        // Build warnings come only from a manifest built on this run.
        for message in built.all_warnings() {
            eprintln!("toolr: warning: {message}");
        }
    }
    let manifest = built.manifest;
    let mut command = cli::build_command(&manifest);
    // try_get_matches_from, so a `subcommand_required` error on a help request
    // (`toolr self --help`) still renders that command's help instead of the error.
    let matches = match command.clone().try_get_matches_from(run.argv.words()) {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind;
            if let Some(mode) = run.argv.help_mode()
                && matches!(
                    e.kind(),
                    ErrorKind::MissingSubcommand | ErrorKind::MissingRequiredArgument
                )
            {
                dispatch::print_help_from_argv(&run.argv, &command, mode);
                return Ok(ExitCode::SUCCESS);
            }
            if let Some(hint) = misplaced_root_flag_hint(&e, &command, &run.argv) {
                let _ = e.print();
                eprintln!("{hint}");
                return Ok(ExitCode::from(e.exit_code() as u8));
            }
            // Genuine error: print it the way clap does; outside a project an
            // unknown command is most likely a wrong directory, so say so.
            if e.kind() == ErrorKind::InvalidSubcommand
                && run.argv.names_user_command()
                && let Some(hint) = no_project_hint(&run)
            {
                let _ = e.print();
                eprintln!("{hint}");
                return Ok(ExitCode::from(e.exit_code() as u8));
            }
            e.exit()
        }
    };
    maybe_emit_cache_hint(&matches);
    dispatch::dispatch(&matches, &manifest, &mut command, run)
}

/// The line to follow clap's "unexpected argument" when the argument is one of toolr's root
/// flags given after a subcommand: they are accepted only before the first one.
fn misplaced_root_flag_hint(
    err: &clap::Error,
    root: &clap::Command,
    argv: &RootArgv,
) -> Option<String> {
    use clap::error::{ContextKind, ContextValue, ErrorKind};
    if err.kind() != ErrorKind::UnknownArgument {
        return None;
    }
    let Some(ContextValue::String(bad)) = err.get(ContextKind::InvalidArg) else {
        return None;
    };
    let spelling = bad.split('=').next()?;
    let words: Vec<&str> = argv.words().iter().skip(1).map(String::as_str).collect();
    let at = words
        .iter()
        .position(|w| *w == spelling || w.starts_with(&format!("{spelling}=")))
        .unwrap_or(words.len());
    let path = argv.command_path(root).1.join(" ");
    let found = words.get(at..).unwrap_or_default();
    let found: &[&str] = if found.is_empty() { &[spelling] } else { found };
    cli::misplaced_flag_error(found, &path, argv.typed_before()).map(|m| m.hint)
}

/// The line to follow clap's "unrecognized subcommand" when `cwd` is not in a
/// project at all. A bare `tools/` already gets its own warning.
fn no_project_hint(run: &Invocation) -> Option<String> {
    match run.root() {
        Err(e @ DiscoveryError::NotFound(_)) => Some(format!("toolr: {e}")),
        _ => None,
    }
}

/// The passive cache hint is advice, so `-q` drops it; help and version output stay clean
/// (they never reach this point: clap prints `-V` itself, and a `--help` that clap parsed
/// is `help`). The `--` separator is clap's to find, so a wrapped command's own `--quiet`
/// (`run -- pytest --quiet`) is not toolr's.
fn maybe_emit_cache_hint(matches: &clap::ArgMatches) {
    if toolr_core::env::env_truthy("TOOLR_NO_CACHE_HINT")
        || matches.get_flag("quiet")
        || matches.get_flag("help")
        // Tab completion and `self cache …` get no hint.
        || matches.subcommand_name() == Some("__complete")
        || matches
            .subcommand_matches("self")
            .is_some_and(|m| m.subcommand_name() == Some("cache"))
    {
        return;
    }
    let Ok(cache_root) = self_cache::resolve_cache_root() else {
        return;
    };
    if let Ok(Some(msg)) = toolr_core::cache::compute_hint(
        &cache_root,
        toolr_core::cache::HINT_ORPHAN_THRESHOLD,
        chrono::Utc::now(),
    ) {
        eprintln!("{msg}");
    }
}

/// Read the on-disk manifest; "no project" and "no manifest file" mean an
/// empty command set.
///
/// Any other load failure is an error for user commands: an empty manifest
/// there turns a corrupt file into clap's misleading `unrecognized
/// subcommand` (#542). Built-ins don't need the user's commands, and
/// `project manifest rebuild` is the repair path, so they only warn.
fn load_or_empty(run: &Invocation) -> anyhow::Result<Manifest> {
    let root = match run.root() {
        Ok(root) => root,
        Err(e) => {
            // A bare `tools/` explains why a user command is missing; plain
            // `--help` stays quiet, since an unrelated `tools/` (such as
            // `C:\tools`) may sit above any directory.
            if matches!(e, DiscoveryError::MissingPyproject { .. }) && run.argv.names_user_command()
            {
                eprintln!("toolr: warning: {e}");
            }
            return Ok(Manifest::empty());
        }
    };
    let manifest_path = toolr_core::manifest::manifest_path(root);
    match load_manifest(&manifest_path) {
        Ok(m) => Ok(m),
        Err(ManifestError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(Manifest::empty())
        }
        Err(e) if run.argv.skips_manifest_build() => {
            eprintln!(
                "toolr: warning: ignoring unreadable {}: {e}",
                manifest_path.display()
            );
            Ok(Manifest::empty())
        }
        Err(e) => Err(anyhow::anyhow!(
            "cannot read {}: {e}; run `toolr project manifest rebuild` to regenerate it",
            manifest_path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::misplaced_root_flag_hint;
    use crate::cli;
    use toolr_core::manifest::Manifest;

    fn hint(args: &[&str]) -> Option<String> {
        let argv: Vec<String> = std::iter::once("toolr")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        let root = cli::build_command(&Manifest::empty());
        let err = root.clone().try_get_matches_from(&argv).err()?;
        misplaced_root_flag_hint(&err, &root, &crate::invocation::RootArgv::new(argv))
    }

    #[test]
    fn a_misplaced_root_flag_gets_an_example_built_from_the_matched_command() {
        let ends = |args: &[&str], tail: &str| {
            let h = hint(args).unwrap_or_default();
            assert!(h.ends_with(tail), "{args:?}: {h}");
        };
        ends(
            &["project", "venv", "sync", "-q"],
            "`toolr -q project venv sync ...`",
        );
        ends(
            &["project", "venv", "sync", "--force", "--timeout-secs", "5"],
            "`toolr --timeout-secs 5 project venv sync ...`",
        );
        ends(
            &["project", "venv", "sync", "--timeout=5"],
            "`toolr --timeout=5 project venv sync ...`",
        );
        // A missing or unsafe value gets the placeholder.
        ends(
            &["project", "venv", "sync", "--timeout-secs"],
            "`toolr --timeout-secs <SECONDS> project venv sync ...`",
        );
        ends(
            &["project", "venv", "sync", "--timeout-secs", "a b"],
            "`toolr --timeout-secs <SECONDS> project venv sync ...`",
        );
        // A spaced argument elsewhere never reaches the example.
        ends(
            &["project", "venv", "add", "a b", "-q"],
            "`toolr -q project venv add ...`",
        );
        // A cluster names only its root letter.
        ends(
            &["project", "venv", "sync", "-qx"],
            "`toolr -q project venv sync ...`",
        );
        ends(
            &["project", "venv", "sync", "-dev"],
            "`toolr -d project venv sync ...`",
        );
        ends(
            &["project", "venv", "sync", "-q", "-d"],
            "`toolr -q project venv sync ...`",
        );
        // Root flags typed before the command stand as a literal ellipsis.
        ends(
            &["--timeout-secs", "5", "project", "venv", "sync", "-q"],
            "`toolr \u{2026} -q project venv sync ...`",
        );
        ends(
            &["-q", "project", "venv", "sync", "-d"],
            "`toolr \u{2026} -d project venv sync ...`",
        );
    }

    #[test]
    fn other_errors_get_no_hint() {
        assert_eq!(hint(&["project", "venv", "sync", "--nope"]), None);
        assert_eq!(hint(&["project", "venv", "sync", "-x"]), None);
        assert_eq!(hint(&["project", "venv", "sync", "-h"]), None);
        assert_eq!(hint(&["project", "venv", "nope"]), None);
        assert_eq!(hint(&["-q", "project", "venv", "sync"]), None);
    }
}
