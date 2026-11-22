use super::*;

#[test]
fn extracts_positional_optional_flag_and_repeated() {
    let source = r#"
"""Migrate the database.

Handles schema migrations and rolling back.
"""
def add_arguments(self, parser):
    parser.add_argument('app_label')
    parser.add_argument('--database', default='default', help='Target DB')
    parser.add_argument('--check', action='store_true', help='Dry run')
    parser.add_argument('--exclude', action='append')
"#;
    let scanned = scan_source("migrate", source).unwrap();
    assert_eq!(scanned.name, "migrate");
    assert_eq!(scanned.summary, "Migrate the database.");
    assert!(scanned.description.contains("Handles schema migrations"));

    let names: Vec<_> = scanned.arguments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["app_label", "database", "check", "exclude"]);

    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Positional);
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::Optional);
    assert_eq!(scanned.arguments[1].default.as_deref(), Some("default"));
    assert_eq!(scanned.arguments[2].kind, ArgumentKind::Flag);
    assert_eq!(scanned.arguments[3].kind, ArgumentKind::Repeated);
}

#[test]
fn nargs_plus_and_star_classify_as_repeated() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--names', nargs='+', help='Names to greet')
    parser.add_argument('--tags', nargs='*', help='Optional tags')
"#;
    let scanned = scan_source("greet", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Repeated);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Plus));
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::Repeated);
    assert_eq!(scanned.arguments[1].metadata.nargs, Some(Nargs::Star));
}

#[test]
fn nargs_int_classifies_as_fixed_arity_keyword_and_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--pair', nargs=2, help='Exactly two values')
    parser.add_argument('files', nargs=3, help='Exactly three files')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::FixedArity);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Fixed(2)));
    assert!(scanned.arguments[0].long_flag.is_some());
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::FixedArity);
    assert_eq!(scanned.arguments[1].metadata.nargs, Some(Nargs::Fixed(3)));
    assert!(scanned.arguments[1].long_flag.is_none());
    assert!(
        scanned
            .warnings
            .iter()
            .all(|w| !w.contains("unsupported nargs")),
        "fixed-arity nargs shouldn't warn any more, got {:?}",
        scanned.warnings
    );
}

#[test]
fn nargs_int_with_append_action_still_warns() {
    // Repeating groups of N (nargs=2 + action="append") isn't
    // representable — out of scope, still degrades to a single
    // value with a warning.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--pairs', nargs=2, action='append', help='Pairs')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Repeated);
    assert!(
        scanned
            .warnings
            .iter()
            .any(|w| w.contains("--pairs") && w.contains("nargs=2")),
        "expected an unsupported-nargs warning, got {:?}",
        scanned.warnings
    );
}

#[test]
fn positional_nargs_question_mark_classifies_as_optional_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--maybe', nargs='?', help='Optional single value')
    parser.add_argument('maybe_label', nargs='?', help='Optional positional')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Optional);
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::OptionalPositional);
    assert!(
        scanned.warnings.iter().all(|w| !w.contains("nargs")),
        "nargs=\"?\" on either style shouldn't warn any more, got {:?}",
        scanned.warnings
    );
}

#[test]
fn positional_nargs_plus_and_star_classify_as_var_positional() {
    // Mirrors Django's `manage.py test app1 app2` shape:
    // `parser.add_argument('args', nargs='+')`.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('labels', nargs='+', help='One or more labels')
    parser.add_argument('extra', nargs='*', help='Zero or more extras')
"#;
    let scanned = scan_source("test", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Plus));
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[1].metadata.nargs, Some(Nargs::Star));
}

#[test]
fn positional_nargs_remainder_classifies_as_var_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('rest', nargs=argparse.REMAINDER, help='Everything after')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Star));
    assert!(
        scanned.warnings.iter().all(|w| !w.contains("nargs")),
        "REMAINDER on a positional shouldn't warn any more, got {:?}",
        scanned.warnings
    );
}

#[test]
fn positional_nargs_bare_remainder_name_classifies_as_var_positional() {
    // `from argparse import REMAINDER` then `nargs=REMAINDER` (a bare
    // name, not the dotted `argparse.REMAINDER` attribute) must be
    // recognised too.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('rest', nargs=REMAINDER, help='Everything after')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Star));
}

#[test]
fn keyword_style_remainder_still_warns() {
    // argparse.REMAINDER on a keyword-style arg is nonsensical in
    // argparse itself; scope this to positionals only and keep
    // warning.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--rest', nargs=argparse.REMAINDER, help='Weird')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert!(
        scanned
            .warnings
            .iter()
            .any(|w| w.contains("--rest") && w.contains("REMAINDER")),
        "expected an unsupported-nargs warning, got {:?}",
        scanned.warnings
    );
}

#[test]
fn nargs_float_is_neither_recognised_nor_warned_about() {
    // `nargs=1.5` is nonsense argparse itself would reject, but
    // `nargs_value` must not panic on it — it falls through the
    // `Number::Int` match's catch-all arm to `None`. `nargs_kwarg_repr`
    // falls through the same way, so there's no renderable value to
    // warn with either: the arg just quietly falls back to a plain
    // `Optional`.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--float-count', nargs=1.5, help='Nonsense')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Optional);
    assert_eq!(scanned.arguments[0].metadata.nargs, None);
    assert!(
        scanned
            .warnings
            .iter()
            .all(|w| !w.contains("--float-count")),
        "a float nargs has no renderable repr, so it can't be warned about, got {:?}",
        scanned.warnings
    );
}

#[test]
fn nargs_dynamic_name_other_than_remainder_still_warns() {
    // A bare name that isn't `REMAINDER` (e.g. a local variable
    // holding some other value) is a shape `nargs_value` can't
    // recognise — it falls through to `None` — but `nargs_kwarg_repr`
    // can still render it via `type_kwarg_repr`, so it does surface
    // an unsupported-nargs warning rather than silently degrading.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--dynamic', nargs=some_var, help='Nonsense')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].metadata.nargs, None);
    assert!(
        scanned
            .warnings
            .iter()
            .any(|w| w.contains("--dynamic") && w.contains("some_var")),
        "expected an unsupported-nargs warning for the dynamic name, got {:?}",
        scanned.warnings
    );
}

#[test]
fn underscored_long_flag_registers_alias_for_both_spellings() {
    // argparse lets users write `--skip_warm_cache`; cli.rs
    // normalises that to `--skip-warm-cache` for the canonical CLI
    // form, but the user-typed underscored spelling must still
    // resolve. The scanner records the underscored form as an
    // alias so clap accepts both.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--skip_warm_cache', action='store_true')
    parser.add_argument('--already-dashed')
"#;
    let scanned = scan_source("x", source).unwrap();
    assert_eq!(scanned.arguments.len(), 2);

    let underscored = &scanned.arguments[0];
    assert_eq!(underscored.name, "skip_warm_cache");
    assert_eq!(underscored.metadata.aliases, vec!["--skip_warm_cache"]);
    assert_eq!(underscored.long_flag.as_deref(), Some("--skip_warm_cache"));

    // A flag that was already dashed in the source needs no alias —
    // cli.rs emits the same spelling for `.long()`.
    let dashed = &scanned.arguments[1];
    assert_eq!(dashed.name, "already-dashed");
    assert!(dashed.metadata.aliases.is_empty());
}

#[test]
fn short_and_long_flag_pair_registers_short_as_alias() {
    // Django commands routinely declare both spellings on one call:
    // `parser.add_argument("-s", "--run-synchronously", action="store_true", ...)`.
    // The short flag must survive as a clap short alias, not be dropped.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('-s', '--run-synchronously', action='store_true', help='Run sync')
"#;
    let scanned = scan_source("propagate", source).unwrap();
    assert_eq!(scanned.arguments.len(), 1);
    let arg = &scanned.arguments[0];
    assert_eq!(arg.name, "run-synchronously");
    assert_eq!(arg.kind, ArgumentKind::Flag);
    assert_eq!(arg.metadata.aliases, vec!["-s".to_string()]);
}

#[test]
fn more_than_two_flag_spellings_all_become_aliases() {
    // argparse allows arbitrarily many flag spellings on one call, not
    // just a short/long pair.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('-s', '-x', '--run-synchronously', action='store_true', help='Run sync')
"#;
    let scanned = scan_source("propagate", source).unwrap();
    assert_eq!(scanned.arguments.len(), 1);
    let arg = &scanned.arguments[0];
    assert_eq!(arg.name, "run-synchronously");
    assert_eq!(
        arg.metadata.aliases,
        vec!["-s".to_string(), "-x".to_string()]
    );
}

#[test]
fn mixed_short_and_multiple_long_flag_spellings_all_become_aliases() {
    // Short flags and extra long spellings can both appear alongside
    // the canonical (longest) `--flag`.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('-s', '-x', '--rs', '--run-synchronously', action='store_true', help='Run sync')
"#;
    let scanned = scan_source("propagate", source).unwrap();
    assert_eq!(scanned.arguments.len(), 1);
    let arg = &scanned.arguments[0];
    assert_eq!(arg.name, "run-synchronously");
    assert_eq!(
        arg.metadata.aliases,
        vec!["-s".to_string(), "-x".to_string(), "--rs".to_string()]
    );
}

#[test]
fn empty_file_yields_command_with_no_args() {
    let scanned = scan_source("empty", "").unwrap();
    assert!(scanned.arguments.is_empty());
    assert_eq!(scanned.name, "empty");
}

#[test]
fn unresolvable_type_emits_warning_and_no_type_annotation() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--count', type=parse_count)
"#;
    let scanned = scan_source("x", source).unwrap();
    assert_eq!(scanned.arguments.len(), 1);
    assert_eq!(scanned.arguments[0].type_annotation, None);
    assert!(scanned.warnings.iter().any(|w| w.contains("type=")));
}

#[test]
fn matches_any_receiver_name() {
    // Django-style (`parser`), nested attribute (`self.parser`), and
    // a custom name should all be picked up — receiver is not checked.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('a')
    self.parser.add_argument('--b', action='store_true')
    sub_parser.add_argument('c')
"#;
    let scanned = scan_source("x", source).unwrap();
    let names: Vec<_> = scanned.arguments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn skips_calls_whose_positional_is_not_a_string_literal() {
    // First positional is an int — not an argparse name/flag, skip.
    let source = "def f(): mylist.add_argument(42, default='x')";
    let scanned = scan_source("x", source).unwrap();
    assert!(scanned.arguments.is_empty());
    assert!(
        scanned.warnings.is_empty(),
        "skipping non-argparse calls should not warn"
    );
}

#[test]
fn skips_calls_with_unknown_kwarg() {
    // `frobnicate` is not in the argparse kwarg set; this is some other
    // method named `add_argument`. Skip silently.
    let source = "def f(): widget.add_argument('--x', frobnicate=True)";
    let scanned = scan_source("x", source).unwrap();
    assert!(scanned.arguments.is_empty());
    assert!(scanned.warnings.is_empty());
}

#[test]
fn skips_calls_with_no_positional_arguments() {
    let source = "def f(): widget.add_argument(action='store_true')";
    let scanned = scan_source("x", source).unwrap();
    assert!(scanned.arguments.is_empty());
    assert!(scanned.warnings.is_empty());
}

#[test]
fn common_args_are_appended_when_not_shadowed() {
    let scanned = ScannedCommand {
        name: "migrate".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![Argument {
            name: "verbosity".into(),
            kind: ArgumentKind::Optional,
            help: "local".into(),
            default: Some("2".into()),
            type_annotation: None,
            resolved_type: None,
            allowed_values: vec![],
            metadata: Default::default(),
            long_flag: Some("--verbosity".into()),
        }],
        warnings: vec![],
        is_django_command_class: false,
    };
    let common = vec![
        CommonArg {
            name: "verbosity".into(),
            kind: ArgumentKind::Optional,
            help: "common".into(),
            default: Some("0".into()),
            choices: None,
        },
        CommonArg {
            name: "traceback".into(),
            kind: ArgumentKind::Flag,
            help: "tb".into(),
            default: None,
            choices: None,
        },
    ];
    let merged = with_common_args(scanned, &common);
    let names: Vec<_> = merged.arguments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["verbosity", "traceback"]);
    // The local "verbosity" wins.
    assert_eq!(merged.arguments[0].help, "local");
    assert_eq!(merged.arguments[0].default.as_deref(), Some("2"));
}

#[test]
fn scan_paths_expands_globs_and_skips_unparsable() {
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(
        cmds.join("migrate.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('app_label')\n",
    )
    .unwrap();
    std::fs::write(cmds.join("runserver.py"), "def add_arguments(self, parser):\n    parser.add_argument('--insecure', action='store_true')\n").unwrap();
    std::fs::write(cmds.join("broken.py"), "def add_arguments(self, parser:\n").unwrap(); // syntax error

    let scanned = scan_block_paths(
        project.path(),
        &["apps/*/management/commands/*.py".to_string()],
        false,
    )
    .unwrap();

    // Expect migrate + runserver scanned successfully, broken recorded as a warning placeholder.
    let names: std::collections::BTreeSet<_> = scanned.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains("migrate"));
    assert!(names.contains("runserver"));
    // The broken file should still produce a ScannedCommand placeholder with the
    // failure recorded in its `warnings` field — confirm at least one ScannedCommand
    // carries a warning mentioning "broken".
    assert!(
        scanned
            .iter()
            .any(|s| s.warnings.iter().any(|w| w.contains("broken")))
    );
}

/// The file set the static hash shares with the scanner: every match,
/// `_`-prefixed ones included, sorted and deduplicated across patterns;
/// directories are dropped.
#[test]
fn expand_scan_paths_lists_every_matching_file() {
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(cmds.join("pkg.py")).unwrap();
    for name in ["runserver.py", "migrate.py", "_helpers.py", "notes.txt"] {
        std::fs::write(cmds.join(name), "").unwrap();
    }
    let patterns = [
        "apps/*/management/commands/*.py".to_string(),
        "apps/x/management/commands/migrate.py".to_string(),
    ];
    let paths = expand_scan_paths(project.path(), &patterns).unwrap();
    let expected: Vec<_> = ["_helpers.py", "migrate.py", "runserver.py"]
        .iter()
        .map(|n| cmds.join(n))
        .collect();
    assert_eq!(paths, expected);

    assert!(expand_scan_paths(project.path(), &["apps/[/x.py".to_string()]).is_err());
}

#[test]
fn scan_paths_glob_under_bracketed_root() {
    let tmp = tempfile::tempdir().unwrap();
    // Canonical, as `discover_project_root` returns it.
    let root = dunce::canonicalize(tmp.path())
        .unwrap()
        .join("[wip]")
        .join("project");
    let cmds = root.join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(
        cmds.join("migrate.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('app_label')\n",
    )
    .unwrap();
    std::fs::write(cmds.join("runserver.py"), "def add_arguments(self, parser):\n    parser.add_argument('--insecure', action='store_true')\n").unwrap();

    let scanned = scan_block_paths(
        &root,
        &["apps/*/management/commands/*.py".to_string()],
        false,
    )
    .unwrap();

    let names: std::collections::BTreeSet<_> = scanned.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["migrate", "runserver"].into());
}

#[cfg(unix)]
#[test]
fn non_utf8_root_is_a_parse_error() {
    use std::os::unix::ffi::OsStrExt;
    let root = Path::new(std::ffi::OsStr::from_bytes(b"/src/\xff/project"));

    let err = scan_block_paths(root, &["*.py".to_string()], false).unwrap_err();

    let ScanError::Parse { message, .. } = err;
    assert_eq!(message, "non-utf8 project root");
}

#[test]
fn absolute_scan_path_ignores_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let ext = tmp.path().join("ext");
    std::fs::create_dir_all(&ext).unwrap();
    std::fs::write(
        ext.join("migrate.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('app_label')\n",
    )
    .unwrap();
    let pattern = format!("{}/*.py", ext.display());

    let scanned = scan_block_paths(&tmp.path().join("root"), &[pattern], false).unwrap();

    let names: Vec<_> = scanned.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["migrate"]);
}

#[test]
fn scan_paths_skips_files_with_no_add_argument_calls() {
    // Regression: helper modules (`__init__.py`, `_setup_*.py`, shared utilities) parse fine
    // but contain zero `add_argument` calls. They must not become commands — multiple
    // `__init__.py` files under one parent would collide on the stem and crash clap with a
    // "command name is duplicated" panic at startup. Both are caught by the (unconditional)
    // underscore filter here, regardless of `django`.
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();

    std::fs::write(
        cmds.join("real_command.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('app_label')\n",
    )
    .unwrap();
    // `__init__.py` parses fine but has no add_argument calls.
    std::fs::write(cmds.join("__init__.py"), "# package marker\n").unwrap();
    // `_helpers.py` — a private helper with no add_argument calls.
    std::fs::write(
        cmds.join("_helpers.py"),
        "def configure(parser):\n    pass\n",
    )
    .unwrap();

    let scanned = scan_block_paths(
        project.path(),
        &["apps/*/management/commands/*.py".to_string()],
        false,
    )
    .unwrap();

    let names: std::collections::BTreeSet<_> = scanned.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains("real_command"), "real command must scan");
    assert!(
        !names.contains("__init__"),
        "__init__.py has no add_argument calls — must not become a command",
    );
    assert!(
        !names.contains("_helpers"),
        "helper module has no add_argument calls — must not become a command",
    );
}

#[test]
fn scan_source_detects_literal_command_class() {
    let source =
        "class Command(BaseCommand):\n    def handle(self, *args, **options):\n        pass\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(scanned.is_django_command_class);
    assert!(scanned.arguments.is_empty());
}

#[test]
fn scan_source_detects_command_alias_to_same_module_class() {
    // Real-world shape: a command class under its own name, aliased to `Command` for Django's
    // loader, with no `add_arguments` at all.
    let source = "\
class SegmentationDocumentsUpdater(BaseCommand):\n    def handle(self, *args, **options):\n        pass\n\nCommand = SegmentationDocumentsUpdater\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(scanned.is_django_command_class);
    assert!(scanned.arguments.is_empty());
}

#[test]
fn scan_source_does_not_guess_alias_to_imported_class() {
    // `Command = SomeImportedClass` isn't verifiable without import resolution — treated as
    // unresolved, not guessed at.
    let source = "from elsewhere import SomeImportedClass\n\nCommand = SomeImportedClass\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(!scanned.is_django_command_class);
}

#[test]
fn scan_source_ignores_unrelated_module_level_assignment() {
    // `LOGGER = ...` is a common module-level assignment that isn't `Command` at all — must not
    // be mistaken for one.
    let source = "class Foo(BaseCommand):\n    pass\n\nLOGGER = 'x'\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(!scanned.is_django_command_class);
}

#[test]
fn scan_source_ignores_command_assignment_with_non_name_target() {
    // `obj.Command = Foo` targets an attribute, not a bare name — doesn't satisfy Django's
    // module-level `Command` contract.
    let source = "class Foo(BaseCommand):\n    pass\n\nobj.Command = Foo\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(!scanned.is_django_command_class);
}

#[test]
fn scan_source_ignores_command_assignment_with_multiple_targets() {
    // `a = Command = Foo` has two assignment targets — not the single-target `Command = <Name>`
    // shape this detects.
    let source = "class Foo(BaseCommand):\n    pass\n\na = Command = Foo\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(!scanned.is_django_command_class);
}

#[test]
fn scan_source_ignores_command_assignment_with_non_name_value() {
    // `Command = make_command()` doesn't alias to a same-module class by name — nothing to
    // resolve statically.
    let source = "class Foo(BaseCommand):\n    pass\n\nCommand = make_command()\n";
    let scanned = scan_source("x", source).unwrap();
    assert!(!scanned.is_django_command_class);
}

#[test]
fn scan_paths_keeps_zero_arg_command_alias_files_when_django() {
    // A zero-arg command (e.g. a PubSubConsumer-style listener) still defines `Command`, so it
    // must survive when `django = true`.
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();

    std::fs::write(
        cmds.join("segmentation_documents_updater.py"),
        "class SegmentationDocumentsUpdater(BaseCommand):\n    def handle(self, *args, **options):\n        pass\n\nCommand = SegmentationDocumentsUpdater\n",
    )
    .unwrap();
    // A stray helper with no underscore prefix and no `Command` symbol must still be dropped.
    std::fs::write(
        cmds.join("shared_utils.py"),
        "def configure(parser):\n    pass\n",
    )
    .unwrap();

    let scanned = scan_block_paths(
        project.path(),
        &["apps/*/management/commands/*.py".to_string()],
        true,
    )
    .unwrap();

    let names: std::collections::BTreeSet<_> = scanned.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains("segmentation_documents_updater"),
        "zero-arg command that defines Command must still scan when django = true",
    );
    assert!(
        !names.contains("shared_utils"),
        "helper module with no Command symbol and no add_argument calls must not become a command",
    );
}

#[test]
fn scan_paths_drops_zero_arg_command_alias_files_without_django() {
    // Same file as above, but the block doesn't opt in — the carve-out must not apply to plain
    // argparse blocks.
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();

    std::fs::write(
        cmds.join("segmentation_documents_updater.py"),
        "class SegmentationDocumentsUpdater(BaseCommand):\n    def handle(self, *args, **options):\n        pass\n\nCommand = SegmentationDocumentsUpdater\n",
    )
    .unwrap();

    let scanned = scan_block_paths(
        project.path(),
        &["apps/*/management/commands/*.py".to_string()],
        false,
    )
    .unwrap();

    assert!(
        scanned.is_empty(),
        "zero-arg Command file must be dropped when django = false",
    );
}

#[test]
fn scan_paths_skips_underscore_prefixed_files_regardless_of_django() {
    // The underscore convention is plain Python, not Django-specific — must be skipped
    // even with real `add_argument` calls, so the empty-args rule can't be what's doing it.
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(
        cmds.join("_internal_command.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('--flag', action='store_true')\n",
    )
    .unwrap();

    for django in [true, false] {
        let scanned = scan_block_paths(
            project.path(),
            &["apps/*/management/commands/*.py".to_string()],
            django,
        )
        .unwrap();
        assert!(
            scanned.is_empty(),
            "underscore-prefixed stem with real args must still be skipped (django={django})",
        );
    }
}
