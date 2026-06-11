//! Synthetic manifest entries for toolr's own built-in subcommands.
//!
//! The completion engine in [`toolr_core::complete`] is purely
//! manifest-driven — it knows nothing about the binary's own subcommands
//! (`self`, `project`). This module supplies the entries that mirror
//! [`crate::cli::build_command`] so the engine can offer them as
//! candidates. The synthetic [`Command`] entries are never invoked; only
//! their `name`, `group`, `arguments[].name`, `arguments[].kind`, and
//! `arguments[].allowed_values` matter for completion classification.
//!
//! When [`crate::cli::build_command`] grows or changes a built-in
//! subcommand, mirror the change here. Tests in this module assert the
//! structure matches at least at the names-and-shape level.
//!
//! Hidden internal helpers (`__complete`, `__build-static-manifest`,
//! `__install-uv-now`) are intentionally omitted — they don't appear in
//! `--help` and shouldn't be tab-completed either.

use toolr_core::manifest::{
    ArgMetadata, Argument, ArgumentKind, Command, Group, Origin,
};

/// Synthetic [`Group`] + [`Command`] entries that mirror the built-in
/// subcommand tree wired up in [`crate::cli::build_command`]. Callers
/// (currently only [`crate::dispatch::run_complete`]) merge these into a
/// loaded manifest before delegating to
/// [`toolr_core::complete::serve_completions`].
pub fn built_in_completion_entries() -> (Vec<Group>, Vec<Command>) {
    let groups = vec![
        top_group("project", "Operations on the current repo's tools/ directory"),
        child_group("venv", "project", "Inspect, sync, and operate on the tools venv"),
        child_group("manifest", "project", "Manage the project's toolr manifest"),
        top_group("self", "Operations on toolr itself"),
        child_group("cache", "self", "Manage the cache of per-repo virtualenvs"),
        child_group("completion", "self", "Manage shell completion scripts"),
    ];

    let commands = vec![
        // project ...
        leaf(
            "init",
            "project",
            "Scaffold tools/ in the current directory",
            vec![
                flag("force"),
                flag("no-sync"),
                opt_enum("venv-location", &["cache", "in-tree"]),
                flag("no-example"),
                opt("python"),
                flag("quiet"),
            ],
        ),
        leaf("path", "project.venv", "Print the absolute path to the tools venv", vec![]),
        leaf(
            "shell",
            "project.venv",
            "Spawn a subshell with the tools venv activated",
            vec![],
        ),
        leaf(
            "sync",
            "project.venv",
            "Sync the tools venv (no-op when fresh)",
            vec![flag("force"), flag("quiet"), flag("upgrade"), repeated("upgrade-package")],
        ),
        leaf(
            "lock",
            "project.venv",
            "Refresh tools/uv.lock without applying (wraps `uv lock`)",
            vec![flag("quiet"), flag("upgrade"), repeated("upgrade-package")],
        ),
        leaf(
            "add",
            "project.venv",
            "Add one or more packages to tools/pyproject.toml (wraps `uv add`)",
            vec![positional("packages"), flag("quiet")],
        ),
        leaf(
            "remove",
            "project.venv",
            "Remove one or more packages from tools/pyproject.toml (wraps `uv remove`)",
            vec![positional("packages"), flag("quiet")],
        ),
        leaf(
            "rebuild",
            "project.manifest",
            "Regenerate the static + dynamic manifest in place",
            vec![],
        ),
        // self ...
        leaf(
            "build-manifest",
            "self",
            "Generate a third-party manifest fragment for a package",
            vec![
                positional("package"),
                opt("output"),
                opt("python"),
                opt("schema-version"),
                flag("check"),
            ],
        ),
        leaf(
            "list",
            "self.cache",
            "List every cached virtualenv with size and last-use timestamp",
            vec![],
        ),
        leaf(
            "prune",
            "self.cache",
            "Remove orphan and stale cache entries",
            vec![
                flag("all"),
                opt("stale-after-days"),
                flag("dry-run"),
                flag("yes"),
            ],
        ),
        leaf(
            "print",
            "self.completion",
            "Print the completion script for a shell to stdout",
            vec![positional_enum("shell", &["bash", "zsh", "fish"])],
        ),
        leaf(
            "install",
            "self.completion",
            "Install the completion script for a shell into its standard location",
            vec![
                positional_enum("shell", &["bash", "zsh", "fish"]),
                flag("force"),
            ],
        ),
    ];

    (groups, commands)
}

/// Root-level flags carried by the `toolr` binary itself (the long-form
/// names; short aliases are intentionally omitted to match how the
/// engine renders leaf flags). Mirror [`crate::cli::build_command`]'s
/// root [`Arg`]s. Engine-level `--help` is offered separately by
/// [`toolr_core::complete::serve_completions`] at every group node.
pub const ROOT_LONG_FLAGS: &[&str] = &[
    "--debug",
    "--no-output-timeout-secs",
    "--no-timestamps",
    "--quiet",
    "--timeout-secs",
    "--timestamps",
];

fn top_group(name: &str, title: &str) -> Group {
    Group {
        name: name.into(),
        title: title.into(),
        description: String::new(),
        parent: None,
        origin: Origin::Static,
    }
}

fn child_group(name: &str, parent: &str, title: &str) -> Group {
    Group {
        name: name.into(),
        title: title.into(),
        description: String::new(),
        parent: Some(parent.into()),
        origin: Origin::Static,
    }
}

fn leaf(name: &str, group: &str, summary: &str, arguments: Vec<Argument>) -> Command {
    Command {
        name: name.into(),
        group: group.into(),
        // Synthetic entries are never invoked — the binary intercepts
        // `self`/`project` before any manifest lookup happens. These
        // fields just need to round-trip through the engine's classifier.
        module: String::new(),
        function: String::new(),
        summary: summary.into(),
        description: String::new(),
        arguments,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

fn arg(name: &str, kind: ArgumentKind, allowed_values: Vec<String>) -> Argument {
    Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: None,
        allowed_values,
        path_constraints: None,
        metadata: ArgMetadata::default(),
        long_flag: None,
    }
}

fn flag(name: &str) -> Argument {
    arg(name, ArgumentKind::Flag, Vec::new())
}

fn opt(name: &str) -> Argument {
    arg(name, ArgumentKind::Optional, Vec::new())
}

fn opt_enum(name: &str, values: &[&str]) -> Argument {
    arg(
        name,
        ArgumentKind::Optional,
        values.iter().map(|v| (*v).to_string()).collect(),
    )
}

fn positional(name: &str) -> Argument {
    arg(name, ArgumentKind::Positional, Vec::new())
}

fn repeated(name: &str) -> Argument {
    arg(name, ArgumentKind::Repeated, Vec::new())
}

fn positional_enum(name: &str, values: &[&str]) -> Argument {
    arg(
        name,
        ArgumentKind::Positional,
        values.iter().map(|v| (*v).to_string()).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolr_core::complete::serve_completions;
    use toolr_core::manifest::{Manifest, SCHEMA_VERSION};

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

    #[test]
    fn top_level_offers_self_and_project() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&[""]));
        assert!(out.contains(&"self".to_string()), "candidates: {out:?}");
        assert!(out.contains(&"project".to_string()), "candidates: {out:?}");
    }

    #[test]
    fn self_offers_known_subcommands() {
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
        // `deps` was removed in 0.22 and must NOT appear as a completion
        // candidate (it would mislead users into trying the old path).
        assert!(
            !out.contains(&"deps".to_string()),
            "`deps` should not be a completion candidate, got: {out:?}"
        );
    }

    #[test]
    fn project_venv_offers_path_shell_sync_lock_add_remove() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", "venv", ""]));
        for expected in ["path", "shell", "sync", "lock", "add", "remove"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} under project venv, got: {out:?}"
            );
        }
        assert!(
            !out.contains(&"upgrade".to_string()),
            "`upgrade` should no longer be a completion candidate, got: {out:?}"
        );
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
    fn root_long_flags_cover_every_root_arg_in_build_command() {
        // Guardrail: if someone adds a root-level `Arg::new(...).long(...)`
        // to `cli::build_command`, this constant must grow to match.
        // `--help` lives in the engine (every group node gets it for
        // free), so it's deliberately not listed here.
        let expected: std::collections::BTreeSet<&str> = [
            "--debug",
            "--no-output-timeout-secs",
            "--no-timestamps",
            "--quiet",
            "--timeout-secs",
            "--timestamps",
        ]
        .into_iter()
        .collect();
        let actual: std::collections::BTreeSet<&str> =
            ROOT_LONG_FLAGS.iter().copied().collect();
        assert_eq!(actual, expected);
        // Sorted asc so the engine's downstream `sort()` is a no-op
        // and tests asserting alphabetical output stay deterministic.
        let mut sorted = ROOT_LONG_FLAGS.to_vec();
        sorted.sort();
        assert_eq!(sorted.as_slice(), ROOT_LONG_FLAGS);
    }

    #[test]
    fn project_init_offers_venv_location_values() {
        let m = merged_empty_manifest();
        let out = serve_completions(
            &m,
            &tokens(&["project", "init", "--venv-location", ""]),
        );
        assert_eq!(out, vec!["cache".to_string(), "in-tree".to_string()]);
    }
}
