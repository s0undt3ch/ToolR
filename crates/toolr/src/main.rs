mod bootstrap;
mod build_manifest_resolve;
mod builtin_completions;
mod cli;
mod dispatch;
mod execute_build;
mod help;
mod init_scaffold;
mod init_templates;
mod project;
mod self_cache;
mod self_cache_prune;
mod value_parsers;

use std::process::ExitCode;

use toolr_core::discovery::{DiscoveryError, discover_project_root};
use toolr_core::manifest::{Manifest, ManifestError, SCHEMA_VERSION, load_manifest};

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
    let argv: Vec<String> = std::env::args().collect();
    let mut built = match bootstrap::ensure_manifest(&cwd, &argv)? {
        Some(built) => built,
        // `__complete` resolves the manifest itself; loading it here too
        // would parse it twice per TAB and warn about a corrupt one.
        None if bootstrap::is_tab_completion(&argv) => empty_manifest().into(),
        None => load_or_empty(&cwd, &argv)?.into(),
    };
    // Covers fragments and a hand-edited committed manifest alike. Tab
    // completion drops the same entries in `resolve_manifest_at_tab`.
    built.manifest.drop_unsafe_names();
    if !bootstrap::should_skip_auto_rebuild(&argv) {
        // Printed under `-q` too (the rule is on the `-q` arg in `cli::root_flag_args`).
        // Build warnings come only from a manifest built on this run.
        for message in built.all_warnings() {
            eprintln!("toolr: warning: {message}");
        }
    }
    let manifest = built.manifest;
    let mut command = cli::build_command(&manifest);
    // Use try_get_matches_from so that `subcommand_required` validation errors
    // (e.g. `toolr self --help`) don't exit before dispatch can intercept
    // help flags. When clap reports MissingRequiredArgument / NoSubcommand AND
    // --help or -h is present in argv, we fall through to dispatch which
    // renders help. For genuine errors (bad flags, unknown subcommands) we let
    // clap print its error and exit.
    let matches = match command.clone().try_get_matches_from(std::env::args_os()) {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind;
            let want_help = argv.iter().any(|a| a == "--help");
            let want_short = !want_help && argv.iter().any(|a| a == "-h");
            if (want_help || want_short)
                && matches!(
                    e.kind(),
                    ErrorKind::MissingSubcommand | ErrorKind::MissingRequiredArgument
                )
            {
                // Help was requested but clap failed validation first.
                // Delegate to dispatch with a synthesised "root" match by
                // passing an empty argv so clap can build a valid root-level
                // ArgMatches. Dispatch's resolve_help_target will then walk
                // the argv to find the right level.
                dispatch::dispatch_help_from_argv(&argv, &mut command)?;
                return Ok(ExitCode::SUCCESS);
            }
            if let Some(hint) = misplaced_root_flag_hint(&e, &command, &argv) {
                let _ = e.print();
                eprintln!("{hint}");
                return Ok(ExitCode::from(e.exit_code() as u8));
            }
            // Genuine error: print it the way clap does; outside a project an
            // unknown command is most likely a wrong directory, so say so.
            if e.kind() == ErrorKind::InvalidSubcommand
                && bootstrap::names_user_command(&argv)
                && let Some(hint) = no_project_hint(&cwd)
            {
                let _ = e.print();
                eprintln!("{hint}");
                return Ok(ExitCode::from(e.exit_code() as u8));
            }
            e.exit()
        }
    };
    maybe_emit_cache_hint(&matches, &argv);
    dispatch::dispatch(&matches, &manifest, &mut command)
}

/// The line to follow clap's "unexpected argument" when the argument is one of toolr's root
/// flags given after a subcommand: they are accepted only before the first one.
fn misplaced_root_flag_hint(
    err: &clap::Error,
    root: &clap::Command,
    argv: &[String],
) -> Option<String> {
    use clap::error::{ContextKind, ContextValue, ErrorKind};
    if err.kind() != ErrorKind::UnknownArgument {
        return None;
    }
    let Some(ContextValue::String(bad)) = err.get(ContextKind::InvalidArg) else {
        return None;
    };
    let spelling = bad.split('=').next()?;
    let words: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    let at = words
        .iter()
        .position(|w| *w == spelling || w.starts_with(&format!("{spelling}=")))
        .unwrap_or(words.len());
    let (path, typed_before) = cli::command_path(root, argv);
    let found = words.get(at..).unwrap_or_default();
    let found: &[&str] = if found.is_empty() { &[spelling] } else { found };
    cli::misplaced_flag_error(found, &path, typed_before).map(|m| m.hint)
}

/// The line to follow clap's "unrecognized subcommand" when `cwd` is not in a
/// project at all. A bare `tools/` already gets its own warning.
fn no_project_hint(cwd: &std::path::Path) -> Option<String> {
    match discover_project_root(cwd) {
        Err(e @ DiscoveryError::NotFound(_)) => Some(format!("toolr: {e}")),
        _ => None,
    }
}

/// The passive cache hint is advice, so `-q` drops it; help and version output stay clean
/// (they never reach this point: clap prints `-V` itself, and a `--help` that clap parsed
/// is `help`). The `--` separator is clap's to find, so a wrapped command's own `--quiet`
/// (`run -- pytest --quiet`) is not toolr's.
fn maybe_emit_cache_hint(matches: &clap::ArgMatches, argv: &[String]) {
    if std::env::var_os("TOOLR_NO_CACHE_HINT").is_some()
        || matches.get_flag("quiet")
        || matches.get_flag("help")
    {
        return;
    }
    // Suppress for tab-completion and `self cache ...` invocations.
    if bootstrap::is_tab_completion(argv) {
        return;
    }
    if matches
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
fn load_or_empty(cwd: &std::path::Path, argv: &[String]) -> anyhow::Result<Manifest> {
    let root = match discover_project_root(cwd) {
        Ok(root) => root,
        Err(e) => {
            // A bare `tools/` explains why a user command is missing; plain
            // `--help` stays quiet, since an unrelated `tools/` (such as
            // `C:\tools`) may sit above any directory.
            if matches!(e, DiscoveryError::MissingPyproject { .. })
                && bootstrap::names_user_command(argv)
            {
                eprintln!("toolr: warning: {e}");
            }
            return Ok(empty_manifest());
        }
    };
    let manifest_path = toolr_core::manifest::manifest_path(&root);
    match load_manifest(&manifest_path) {
        Ok(m) => Ok(m),
        Err(ManifestError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(empty_manifest())
        }
        Err(e) if bootstrap::should_skip_auto_rebuild(argv) => {
            eprintln!(
                "toolr: warning: ignoring unreadable {}: {e}",
                manifest_path.display()
            );
            Ok(empty_manifest())
        }
        Err(e) => Err(anyhow::anyhow!(
            "cannot read {}: {e}; run `toolr project manifest rebuild` to regenerate it",
            manifest_path.display()
        )),
    }
}

fn empty_manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
        groups: Vec::new(),
        commands: Vec::new(),
        plugin_warnings: Vec::new(),
        input_records: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::misplaced_root_flag_hint;
    use crate::cli;

    fn hint(args: &[&str]) -> Option<String> {
        let argv: Vec<String> = std::iter::once("toolr")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        let root = cli::build_command(&super::empty_manifest());
        let err = root.clone().try_get_matches_from(&argv).err()?;
        misplaced_root_flag_hint(&err, &root, &argv)
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
