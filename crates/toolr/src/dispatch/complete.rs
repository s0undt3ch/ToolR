//! `toolr __complete`: the shell scripts' Tab requests.

use std::path::PathBuf;
use std::process::ExitCode;

use toolr_core::complete::{
    Candidate, Completion, Matching, complete_matching, resolve_manifest_at_tab,
    sort_and_dedup_by_value,
};
use toolr_core::manifest::Manifest;

use crate::builtin_completions::RootScan;
use crate::invocation::RootArgv;

/// `toolr __complete` exits with this, printing nothing, to have the shell
/// complete file system paths. Old scripts treat it as "no candidates".
/// The scripts in `crates/toolr-core/src/complete/scripts/` (`bash.sh`, `fish.fish`, `zsh.zsh`,
/// `powershell.ps1`)
/// and the exit-code table in `docs/cli.md` must match both values.
const COMPLETE_PATHS: u8 = 10;
/// As [`COMPLETE_PATHS`], for directories only.
const COMPLETE_DIRECTORIES: u8 = 11;

/// Set by the zsh and fish scripts for their `__complete` call. An environment variable, not a
/// flag, so a new script on an old binary still parses (and gets prefix-filtered candidates).
const COMPLETE_MATCH_VAR: &str = "TOOLR_COMPLETE_MATCH";

fn matching_from_env() -> Matching {
    matching_from(std::env::var(COMPLETE_MATCH_VAR).ok().as_deref())
}

/// `shell` lets the shell match the typed word; anything else, a typo included, keeps the
/// engine's prefix filtering.
fn matching_from(value: Option<&str>) -> Matching {
    match value {
        Some("shell") => Matching::Shell,
        _ => Matching::Prefix,
    }
}

pub(super) fn run_complete(
    matches: &clap::ArgMatches,
    argv: &RootArgv,
) -> anyhow::Result<ExitCode> {
    // Tab completion must be quiet: any error produces a silent exit
    // code 1 so the shell falls back to its default completion. We do
    // not write to stderr here — that would clobber the user's prompt.
    let Some(cwd) = matches.get_one::<String>("cwd").map(PathBuf::from) else {
        return Ok(ExitCode::from(1));
    };
    let raw = argv.complete_tokens();
    // Root flags are not global and the engine has never heard of them: strip the leading run so
    // `toolr --quiet ci <TAB>` reaches it as `ci <TAB>`. A line clap would reject completes to nothing.
    let scan = crate::builtin_completions::scan_root_flags(raw);
    if scan.dead_end {
        return Ok(ExitCode::SUCCESS);
    }
    // No `tools/` ancestor → no user-defined commands to complete, but
    // the binary's own `self` / `project` subtree must still be offered
    // (it doesn't depend on a project root). Fall back to an empty
    // manifest so the built-in injection below covers the cwd.
    let mut manifest = resolve_manifest_at_tab(&cwd)
        .map(|r| r.manifest)
        .unwrap_or_else(|_| Manifest::empty());
    // A normal run ignores user groups named like a built-in; Tab must not offer them either.
    let _ = crate::cli::drop_reserved_user_groups(&mut manifest);
    // The engine is purely manifest-driven, but the binary owns its own
    // built-in `self` / `project` subtree. Inject synthetic entries that
    // mirror `cli::build_command` so those subcommands also complete.
    let (extra_groups, extra_commands) = crate::builtin_completions::built_in_completion_entries();
    manifest.groups.extend(extra_groups);
    manifest.commands.extend(extra_commands);
    let matching = matching_from_env();
    let mut candidates = match complete_matching(&manifest, &scan.rest, matching) {
        Completion::Values(candidates) => candidates,
        Completion::Paths => return Ok(ExitCode::from(COMPLETE_PATHS)),
        Completion::Directories => return Ok(ExitCode::from(COMPLETE_DIRECTORIES)),
    };
    // The engine emits `--help` at every group node (it's universal for
    // clap subcommands), but the binary's other root-level flags
    // (`--debug`, `--quiet`, `--timestamps`, …) live only in
    // `cli::build_command` and need to be merged in for top-level
    // `toolr --<TAB>` to surface them.
    merge_root_flags(&scan, &mut candidates, matching);
    for candidate in candidates {
        println!("{}\t{}", candidate.value, candidate.description);
    }
    Ok(ExitCode::SUCCESS)
}

/// When the user is completing a flag at the root (`toolr --<TAB>`, `toolr -<TAB>`, or after other root
/// flags), merge the binary's own root flags into the engine's candidates, minus those already given or
/// conflicting with one. No-op for any other slot.
fn merge_root_flags(scan: &RootScan, candidates: &mut Vec<Candidate>, matching: Matching) {
    let [prefix] = scan.rest.as_slice() else {
        return;
    };
    if !(prefix.starts_with("--") || prefix == "-") {
        return;
    }
    // The engine offers `--help` at every group node; it must not repeat once given.
    if scan.seen.iter().any(|id| id == "help") {
        candidates.retain(|c| c.value != "--help");
    }
    for flag in scan.offerable_long_flags() {
        if matching.keeps(&flag.value, prefix) {
            candidates.push(flag);
        }
    }
    sort_and_dedup_by_value(candidates);
}

#[cfg(test)]
mod tests;
