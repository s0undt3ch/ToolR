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
    // Emit the passive cache hint before clap touches argv.
    maybe_emit_cache_hint_from_argv();
    let persist = bootstrap::persist_manifest(&cwd, &argv);
    let bootstrap_warnings = bootstrap::ensure_manifest_present_or_bootstrap(&cwd, &argv, persist)?;
    let (mut manifest, fresh_warnings) =
        match bootstrap::ensure_manifest_fresh(&cwd, &argv, persist)? {
            Some(built) => (built.manifest, built.warnings),
            // `__complete` resolves the manifest itself; loading it here too
            // would parse it twice per TAB and warn about a corrupt one.
            None if bootstrap::is_tab_completion(&argv) => (empty_manifest(), Vec::new()),
            None => (load_or_empty(&cwd, &argv)?, Vec::new()),
        };
    // Covers fragments and a hand-edited committed manifest alike. Tab
    // completion drops the same entries in `resolve_manifest_at_tab`.
    manifest.drop_unsafe_names();
    if !bootstrap::should_skip_auto_rebuild(&argv) {
        // Printed under `-q` too (the rule is on the `-q` arg in `cli::build_command`).
        // Build warnings come only from a manifest built on this run. When the
        // bootstrap built it, the freshness step's rebuild of the same tree
        // repeats them, so only the bootstrap's print.
        let built = bootstrap_warnings.as_ref().unwrap_or(&fresh_warnings);
        let plugin = manifest.plugin_warnings.iter().map(|w| &w.message);
        for message in built.iter().chain(plugin) {
            eprintln!(
                "toolr: warning: {}",
                toolr_core::text::strip_control_chars(message)
            );
        }
    }
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
    dispatch::dispatch(&matches, &manifest, &mut command)
}

/// The line to follow clap's "unrecognized subcommand" when `cwd` is not in a
/// project at all. A bare `tools/` already gets its own warning.
fn no_project_hint(cwd: &std::path::Path) -> Option<String> {
    match discover_project_root(cwd) {
        Err(e @ DiscoveryError::NotFound(_)) => Some(format!("toolr: {e}")),
        _ => None,
    }
}

fn maybe_emit_cache_hint_from_argv() {
    if std::env::var_os("TOOLR_NO_CACHE_HINT").is_some() {
        return;
    }
    let argv: Vec<String> = std::env::args().collect();
    // The passive hint is advice, which `-q` drops. We scan argv by hand
    // because this runs before clap and because `--quiet` is accepted
    // both root-level and per-subcommand. Detect both `--quiet` and any
    // short cluster containing `q` (e.g. `-q`), and stop at the `--`
    // separator so a wrapped command's own `--quiet`
    // (`run -- pytest --quiet`) doesn't count.
    if argv_requests_quiet(&argv) {
        return;
    }
    if argv_requests_version_or_help(&argv) {
        return;
    }
    // Suppress for tab-completion and `self cache ...` invocations.
    if bootstrap::is_tab_completion(&argv) {
        return;
    }
    let positional: Vec<&str> = argv
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();
    if positional.first().copied() == Some("self") && positional.get(1).copied() == Some("cache") {
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

/// Best-effort scan of `argv` for toolr's own flags, used to gate the passive
/// cache hint. Matches any of `exact`, or, when `cluster_char` is set, a short
/// cluster containing it (`-q`, `-dq`, …). Stops at the `--` separator so a
/// wrapped command's flags (`toolr project venv run -- <cmd> --quiet`) never count.
fn argv_requests(argv: &[String], exact: &[&str], cluster_char: Option<char>) -> bool {
    argv.iter().skip(1).take_while(|a| *a != "--").any(|a| {
        exact.contains(&a.as_str())
            || cluster_char
                .is_some_and(|c| a.starts_with('-') && !a.starts_with("--") && a[1..].contains(c))
    })
}

fn argv_requests_quiet(argv: &[String]) -> bool {
    argv_requests(argv, &["--quiet"], Some('q'))
}

fn argv_requests_version_or_help(argv: &[String]) -> bool {
    argv_requests(argv, &["--version", "-V", "--help", "-h"], None)
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
    let manifest_path = root.join("tools").join(".toolr-manifest.json");
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
    use super::{argv_requests_quiet, argv_requests_version_or_help};

    fn argv(args: &[&str]) -> Vec<String> {
        std::iter::once("toolr")
            .chain(args.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn detects_long_and_short_quiet() {
        assert!(argv_requests_quiet(&argv(&[
            "project", "venv", "sync", "--quiet"
        ])));
        assert!(argv_requests_quiet(&argv(&[
            "-q", "project", "venv", "sync"
        ])));
        // Short cluster.
        assert!(argv_requests_quiet(&argv(&["-dq", "ci", "hello"])));
    }

    #[test]
    fn absent_quiet_is_not_detected() {
        assert!(!argv_requests_quiet(&argv(&["project", "venv", "sync"])));
        assert!(!argv_requests_quiet(&argv(&["--debug", "ci", "hello"])));
        // `--quiet` is a long flag; a bare `q` in a value must not match.
        assert!(!argv_requests_quiet(&argv(&["ci", "hello", "queen"])));
    }

    #[test]
    fn wrapped_command_quiet_after_separator_is_ignored() {
        // The `--quiet` belongs to pytest, not toolr.
        assert!(!argv_requests_quiet(&argv(&[
            "project", "venv", "run", "--", "pytest", "--quiet",
        ])));
        // But toolr's own quiet before the separator still counts.
        assert!(argv_requests_quiet(&argv(&[
            "project", "venv", "run", "--quiet", "--", "pytest",
        ])));
    }

    #[test]
    fn detects_version_and_help_requests() {
        for flag in ["--version", "-V", "--help", "-h"] {
            assert!(argv_requests_version_or_help(&argv(&[flag])), "{flag}");
            assert!(
                argv_requests_version_or_help(&argv(&["ci", flag])),
                "{flag}"
            );
        }
    }

    #[test]
    fn ignores_version_and_help_after_the_separator_or_absent() {
        assert!(!argv_requests_version_or_help(&argv(&["ci", "hello"])));
        assert!(!argv_requests_version_or_help(&argv(&[
            "project", "venv", "run", "--", "pytest", "--help"
        ])));
    }
}
