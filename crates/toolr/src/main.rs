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

use toolr_core::discovery::discover_project_root;
use toolr_core::manifest::{Manifest, SCHEMA_VERSION, load_manifest};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("toolr: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let argv: Vec<String> = std::env::args().collect();
    // Emit the passive cache hint before clap touches argv, so `--version`
    // and `--help` (which would otherwise exit inside clap) still see it.
    maybe_emit_cache_hint_from_argv();
    bootstrap::ensure_manifest_present_or_bootstrap(&cwd, &argv)?;
    bootstrap::ensure_manifest_fresh(&cwd, &argv)?;
    let manifest = load_or_empty(&cwd);
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
            if (want_help || want_short) && matches!(
                e.kind(),
                ErrorKind::MissingSubcommand | ErrorKind::MissingRequiredArgument
            ) {
                // Help was requested but clap failed validation first.
                // Delegate to dispatch with a synthesised "root" match by
                // passing an empty argv so clap can build a valid root-level
                // ArgMatches. Dispatch's resolve_help_target will then walk
                // the argv to find the right level.
                dispatch::dispatch_help_from_argv(&argv, &manifest, &mut command)?;
                return Ok(ExitCode::SUCCESS);
            }
            // Genuine error: let clap print it and exit.
            e.exit()
        }
    };
    dispatch::dispatch(&matches, &manifest, &mut command)
}

fn maybe_emit_cache_hint_from_argv() {
    if std::env::var_os("TOOLR_NO_CACHE_HINT").is_some() {
        return;
    }
    let argv: Vec<String> = std::env::args().collect();
    // `--quiet` promises to "suppress non-error output"; the passive
    // hint is non-error output, so honour it here. We scan argv by hand
    // because this runs before clap and because `--quiet` is accepted
    // both root-level and per-subcommand. Detect both `--quiet` and any
    // short cluster containing `q` (e.g. `-q`), and stop at the `--`
    // separator so a wrapped command's own `--quiet`
    // (`run -- pytest --quiet`) doesn't count.
    if argv_requests_quiet(&argv) {
        return;
    }
    // Suppress for tab-completion and `self cache ...` invocations.
    let positional: Vec<&str> = argv
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();
    if positional.first().copied() == Some("__complete") {
        return;
    }
    if positional.first().copied() == Some("self") && positional.get(1).copied() == Some("cache") {
        return;
    }
    let Ok(cache_root) = self_cache::resolve_cache_root() else {
        return;
    };
    let cfg = toolr_core::cache::HintConfig::default();
    if let Ok(Some(msg)) = toolr_core::cache::compute_hint(&cache_root, &cfg, chrono::Utc::now()) {
        eprintln!("{msg}");
    }
}

/// Best-effort scan of `argv` for a `--quiet` / `-q` request, used to
/// gate the passive cache hint. Stops at the `--` separator so args
/// belonging to a wrapped command (`toolr project venv run -- <cmd>
/// --quiet`) are never mistaken for toolr's own quiet flag.
fn argv_requests_quiet(argv: &[String]) -> bool {
    for arg in argv.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--quiet" {
            return true;
        }
        // Short-flag cluster (`-q`, `-dq`, …) but not a long flag.
        if arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('q') {
            return true;
        }
    }
    false
}

fn load_or_empty(cwd: &std::path::Path) -> Manifest {
    let Ok(root) = discover_project_root(cwd) else {
        return empty_manifest();
    };
    let manifest_path = root.join("tools").join(".toolr-manifest.json");
    load_manifest(&manifest_path).unwrap_or_else(|_| empty_manifest())
}

fn empty_manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        third_party_hash: String::new(),
        groups: Vec::new(),
        commands: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::argv_requests_quiet;

    fn argv(args: &[&str]) -> Vec<String> {
        std::iter::once("toolr")
            .chain(args.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn detects_long_and_short_quiet() {
        assert!(argv_requests_quiet(&argv(&["project", "venv", "sync", "--quiet"])));
        assert!(argv_requests_quiet(&argv(&["-q", "project", "venv", "sync"])));
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
}
