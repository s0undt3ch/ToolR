use crate::complete::serve_completions;
use crate::manifest::{
    Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION,
};

fn fixture() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        groups: vec![
            Group {
                name: "ci".into(),
                title: "CI utilities".into(),
                description: String::new(),
                parent: None,
                origin: Origin::Static,
            },
            Group {
                name: "data".into(),
                title: "Data utilities".into(),
                description: String::new(),
                parent: None,
                origin: Origin::Static,
            },
        ],
        commands: vec![
            Command {
                name: "hello".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "hello".into(),
                summary: "Say hello.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "name".into(),
                    kind: ArgumentKind::Optional,
                    help: "Who to greet".into(),
                    default: Some("\"world\"".into()),
                    type_annotation: Some("str".into()),
                    resolved_type: None,
                    path_constraints: None,
                    metadata: crate::manifest::ArgMetadata::default(),
                    long_flag: None,
                    allowed_values: vec![],
                }],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
            Command {
                name: "deploy".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "deploy".into(),
                summary: "Deploy something.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "env".into(),
                    kind: ArgumentKind::Optional,
                    help: "Target env".into(),
                    default: None,
                    type_annotation: Some("Literal".into()),
                    resolved_type: None,
                    path_constraints: None,
                    metadata: crate::manifest::ArgMetadata::default(),
                    long_flag: None,
                    allowed_values: vec!["staging".into(), "production".into()],
                }],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
            Command {
                name: "load".into(),
                group: "data".into(),
                module: "tools.data".into(),
                function: "load".into(),
                summary: "Load data.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "shape".into(),
                    kind: ArgumentKind::Positional,
                    help: "Shape".into(),
                    default: None,
                    type_annotation: Some("Literal".into()),
                    resolved_type: None,
                    path_constraints: None,
                    metadata: crate::manifest::ArgMetadata::default(),
                    long_flag: None,
                    allowed_values: vec!["wide".into(), "tall".into()],
                }],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
        ],
    }
}

fn tokens(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn empty_tokens_lists_all_groups() {
    let out = serve_completions(&fixture(), &tokens(&[""]));
    assert_eq!(out, vec!["ci".to_string(), "data".to_string()]);
}

#[test]
fn group_prefix_filters_groups() {
    let out = serve_completions(&fixture(), &tokens(&["c"]));
    assert_eq!(out, vec!["ci".to_string()]);
}

#[test]
fn after_group_lists_its_commands() {
    let out = serve_completions(&fixture(), &tokens(&["ci", ""]));
    assert_eq!(out, vec!["deploy".to_string(), "hello".to_string()]);
}

#[test]
fn command_prefix_filters_commands() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "h"]));
    assert_eq!(out, vec!["hello".to_string()]);
}

#[test]
fn flag_prefix_lists_argument_flags() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--"]));
    assert_eq!(out, vec!["--name".to_string()]);
}

#[test]
fn flag_value_completes_to_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", ""]));
    assert_eq!(out, vec!["production".to_string(), "staging".to_string()]);
}

#[test]
fn flag_value_partial_filters_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", "s"]));
    assert_eq!(out, vec!["staging".to_string()]);
}

#[test]
fn positional_value_completes_to_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["data", "load", ""]));
    assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
}

#[test]
fn unknown_group_returns_no_completions() {
    let out = serve_completions(&fixture(), &tokens(&["nope", ""]));
    assert!(out.is_empty());
}

#[test]
fn flag_without_allowed_values_returns_empty() {
    // `--name` has no allowed_values → shell falls back to filename completion.
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--name", ""]));
    assert!(out.is_empty());
}

fn nested_fixture() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        groups: vec![
            Group {
                name: "docker".into(),
                title: "Docker".into(),
                description: String::new(),
                parent: None,
                origin: Origin::Static,
            },
            Group {
                name: "image".into(),
                title: "Image".into(),
                description: String::new(),
                parent: Some("docker".into()),
                origin: Origin::Static,
            },
            Group {
                name: "container".into(),
                title: "Container".into(),
                description: String::new(),
                parent: Some("docker".into()),
                origin: Origin::Static,
            },
        ],
        commands: vec![
            Command {
                name: "build".into(),
                group: "docker.image".into(),
                module: "tools.docker".into(),
                function: "build".into(),
                summary: "Build an image.".into(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
            Command {
                name: "start".into(),
                group: "docker.container".into(),
                module: "tools.docker".into(),
                function: "start".into(),
                summary: "Start a container.".into(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
        ],
    }
}

#[test]
fn top_level_completion_lists_only_top_level_groups() {
    // `docker.image` is nested under `docker` — it must not appear at
    // the top level, otherwise the shell would offer it as a sibling
    // of `docker`.
    let out = serve_completions(&nested_fixture(), &tokens(&[""]));
    assert_eq!(out, vec!["docker".to_string()]);
}

#[test]
fn nested_group_completion_lists_child_groups() {
    let out = serve_completions(&nested_fixture(), &tokens(&["docker", ""]));
    assert_eq!(out, vec!["container".to_string(), "image".to_string()]);
}

#[test]
fn nested_command_completion_traverses_full_path() {
    let out = serve_completions(&nested_fixture(), &tokens(&["docker", "image", ""]));
    assert_eq!(out, vec!["build".to_string()]);
}

#[test]
fn nested_command_completion_filters_by_prefix() {
    let out =
        serve_completions(&nested_fixture(), &tokens(&["docker", "container", "st"]));
    assert_eq!(out, vec!["start".to_string()]);
}

fn dispatcher_fixture() -> Manifest {
    // Mirrors the dashtastic shape: `jenkins job` is a dispatcher with
    // a couple of its own flags (`--dry-run`, `--cpu`) plus grafted
    // children at `group = "jenkins.job"`.
    let str_arg = |name: &str, kind, default: Option<&str>, values: &[&str]| Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: default.map(str::to_string),
        type_annotation: Some("str".into()),
        resolved_type: None,
        path_constraints: None,
        metadata: crate::manifest::ArgMetadata::default(),
        long_flag: None,
        allowed_values: values.iter().map(|s| (*s).to_string()).collect(),
    };
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        groups: vec![Group {
            name: "jenkins".into(),
            title: "Jenkins".into(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }],
        commands: vec![
            // The dispatcher itself — its `is_dispatcher` flag is what
            // tells the completion engine to look for grafted children
            // at `group == "jenkins.job"`.
            Command {
                name: "job".into(),
                group: "jenkins".into(),
                module: "tools.job".into(),
                function: "job".into(),
                summary: "Trigger Jenkins.".into(),
                description: String::new(),
                arguments: vec![
                    str_arg("cpu", ArgumentKind::Optional, Some("200m"), &[]),
                    str_arg("dry-run", ArgumentKind::Flag, None, &[]),
                ],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: true,
            },
            // Two grafted children under `jenkins.job`. The leaf
            // command's own argument (a positional with allowed values)
            // is what we expect to see after the child name is chosen.
            Command {
                name: "delete_orphans".into(),
                group: "jenkins.job".into(),
                module: "tools.job".into(),
                function: "job".into(),
                summary: "Delete orphans.".into(),
                description: String::new(),
                arguments: vec![str_arg(
                    "scope",
                    ArgumentKind::Positional,
                    None,
                    &["all", "stale"],
                )],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: Some("argparse:django".into()),
                is_dispatcher: false,
            },
            Command {
                name: "delete_stale".into(),
                group: "jenkins.job".into(),
                module: "tools.job".into(),
                function: "job".into(),
                summary: "Delete stale entries.".into(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: Some("argparse:django".into()),
                is_dispatcher: false,
            },
        ],
    }
}

#[test]
fn dispatcher_completion_lists_grafted_children() {
    // Regression for the dashtastic case: `toolr jenkins job <TAB>`
    // returned nothing even though `--help` showed the children.
    let out = serve_completions(&dispatcher_fixture(), &tokens(&["jenkins", "job", ""]));
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_completion_filters_children_by_prefix() {
    let out = serve_completions(&dispatcher_fixture(), &tokens(&["jenkins", "job", "delete_"]));
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_flag_prefix_lists_dispatcher_flags_not_children() {
    // `toolr jenkins job --<TAB>` must offer the dispatcher's own
    // flags, not the grafted child names.
    let out = serve_completions(&dispatcher_fixture(), &tokens(&["jenkins", "job", "--"]));
    assert_eq!(out, vec!["--cpu".to_string(), "--dry-run".to_string()]);
}

#[test]
fn dispatcher_completion_offers_children_after_parent_flag() {
    // `toolr jenkins job --dry-run <TAB>` should still suggest children
    // — the parent flag was consumed but no child has been picked yet.
    let out = serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--dry-run", ""]),
    );
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_recurses_into_child_args_once_child_is_chosen() {
    // `toolr jenkins job delete_orphans <TAB>` — the child has a
    // positional with allowed values; surface them.
    let out = serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "delete_orphans", ""]),
    );
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

#[test]
fn dispatcher_recurses_through_parent_flag_value_pair() {
    // `toolr jenkins job --cpu 200m delete_orphans <TAB>` must skip
    // both `--cpu` and its value when locating the chosen child.
    let out = serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--cpu", "200m", "delete_orphans", ""]),
    );
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

fn flags_only_child_fixture() -> Manifest {
    // Mirrors the dashtastic `delete_companyuser` shape: every grafted
    // arg is `--flag value` style (no positionals at all). Tab on an
    // empty prefix after the child name must surface the available
    // flag names rather than returning nothing.
    let opt = |name: &str, help: &str| Argument {
        name: name.into(),
        kind: ArgumentKind::Optional,
        help: help.into(),
        default: None,
        type_annotation: Some("int".into()),
        resolved_type: None,
        path_constraints: None,
        metadata: crate::manifest::ArgMetadata::default(),
        long_flag: Some(format!("--{}", name)),
        allowed_values: vec![],
    };
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        groups: vec![Group {
            name: "jenkins".into(),
            title: "Jenkins".into(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }],
        commands: vec![
            Command {
                name: "job".into(),
                group: "jenkins".into(),
                module: "tools.job".into(),
                function: "job".into(),
                summary: "Dispatcher.".into(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: true,
            },
            Command {
                name: "delete_companyuser".into(),
                group: "jenkins.job".into(),
                module: "tools.job".into(),
                function: "job".into(),
                summary: "Delete a CompanyUser.".into(),
                description: String::new(),
                arguments: vec![
                    opt("company-id", "Company ID"),
                    opt("user-ids", "User IDs"),
                    opt("user-emails", "User emails"),
                ],
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: Some("argparse:django".into()),
                is_dispatcher: false,
            },
        ],
    }
}

#[test]
fn child_with_only_optional_flags_offers_flags_on_empty_prefix() {
    // Regression for dashtastic `toolr jenkins job delete_companyuser <TAB>`:
    // the child has no positional schema, so the engine used to return
    // `Slot::None` and the shell saw nothing. Now we fall back to the
    // child's flag names.
    let out = serve_completions(
        &flags_only_child_fixture(),
        &tokens(&["jenkins", "job", "delete_companyuser", ""]),
    );
    assert_eq!(
        out,
        vec![
            "--company-id".to_string(),
            "--user-emails".to_string(),
            "--user-ids".to_string(),
        ]
    );
}

#[test]
fn child_flag_fallback_filters_by_prefix() {
    // `toolr jenkins job delete_companyuser --user<TAB>` must filter
    // to flags starting with `--user`.
    let out = serve_completions(
        &flags_only_child_fixture(),
        &tokens(&["jenkins", "job", "delete_companyuser", "--user"]),
    );
    assert_eq!(
        out,
        vec!["--user-emails".to_string(), "--user-ids".to_string()]
    );
}

#[test]
fn leaf_with_only_flags_offers_flags_on_empty_prefix() {
    // The fallback should also apply to regular (non-dispatcher) leaf
    // commands. `toolr ci hello <TAB>` where `hello` has only `--name`
    // should suggest `--name`, not nothing.
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", ""]));
    assert_eq!(out, vec!["--name".to_string()]);
}

use crate::complete::{ResolvedManifest, resolve_manifest_at_tab};
use crate::dynamic::empty_third_party_hash;
use crate::manifest::{write_manifest};
use tempfile::TempDir;

fn make_tree(py_files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("tools")).unwrap();
    for (name, contents) in py_files {
        let path = tmp.path().join("tools").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    tmp
}

#[test]
fn returns_fresh_manifest_when_no_cache_exists() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    let ResolvedManifest {
        manifest,
        from_cache,
        project_root,
    } = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!from_cache, "no cache file existed");
    assert_eq!(project_root, tmp.path().canonicalize().unwrap());
    assert!(manifest.groups.iter().any(|g| g.name == "ci"));
    assert!(manifest.commands.iter().any(|c| c.name == "hello"));
}

#[test]
fn returns_cached_manifest_when_hash_matches() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    // Build once and write to disk.  Stamp the third_party_hash with the
    // empty-venv sentinel so `freshness::compare` (venv_dir=None) sees a
    // matching hash and returns Fresh.
    let mut built = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
    built.third_party_hash = empty_third_party_hash();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &built).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(resolved.from_cache);
    assert_eq!(resolved.manifest, built);
}

#[test]
fn re_parses_when_cached_hash_is_stale() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    // Write a stale manifest with a bogus hash.
    let mut stale = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
    stale.static_hash = "deliberately-stale".into();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &stale).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache, "stale hash should trigger reparse");
    assert_ne!(resolved.manifest.static_hash, "deliberately-stale");
}

#[test]
fn preserves_dynamic_entries_from_cache_when_reparsing() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    // Seed a manifest with a fake dynamic command and a stale static_hash
    // so the re-parse path runs.
    let mut seeded = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
    seeded.static_hash = "stale".into();
    seeded.commands.push(crate::manifest::Command {
        name: "from-plugin".into(),
        group: "dyn-group".into(),
        module: "third_party_pkg".into(),
        function: "from_plugin".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        imports: vec![],
        origin: Origin::Dynamic,
        dispatched_from: None,
        is_dispatcher: false,
    });
    seeded.groups.push(crate::manifest::Group {
        name: "dyn-group".into(),
        title: "Dynamic group".into(),
        description: String::new(),
        parent: None,
        origin: Origin::Dynamic,
    });
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &seeded).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache);
    // Static-layer entry survives.
    assert!(resolved.manifest.commands.iter().any(|c| c.name == "hello"));
    // Dynamic-layer entry from the cache is preserved through the reparse.
    assert!(
        resolved
            .manifest
            .commands
            .iter()
            .any(|c| c.name == "from-plugin" && matches!(c.origin, Origin::Dynamic))
    );
    assert!(
        resolved
            .manifest
            .groups
            .iter()
            .any(|g| g.name == "dyn-group" && matches!(g.origin, Origin::Dynamic))
    );
}

#[test]
fn errors_when_no_tools_dir_exists() {
    let tmp = TempDir::new().unwrap();
    // GHA Windows runners ship with `C:\tools\` populated, which makes
    // the discovery walk succeed when it crawls past the drive root.
    // Same hazard on any host with `/tools`. Skip when the host
    // violates the test precondition.
    let mut walker = tmp.path().canonicalize().unwrap_or_else(|_| tmp.path().to_path_buf());
    let ancestor_has_tools = loop {
        if walker.join("tools").is_dir() {
            break true;
        }
        if !walker.pop() {
            break false;
        }
    };
    if ancestor_has_tools {
        eprintln!(
            "skipping: an ancestor of {} has a tools/ dir; \
             this host violates the test precondition.",
            tmp.path().display(),
        );
        return;
    }
    let err = resolve_manifest_at_tab(tmp.path()).expect_err("no tools/");
    let msg = err.to_string();
    assert!(msg.contains("tools"), "expected hint about tools/, got: {msg}");
}

use crate::complete::{Shell, completion_script};

#[test]
fn bash_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Bash);
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("complete -F _toolr_complete toolr"));
}

#[test]
fn zsh_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Zsh);
    assert!(script.starts_with("#compdef toolr"));
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("compdef _toolr toolr"));
}

#[test]
fn fish_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Fish);
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("complete -c toolr"));
}

#[test]
fn fish_script_does_not_inject_literal_dash_dash_into_args() {
    // Regression: an earlier version used `set -a args -- $current` on
    // the assumption that `--` was an end-of-options marker for fish's
    // `set` builtin. It is not — fish appends `--` as a literal value,
    // which then short-circuits clap's option parsing on the binary side
    // and drops the trailing in-progress token.
    let script = completion_script(Shell::Fish);
    assert!(
        !script.contains("set -a args --"),
        "fish completion script must not append a literal `--` into args"
    );
}

use crate::complete::install::{
    InstallOptions, InstallOutcome, PriorState, install_path_for, install_script,
};

#[test]
fn install_path_for_bash_uses_xdg_data_home() {
    let tmp = TempDir::new().unwrap();
    let xdg_data = tmp.path().join("share");
    let path = install_path_for(Shell::Bash, Some(&xdg_data), tmp.path()).unwrap();
    assert_eq!(path, xdg_data.join("bash-completion/completions/toolr"));
}

#[test]
fn install_path_for_zsh_uses_home_zfunc() {
    let tmp = TempDir::new().unwrap();
    let path = install_path_for(Shell::Zsh, None, tmp.path()).unwrap();
    assert_eq!(path, tmp.path().join(".zfunc/_toolr"));
}

#[test]
fn install_path_for_fish_uses_xdg_config_home() {
    let tmp = TempDir::new().unwrap();
    let xdg_config = tmp.path().join("config");
    let path = install_path_for(Shell::Fish, Some(&xdg_config), tmp.path()).unwrap();
    assert_eq!(path, xdg_config.join("fish/completions/toolr.fish"));
}

#[test]
fn install_creates_file_when_absent() {
    let tmp = TempDir::new().unwrap();
    let opts = InstallOptions {
        shell: Shell::Bash,
        xdg_data_home: Some(tmp.path().join("data")),
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: false,
        interactive: false,
    };
    let outcome = install_script(&opts).unwrap();
    assert!(matches!(
        outcome,
        InstallOutcome::Wrote {
            prior: PriorState::None,
            ..
        }
    ));
    let target = tmp.path().join("data/bash-completion/completions/toolr");
    assert!(target.exists());
}

#[test]
fn install_refuses_to_overwrite_differing_file_without_force() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("data/bash-completion/completions/toolr");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "# someone else's script\n").unwrap();
    let opts = InstallOptions {
        shell: Shell::Bash,
        xdg_data_home: Some(tmp.path().join("data")),
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: false,
        interactive: false,
    };
    let outcome = install_script(&opts).unwrap();
    assert!(matches!(outcome, InstallOutcome::SkippedNeedsForce { .. }));
    let contents = std::fs::read_to_string(&target).unwrap();
    assert_eq!(contents, "# someone else's script\n");
}

#[test]
fn install_is_idempotent_when_content_matches() {
    let tmp = TempDir::new().unwrap();
    let opts = InstallOptions {
        shell: Shell::Bash,
        xdg_data_home: Some(tmp.path().join("data")),
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: false,
        interactive: false,
    };
    let first = install_script(&opts).unwrap();
    let second = install_script(&opts).unwrap();
    assert!(matches!(
        first,
        InstallOutcome::Wrote {
            prior: PriorState::None,
            ..
        }
    ));
    assert!(matches!(second, InstallOutcome::AlreadyInstalled { .. }));
}

#[test]
fn install_with_force_overwrites_even_when_content_matches() {
    // Regression: a stale binary that ships a buggy completion script
    // would write the same buggy script to disk, then refuse to refresh
    // because the on-disk content matched the (still buggy) payload.
    // `--force` must be honoured literally so users can re-deploy
    // unconditionally.
    let tmp = TempDir::new().unwrap();
    let bootstrap = InstallOptions {
        shell: Shell::Bash,
        xdg_data_home: Some(tmp.path().join("data")),
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: false,
        interactive: false,
    };
    assert!(matches!(
        install_script(&bootstrap).unwrap(),
        InstallOutcome::Wrote {
            prior: PriorState::None,
            ..
        }
    ));
    let forced = InstallOptions {
        shell: Shell::Bash,
        xdg_data_home: Some(tmp.path().join("data")),
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: true,
        interactive: false,
    };
    assert!(matches!(
        install_script(&forced).unwrap(),
        InstallOutcome::Wrote {
            prior: PriorState::Identical,
            ..
        }
    ));
}

#[test]
fn install_with_force_overwrites_existing() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("data/bash-completion/completions/toolr");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "# stale\n").unwrap();
    let opts = InstallOptions {
        shell: Shell::Bash,
        xdg_data_home: Some(tmp.path().join("data")),
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: true,
        interactive: false,
    };
    let outcome = install_script(&opts).unwrap();
    assert!(matches!(
        outcome,
        InstallOutcome::Wrote {
            prior: PriorState::Differed,
            ..
        }
    ));
    let contents = std::fs::read_to_string(&target).unwrap();
    assert!(contents.contains("toolr __complete"));
}

#[test]
fn install_fish_uses_xdg_config_home_override() {
    let tmp = TempDir::new().unwrap();
    let opts = InstallOptions {
        shell: Shell::Fish,
        xdg_data_home: None,
        xdg_config_home: Some(tmp.path().join("config")),
        home: tmp.path().to_path_buf(),
        force: false,
        interactive: false,
    };
    let outcome = install_script(&opts).unwrap();
    assert!(matches!(outcome, InstallOutcome::Wrote { .. }));
    let target = tmp.path().join("config/fish/completions/toolr.fish");
    assert!(target.exists());
    let contents = std::fs::read_to_string(&target).unwrap();
    assert!(contents.contains("complete -c toolr"));
}

#[test]
fn install_zsh_lands_under_home_zfunc() {
    let tmp = TempDir::new().unwrap();
    let opts = InstallOptions {
        shell: Shell::Zsh,
        xdg_data_home: None,
        xdg_config_home: None,
        home: tmp.path().to_path_buf(),
        force: false,
        interactive: false,
    };
    let outcome = install_script(&opts).unwrap();
    assert!(matches!(outcome, InstallOutcome::Wrote { .. }));
    let target = tmp.path().join(".zfunc/_toolr");
    assert!(target.exists());
    let contents = std::fs::read_to_string(&target).unwrap();
    assert!(contents.contains("toolr __complete"));
}

#[test]
fn install_fish_without_xdg_falls_back_to_home_config() {
    // Exercises the `unwrap_or_else(|| home.join(".config"))` arm for
    // the Fish branch of `install_path_for` (and for `install_script`).
    let tmp = TempDir::new().unwrap();
    let path = install_path_for(Shell::Fish, None, tmp.path()).unwrap();
    assert_eq!(path, tmp.path().join(".config/fish/completions/toolr.fish"));
}

#[test]
fn install_bash_without_xdg_falls_back_to_home_local_share() {
    // Same fallback story for Bash.
    let tmp = TempDir::new().unwrap();
    let path = install_path_for(Shell::Bash, None, tmp.path()).unwrap();
    assert_eq!(
        path,
        tmp.path().join(".local/share/bash-completion/completions/toolr")
    );
}
