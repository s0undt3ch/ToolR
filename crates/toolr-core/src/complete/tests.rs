use crate::complete::{Candidate, Completion, complete, serve_completions};
use crate::manifest::{
    Argument, ArgumentKind, Command, Group, Manifest, Origin, PluginWarning, PluginWarningKind,
    SCHEMA_VERSION,
};
use crate::parser::SupportedType;

/// Most existing assertions only care about candidate values, not their
/// descriptions; keep them terse by projecting down to `Vec<String>`.
fn values(candidates: Vec<Candidate>) -> Vec<String> {
    candidates.into_iter().map(|c| c.value).collect()
}

fn fixture() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
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
                    metadata: crate::manifest::ArgMetadata::default(),
                    long_flag: None,
                    allowed_values: vec![],
                }],
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
                    metadata: crate::manifest::ArgMetadata::default(),
                    long_flag: None,
                    allowed_values: vec!["staging".into(), "production".into()],
                }],
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
                    metadata: crate::manifest::ArgMetadata::default(),
                    long_flag: None,
                    allowed_values: vec!["wide".into(), "tall".into()],
                }],
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
        ],
        plugin_warnings: Vec::new(),
        input_records: Default::default(),
    }
}

fn tokens(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn empty_tokens_lists_all_groups() {
    let out = values(serve_completions(&fixture(), &tokens(&[""])));
    assert_eq!(out, vec!["ci".to_string(), "data".to_string()]);
}

#[test]
fn groups_carry_their_title_as_description() {
    let out = serve_completions(&fixture(), &tokens(&[""]));
    assert_eq!(
        out,
        vec![
            Candidate::new("ci", "CI utilities"),
            Candidate::new("data", "Data utilities"),
        ]
    );
}

#[test]
fn commands_carry_their_summary_as_description() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "h"]));
    assert_eq!(out, vec![Candidate::new("hello", "Say hello.")]);
}

#[test]
fn candidate_description_strips_embedded_tabs_and_newlines() {
    // A literal tab/newline would corrupt the one-line-per-candidate wire format.
    let candidate = Candidate::new("x", "line one\nline two\twith a tab\x1b]0;t\x07");
    assert_eq!(candidate.description, "line one line two with a tab ]0;t ");
}

#[test]
fn engine_drops_control_char_values() {
    let mut manifest = fixture();
    let load = manifest
        .commands
        .iter_mut()
        .find(|c| c.name == "load")
        .unwrap();
    load.arguments[0].allowed_values = vec!["ok".into(), "bad\x1b[2J".into()];
    let out = serve_completions(&manifest, &tokens(&["data", "load", ""]));
    assert_eq!(out, vec![Candidate::bare("ok")]);
}

#[test]
fn engine_drops_invisible_format_values() {
    let mut manifest = fixture();
    let load = manifest
        .commands
        .iter_mut()
        .find(|c| c.name == "load")
        .unwrap();
    load.arguments[0].allowed_values =
        vec!["ok".into(), "o\u{200b}k".into(), "\u{202e}kayo".into()];
    let out = serve_completions(&manifest, &tokens(&["data", "load", ""]));
    assert_eq!(out, vec![Candidate::bare("ok")]);
}

#[test]
fn allowed_values_have_no_description() {
    let out = serve_completions(&fixture(), &tokens(&["data", "load", ""]));
    assert_eq!(out, vec![Candidate::bare("tall"), Candidate::bare("wide")]);
}

#[test]
fn group_prefix_filters_groups() {
    let out = values(serve_completions(&fixture(), &tokens(&["c"])));
    assert_eq!(out, vec!["ci".to_string()]);
}

#[test]
fn after_group_lists_its_commands() {
    let out = values(serve_completions(&fixture(), &tokens(&["ci", ""])));
    assert_eq!(out, vec!["deploy".to_string(), "hello".to_string()]);
}

#[test]
fn command_prefix_filters_commands() {
    let out = values(serve_completions(&fixture(), &tokens(&["ci", "h"])));
    assert_eq!(out, vec!["hello".to_string()]);
}

#[test]
fn flag_prefix_lists_argument_flags() {
    // `--help` is offered alongside the command's own flags whenever the
    // user is explicitly probing flags (`--` prefix). clap injects it on
    // every leaf, so the engine reflects that.
    let out = values(serve_completions(
        &fixture(),
        &tokens(&["ci", "hello", "--"]),
    ));
    assert_eq!(out, vec!["--help".to_string(), "--name".to_string()]);
}

#[test]
fn flag_value_completes_to_allowed_values() {
    let out = values(serve_completions(
        &fixture(),
        &tokens(&["ci", "deploy", "--env", ""]),
    ));
    assert_eq!(out, vec!["production".to_string(), "staging".to_string()]);
}

#[test]
fn flag_value_partial_filters_allowed_values() {
    let out = values(serve_completions(
        &fixture(),
        &tokens(&["ci", "deploy", "--env", "s"]),
    ));
    assert_eq!(out, vec!["staging".to_string()]);
}

#[test]
fn positional_value_completes_to_allowed_values() {
    let out = values(serve_completions(
        &fixture(),
        &tokens(&["data", "load", ""]),
    ));
    assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
}

#[test]
fn unknown_group_returns_no_completions() {
    let out = values(serve_completions(&fixture(), &tokens(&["nope", ""])));
    assert!(out.is_empty());
}

#[test]
fn flag_without_allowed_values_returns_empty() {
    // `--name` has no allowed_values → shell falls back to filename completion.
    let out = values(serve_completions(
        &fixture(),
        &tokens(&["ci", "hello", "--name", ""]),
    ));
    assert!(out.is_empty());
}

fn nested_fixture() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
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
                origin: Origin::Static,
                dispatched_from: None,
                is_dispatcher: false,
            },
        ],
        plugin_warnings: Vec::new(),
        input_records: Default::default(),
    }
}

#[test]
fn top_level_completion_lists_only_top_level_groups() {
    // `docker.image` is nested under `docker` — it must not appear at
    // the top level, otherwise the shell would offer it as a sibling
    // of `docker`.
    let out = values(serve_completions(&nested_fixture(), &tokens(&[""])));
    assert_eq!(out, vec!["docker".to_string()]);
}

#[test]
fn nested_group_completion_lists_child_groups() {
    let out = values(serve_completions(
        &nested_fixture(),
        &tokens(&["docker", ""]),
    ));
    assert_eq!(out, vec!["container".to_string(), "image".to_string()]);
}

#[test]
fn nested_command_completion_traverses_full_path() {
    let out = values(serve_completions(
        &nested_fixture(),
        &tokens(&["docker", "image", ""]),
    ));
    assert_eq!(out, vec!["build".to_string()]);
}

#[test]
fn nested_command_completion_filters_by_prefix() {
    let out = values(serve_completions(
        &nested_fixture(),
        &tokens(&["docker", "container", "st"]),
    ));
    assert_eq!(out, vec!["start".to_string()]);
}

#[test]
fn root_flag_prefix_offers_help() {
    // Regression: `toolr --<TAB>` at the very top used to return an
    // empty candidate list because the classifier landed on
    // `Slot::Group` and `groups()` filtered out anything that didn't
    // start with the literal prefix `--`. Group nodes carry no schema,
    // so the engine emits `--help` directly.
    let out = values(serve_completions(&fixture(), &tokens(&["--"])));
    assert_eq!(out, vec!["--help".to_string()]);
}

#[test]
fn nested_group_flag_prefix_offers_help() {
    // Regression: `toolr docker --<TAB>` (sitting on a group, not a
    // leaf) also dropped to nothing. `--help` must still surface.
    let out = values(serve_completions(
        &nested_fixture(),
        &tokens(&["docker", "--"]),
    ));
    assert_eq!(out, vec!["--help".to_string()]);
}

#[test]
fn root_single_dash_prefix_offers_help() {
    // `-` is a valid flag prefix even though no short alias is emitted
    // by this engine. It must still produce `--help`.
    let out = values(serve_completions(&fixture(), &tokens(&["-"])));
    assert_eq!(out, vec!["--help".to_string()]);
}

#[test]
fn group_node_empty_prefix_does_not_offer_help() {
    // `toolr <TAB>` with an empty prefix must keep listing top-level
    // groups, not inject `--help`. Help is for explicit flag probes.
    let out = values(serve_completions(&fixture(), &tokens(&[""])));
    assert_eq!(out, vec!["ci".to_string(), "data".to_string()]);
}

#[test]
fn leaf_help_prefix_filters_to_help_only() {
    // `toolr ci hello --he<TAB>` should narrow to `--help` even though
    // the leaf has no `help` argument of its own.
    let out = values(serve_completions(
        &fixture(),
        &tokens(&["ci", "hello", "--he"]),
    ));
    assert_eq!(out, vec!["--help".to_string()]);
}

fn dispatcher_fixture() -> Manifest {
    // Mirrors the dashtastic shape: `jenkins job` is a dispatcher with
    // its own flags plus grafted children at `group = "jenkins.job"`.
    // Names are the Python identifiers the parser writes (`log_level`,
    // not `log-level`); a kebab-case stand-in here once hid a lookup bug.
    let str_arg = |name: &str, kind, default: Option<&str>, values: &[&str]| Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: default.map(str::to_string),
        type_annotation: Some("str".into()),
        resolved_type: None,
        metadata: crate::manifest::ArgMetadata::default(),
        long_flag: None,
        allowed_values: values.iter().map(|s| (*s).to_string()).collect(),
    };
    let aliased = |mut arg: Argument, aliases: &[&str]| {
        arg.metadata.aliases = aliases.iter().map(|s| (*s).to_string()).collect();
        arg
    };
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
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
                    str_arg("log_level", ArgumentKind::Optional, Some("INFO"), &[]),
                    aliased(
                        str_arg("reference", ArgumentKind::Optional, Some("latest"), &[]),
                        &["--ref"],
                    ),
                    str_arg("on_demand", ArgumentKind::Flag, None, &[]),
                    str_arg("dry_run", ArgumentKind::Flag, None, &[]),
                    str_arg("no_follow", ArgumentKind::Flag, None, &[]),
                ],
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
                arguments: vec![
                    aliased(
                        str_arg("batch_size", ArgumentKind::Optional, None, &[]),
                        &["-b"],
                    ),
                    str_arg("verbose", ArgumentKind::Count, None, &[]),
                    str_arg("scope", ArgumentKind::Positional, None, &["all", "stale"]),
                ],
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
                origin: Origin::Static,
                dispatched_from: Some("argparse:django".into()),
                is_dispatcher: false,
            },
        ],
        plugin_warnings: Vec::new(),
        input_records: Default::default(),
    }
}

#[test]
fn dispatcher_completion_lists_grafted_children() {
    // Regression for the dashtastic case: `toolr jenkins job <TAB>`
    // returned nothing even though `--help` showed the children.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", ""]),
    ));
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_completion_filters_children_by_prefix() {
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "delete_"]),
    ));
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_flag_prefix_lists_dispatcher_flags_not_children() {
    // `toolr jenkins job --<TAB>` must offer the dispatcher's own
    // flags, not the grafted child names. `--help` is added by the
    // engine because clap injects it on every subcommand.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--"]),
    ));
    assert_eq!(
        out,
        vec![
            "--cpu".to_string(),
            "--dry-run".to_string(),
            "--help".to_string(),
            "--log-level".to_string(),
            "--no-follow".to_string(),
            "--on-demand".to_string(),
            "--reference".to_string(),
        ]
    );
}

#[test]
fn dispatcher_completion_offers_children_after_parent_flag() {
    // `toolr jenkins job --dry-run <TAB>` should still suggest children
    // — the parent flag was consumed but no child has been picked yet.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--dry-run", ""]),
    ));
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_recurses_into_child_args_once_child_is_chosen() {
    // `toolr jenkins job delete_orphans <TAB>` — the child has a
    // positional with allowed values; surface them.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "delete_orphans", ""]),
    ));
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

#[test]
fn dispatcher_recurses_through_parent_flag_value_pair() {
    // `toolr jenkins job --cpu 200m delete_orphans <TAB>` must skip
    // both `--cpu` and its value when locating the chosen child.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--cpu", "200m", "delete_orphans", ""]),
    ));
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

#[test]
fn dispatcher_flag_completes_after_hyphenated_flag_value_pair() {
    // Reported shape: `toolr jenkins job --log-level debug --on-demand
    // --no-fol<TAB>` returned nothing. `--log-level` didn't match the
    // manifest's `log_level`, so `debug` was taken for the child name.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&[
            "jenkins",
            "job",
            "--log-level",
            "debug",
            "--on-demand",
            "--no-fol",
        ]),
    ));
    assert_eq!(out, vec!["--no-follow".to_string()]);
}

#[test]
fn dispatcher_hyphenated_flag_value_slot_does_not_offer_children() {
    // `--log-level <TAB>` is the flag's value slot, not the child slot.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--log-level", ""]),
    ));
    assert!(out.is_empty(), "expected no candidates, got {out:?}");
}

#[test]
fn dispatcher_offers_children_after_hyphenated_flag_value_pair() {
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--log-level", "debug", ""]),
    ));
    assert_eq!(
        out,
        vec!["delete_orphans".to_string(), "delete_stale".to_string()]
    );
}

#[test]
fn dispatcher_flag_completes_after_long_alias_value_pair() {
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "--ref", "abc123", "--no-fol"]),
    ));
    assert_eq!(out, vec!["--no-follow".to_string()]);
}

#[test]
fn dispatcher_recurses_through_hyphenated_flag_value_pair() {
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&[
            "jenkins",
            "job",
            "--log-level",
            "debug",
            "delete_orphans",
            "",
        ]),
    ));
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

#[test]
fn leaf_positional_completes_after_hyphenated_flag_value_pair() {
    // In a leaf, the misread value took the positional's slot, so the
    // positional's candidates never showed.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "delete_orphans", "--batch-size", "10", ""]),
    ));
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

#[test]
fn leaf_positional_completes_after_short_alias_value_pair() {
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "delete_orphans", "-b", "10", ""]),
    ));
    assert_eq!(out, vec!["all".to_string(), "stale".to_string()]);
}

#[test]
fn leaf_positional_completes_after_count_flag() {
    // A count flag takes no value; the next token is the positional.
    let out = values(serve_completions(
        &dispatcher_fixture(),
        &tokens(&["jenkins", "job", "delete_orphans", "--verbose", ""]),
    ));
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
        metadata: crate::manifest::ArgMetadata::default(),
        long_flag: Some(format!("--{}", name)),
        allowed_values: vec![],
    };
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
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
                origin: Origin::Static,
                dispatched_from: Some("argparse:django".into()),
                is_dispatcher: false,
            },
        ],
        plugin_warnings: Vec::new(),
        input_records: Default::default(),
    }
}

#[test]
fn child_with_only_optional_flags_offers_flags_on_empty_prefix() {
    // Regression for dashtastic `toolr jenkins job delete_companyuser <TAB>`:
    // the child has no positional schema, so the engine used to return
    // `Slot::None` and the shell saw nothing. Now we fall back to the
    // child's flag names.
    let out = values(serve_completions(
        &flags_only_child_fixture(),
        &tokens(&["jenkins", "job", "delete_companyuser", ""]),
    ));
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
    let out = values(serve_completions(
        &flags_only_child_fixture(),
        &tokens(&["jenkins", "job", "delete_companyuser", "--user"]),
    ));
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
    let out = values(serve_completions(&fixture(), &tokens(&["ci", "hello", ""])));
    assert_eq!(out, vec!["--name".to_string()]);
}

use crate::complete::{ResolvedManifest, resolve_manifest_at_tab};
use crate::manifest::write_manifest;
use crate::manifest_build::empty_third_party_hash;
use tempfile::TempDir;

fn make_tree(py_files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("tools")).unwrap();
    std::fs::write(tmp.path().join("tools").join("pyproject.toml"), "").unwrap();
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
        "group = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Do the thing.\"\"\"\n",
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
        "group = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Do the thing.\"\"\"\n",
    )]);
    // Build once and write to disk.  Stamp the third_party_hash with the
    // empty-venv sentinel so `freshness::compare` (venv_dir=None) sees a
    // matching hash and returns Fresh.
    let mut built = crate::parser::build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    built.third_party_hash = empty_third_party_hash();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &built).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(resolved.from_cache);
    assert_eq!(resolved.manifest, built);
}

/// #656: a corrupt manifest at TAB is rebuilt in memory, without a panic
/// path, and left as it is for the next non-TAB run to report and repair.
#[test]
fn corrupt_manifest_at_tab_rebuilds_in_memory() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Do the thing.\"\"\"\n",
    )]);
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    std::fs::write(&manifest_path, "{\"schema_ver").unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache);
    assert!(resolved.manifest.groups.iter().any(|g| g.name == "ci"));
    assert_eq!(
        std::fs::read_to_string(&manifest_path).unwrap(),
        "{\"schema_ver"
    );
}

#[test]
fn re_parses_when_cached_hash_is_stale() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Do the thing.\"\"\"\n",
    )]);
    // Write a stale manifest with a bogus hash.
    let mut stale = crate::parser::build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    stale.static_hash = "deliberately-stale".into();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &stale).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache, "stale hash should trigger reparse");
    assert_ne!(resolved.manifest.static_hash, "deliberately-stale");
}

#[test]
fn preserves_third_party_entries_from_cache_when_reparsing() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Do the thing.\"\"\"\n",
    )]);
    // Seed a manifest with a fake third-party command and a stale
    // static_hash so the re-parse path runs.
    let mut seeded = crate::parser::build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    seeded.static_hash = "stale".into();
    seeded.commands.push(crate::manifest::Command {
        name: "from-plugin".into(),
        group: "plugin-group".into(),
        module: "third_party_pkg".into(),
        function: "from_plugin".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::ThirdParty,
        dispatched_from: None,
        is_dispatcher: false,
    });
    seeded.groups.push(crate::manifest::Group {
        name: "plugin-group".into(),
        title: "Plugin group".into(),
        description: String::new(),
        parent: None,
        origin: Origin::ThirdParty,
    });
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &seeded).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache);
    // Static-layer entry survives.
    assert!(resolved.manifest.commands.iter().any(|c| c.name == "hello"));
    // Third-party entry from the cache is preserved through the reparse.
    assert!(
        resolved
            .manifest
            .commands
            .iter()
            .any(|c| c.name == "from-plugin" && matches!(c.origin, Origin::ThirdParty))
    );
    assert!(
        resolved
            .manifest
            .groups
            .iter()
            .any(|g| g.name == "plugin-group" && matches!(g.origin, Origin::ThirdParty))
    );
}

/// A pyproject mid-edit (invalid TOML outside the argparse table) or a
/// typo'd `scan_paths` glob fails the rebuild; TAB serves the cache, and
/// without one the error reaches `run_complete`.
#[test]
fn failed_rebuild_serves_the_cache() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Do the thing.\"\"\"\n",
    )]);
    let tools = tmp.path().join("tools");
    let mut built = crate::parser::build_static_manifest(&tools)
        .unwrap()
        .manifest;
    built.third_party_hash = empty_third_party_hash();
    let manifest_path = tools.join(".toolr-manifest.json");
    for pyproject in [
        "[project]\nname = \"tools\"\ndependencies = [\"django\"\n",
        "[tool.toolr.argparse.django]\nscan_paths = [\"apps/[/x.py\"]\n",
    ] {
        std::fs::write(tools.join("pyproject.toml"), pyproject).unwrap();
        write_manifest(&manifest_path, &built).unwrap();
        let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
        assert!(resolved.from_cache, "{pyproject}");
        assert_eq!(resolved.manifest, built, "{pyproject}");

        std::fs::remove_file(&manifest_path).unwrap();
        assert!(resolve_manifest_at_tab(tmp.path()).is_err(), "{pyproject}");
    }
}

/// F-17: a symlinked module fails the TAB-time resolve whether the walk is
/// reached through the rebuild (no cache) or the freshness hash (cache);
/// `run_complete` turns that into built-ins only.
#[cfg(unix)]
#[test]
fn symlinked_module_fails_resolve_with_and_without_a_cache() {
    let tmp = make_tree(&[(
        "demo.py",
        "group = command_group(\"demo\", \"Demo\", \"Demo.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n",
    )]);
    let tools = tmp.path().join("tools");
    let manifest = resolve_manifest_at_tab(tmp.path()).unwrap().manifest;
    write_manifest(&tools.join(".toolr-manifest.json"), &manifest).unwrap();

    std::fs::create_dir(tmp.path().join("shared")).unwrap();
    std::fs::write(tmp.path().join("shared/linked.py"), "").unwrap();
    std::os::unix::fs::symlink("../shared/linked.py", tools.join("linked.py")).unwrap();

    let with_cache = format!("{:#}", resolve_manifest_at_tab(tmp.path()).unwrap_err());
    std::fs::remove_file(tools.join(".toolr-manifest.json")).unwrap();
    let without_cache = format!("{:#}", resolve_manifest_at_tab(tmp.path()).unwrap_err());
    for msg in [with_cache, without_cache] {
        assert!(
            msg.contains("symlinked modules are not supported (1):\n  - tools/linked.py\n"),
            "{msg}"
        );
    }
}

#[test]
fn errors_when_no_tools_dir_exists() {
    let tmp = TempDir::new().unwrap();
    // GHA Windows runners ship with `C:\tools\` populated, which makes
    // the discovery walk succeed when it crawls past the drive root.
    // Same hazard on any host with `/tools`. Skip when the host
    // violates the test precondition.
    let mut walker = tmp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| tmp.path().to_path_buf());
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
    assert!(
        msg.contains("tools"),
        "expected hint about tools/, got: {msg}"
    );
}

use crate::complete::{Shell, completion_script};

#[test]
fn bash_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Bash);
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("complete -F _toolr_complete toolr"));
}

#[test]
fn bash_script_strips_description_before_compgen() {
    // Regression: `__complete` now prints `value<TAB>description` per line.
    // bash has no per-candidate description support, so the script must
    // split each line on the tab before handing values to `compgen -W` —
    // otherwise the description text (and the tab itself) lands on the
    // command line as part of the inserted candidate.
    let script = completion_script(Shell::Bash);
    assert!(script.contains("IFS=$'\\t'"));
    assert!(!script.contains(r#"compgen -W "$candidates""#));
}

#[test]
fn zsh_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Zsh);
    assert!(script.starts_with("#compdef toolr"));
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("compdef _toolr toolr"));
    assert!(script.contains("loadautofunc"));
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
        tmp.path()
            .join(".local/share/bash-completion/completions/toolr")
    );
}

fn manifest_with_leaf_args(arguments: Vec<Argument>) -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
        groups: vec![Group {
            name: "cmd".into(),
            title: String::new(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }],
        commands: vec![Command {
            name: "run".into(),
            group: "cmd".into(),
            module: "tools.cmd".into(),
            function: "run".into(),
            summary: String::new(),
            description: String::new(),
            arguments,
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }],
        plugin_warnings: Vec::new(),
        input_records: Default::default(),
    }
}

fn empty_arg(name: &str, kind: ArgumentKind) -> Argument {
    Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: None,
        allowed_values: vec![],
        metadata: Default::default(),
        long_flag: None,
    }
}

#[test]
fn optional_positional_is_a_completable_positional_slot() {
    // `allowed_values` only surface via `Slot::Positional` — if this arg
    // were mistakenly excluded from the positional-slot filter, the
    // no-flags fallback would return no completions instead.
    let mut arg = empty_arg("maybe", ArgumentKind::OptionalPositional);
    arg.allowed_values = vec!["tall".into(), "wide".into()];
    let manifest = manifest_with_leaf_args(vec![arg]);
    let out = values(serve_completions(&manifest, &tokens(&["cmd", "run", ""])));
    assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
}

#[test]
fn positional_fixed_arity_is_a_completable_positional_slot() {
    let mut arg = empty_arg("pair", ArgumentKind::FixedArity);
    arg.allowed_values = vec!["tall".into(), "wide".into()];
    let manifest = manifest_with_leaf_args(vec![arg]);
    let out = values(serve_completions(&manifest, &tokens(&["cmd", "run", ""])));
    assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
}

#[test]
fn keyword_fixed_arity_offers_flag_completion() {
    let mut arg = empty_arg("pair", ArgumentKind::FixedArity);
    arg.long_flag = Some("--pair".to_string());
    let manifest = manifest_with_leaf_args(vec![arg]);
    let out = values(serve_completions(&manifest, &tokens(&["cmd", "run", ""])));
    assert_eq!(out, vec!["--pair".to_string()]);
}

#[test]
fn preserve_non_static_keeps_same_leaf_groups_with_different_parents() {
    let group = |parent: &str, origin| Group {
        name: "image".into(),
        title: "Image".into(),
        description: String::new(),
        parent: Some(parent.into()),
        origin,
    };
    let mut fresh = fixture();
    fresh.groups = vec![group("ci", Origin::Static)];
    let mut cached = fixture();
    cached.groups = vec![group("docker", Origin::ThirdParty)];
    super::freshness::preserve_non_static_entries(&mut fresh, cached);
    let paths: Vec<String> = fresh.groups.iter().map(Group::full_path).collect();
    assert_eq!(paths, ["ci.image", "docker.image"]);
}

#[test]
fn preserve_non_static_carries_cached_plugin_warnings() {
    let warning = PluginWarning {
        package: "demo".into(),
        path: "site-packages/demo/toolr-manifest.json".into(),
        kind: PluginWarningKind::Shadowed,
        message: "tools/ci.py defines ci lint, hiding the one from demo".into(),
    };
    let mut fresh = fixture();
    let mut cached = fixture();
    cached.plugin_warnings = vec![warning.clone()];
    super::freshness::preserve_non_static_entries(&mut fresh, cached);
    assert_eq!(fresh.plugin_warnings, [warning]);
}

// --- `complete`: path-typed slots hand off to the shell ---

fn typed_arg(name: &str, kind: ArgumentKind, ty: SupportedType, values: &[&str]) -> Argument {
    Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: Some(ty),
        metadata: crate::manifest::ArgMetadata::default(),
        long_flag: None,
        allowed_values: values.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn leaf(name: &str, group: &str, arguments: Vec<Argument>, is_dispatcher: bool) -> Command {
    Command {
        name: name.into(),
        group: group.into(),
        module: "tools.ci".into(),
        function: name.into(),
        summary: String::new(),
        description: String::new(),
        arguments,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher,
    }
}

fn path_fixture() -> Manifest {
    use ArgumentKind::{Optional, Positional, VarPositional};
    let boxed = |t| Box::new(t);
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
        groups: vec![Group {
            name: "ci".into(),
            title: String::new(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }],
        commands: vec![
            leaf(
                "build",
                "ci",
                vec![
                    typed_arg(
                        "cfg",
                        Optional,
                        SupportedType::Optional(boxed(SupportedType::FilePath)),
                        &[],
                    ),
                    typed_arg("out", Optional, SupportedType::DirectoryPath, &[]),
                    typed_arg("exe", Optional, SupportedType::ExecutablePath, &[]),
                    typed_arg("pick", Optional, SupportedType::Path, &["x", "y"]),
                    typed_arg(
                        "mode",
                        Optional,
                        SupportedType::Literal(vec!["a".into(), "b".into()]),
                        &["a", "b"],
                    ),
                    typed_arg("src", Positional, SupportedType::Path, &[]),
                    typed_arg(
                        "rest",
                        VarPositional,
                        SupportedType::List(boxed(SupportedType::Path)),
                        &[],
                    ),
                ],
                false,
            ),
            leaf("job", "ci", vec![], true),
            leaf(
                "run",
                "ci.job",
                vec![typed_arg("cfg", Optional, SupportedType::FilePath, &[])],
                false,
            ),
        ],
        plugin_warnings: vec![],
        input_records: Default::default(),
    }
}

fn complete_values(c: Completion) -> Vec<String> {
    match c {
        Completion::Values(v) => values(v),
        other => panic!("expected values, got {other:?}"),
    }
}

#[test]
fn complete_flag_values_by_path_type() {
    let m = path_fixture();
    for (flag, want) in [
        ("--cfg", Completion::Paths),
        ("--out", Completion::Directories),
        ("--exe", Completion::Paths),
    ] {
        assert_eq!(
            complete(&m, &tokens(&["ci", "build", flag, ""])),
            want,
            "{flag}"
        );
    }
}

#[test]
fn complete_positional_and_variadic_path_slots() {
    let m = path_fixture();
    assert_eq!(
        complete(&m, &tokens(&["ci", "build", ""])),
        Completion::Paths
    );
    assert_eq!(
        complete(&m, &tokens(&["ci", "build", "a", "b", ""])),
        Completion::Paths
    );
}

#[test]
fn complete_literal_still_offers_values() {
    let m = path_fixture();
    let got = complete_values(complete(&m, &tokens(&["ci", "build", "--mode", ""])));
    assert_eq!(got, vec!["a", "b"]);
}

#[test]
fn complete_flag_names_after_a_path_argument() {
    let m = path_fixture();
    let got = complete_values(complete(
        &m,
        &tokens(&["ci", "build", "--cfg", "x.toml", "--"]),
    ));
    assert!(got.contains(&"--cfg".to_string()), "{got:?}");
    assert!(got.contains(&"--out".to_string()), "{got:?}");
}

#[test]
fn complete_allowed_values_win_over_a_path_type() {
    let m = path_fixture();
    let got = complete_values(complete(&m, &tokens(&["ci", "build", "--pick", ""])));
    assert_eq!(got, vec!["x", "y"]);
}

#[test]
fn complete_dispatcher_child_path_flag() {
    let m = path_fixture();
    assert_eq!(
        complete(&m, &tokens(&["ci", "job", "run", "--cfg", ""])),
        Completion::Paths
    );
}

#[test]
fn complete_matches_serve_completions_outside_path_slots() {
    let m = path_fixture();
    for words in [
        &["ci", ""][..],
        &["ci", "b"],
        &["nope", ""],
        &["ci", "job", ""],
    ] {
        let t = tokens(words);
        assert_eq!(
            complete(&m, &t),
            Completion::Values(serve_completions(&m, &t)),
            "{words:?}"
        );
    }
}

mod bash_completion_probe {
    use std::ffi::OsString;
    use std::path::Path;

    use tempfile::TempDir;

    use crate::complete::install::{
        BASH_COMPLETION_SYSROOT_PATHS, bash_completion_candidates, bash_completion_installed,
        shell_quote,
    };

    fn plant(root: &Path, rel: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "# loader\n").unwrap();
    }

    #[test]
    fn empty_sysroot_reads_as_not_installed() {
        let tmp = TempDir::new().unwrap();
        assert!(!bash_completion_installed(tmp.path(), None, None));
    }

    #[test]
    fn each_well_known_loader_is_detected() {
        for rel in BASH_COMPLETION_SYSROOT_PATHS {
            let tmp = TempDir::new().unwrap();
            plant(tmp.path(), rel);
            assert!(bash_completion_installed(tmp.path(), None, None), "{rel}");
        }
    }

    #[test]
    fn bash_completion_1x_loaders_are_not_detected() {
        // 1.x never reads ~/.local/share/bash-completion/completions/.
        for rel in [
            "usr/local/etc/bash_completion",
            "opt/homebrew/etc/bash_completion",
            // Homebrew's 1.x formula installs the profile.d loader as well.
            "usr/local/etc/profile.d/bash_completion.sh",
            "opt/homebrew/etc/profile.d/bash_completion.sh",
            "home/linuxbrew/.linuxbrew/etc/profile.d/bash_completion.sh",
            "opt/local/etc/profile.d/bash_completion.sh",
        ] {
            let tmp = TempDir::new().unwrap();
            plant(tmp.path(), rel);
            assert!(!bash_completion_installed(tmp.path(), None, None), "{rel}");
        }
    }

    #[test]
    fn homebrew_prefix_profile_d_loader_is_not_detected() {
        let sysroot = TempDir::new().unwrap();
        let prefix = TempDir::new().unwrap();
        plant(prefix.path(), "etc/profile.d/bash_completion.sh");
        assert!(!bash_completion_installed(
            sysroot.path(),
            None,
            Some(prefix.path())
        ));
    }

    #[test]
    fn shell_quote_leaves_safe_paths_bare() {
        assert_eq!(shell_quote("/a/b-c_d.e/toolr"), "/a/b-c_d.e/toolr");
    }

    #[test]
    fn shell_quote_single_quotes_unsafe_paths() {
        assert_eq!(shell_quote("/My Data/toolr"), "'/My Data/toolr'");
        assert_eq!(shell_quote("/a$b/toolr"), "'/a$b/toolr'");
        assert_eq!(shell_quote("/it's/toolr"), r"'/it'\''s/toolr'");
        assert_eq!(shell_quote(r"C:\Users\u\toolr"), r"'C:\Users\u\toolr'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn completions_directory_alone_is_not_the_loader() {
        let tmp = TempDir::new().unwrap();
        plant(
            tmp.path(),
            "opt/homebrew/share/bash-completion/completions/brew",
        );
        assert!(!bash_completion_installed(tmp.path(), None, None));
    }

    #[test]
    fn loader_path_that_is_a_directory_is_not_detected() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("etc/bash_completion")).unwrap();
        assert!(!bash_completion_installed(tmp.path(), None, None));
    }

    #[test]
    fn xdg_data_dirs_entries_are_probed_and_empty_ones_skipped() {
        let sysroot = TempDir::new().unwrap();
        let data = TempDir::new().unwrap();
        let first = data.path().join("first");
        let second = data.path().join("second");
        let dirs =
            std::env::join_paths([first.as_path(), Path::new(""), second.as_path()]).unwrap();

        let candidates = bash_completion_candidates(sysroot.path(), Some(&dirs), None);
        assert!(candidates.contains(&first.join("bash-completion/bash_completion")));
        assert!(candidates.contains(&second.join("bash-completion/bash_completion")));
        assert!(
            !candidates.contains(&Path::new("bash-completion/bash_completion").to_path_buf()),
            "an empty XDG_DATA_DIRS entry must not probe a relative path: {candidates:?}"
        );
        assert!(!bash_completion_installed(
            sysroot.path(),
            Some(&dirs),
            None
        ));

        plant(&second, "bash-completion/bash_completion");
        assert!(bash_completion_installed(sysroot.path(), Some(&dirs), None));
    }

    #[test]
    fn empty_xdg_data_dirs_adds_nothing() {
        let tmp = TempDir::new().unwrap();
        let empty = OsString::new();
        assert_eq!(
            bash_completion_candidates(tmp.path(), Some(&empty), None),
            bash_completion_candidates(tmp.path(), None, None),
        );
    }

    #[test]
    fn homebrew_prefix_share_loader_is_probed() {
        let sysroot = TempDir::new().unwrap();
        let prefix = TempDir::new().unwrap();
        assert!(!bash_completion_installed(
            sysroot.path(),
            None,
            Some(prefix.path())
        ));
        plant(prefix.path(), "share/bash-completion/bash_completion");
        assert!(bash_completion_installed(
            sysroot.path(),
            None,
            Some(prefix.path())
        ));
    }
}

// --- `--name=partial`: the attached value of `--name` ---

fn attached_fixture() -> Manifest {
    use ArgumentKind::{Flag, Optional, Positional};
    let mut mode = typed_arg(
        "mode",
        Optional,
        SupportedType::Literal(vec!["alpha".into(), "abc".into(), "x=~root".into()]),
        &["alpha", "abc", "x=~root"],
    );
    mode.metadata.aliases = vec!["--style".into()];
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
        groups: vec![Group {
            name: "ci".into(),
            title: String::new(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }],
        commands: vec![
            leaf(
                "lint",
                "ci",
                vec![
                    mode,
                    typed_arg(
                        "cfg",
                        Optional,
                        SupportedType::Optional(Box::new(SupportedType::FilePath)),
                        &[],
                    ),
                    typed_arg("out", Optional, SupportedType::DirectoryPath, &[]),
                    typed_arg("verbose", Flag, SupportedType::Bool, &[]),
                    typed_arg(
                        "shape",
                        Positional,
                        SupportedType::Str,
                        &["wide", "tall", "k=v"],
                    ),
                ],
                false,
            ),
            leaf(
                "job",
                "ci",
                vec![typed_arg(
                    "level",
                    Optional,
                    SupportedType::Str,
                    &["debug", "info"],
                )],
                true,
            ),
            leaf(
                "run",
                "ci.job",
                vec![typed_arg("cfg", Optional, SupportedType::FilePath, &[])],
                false,
            ),
        ],
        plugin_warnings: vec![],
        input_records: Default::default(),
    }
}

#[test]
fn attached_value_completes_the_flags_choices_bare() {
    let m = attached_fixture();
    for (word, want) in [
        ("--mode=", &["abc", "alpha", "x=~root"][..]),
        ("--mode=a", &["abc", "alpha"]),
        ("--mode=x=", &["x=~root"]),
        ("--style=a", &["abc", "alpha"]),
        ("--mode=z", &[]),
    ] {
        let got = values(serve_completions(&m, &tokens(&["ci", "lint", word])));
        assert_eq!(got, want, "{word}");
    }
}

#[test]
fn attached_value_of_a_boolean_or_unknown_flag_offers_nothing() {
    let m = attached_fixture();
    for word in ["--verbose=", "--nope=", "--=", "--=x"] {
        let got = serve_completions(&m, &tokens(&["ci", "lint", word]));
        assert!(got.is_empty(), "{word}: {got:?}");
    }
}

#[test]
fn attached_value_hands_path_slots_to_the_shell() {
    let m = attached_fixture();
    for (word, want) in [
        ("--cfg=", Completion::Paths),
        ("--cfg=files/", Completion::Paths),
        ("--out=", Completion::Directories),
    ] {
        assert_eq!(complete(&m, &tokens(&["ci", "lint", word])), want, "{word}");
    }
    let got = complete_values(complete(&m, &tokens(&["ci", "lint", "--mode="])));
    assert_eq!(got, vec!["abc", "alpha", "x=~root"]);
}

#[test]
fn attached_value_on_a_dispatcher_and_its_child() {
    let m = attached_fixture();
    let got = values(serve_completions(&m, &tokens(&["ci", "job", "--level=i"])));
    assert_eq!(got, vec!["info"]);
    assert_eq!(
        complete(&m, &tokens(&["ci", "job", "run", "--cfg="])),
        Completion::Paths
    );
    // A committed `--level=info` is one token, so the child name still completes.
    let got = values(serve_completions(
        &m,
        &tokens(&["ci", "job", "--level=info", "r"]),
    ));
    assert_eq!(got, vec!["run"]);
}

#[test]
fn committed_attached_value_is_one_token_for_positionals() {
    let m = attached_fixture();
    let got = values(serve_completions(
        &m,
        &tokens(&["ci", "lint", "--mode=alpha", ""]),
    ));
    assert_eq!(got, vec!["k=v", "tall", "wide"]);
}

#[test]
fn positional_word_holding_an_equals_is_not_an_attached_value() {
    let m = attached_fixture();
    let got = values(serve_completions(&m, &tokens(&["ci", "lint", "k="])));
    assert_eq!(got, vec!["k=v"]);
}

// --- a committed `--` ends option parsing; dash-leading choices ---

fn dash_fixture() -> Manifest {
    use ArgumentKind::{Flag, Optional, Positional, VarPositional};
    let mut m = attached_fixture();
    m.commands = vec![
        // `--mode` carries every dash shape (`--dash`, `-x`, bare `-`, numeric `-1`);
        // `target` has a `--dir` choice only a post-`--` positional may offer.
        leaf(
            "lint",
            "ci",
            vec![
                typed_arg(
                    "mode",
                    Optional,
                    SupportedType::Str,
                    &["fast", "full", "--dash", "-x", "-", "-1"],
                ),
                typed_arg("verbose", Flag, SupportedType::Bool, &[]),
                typed_arg(
                    "target",
                    Positional,
                    SupportedType::Str,
                    &["alpha", "beta", "--dir"],
                ),
            ],
            false,
        ),
        // A positional whose choices include a bare `-` and a `--n`.
        leaf(
            "pos",
            "ci",
            vec![typed_arg(
                "p",
                Positional,
                SupportedType::Str,
                &["a", "-", "--n"],
            )],
            false,
        ),
        // A variadic positional keeps completing after `--`.
        leaf(
            "cat",
            "ci",
            vec![typed_arg(
                "files",
                VarPositional,
                SupportedType::Str,
                &["one", "two"],
            )],
            false,
        ),
        // A path positional hands the slot to the shell after `--`.
        leaf(
            "open",
            "ci",
            vec![typed_arg("path", Positional, SupportedType::FilePath, &[])],
            false,
        ),
        // A dispatcher and its child: clap rejects a `--` before the child name.
        leaf(
            "job",
            "ci",
            vec![typed_arg(
                "level",
                Optional,
                SupportedType::Str,
                &["debug", "info"],
            )],
            true,
        ),
        leaf(
            "run",
            "ci.job",
            vec![typed_arg("target", Positional, SupportedType::Str, &["x"])],
            false,
        ),
    ];
    m
}

fn dash_values(words: &[&str]) -> Vec<String> {
    values(serve_completions(&dash_fixture(), &tokens(words)))
}

/// Every case is a line clap parses as the table says; re-verify one with
/// `TOOLR_TRUST_ALL=1 toolr <line>` against a project holding the same command.
#[test]
fn double_dash_ends_option_parsing() {
    let all = &["--dir", "alpha", "beta"][..];
    for (words, want) in [
        (&["ci", "lint", "--", "--"][..], &["--dir"][..]),
        (&["ci", "lint", "--", ""], all),
        (&["ci", "lint", "--mode", "fast", "--", ""], all),
        (&["ci", "lint", "--mode=fast", "--", ""], all),
        (&["ci", "cat", "--", "one", ""], &["one", "two"]),
        (&["ci", "job", "run", "--", ""], &["x"]),
    ] {
        assert_eq!(dash_values(words), want, "{words:?}");
    }
}

#[test]
fn completes_nothing_where_clap_errors_or_no_slot_is_left() {
    for words in [
        // Flag-looking words after `--` are positionals, so no name or value slot.
        &["ci", "lint", "--", "--m"][..],
        &["ci", "lint", "--", "--mode="],
        &["ci", "lint", "--", "--mode", ""],
        &["ci", "lint", "--", "--mode", "alpha", ""],
        // The single positional is filled, whether before or after `--`.
        &["ci", "lint", "--", "alpha", ""],
        &["ci", "lint", "alpha", "--", ""],
        // A `--` before the dispatcher's child name, and past the child's positional.
        &["ci", "job", "--", ""],
        &["ci", "job", "run", "x", "--", ""],
        // clap: "a value is required for '--mode <mode>'", bare or with an empty `=`.
        &["ci", "lint", "--mode", "--", ""],
        &["ci", "lint", "--mode=", "--", ""],
        &["ci", "job", "--level", "--", ""],
        &["ci", "job", "--level=", "--", ""],
    ] {
        assert!(dash_values(words).is_empty(), "{words:?}");
    }
}

#[test]
fn path_positional_after_double_dash_is_paths() {
    let m = dash_fixture();
    for last in ["", "--x"] {
        assert_eq!(
            complete(&m, &tokens(&["ci", "open", "--", last])),
            Completion::Paths,
            "{last:?}"
        );
    }
}

/// clap reads a dash-leading choice as a flag, except after `=` or `--`; a bare `-` is
/// always a plain value.
#[test]
fn dash_leading_choices_are_offered_only_where_clap_takes_them() {
    for (words, want) in [
        // Separate form: `--dash`, `-x` and `-1` dropped, bare `-` kept.
        (
            &["ci", "lint", "--mode", ""][..],
            &["-", "fast", "full"][..],
        ),
        (&["ci", "lint", "--mode", "-"], &["-"]),
        (&["ci", "lint", "--mode", "-1"], &[]),
        // Attached form: everything is offered.
        (
            &["ci", "lint", "--mode="],
            &["-", "--dash", "-1", "-x", "fast", "full"],
        ),
        (&["ci", "lint", "--mode=-"], &["-", "--dash", "-1", "-x"]),
        (&["ci", "lint", "--mode=--d"], &["--dash"]),
        (&["ci", "lint", "--mode=-1"], &["-1"]),
        // Positionals: dropped before `--`, offered after it.
        (&["ci", "lint", ""], &["alpha", "beta"]),
        (&["ci", "lint", "--", ""], &["--dir", "alpha", "beta"]),
        (&["ci", "pos", ""], &["-", "a"]),
        (&["ci", "pos", "--", ""], &["-", "--n", "a"]),
    ] {
        assert_eq!(dash_values(words), want, "{words:?}");
    }
}
