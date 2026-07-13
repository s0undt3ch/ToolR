//! Synthetic manifest entries for toolr's own built-in subcommands.
//!
//! The completion engine in [`toolr_core::complete`] is purely
//! manifest-driven — it knows nothing about the binary's own subcommands
//! (`self`, `project`). This module *derives* the entries directly from
//! [`crate::cli::build_command`] by walking the clap tree, so the engine
//! can offer them as candidates. The synthetic [`Command`] entries are
//! never invoked; only their `name`, `group`, `arguments[].name`,
//! `arguments[].kind`, and `arguments[].allowed_values` matter for
//! completion classification.
//!
//! Because the entries are derived, there is nothing to hand-maintain
//! here: adding or changing a `self`/`project` subcommand in
//! [`crate::cli::build_command`] flows through automatically. Hidden
//! internal helpers (`__complete`, `__build-static-manifest`,
//! `__install-uv-now`, the `project deps` migration shim) carry
//! `.hide(true)` in the CLI builder and are skipped — they don't appear
//! in `--help` and shouldn't be tab-completed either.

use clap::{Arg, ArgAction, Command as ClapCommand};
use toolr_core::manifest::{
    ArgMetadata, Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION,
};

/// The two top-level builtin subtrees, walked out of the clap command.
const BUILTIN_ROOTS: &[&str] = &["project", "self"];

/// A manifest with no user/third-party entries, so
/// [`crate::cli::build_command`] yields *only* the hardcoded `self` /
/// `project` builtin subtrees plus the root flags.
fn empty_manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        third_party_hash: String::new(),
        groups: Vec::new(),
        commands: Vec::new(),
    }
}

/// Synthetic [`Group`] + [`Command`] entries derived from the built-in
/// subcommand tree wired up in [`crate::cli::build_command`]. Callers
/// (currently only [`crate::dispatch::run_complete`]) merge these into a
/// loaded manifest before delegating to
/// [`toolr_core::complete::serve_completions`].
pub fn built_in_completion_entries() -> (Vec<Group>, Vec<Command>) {
    let root = crate::cli::build_command(&empty_manifest());
    let mut groups = Vec::new();
    let mut commands = Vec::new();

    for name in BUILTIN_ROOTS {
        let Some(sub) = root
            .get_subcommands()
            .find(|c| c.get_name() == *name && !c.is_hide_set())
        else {
            continue;
        };
        // Each builtin root (`self`, `project`) is itself a group node.
        walk_group(sub, None, &mut groups, &mut commands);
    }

    (groups, commands)
}

/// Recursively walk a clap subcommand that represents a *group* node,
/// emitting a [`Group`] for it and classifying each visible child as
/// either a nested group (has its own subcommands) or a leaf
/// [`Command`].
fn walk_group(
    cmd: &ClapCommand,
    parent: Option<&str>,
    groups: &mut Vec<Group>,
    commands: &mut Vec<Command>,
) {
    let name = cmd.get_name().to_string();
    let full_path = match parent {
        Some(p) => format!("{p}.{name}"),
        None => name.clone(),
    };
    groups.push(Group {
        name: name.clone(),
        title: about_text(cmd),
        description: String::new(),
        parent: parent.map(str::to_string),
        origin: Origin::Static,
    });

    for child in cmd.get_subcommands() {
        if child.is_hide_set() {
            continue;
        }
        if child.get_subcommands().next().is_some() {
            // A child that itself owns subcommands is a nested group.
            walk_group(child, Some(&full_path), groups, commands);
        } else {
            commands.push(leaf_command(child, &full_path));
        }
    }
}

/// Build a synthetic leaf [`Command`] from a clap subcommand. The
/// synthetic entry is never invoked — the binary intercepts
/// `self`/`project` before any manifest lookup — so `module`/`function`
/// are empty; only the shape the completion classifier reads is filled.
fn leaf_command(cmd: &ClapCommand, group: &str) -> Command {
    Command {
        name: cmd.get_name().to_string(),
        group: group.to_string(),
        module: String::new(),
        function: String::new(),
        summary: about_text(cmd),
        description: String::new(),
        arguments: cmd.get_arguments().filter_map(derive_argument).collect(),
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

/// Map a clap [`Arg`] onto a synthetic [`Argument`], or `None` when the
/// arg shouldn't surface as a completion candidate.
///
/// `--help` is filtered out: the engine offers it for free at every
/// group node, so listing it here would duplicate it. Hidden args are
/// skipped for the same reason hidden subcommands are.
///
/// Arg → [`ArgumentKind`] mapping:
/// - positional                     → `Positional`
/// - `ArgAction::SetTrue`/`SetFalse` → `Flag`
/// - `ArgAction::Count`             → `Count`
/// - `ArgAction::Append`            → `Repeated`
/// - value-taking option (`Set`, …) → `Optional`
fn derive_argument(arg: &Arg) -> Option<Argument> {
    if arg.get_id() == "help" || arg.is_hide_set() {
        return None;
    }
    let kind = if arg.is_positional() {
        ArgumentKind::Positional
    } else {
        match arg.get_action() {
            ArgAction::SetTrue | ArgAction::SetFalse => ArgumentKind::Flag,
            ArgAction::Count => ArgumentKind::Count,
            ArgAction::Append => ArgumentKind::Repeated,
            _ => ArgumentKind::Optional,
        }
    };
    let allowed_values = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();
    Some(Argument {
        // The engine matches a committed `--flag` against `a.name` after
        // stripping `--`, and renders flag candidates as
        // `--{name.replace('_', "-")}`. clap's id is already the
        // hyphenated long-flag stem, so it round-trips.
        name: arg.get_id().to_string(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: None,
        allowed_values,
        path_constraints: None,
        metadata: ArgMetadata::default(),
        long_flag: None,
    })
}

/// clap's `about` rendered to a plain string (empty when unset).
fn about_text(cmd: &ClapCommand) -> String {
    cmd.get_about().map(ToString::to_string).unwrap_or_default()
}

/// Root-level long flags carried by the `toolr` binary itself, derived
/// from [`crate::cli::build_command`]'s root [`Arg`]s. Short aliases are
/// intentionally omitted (the engine renders leaf flags long-form only),
/// as is `--help` (offered separately by
/// [`toolr_core::complete::serve_completions`] at every group node). The
/// list is sorted so the engine's downstream `sort()` is a no-op.
pub fn root_long_flags() -> Vec<String> {
    let root = crate::cli::build_command(&empty_manifest());
    let mut flags: Vec<String> = root
        .get_arguments()
        .filter(|a| a.get_id() != "help" && !a.is_hide_set())
        .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
        .collect();
    flags.sort();
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolr_core::complete::serve_completions;

    fn merged_empty_manifest() -> Manifest {
        let (groups, commands) = built_in_completion_entries();
        Manifest {
            schema_version: SCHEMA_VERSION,
            static_hash: String::new(),
            third_party_hash: String::new(),
            groups,
            commands,
        }
    }

    fn tokens(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| (*s).to_string()).collect()
    }

    // --- Unit: the clap Arg -> ArgumentKind classifier ---

    #[test]
    fn derive_argument_maps_each_clap_action() {
        // `Count` and `Append` aren't used by any built-in `self`/`project`
        // arg today, so exercise `derive_argument` directly to pin the full
        // mapping table (and the help/hidden drop) regardless of which
        // actions the live CLI happens to use.
        let count = Arg::new("verbose").long("verbose").action(ArgAction::Count);
        assert_eq!(derive_argument(&count).unwrap().kind, ArgumentKind::Count);

        let append = Arg::new("define").long("define").action(ArgAction::Append);
        assert_eq!(derive_argument(&append).unwrap().kind, ArgumentKind::Repeated);

        let set_true = Arg::new("force").long("force").action(ArgAction::SetTrue);
        assert_eq!(derive_argument(&set_true).unwrap().kind, ArgumentKind::Flag);

        let optional = Arg::new("prefix").long("prefix").action(ArgAction::Set);
        assert_eq!(derive_argument(&optional).unwrap().kind, ArgumentKind::Optional);

        // `help` and hidden args are dropped from completion output.
        let help = Arg::new("help").long("help").action(ArgAction::SetTrue);
        assert!(derive_argument(&help).is_none());
        let hidden = Arg::new("secret")
            .long("secret")
            .action(ArgAction::SetTrue)
            .hide(true);
        assert!(derive_argument(&hidden).is_none());
    }

    // --- Behavioural: the derived tree drives real completion output ---

    #[test]
    fn top_level_offers_self_and_project() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&[""]));
        assert!(out.contains(&"self".to_string()), "candidates: {out:?}");
        assert!(out.contains(&"project".to_string()), "candidates: {out:?}");
    }

    #[test]
    fn self_offers_known_subcommands() {
        // Proves a `self` subcommand defined only in `cli.rs` surfaces
        // here without editing this file.
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["self", ""]));
        for expected in ["build-manifest", "cache", "completion"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} in {out:?}"
            );
        }
    }

    #[test]
    fn project_offers_known_subcommands() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", ""]));
        for expected in ["init", "venv", "manifest"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} in {out:?}"
            );
        }
        // `deps` is `.hide(true)` in cli.rs (a removed-command migration
        // shim) and must NOT leak into completion candidates.
        assert!(
            !out.contains(&"deps".to_string()),
            "`deps` is hidden and should not be a completion candidate, got: {out:?}"
        );
    }

    #[test]
    fn project_venv_offers_run_path_shell_sync_lock_add_remove() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", "venv", ""]));
        for expected in ["run", "path", "shell", "sync", "lock", "add", "remove"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} under project venv, got: {out:?}"
            );
        }
    }

    #[test]
    fn project_venv_sync_offers_force_and_quiet_flags() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", "venv", "sync", "--"]));
        for expected in ["--force", "--quiet"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} in project venv sync flags, got: {out:?}"
            );
        }
    }

    #[test]
    fn self_cache_offers_list_and_prune() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["self", "cache", ""]));
        assert_eq!(out, vec!["list".to_string(), "prune".to_string()]);
    }

    #[test]
    fn self_completion_install_offers_shells() {
        // A leaf with an enum positional offers its possible values,
        // proving `allowed_values` is derived from clap's value_parser.
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["self", "completion", "install", ""]));
        assert_eq!(
            out,
            vec!["bash".to_string(), "fish".to_string(), "zsh".to_string()]
        );
    }

    #[test]
    fn self_cache_prune_offers_known_flags() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["self", "cache", "prune", "--"]));
        for expected in ["--all", "--stale-after-days", "--dry-run", "--yes"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} in {out:?}"
            );
        }
    }

    #[test]
    fn project_init_offers_venv_location_values() {
        let m = merged_empty_manifest();
        let out =
            serve_completions(&m, &tokens(&["project", "init", "--venv-location", ""]));
        assert_eq!(out, vec!["cache".to_string(), "in-tree".to_string()]);
    }

    // --- Derivation invariants ---

    #[test]
    fn hidden_internal_helpers_are_absent() {
        // The `__*` helpers are `.hide(true)` top-level subcommands and
        // must never appear as completion candidates.
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&[""]));
        for hidden in ["__complete", "__build-static-manifest", "__install-uv-now"] {
            assert!(
                !out.contains(&hidden.to_string()),
                "hidden helper {hidden} leaked into candidates: {out:?}"
            );
        }
    }

    #[test]
    fn root_long_flags_are_sorted_and_exclude_help() {
        let flags = root_long_flags();
        assert!(!flags.is_empty(), "expected some root flags");
        assert!(
            !flags.contains(&"--help".to_string()),
            "--help is engine-provided and should be excluded: {flags:?}"
        );
        let mut sorted = flags.clone();
        sorted.sort();
        assert_eq!(flags, sorted, "root flags must be pre-sorted");
        // Spot-check a couple known root flags carry through.
        for expected in ["--debug", "--quiet"] {
            assert!(
                flags.contains(&expected.to_string()),
                "missing {expected} in {flags:?}"
            );
        }
    }
}
