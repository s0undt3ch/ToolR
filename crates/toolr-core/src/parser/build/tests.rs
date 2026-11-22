use super::*;
use crate::manifest::SCHEMA_VERSION;
use crate::manifest::{ArgMetadata, Argument, Command, Origin};
use crate::parser::commands::UnsupportedCommandKind;
use crate::parser::names::{CONTROL, NameItem, WHITESPACE};
use crate::parser::types::{BoolFlagError, UnsupportedType};
use tempfile::TempDir;

fn arg(kind: ArgumentKind, name: &str, long_flag: Option<&str>) -> Argument {
    Argument {
        name: name.to_string(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: None,
        allowed_values: vec![],
        metadata: ArgMetadata::default(),
        long_flag: long_flag.map(str::to_string),
    }
}

fn cmd_with_args(name: &str, arguments: Vec<Argument>) -> Command {
    Command {
        name: name.to_string(),
        group: String::new(),
        module: format!("tools.{name}"),
        function: name.to_string(),
        summary: String::new(),
        description: String::new(),
        arguments,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

/// The build records what it hashed, so the next check can reuse it.
#[test]
fn built_manifest_carries_input_records() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(tools.join("sub")).unwrap();
    std::fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\n",
    )
    .unwrap();
    std::fs::write(tools.join("sub/a.py"), "x = 1\n").unwrap();
    let manifest = build_static_manifest(&tools).unwrap().manifest;
    let keys: Vec<_> = manifest
        .input_records
        .files
        .iter()
        .map(|f| f.key.as_str())
        .collect();
    assert_eq!(keys, ["tools/pyproject.toml", "tools/sub/a.py"]);
    assert_eq!(
        manifest.static_hash,
        crate::hash::compute_static_hash(&tools).unwrap()
    );
}

#[test]
fn optional_positional_participates_in_zero_or_one_rules() {
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "first", None),
            arg(ArgumentKind::OptionalPositional, "second", None),
        ],
    )];
    let errors = validate_positional_arity(&commands);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].kind,
        PositionalArityErrorKind::MultipleZeroOrOne { .. }
    ));
}

#[test]
fn required_positional_after_optional_positional_errors() {
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "opt", None),
            arg(ArgumentKind::Positional, "req", None),
        ],
    )];
    let errors = validate_positional_arity(&commands);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].kind,
        PositionalArityErrorKind::RequiredAfterZeroOrOne { .. }
    ));
}

#[test]
fn positional_fixed_arity_after_optional_positional_errors() {
    // A fixed-N positional slot is always present, same as a plain
    // required Positional — can't follow a zero-or-one slot either.
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "opt", None),
            arg(ArgumentKind::FixedArity, "pair", None),
        ],
    )];
    let errors = validate_positional_arity(&commands);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].kind,
        PositionalArityErrorKind::RequiredAfterZeroOrOne { .. }
    ));
}

#[test]
fn keyword_fixed_arity_does_not_compete_for_positional_slots() {
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "opt", None),
            arg(ArgumentKind::FixedArity, "pair", Some("--pair")),
        ],
    )];
    assert!(validate_positional_arity(&commands).is_empty());
}

fn write(tmp: &Path, name: &str, contents: &str) {
    let path = tmp.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

use crate::third_party::ManifestFragment;

#[test]
fn build_with_venv_merges_local_and_third_party() {
    let tmp = TempDir::new().unwrap();
    // Local tools/ side.
    write(
        tmp.path(),
        "tools/ci.py",
        r#""""CI utilities."""
group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#,
    );
    // Fake tools venv with a third-party fragment.
    let venv = tmp.path().join("venv");
    let site = venv.join("lib").join("python3.13").join("site-packages");
    std::fs::create_dir_all(site.join("ext_pkg")).unwrap();
    let frag = ManifestFragment {
        toolr_schema_version: SCHEMA_VERSION,
        package: "ext_pkg".into(),
        groups: vec![Group {
            name: "deploy".into(),
            title: "Deploy".into(),
            description: String::new(),
            parent: None,
            origin: Origin::ThirdParty,
        }],
        commands: vec![Command {
            name: "rollout".into(),
            group: "deploy".into(),
            module: "ext_pkg.commands".into(),
            function: "rollout".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            origin: Origin::ThirdParty,
            dispatched_from: None,
            is_dispatcher: false,
        }],
    };
    std::fs::write(
        site.join("ext_pkg").join("toolr-manifest.json"),
        serde_json::to_string(&frag).unwrap(),
    )
    .unwrap();

    let m = build_static_manifest_with_venv(&tmp.path().join("tools"), &venv)
        .unwrap()
        .manifest;
    let groups: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(groups.contains(&"ci"));
    assert!(groups.contains(&"deploy"));
    let cmds: Vec<_> = m.commands.iter().map(|c| c.name.as_str()).collect();
    assert!(cmds.contains(&"hello"));
    assert!(cmds.contains(&"rollout"));
}

/// The local build's warnings survive the merge, and a plugin declaring
/// the same group adds none: the local one wins silently.
#[test]
fn build_with_venv_keeps_the_local_build_warnings() {
    let tmp = TempDir::new().unwrap();
    for (module, title) in [("m1", "Grp"), ("m2", "Other")] {
        write(
            tmp.path(),
            &format!("tools/{module}.py"),
            &format!("g = command_group(\"grp\", \"{title}\", \"Grp\")\n"),
        );
    }
    let venv = tmp.path().join("venv");
    let pkg = venv.join("lib/python3.13/site-packages/ext_pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    let frag = ManifestFragment {
        toolr_schema_version: SCHEMA_VERSION,
        package: "ext_pkg".into(),
        groups: vec![Group {
            name: "grp".into(),
            title: "Plugin".into(),
            description: "Plugin group.".into(),
            parent: None,
            origin: Origin::ThirdParty,
        }],
        commands: vec![],
    };
    std::fs::write(
        pkg.join("toolr-manifest.json"),
        serde_json::to_string(&frag).unwrap(),
    )
    .unwrap();

    let built = build_static_manifest_with_venv(&tmp.path().join("tools"), &venv).unwrap();
    assert_eq!(built.manifest.groups.len(), 1);
    assert_eq!(built.manifest.groups[0].title, "Grp");
    assert_eq!(
        built.warnings,
        [
            "group `grp` is declared more than once; keeping the title from tools.m1 and \
             ignoring the different title in tools.m2"
        ]
    );
}

/// `list_python_files` must NOT recurse into dot-prefixed
/// directories like `tools/.venv/`. Without this, walking `tools/`
/// after `uv sync` produces a manifest polluted with the installed
/// packages' modules — bare `module_path_for` then yields garbage
/// like `tools..venv.lib.site-packages.<pkg>.<mod>` that the runner
/// fails to import (`No module named 'tools.'`).
#[test]
fn list_python_files_skips_dot_prefixed_directories() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    write(&tools, "ci.py", "x = 1\n");
    write(
        &tools,
        ".venv/lib/python3.13/site-packages/pkg/__init__.py",
        "",
    );
    write(
        &tools,
        ".venv/lib/python3.13/site-packages/pkg/commands.py",
        "y = 2\n",
    );
    write(&tools, ".git/hooks/pre-commit.py", "z = 3\n");

    let files = list_python_files(&tools).unwrap();
    let rels: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(&tools)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(rels, vec!["ci.py".to_string()], "got: {rels:?}");
}

/// Plugin fragments share the walk: the paths and the hint name the
/// package directory.
#[cfg(unix)]
#[test]
fn build_commands_rejects_symlinked_modules_in_a_package() {
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("my_plugin");
    write(&pkg, "__init__.py", "");
    write(tmp.path(), "shared/linked.py", "x = 1\n");
    std::os::unix::fs::symlink("../shared/linked.py", pkg.join("linked.py")).unwrap();
    let err = build_commands(&pkg, "my_plugin").unwrap_err();
    assert!(matches!(err, BuildError::SymlinkedModules(_)), "{err:?}");
    assert_eq!(
        err.to_string(),
        "symlinked modules are not supported (1):\n  - my_plugin/linked.py\nPut the file itself under my_plugin/, or share it as a Python package."
    );
}

fn invalid_names_for<S: AsRef<str>>(files: &[(&str, S)]) -> Vec<InvalidName> {
    let tmp = TempDir::new().unwrap();
    for (name, src) in files {
        write(tmp.path(), name, src.as_ref());
    }
    expect_invalid_names(build_static_manifest_inner(&tmp.path().join("tools")).unwrap_err())
}

fn expect_invalid_names(err: BuildError) -> Vec<InvalidName> {
    let BuildError::InvalidNames(errs) = err else {
        panic!("expected InvalidNames, got {err}");
    };
    errs
}

/// A `django` dispatcher in `group` with one scanned argparse argument.
fn django_argparse(group: &str, add_argument: &str) -> [(&'static str, String); 3] {
    let parent = if group == "django" {
        "django".to_string()
    } else {
        format!("{group}.django")
    };
    [
        (
            "tools/dispatcher.py",
            format!(
                "group = command_group(\"{group}\", \"Django\", \"Django.\")\n\n@group.command\ndef django(ctx):\n    \"\"\"Dispatch.\"\"\"\n"
            ),
        ),
        (
            "apps/x/management/commands/migrate.py",
            format!("def add_arguments(self, parser):\n    parser.add_argument({add_argument})\n"),
        ),
        (
            "tools/pyproject.toml",
            format!(
                "[tool.toolr.argparse.django]\nscan_paths = [\"apps/*/management/commands/*.py\"]\n\n[[tool.toolr.argparse.django.attach]]\nparent = \"{parent}\"\n"
            ),
        ),
    ]
}

#[test]
fn group_name_with_space_rejected() {
    let errs = invalid_names_for(&[(
        "tools/g.py",
        "group = command_group(\"my group\", \"T\", \"D\")\n\n@group.command\ndef hi(ctx):\n    \"\"\"Hi.\"\"\"\n",
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].item, NameItem::Group);
    assert_eq!(errs[0].reason, WHITESPACE);
}

#[test]
fn command_override_name_with_newline_rejected() {
    let errs = invalid_names_for(&[(
        "tools/g.py",
        "group = command_group(\"g\", \"T\", \"D\")\n\n@group.command(name=\"a\\nb\")\ndef hi(ctx):\n    \"\"\"Hi.\"\"\"\n",
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].item, NameItem::Command);
    assert_eq!(errs[0].value, "a\\u{a}b");
    assert_eq!(errs[0].reason, CONTROL);
}

const LITERAL_SRC: &str = "from typing import Literal\n\ngroup = command_group(\"g\", \"T\", \"D\")\n\n@group.command\ndef hi(ctx, mode: Literal[LITERAL]):\n    \"\"\"Hi.\"\"\"\n";

#[test]
fn literal_value_with_esc_rejected() {
    let src = LITERAL_SRC.replace("LITERAL", "\"ok\", \"bad\\x1b[2J\"");
    let errs = invalid_names_for(&[("tools/g.py", &src)]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].item, NameItem::Choice);
    assert_eq!(errs[0].value, "bad\\u{1b}[2J");
}

#[test]
fn literal_value_with_space_and_dollar_allowed() {
    let src = LITERAL_SRC.replace("LITERAL", "\"dry run\", \"$(x)\"");
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "tools/g.py", &src);
    build_static_manifest(&tmp.path().join("tools")).unwrap();
}

#[test]
fn argparse_flag_with_space_rejected() {
    let errs = invalid_names_for(&django_argparse(
        "django",
        "'--bad flag', action='store_true'",
    ));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].item, NameItem::Flag);
    assert_eq!(errs[0].value, "--bad flag");
}

const DIRTY_GROUP_REF_SRC: &str = "from toolr import command\ngroup = command_group(\"g\", \"T\", \"D\")\n\n@command(group=\"no\\x1b]0;PWN\\x07pe\")\ndef hi(ctx):\n    \"\"\"Hi.\"\"\"\n";

#[test]
fn command_group_ref_with_esc_rejected() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "tools/g.py", DIRTY_GROUP_REF_SRC);
    let errs =
        expect_invalid_names(build_commands(&tmp.path().join("tools"), "tools").unwrap_err());
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].item, NameItem::GroupRef);
    assert_eq!(errs[0].value, "no\\u{1b}]0;PWN\\u{7}pe");
}

#[test]
fn local_build_escapes_a_dirty_group_ref() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "tools/g.py", DIRTY_GROUP_REF_SRC);
    let msg = build_static_manifest_inner(&tmp.path().join("tools"))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("no\\u{1b}]0;PWN\\u{7}pe"), "{msg}");
    assert!(!msg.contains('\x1b') && !msg.contains('\x07'), "{msg:?}");
}

#[test]
fn argparse_positional_with_esc_rejected() {
    let errs = invalid_names_for(&django_argparse("django", "'po\\x1b]0;POS\\x07s'"));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].item, NameItem::Argument);
    assert_eq!(errs[0].reason, CONTROL);
}

#[test]
fn local_build_escapes_a_dirty_command_with_an_unknown_group() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/g.py",
        "from toolr import command\n@command(name=\"x\\x1b]0;PWN\\x07\", group=\"nope\")\ndef hi(ctx):\n    \"\"\"Hi.\"\"\"\n",
    );
    let msg = build_static_manifest_inner(&tmp.path().join("tools"))
        .unwrap_err()
        .to_string();
    assert!(!msg.contains('\x1b') && !msg.contains('\x07'), "{msg:?}");
}

#[test]
fn unknown_group_message_escapes_control_chars() {
    let msg = BuildError::UnknownGroupRefs(vec![UnknownGroupRef {
        command: "hi".into(),
        module: "tools.g".into(),
        referenced: "no\x1b]0;PWN\x07pe".into(),
        suggestion: Some("s\x1b[2J".into()),
    }])
    .to_string();
    assert!(msg.contains("no\\u{1b}]0;PWN\\u{7}pe"), "{msg}");
    assert!(!msg.contains('\x1b') && !msg.contains('\x07'), "{msg:?}");
}

#[test]
fn group_name_with_esc_is_escaped_in_the_message() {
    let errs = invalid_names_for(&[(
        "tools/g.py",
        "group = command_group(\"x\\x1b[2J\", \"T\", \"D\")\n\n@group.command\ndef hi(ctx):\n    \"\"\"Hi.\"\"\"\n",
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    let msg = BuildError::InvalidNames(errs).to_string();
    assert!(msg.contains("x\\u{1b}[2J"), "{msg}");
    assert!(!msg.contains('\x1b'), "{msg:?}");
}

#[test]
fn fragment_build_renders_command_group_ref_and_choice_violations() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/g.py",
        "from typing import Literal\nfrom toolr import command\n\ngroup = command_group(\"g\", \"T\", \"D\")\n\n@group.command(name=\"a\\nb\")\ndef named(ctx):\n    \"\"\"Hi.\"\"\"\n\n@command(group=\"no\\x1b[2Jpe\")\ndef orphan(ctx):\n    \"\"\"Hi.\"\"\"\n\n@group.command\ndef pick(ctx, mode: Literal[\"ok\", \"bad\\x1b[2J\"]):\n    \"\"\"Hi.\"\"\"\n",
    );
    let msg = BuildError::InvalidNames(expect_invalid_names(
        build_commands(&tmp.path().join("tools"), "tools").unwrap_err(),
    ))
    .to_string();
    assert_eq!(
        msg,
        "invalid names (3):\n\
         \x20 - tools.g::named command name `a\\u{a}b`: contains a control character\n\
         \x20 - tools.g::orphan group reference `no\\u{1b}[2Jpe`: contains a control character\n\
         \x20 - tools.g::pick choice value `bad\\u{1b}[2J`: contains a control character"
    );
}

#[test]
fn fragment_build_renders_zero_width_and_bidi_violations() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/g.py",
        "from typing import Literal\n\ngroup = command_group(\"g\", \"T\", \"D\")\n\n@group.command(name=\"de\u{200b}ploy\")\ndef named(ctx):\n    \"\"\"Hi.\"\"\"\n\n@group.command\ndef pick(ctx, mode: Literal[\"ok\", \"\u{202e}gnp.exe\"]):\n    \"\"\"Hi.\"\"\"\n",
    );
    let msg = BuildError::InvalidNames(expect_invalid_names(
        build_commands(&tmp.path().join("tools"), "tools").unwrap_err(),
    ))
    .to_string();
    assert_eq!(
        msg,
        "invalid names (2):\n\
         \x20 - tools.g::named command name `de\\u{200b}ploy`: contains an invisible formatting character\n\
         \x20 - tools.g::pick choice value `\\u{202e}gnp.exe`: contains an invisible formatting character"
    );
}

#[test]
fn local_build_renders_group_flag_and_argument_violations() {
    let mut files = django_argparse("my group", "'--bad flag', action='store_true'");
    files[1].1 = "def add_arguments(self, parser):\n    parser.add_argument('--bad flag', action='store_true')\n    parser.add_argument('po\\x1b[2Js')\n".to_string();
    let msg = BuildError::InvalidNames(invalid_names_for(&files)).to_string();
    assert_eq!(
        msg,
        "invalid names (3):\n\
         \x20 - my group group name `my group`: contains whitespace\n\
         \x20 - tools.dispatcher::django flag `--bad flag`: contains whitespace\n\
         \x20 - tools.dispatcher::django argument name `po\\u{1b}[2Js`: contains a control character"
    );
}

#[test]
fn bad_group_and_bad_argparse_flag_reported_together() {
    let errs = invalid_names_for(&django_argparse(
        "my group",
        "'--bad flag', action='store_true'",
    ));
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(errs.iter().any(|e| e.item == NameItem::Group), "{errs:?}");
    assert!(errs.iter().any(|e| e.item == NameItem::Flag), "{errs:?}");
}

#[test]
fn builds_manifest_from_single_tools_file() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/ci.py",
        r#""""CI utilities."""
group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#,
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    assert_eq!(m.schema_version, SCHEMA_VERSION);
    assert_eq!(m.groups.len(), 1);
    assert_eq!(m.groups[0].name, "ci");
    assert_eq!(m.commands.len(), 1);
    assert_eq!(m.commands[0].name, "hello");
    assert!(!m.static_hash.is_empty());
}

/// Build `files` and return `(group full paths with titles, build warnings)`.
fn redeclared(files: &[(&str, &str)]) -> (Vec<(String, String, String)>, Vec<String>) {
    let tmp = TempDir::new().unwrap();
    for (name, contents) in files {
        write(tmp.path(), name, contents);
    }
    let built = build_static_manifest(&tmp.path().join("tools")).unwrap();
    let groups = built
        .manifest
        .groups
        .iter()
        .map(|g| (g.full_path(), g.title.clone(), g.description.clone()))
        .collect();
    (groups, built.warnings)
}

fn group_row(path: &str, title: &str, description: &str) -> (String, String, String) {
    (path.into(), title.into(), description.into())
}

const GROUP_IMPORT: &str = "from toolr import command_group\n";

#[test]
fn identical_group_redeclaration_is_silent() {
    let src = format!(
        "{GROUP_IMPORT}a = command_group(\"grp\", \"Grp\", \"Desc\")\nb = command_group(\"grp\", \"Grp\", \"Desc\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(groups, [group_row("grp", "Grp", "Desc")]);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn conflicting_group_redeclaration_in_one_module_warns() {
    let src = format!(
        "{GROUP_IMPORT}a = command_group(\"grp\", \"Grp\", \"Desc\")\nb = command_group(\"grp\", \"Other\", \"Else\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(groups, [group_row("grp", "Grp", "Desc")]);
    assert_eq!(
        warnings,
        [
            "group `grp` is declared more than once; keeping the title, description from tools.m and \
          ignoring the different title, description in tools.m"
        ]
    );
}

#[test]
fn conflicting_group_redeclaration_across_modules_names_both() {
    let first = format!("{GROUP_IMPORT}g = command_group(\"grp\", \"Grp\", \"Desc\")\n");
    let second = format!("{GROUP_IMPORT}g = command_group(\"grp\", \"Other\", \"Desc\")\n");
    let third = format!("{GROUP_IMPORT}g = command_group(\"grp\", \"Grp\", \"Else\")\n");
    let (groups, warnings) = redeclared(&[
        ("tools/m1.py", &first),
        ("tools/m2.py", &second),
        ("tools/m3.py", &third),
    ]);
    assert_eq!(groups, [group_row("grp", "Grp", "Desc")]);
    assert_eq!(
        warnings,
        [
            "group `grp` is declared more than once; keeping the title from tools.m1 and \
             ignoring the different title in tools.m2",
            "group `grp` is declared more than once; keeping the description from tools.m1 and \
             ignoring the different description in tools.m3",
        ]
    );
}

/// A re-declaration without a description fetches the group; it has no
/// opinion on the description, so only a different title counts.
#[test]
fn group_redeclaration_without_description_has_no_opinion_on_it() {
    let first = format!("{GROUP_IMPORT}g = command_group(\"grp\", \"Grp\", \"Desc\")\n");
    let same = format!("{GROUP_IMPORT}g = command_group(\"grp\", \"Grp\")\n");
    let other = format!("{GROUP_IMPORT}g = command_group(\"grp\", \"Other\")\n");
    let (groups, warnings) = redeclared(&[
        ("tools/m1.py", &first),
        ("tools/m2.py", &same),
        ("tools/m3.py", &other),
    ]);
    assert_eq!(groups, [group_row("grp", "Grp", "Desc")]);
    assert_eq!(
        warnings,
        [
            "group `grp` is declared more than once; keeping the title from tools.m1 and \
          ignoring the different title in tools.m3"
        ]
    );
}

#[test]
fn dotted_and_parent_kwarg_declare_the_same_group() {
    let src = format!(
        "{GROUP_IMPORT}p1 = command_group(\"p1\", \"P1\", \"P1\")\n\
         a = command_group(\"p1.kid\", \"Kid\", \"Kid\")\n\
         b = command_group(\"kid\", \"Child\", \"Kid\", parent=\"p1\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(
        groups,
        [
            group_row("p1", "P1", "P1"),
            group_row("p1.kid", "Kid", "Kid")
        ]
    );
    assert_eq!(
        warnings,
        [
            "group `p1.kid` is declared more than once; keeping the title from tools.m and \
          ignoring the different title in tools.m"
        ]
    );
}

#[test]
fn same_leaf_under_two_parents_is_two_groups() {
    let src = format!(
        "{GROUP_IMPORT}p1 = command_group(\"p1\", \"P1\", \"P1\")\n\
         p2 = command_group(\"p2\", \"P2\", \"P2\")\n\
         a = command_group(\"kid\", \"Kid one\", \"One\", parent=\"p1\")\n\
         b = command_group(\"kid\", \"Kid two\", \"Two\", parent=\"p2\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(
        groups,
        [
            group_row("p1", "P1", "P1"),
            group_row("p2", "P2", "P2"),
            group_row("p1.kid", "Kid one", "One"),
            group_row("p2.kid", "Kid two", "Two"),
        ]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn group_without_a_description_anywhere_fails() {
    let lint = format!("{GROUP_IMPORT}g = command_group(\"ci\", \"CI\")\n");
    let build = format!("{GROUP_IMPORT}g = command_group(\"ci\", \"CI\")\n");
    let other = format!(
        "{GROUP_IMPORT}command_group(\"other\", \"Other\")\ncommand_group(\"other\", \"Other\")\n"
    );
    assert_eq!(
        build_error_for(&[
            ("tools/ci/__init__.py", ""),
            ("tools/ci/lint.py", &lint),
            ("tools/ci/build.py", &build),
            ("tools/other.py", &other),
        ]),
        "command groups with no description (2):\n  \
         - `ci` (declared in tools.ci.build, tools.ci.lint): give one declaration a \
         description or docstring.\n  \
         - `other` (declared in tools.other): give one declaration a description or docstring."
    );
}

/// An empty description, or an empty or whitespace-only docstring, leaves a
/// group undescribed; a later declaration with real text still describes it.
#[test]
fn empty_description_or_docstring_does_not_describe_a_group() {
    let src = format!(
        "{GROUP_IMPORT}a = command_group(\"a\", \"A\", description=\"\")\n\
         b = command_group(\"b\", \"B\", docstring=\"  \\n \")\n\
         c = command_group(\"c\", \"C\", docstring=__doc__)\n\
         d = command_group(\"d\", \"D\", description=\"\")\n\
         d = command_group(\"d\", \"D\", description=\"D.\")\n"
    );
    let src = format!("\"\"\"  \"\"\"\n{src}");
    assert_eq!(
        build_error_for(&[("tools/m.py", &src)]),
        "command groups with no description (3):\n  \
         - `a` (declared in tools.m): give one declaration a description or docstring.\n  \
         - `b` (declared in tools.m): give one declaration a description or docstring.\n  \
         - `c` (declared in tools.m): give one declaration a description or docstring the static parser can read: \
         a string literal, or the `__doc__` of a module with a docstring."
    );
}

/// `__doc__` describes a group only when the module has a docstring.
#[test]
fn module_without_a_docstring_does_not_describe_its_group() {
    let src = format!(
        "{GROUP_IMPORT}g = command_group(\"ci\", \"CI\", docstring=__doc__)\n\
         h = command_group(\"cd\", \"CD\", description=__doc__)\n"
    );
    assert_eq!(
        build_error_for(&[("tools/m.py", &src)]),
        "command groups with no description (2):\n  \
         - `ci` (declared in tools.m): give one declaration a description or docstring the static parser can read: \
         a string literal, or the `__doc__` of a module with a docstring.\n  \
         - `cd` (declared in tools.m): give one declaration a description or docstring the static parser can read: \
         a string literal, or the `__doc__` of a module with a docstring."
    );
}

/// Unpacked arguments may hold a description the static parser can't see.
#[test]
fn unpacked_arguments_ask_for_a_readable_description() {
    let src = format!(
        "{GROUP_IMPORT}OPTS = {{\"description\": \"CI.\"}}\n\
         ARGS = (\"CI\", \"CI.\")\n\
         a = command_group(\"ci\", **OPTS)\n\
         b = command_group(\"cd\", *ARGS)\n"
    );
    assert_eq!(
        build_error_for(&[("tools/m.py", &src)]),
        "command groups with no description (2):\n  \
         - `ci` (declared in tools.m): give one declaration a description or docstring the static parser can read: \
         a string literal, or the `__doc__` of a module with a docstring.\n  \
         - `cd` (declared in tools.m): give one declaration a description or docstring the static parser can read: \
         a string literal, or the `__doc__` of a module with a docstring."
    );
}

/// Any declaration may describe the group; title-only ones before it
/// only fetch it, so the description it gives is the group's.
#[test]
fn title_only_declarations_take_a_later_description() {
    let first = format!("{GROUP_IMPORT}g = command_group(\"ci\", \"CI\")\n");
    let second = format!("{GROUP_IMPORT}g = command_group(\"ci\", \"CI\", \"CI commands.\")\n");
    let third = format!("{GROUP_IMPORT}g = command_group(\"ci\", \"CI\", \"Else.\")\n");
    let (groups, warnings) = redeclared(&[
        ("tools/a.py", &first),
        ("tools/b.py", &second),
        ("tools/c.py", &third),
    ]);
    assert_eq!(groups, [group_row("ci", "CI", "CI commands.")]);
    assert_eq!(
        warnings,
        [
            "group `ci` is declared more than once; keeping the description from tools.b and \
             ignoring the different description in tools.c"
        ]
    );
}

/// An empty title or description has no opinion, whether passed or not,
/// so a later declaration fills it without a warning.
#[test]
fn empty_title_and_description_take_later_ones() {
    let first = format!(
        "{GROUP_IMPORT}a = command_group(\"ci\", \"CI\", \"\")\n\
         b = command_group(\"cd\", description=\"CD.\")\n"
    );
    let second = format!(
        "{GROUP_IMPORT}a = command_group(\"ci\", \"CI\", \"CI commands.\")\n\
         b = command_group(\"cd\", \"CD\", \"CD.\")\n\
         c = command_group(\"cd\", description=\"CD.\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/a.py", &first), ("tools/b.py", &second)]);
    assert_eq!(
        groups,
        [
            group_row("ci", "CI", "CI commands."),
            group_row("cd", "CD", "CD.")
        ]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A name is unreadable even when the module assigns it a string: it
/// describes the group, shows empty, and a later literal neither fills it
/// nor warns.
#[test]
fn module_constants_are_unreadable() {
    let src = format!(
        "{GROUP_IMPORT}DESC = \"\"\nDESC += \"CI.\"\n\
         a = command_group(\"ci\", \"CI\", DESC)\n\
         DESCRIPTION = \"CD commands.\"\n\
         b = command_group(\"cd\", \"CD\", description=DESCRIPTION)\n\
         c = command_group(\"cd\", \"CD\", \"Continuous delivery.\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(
        groups,
        [group_row("ci", "CI", ""), group_row("cd", "CD", "")]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A value the static parser can't read has an opinion it can't compare:
/// a later readable one neither fills it nor warns.
#[test]
fn unreadable_fields_are_kept_without_a_warning() {
    let src = format!(
        "{GROUP_IMPORT}from elsewhere import DESC, TITLE\n\
         a = command_group(\"ci\", TITLE, description=DESC)\n\
         b = command_group(\"ci\", \"CI\", description=\"CI.\")\n\
         c = command_group(\"cd\", \"CD\", docstring=opts.doc)\n\
         d = command_group(\"cd\", \"CD\", \"CD.\")\n\
         e = command_group(\"ops\", docstring=make())\n\
         f = command_group(\"ops\", \"Ops\", \"Ops.\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(
        groups,
        [
            group_row("ci", "", ""),
            group_row("cd", "CD", ""),
            group_row("ops", "", "")
        ]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// An argument the parser can't read may be `None` at runtime, so it
/// contradicts nothing.
#[test]
fn unreadable_arguments_are_no_contradiction() {
    let src = format!(
        "\"\"\"Doc.\"\"\"\n{GROUP_IMPORT}LONG = None\n\
         a = command_group(\"ci\", \"CI\", long_description=LONG, docstring=\"CI.\")\n\
         b = command_group(\"cd\", \"CD\", description=make(), docstring=__doc__)\n\
         c = command_group(\"ops\", \"Ops\", \"Ops.\", docstring=DOC)\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(
        groups,
        [
            group_row("ci", "CI", "CI."),
            group_row("cd", "CD", ""),
            group_row("ops", "Ops", "Ops.")
        ]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A docstring the parser can't read may be `None`, so a literal long
/// description beside it is unknown: kept, but never compared.
#[test]
fn long_description_beside_an_unreadable_docstring_is_unreadable() {
    let src = format!(
        "{GROUP_IMPORT}a = command_group(\"ci\", \"CI\", long_description=\"Long.\", docstring=DOC)\n\
         b = command_group(\"ci\", \"CI\", \"CI.\", \"Else.\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(groups, [group_row("ci", "CI", "")]);
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// An empty module docstring is still a docstring: `__doc__` is `""`, not
/// `None`, so it contradicts a description, but doesn't describe a group.
#[test]
fn empty_module_docstring_is_passed_but_does_not_describe() {
    let contradictory = format!(
        "\"\"\"\"\"\"\n{GROUP_IMPORT}g = command_group(\"ci\", \"CI\", description=\"X\", docstring=__doc__)\n"
    );
    assert_eq!(
        build_error_for(&[("tools/m.py", &contradictory)]),
        "command groups with contradictory arguments (1):\n  \
         - `ci` in tools.m: pass `docstring=` or `description=`/`long_description=`, not both."
    );
    let undescribed = format!(
        "\"\"\"\"\"\"\n{GROUP_IMPORT}g = command_group(\"ci\", \"CI\", docstring=__doc__)\n"
    );
    assert_eq!(
        build_error_for(&[("tools/m.py", &undescribed)]),
        "command groups with no description (1):\n  \
         - `ci` (declared in tools.m): give one declaration a description or docstring the \
         static parser can read: a string literal, or the `__doc__` of a module with a docstring."
    );
}

/// A module that binds `__doc__` outside a function or class makes
/// `__doc__` unreadable there, wherever the binding sits.
#[test]
fn rebinding_doc_makes_it_unreadable() {
    let (groups, warnings) = redeclared(&[
        (
            "tools/a.py",
            "__doc__ = \"A.\"\ng = command_group(\"a\", \"A\", docstring=__doc__)\n",
        ),
        (
            "tools/b.py",
            "\"\"\"B doc.\"\"\"\nfrom x import y as __doc__\n\
             g = command_group(\"b\", \"B\", description=\"B.\", docstring=__doc__)\n",
        ),
        (
            "tools/c.py",
            "try:\n    pass\nexcept E as __doc__:\n    pass\n\
             g = command_group(\"c\", \"C\", docstring=__doc__)\n",
        ),
        (
            "tools/d.py",
            "\"\"\"D doc.\"\"\"\nimport os\nclass K:\n    __doc__ = \"k\"\n\
             def f():\n    __doc__ = \"f\"\n\
             try:\n    pass\nexcept E:\n    pass\n\
             g = command_group(\"d\", docstring=__doc__)\n",
        ),
        (
            "tools/e.py",
            "g = command_group(\"e\", \"E\", docstring=__doc__)\n\
             if True:\n    for __doc__ in []:\n        pass\n",
        ),
    ]);
    assert_eq!(
        groups,
        [
            group_row("a", "A", ""),
            group_row("b", "B", "B."),
            group_row("c", "C", ""),
            group_row("d", "D doc.", "D doc."),
            group_row("e", "E", ""),
        ]
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// The warning names the module each kept value came from, which needn't
/// be the first declaration's.
#[test]
fn warning_names_the_module_of_each_kept_field() {
    let (groups, warnings) = redeclared(&[
        (
            "tools/m1.py",
            "g = command_group(\"ci\", description=\"CI.\")\n",
        ),
        ("tools/m2.py", "g = command_group(\"ci\", \"From m2\")\n"),
        ("tools/m3.py", "g = command_group(\"ci\", \"From m3\")\n"),
        (
            "tools/m4.py",
            "g = command_group(\"ci\", \"Other\", \"Else.\", \"Long.\")\n",
        ),
        (
            "tools/m5.py",
            "g = command_group(\"ci\", \"Other\", \"Else.\", \"Longer.\")\n",
        ),
    ]);
    assert_eq!(groups, [group_row("ci", "From m2", "CI.")]);
    assert_eq!(
        warnings,
        [
            "group `ci` is declared more than once; keeping the title from tools.m2 and \
             ignoring the different title in tools.m3",
            "group `ci` is declared more than once; keeping the title from tools.m2 and the \
             description from tools.m1 and ignoring the different title, description in tools.m4",
            "group `ci` is declared more than once; keeping the title from tools.m2 and the \
             description from tools.m1 and the long_description from tools.m4 and ignoring \
             the different title, description, long_description in tools.m5",
        ]
    );
}

/// An unreadable value fills an empty field, which a later readable value
/// then can't.
#[test]
fn unreadable_value_fills_an_empty_field() {
    let src = format!(
        "{GROUP_IMPORT}a = command_group(\"ci\", \"CI\")\n\
         b = command_group(\"ci\", description=make())\n\
         c = command_group(\"ci\", \"CI\", \"CI.\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(groups, [group_row("ci", "CI", "")]);
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// Long descriptions aren't in the manifest, but conflict like the rest.
#[test]
fn conflicting_long_description_warns() {
    let src = format!(
        "{GROUP_IMPORT}a = command_group(\"ci\", \"CI\", \"CI.\")\n\
         b = command_group(\"ci\", \"CI\", \"CI.\", \"Long.\")\n\
         c = command_group(\"ci\", \"CI\", \"CI.\", long_description=\"Long.\")\n\
         d = command_group(\"ci\", \"Other\", \"CI.\", \"Else.\")\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(groups, [group_row("ci", "CI", "CI.")]);
    assert_eq!(
        warnings,
        [
            "group `ci` is declared more than once; keeping the title, long_description from tools.m and \
             ignoring the different title, long_description in tools.m"
        ]
    );
}

/// `title=` and `parent=` read the same by keyword as by position.
#[test]
fn title_and_parent_read_by_keyword_or_position() {
    let src = format!(
        "{GROUP_IMPORT}p = command_group(\"p\", title=\"P\", description=\"P.\")\n\
         command_group(\"p\", \"Other\")\n\
         command_group(\"kid\", \"Kid\", \"Kid.\", None, None, \"p\")\n\
         command_group(\"var\", \"Var\", \"Var.\", None, None, p)\n"
    );
    let (groups, warnings) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(
        groups,
        [
            group_row("p", "P", "P."),
            group_row("p.kid", "Kid", "Kid."),
            group_row("p.var", "Var", "Var.")
        ]
    );
    assert_eq!(
        warnings,
        [
            "group `p` is declared more than once; keeping the title from tools.m and \
             ignoring the different title in tools.m"
        ]
    );
}

/// `d` and `e` pass `None`, which is no argument at all.
#[test]
fn description_and_docstring_together_fail() {
    let src = format!(
        "\"\"\"Doc.\"\"\"\n{GROUP_IMPORT}\
         a = command_group(\"a\", \"A\", description=\"A.\", docstring=__doc__)\n\
         b = command_group(\"b\", \"B\", \"B.\", docstring=\"B.\")\n\
         c = command_group(\"c\", long_description=\"C.\", docstring=__doc__)\n\
         d = command_group(\"d\", \"D\", None, None, \"D.\")\n\
         e = command_group(\"e\", \"E\", description=None, docstring=__doc__)\n\
         f = command_group(\"a.f\", \"F\", \"F.\", None, \"F.\")\n"
    );
    assert_eq!(
        build_error_for(&[("tools/m.py", &src)]),
        "command groups with contradictory arguments (4):\n  \
         - `a` in tools.m: pass `docstring=` or `description=`/`long_description=`, not both.\n  \
         - `b` in tools.m: pass `docstring=` or `description=`/`long_description=`, not both.\n  \
         - `c` in tools.m: pass `docstring=` or `description=`/`long_description=`, not both.\n  \
         - `a.f` in tools.m: pass `docstring=` or `description=`/`long_description=`, not both."
    );
}

/// Without a module docstring, `__doc__` is `None`: no contradiction.
#[test]
fn doc_reference_without_a_module_docstring_is_no_contradiction() {
    let src = format!(
        "{GROUP_IMPORT}g = command_group(\"ci\", \"CI\", description=\"CI.\", docstring=__doc__)\n"
    );
    let (groups, _) = redeclared(&[("tools/m.py", &src)]);
    assert_eq!(groups, [group_row("ci", "CI", "CI.")]);
}

/// The tree `tests/testing/test_groups.py::test_paths_follow_the_parsers_resolution`
/// reads with `toolr.testing._groups`; both must give these paths.
#[test]
fn group_paths_match_the_testers_port() {
    let a = "ci = command_group(\"ci\", \"CI\", \"CI.\")\n\
             command_group(\"bare\", \"Bare\", \"Bare.\")\n\
             z = other(\"ignored\")\nz = 1\npass\n";
    let b = "img = command_group(\"ci.image\", \"Image\", \"Image.\")\n\
             kw = command_group(\"kw\", \"Kw\", \"Kw.\", parent=img)\n\
             lit = command_group(\"lit\", \"Lit\", \"Lit.\", parent=\"ci\")\n\
             dotted = command_group(\"ci.dot\", \"Dot\", \"Dot.\", parent=\"ignored\")\n\
             dotted2 = command_group(\"bare.kid\", \"Kid\", \"Kid.\")\n";
    let (groups, _) = redeclared(&[
        ("tools/a.py", a),
        ("tools/b.py", b),
        (
            "tools/pkg/__init__.py",
            "command_group(\"pkg\", \"Pkg\", \"Pkg.\")\n",
        ),
        (
            "tools/.hidden/m.py",
            "command_group(\"hidden\", \"H\", \"H.\")\n",
        ),
    ]);
    let paths: Vec<&str> = groups.iter().map(|g| g.0.as_str()).collect();
    assert_eq!(
        paths,
        [
            "ci",
            "bare",
            "ci.image",
            "ci.image.kw",
            "ci.lit",
            "ci.dot",
            "bare.kid",
            "pkg"
        ]
    );
}

/// Reported before anything else, as the runtime raises at import.
#[test]
fn contradictory_arguments_are_reported_first() {
    let src = format!(
        "\"\"\"Doc.\"\"\"\n{GROUP_IMPORT}\
         g = command_group(\"ci.lint\", description=\"L.\", docstring=__doc__)\n\
         command_group(\"bare\", \"Bare\")\n\
         @command(group=\"nope\")\ndef f(ctx):\n    \"\"\"F.\"\"\"\n"
    );
    assert_eq!(
        build_error_for(&[("tools/m.py", &src)]),
        "command groups with contradictory arguments (1):\n  \
         - `ci.lint` in tools.m: pass `docstring=` or `description=`/`long_description=`, not both."
    );
}

/// `@command(group="…")` in one file referring to a
/// `command_group(...)` declared in another file resolves cleanly
/// regardless of the file scan order.
#[test]
fn cross_file_command_group_string_path_resolves() {
    let tmp = TempDir::new().unwrap();
    // gh_actions.py declares an @command(group="ci") even though
    // ci's command_group(...) lives in _common.py — which sorts
    // earlier, but the registry collection pass makes order
    // irrelevant anyway.
    write(
        tmp.path(),
        "tools/_common.py",
        r#""""CI utilities."""
command_group("ci", docstring=__doc__)
"#,
    );
    write(
        tmp.path(),
        "tools/gh_actions.py",
        r#""""GH Actions helpers."""
from toolr import command
@command(group="ci")
def check_run_build(ctx):
    """Check."""
    pass
"#,
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    assert_eq!(m.commands.len(), 1);
    assert_eq!(m.commands[0].group, "ci");
    assert_eq!(m.commands[0].name, "check-run-build");
}

/// Typo'd `group=` reference surfaces as a build error with a
/// nearest-neighbour suggestion.
#[test]
fn unknown_group_ref_errors_with_suggestion() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/c.py",
        r#""""CI utilities."""
from toolr import command
command_group("ci.helm-diff-pr-comment", docstring=__doc__)

@command(group="ci.helm-diff-pre-comment")
def backend(ctx):
    """Backend."""
    pass
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown group references"), "got: {msg}");
    assert!(msg.contains("ci.helm-diff-pre-comment"), "got: {msg}");
    assert!(msg.contains("Did you mean"), "got: {msg}");
    assert!(msg.contains("ci.helm-diff-pr-comment"), "got: {msg}");
}

/// Equally close groups suggest the smallest name, whatever order the
/// set yields, as `toolr.testing` does.
#[test]
fn nearest_group_breaks_ties_by_smallest_name() {
    for order in [["ab", "ca", "ac"], ["ca", "ac", "ab"], ["ac", "ab", "ca"]] {
        for _ in 0..16 {
            let registered: HashSet<String> = order.iter().map(|s| s.to_string()).collect();
            assert_eq!(
                nearest_group("aa", &registered).as_deref(),
                Some("ab"),
                "{order:?}"
            );
        }
    }
}

/// Scratch `e`: two defs in one module, the second renamed onto the first.
const DUP_ONE_MODULE: &str = r#"from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def dup(ctx):
    """One."""


@g.command(name="dup")
def dup2(ctx):
    """Two."""
"#;

/// Scratch `f`: the same module twice.
const DUP_MODULE: &str = r#"from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def dup(ctx):
    """Dup."""
"#;

fn build_error_for(files: &[(&str, &str)]) -> String {
    let tmp = TempDir::new().unwrap();
    for (name, contents) in files {
        write(tmp.path(), name, contents);
    }
    build_static_manifest_inner(&tmp.path().join("tools"))
        .expect_err("expected the build to fail")
        .to_string()
}

#[test]
fn duplicate_command_in_one_module_fails() {
    assert_eq!(
        build_error_for(&[("tools/m.py", DUP_ONE_MODULE)]),
        "duplicate commands (1):\n  - `grp dup` is defined more than once: tools.m::dup, \
         tools.m::dup2. Rename all but one, e.g. with `name=\"…\"`."
    );
}

#[test]
fn duplicate_command_across_modules_fails() {
    assert_eq!(
        build_error_for(&[("tools/m2.py", DUP_MODULE), ("tools/m1.py", DUP_MODULE)]),
        "duplicate commands (1):\n  - `grp dup` is defined more than once: tools.m1::dup, \
         tools.m2::dup. Rename all but one, e.g. with `name=\"…\"`."
    );
}

#[test]
fn three_definitions_list_all_three() {
    let third = "from toolr import command\n\n\n@command(\"dup\", group=\"grp\")\ndef other(ctx):\n    \"\"\"Three.\"\"\"\n\n\n@command(group=\"grp\")\ndef solo(ctx):\n    \"\"\"Solo.\"\"\"\n";
    assert_eq!(
        build_error_for(&[
            ("tools/a.py", DUP_ONE_MODULE),
            ("tools/b.py", third),
            (
                "tools/c.py",
                "from toolr import command_group\n\ng = command_group(\"other\", \"O\", \"O\")\n\n\n@g.command\ndef dup(ctx):\n    \"\"\"Elsewhere.\"\"\"\n\n\n@g.command(\"x\\x1b\")\ndef esc(ctx):\n    \"\"\"Esc.\"\"\"\n\n\n@g.command(\"x\\x1b\")\ndef esc2(ctx):\n    \"\"\"Esc.\"\"\"\n"
            ),
        ]),
        "duplicate commands (2):\n  - `grp dup` is defined more than once: tools.a::dup, \
         tools.a::dup2, tools.b::other. Rename all but one, e.g. with `name=\"…\"`.\n  \
         - `other x\\u{1b}` is defined more than once: tools.c::esc, tools.c::esc2. Rename \
         all but one, e.g. with `name=\"…\"`."
    );
}

#[test]
fn unknown_group_reported_before_duplicate() {
    let typo = "from toolr import command\n\n\n@command(group=\"gpr\")\ndef typo(ctx):\n    \"\"\"Typo.\"\"\"\n";
    let msg = build_error_for(&[("tools/m.py", DUP_ONE_MODULE), ("tools/t.py", typo)]);
    assert!(msg.starts_with("unknown group references (1):"), "{msg}");
}

/// `toolr self build-manifest` on a plugin package gets the same error.
#[test]
fn plugin_package_with_a_duplicate_fails() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "my_plugin/__init__.py", "");
    write(tmp.path(), "my_plugin/m1.py", DUP_MODULE);
    write(tmp.path(), "my_plugin/m2.py", DUP_MODULE);
    let err = build_commands(&tmp.path().join("my_plugin"), "my_plugin").unwrap_err();
    assert_eq!(
        err.to_string(),
        "duplicate commands (1):\n  - `grp dup` is defined more than once: my_plugin.m1::dup, \
         my_plugin.m2::dup. Rename all but one, e.g. with `name=\"…\"`."
    );
}

/// `(group, name)` of every command the tree builds, sorted.
fn built_commands(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let tmp = TempDir::new().unwrap();
    for (name, contents) in files {
        write(tmp.path(), name, contents);
    }
    let m = build_static_manifest(&tmp.path().join("tools"))
        .expect("expected a build")
        .manifest;
    let mut out: Vec<_> = m.commands.into_iter().map(|c| (c.group, c.name)).collect();
    out.sort();
    out
}

fn grp(names: &[&str]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|n| ("grp".to_string(), n.to_string()))
        .collect()
}

/// Scratch `a`: `@toolr.command` next to the plain form.
#[test]
fn toolr_attribute_command_builds() {
    let src = r#"import toolr
from toolr import command

toolr.command_group("grp", "Grp", "Grp")


@command(group="grp")
def plain(ctx):
    """Plain."""


@toolr.command(group="grp")
def attr(ctx):
    """Attribute."""


@toolr.command("named", group="grp")
def other(ctx):
    """Named."""
"#;
    assert_eq!(
        built_commands(&[("tools/m.py", src)]),
        grp(&["attr", "named", "plain"])
    );
}

#[test]
fn toolr_module_alias_command_builds() {
    let src = r#"import toolr as t
from toolr import command

t.command_group("grp", "Grp", "Grp")


@command(group="grp")
def plain(ctx):
    """Plain."""


@t.command(name="aliased", group="grp")
def attr(ctx):
    """Alias."""
"#;
    assert_eq!(
        built_commands(&[("tools/m.py", src)]),
        grp(&["aliased", "plain"])
    );
}

/// `import toolr.types` binds `toolr` itself, so `toolr.command` works.
#[test]
fn toolr_submodule_import_still_binds_toolr() {
    let src = "import toolr.types\n\ntoolr.command_group(\"grp\", \"Grp\", \"Grp\")\n\n\n@toolr.command(group=\"grp\")\ndef attr(ctx):\n    \"\"\"Attribute.\"\"\"\n";
    assert_eq!(built_commands(&[("tools/m.py", src)]), grp(&["attr"]));
}

/// Scratch `b`: `from toolr import command as cmd`.
#[test]
fn aliased_command_import_builds() {
    let src = r#"from toolr import command, command_group
from toolr import command as cmd

command_group("grp", "Grp", "Grp")


@command(group="grp")
def plain(ctx):
    """Plain."""


@cmd(group="grp")
def aliased(ctx):
    """Aliased."""
"#;
    assert_eq!(
        built_commands(&[("tools/m.py", src)]),
        grp(&["aliased", "plain"])
    );
}

#[test]
fn bare_toolr_attribute_and_alias_without_group_are_reported() {
    let src = "import toolr\nfrom toolr import command as cmd\n\n\n@toolr.command\ndef attr(ctx):\n    \"\"\"A.\"\"\"\n\n\n@cmd\ndef aliased(ctx):\n    \"\"\"B.\"\"\"\n";
    assert_eq!(
        build_error_for(&[("tools/m.py", src)]),
        "unknown group references (2):\n  - tools.m::attr: `@command` is missing a \
         `group=...` kwarg.\n  - tools.m::aliased: `@command` is missing a `group=...` kwarg."
    );
}

#[test]
fn toolr_attribute_name_conflict_is_reported() {
    let src = "import toolr\n\ntoolr.command_group(\"grp\", \"Grp\", \"Grp\")\n\n\n@toolr.command(\"x\", name=\"y\", group=\"grp\")\ndef f(ctx):\n    \"\"\"F.\"\"\"\n";
    assert!(
        build_error_for(&[("tools/m.py", src)])
            .starts_with("conflicting command name (1):\n  - tools.m::f: "),
    );
}

/// `@other.command` where `other` is neither a group nor `toolr`, and an
/// alias of some other library's `command` called without `group=`, stay ignored.
#[test]
fn foreign_command_decorators_stay_ignored() {
    let src = "import click as other\nfrom click import command as cmd\nfrom toolr import command_group\n\ng = command_group(\"grp\", \"Grp\", \"Grp\")\n\n\n@other.command(group=\"grp\")\ndef a(ctx):\n    pass\n\n\n@cmd(name=\"b\")\ndef b(ctx):\n    pass\n\n\n@cmd\ndef c(ctx):\n    pass\n\n\n@g.command\ndef kept(ctx):\n    \"\"\"Kept.\"\"\"\n";
    assert_eq!(built_commands(&[("tools/m.py", src)]), grp(&["kept"]));
}

/// `command` from a `toolr.*` submodule, or assigned from toolr's, is toolr's;
/// one from a module toolr can't read counts only when called with `group=`.
#[test]
fn command_bindings_toolr_can_follow() {
    let common =
        "from toolr import command, command_group\n\ncommand_group(\"grp\", \"Grp\", \"Grp\")\n";
    let doc = "    \"\"\"Doc.\"\"\"\n";
    let module =
        |imports: &str, decorator: &str| format!("{imports}\n\n\n@{decorator}\ndef f(ctx):\n{doc}");
    let files = [
        ("tools/_common.py", common.to_string()),
        (
            "tools/_reexport.py",
            "from mylib import command\n".to_string(),
        ),
        (
            "tools/a.py",
            module(
                "from toolr._decorators import command",
                "command(\"a\", group=\"grp\")",
            ),
        ),
        (
            "tools/b.py",
            module(
                "import toolr as t\ncommand = t.command\ncmd = command",
                "cmd(\"b\", group=\"grp\")",
            ),
        ),
        (
            "tools/c.py",
            module(
                "import toolr.types\ncommand = toolr.command",
                "command(\"c\", group=\"grp\")",
            ),
        ),
        (
            "tools/d.py",
            module(
                "import tools._common as common\ncommand = common.command",
                "command(\"d\", group=\"grp\")",
            ),
        ),
        (
            "tools/e.py",
            module("from mylib import command", "command(\"e\", group=\"grp\")"),
        ),
        (
            "tools/f.py",
            module("from mylib import *", "command(\"f\", group=\"grp\")"),
        ),
        (
            "tools/g.py",
            module("from toolr.types import *", "command(\"g\", group=\"grp\")"),
        ),
        (
            "tools/h.py",
            module(
                "from tools._reexport import command",
                "command(\"h\", group=\"grp\")",
            ),
        ),
        (
            "tools/i.py",
            module(
                "import mylib\ncommand = mylib.command",
                "command(\"i\", group=\"grp\")",
            ),
        ),
        // Not toolr's, whatever the call.
        (
            "tools/n1.py",
            module(
                "import toolr\ncommand = toolr.arg",
                "command(\"n1\", group=\"grp\")",
            ),
        ),
        (
            "tools/n2.py",
            module(
                "from toolr import arg as command",
                "command(\"n2\", group=\"grp\")",
            ),
        ),
        (
            "tools/n3.py",
            module("command = undefined", "command(\"n3\", group=\"grp\")"),
        ),
        (
            "tools/n4.py",
            module(
                "command = helpers.command",
                "command(\"n4\", group=\"grp\")",
            ),
        ),
        (
            "tools/n5.py",
            module("command = make()", "command(\"n5\", group=\"grp\")"),
        ),
        (
            "tools/n10.py",
            module(
                "import toolr.types\ncommand = toolr.types.command",
                "command(\"n10\", group=\"grp\")",
            ),
        ),
        (
            "tools/n6.py",
            module(
                "a = b\nb = a\ncommand = a",
                "command(\"n6\", group=\"grp\")",
            ),
        ),
        (
            "tools/n7.py",
            module(
                "import toolr\ncommand = toolr.command\n\n\ndef command(f):\n    return f",
                "command(\"n7\", group=\"grp\")",
            ),
        ),
        (
            "tools/n8.py",
            module("from mylib import command", "command(\"n8\")"),
        ),
        (
            "tools/n9.py",
            module("from mylib import command", "command"),
        ),
    ];
    let files: Vec<_> = files.iter().map(|(n, c)| (*n, c.as_str())).collect();
    assert_eq!(
        built_commands(&files),
        grp(&["a", "b", "c", "d", "e", "f", "g", "h", "i"])
    );
}

/// A nested `def` under a `command` from a module toolr can't read fails
/// like toolr's own when called with `group=`.
#[test]
fn nested_command_from_an_unread_module_is_reported() {
    let src = "from mylib import command\nfrom toolr import command_group\n\ncommand_group(\"grp\", \"Grp\", \"Grp\")\n\n\ndef register():\n    @command(group=\"grp\")\n    def nested(ctx):\n        \"\"\"Nested.\"\"\"\n\n    @command()\n    def other(ctx):\n        \"\"\"Other.\"\"\"\n";
    assert_eq!(
        build_error_for(&[("tools/m.py", src)]),
        "unsupported command declarations (1):\n\
         \x20 - tools.m::nested: `@command` must decorate a top-level `def`, which toolr reads without running it."
    );
}

const GROUP_BINDING_LINE: &str = "`command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, which toolr reads without running it.";

#[test]
fn group_binding_forms_fail_the_build() {
    let src = "from toolr import command_group\n\na, b = command_group(\"ga\", \"Ga\", \"Ga\"), command_group(\"gb\", \"Gb\", \"Gb\")\n\nif a:\n    c = command_group(\"gc\", \"Gc\", \"Gc\")\n";
    assert_eq!(
        build_error_for(&[("tools/m.py", src)]),
        format!(
            "unsupported command declarations (3):\n  - tools.m::command_group(\"ga\"): {GROUP_BINDING_LINE}\n  - tools.m::command_group(\"gb\"): {GROUP_BINDING_LINE}\n  - tools.m::command_group(\"gc\"): {GROUP_BINDING_LINE}"
        )
    );
}

#[test]
fn group_bindings_are_reported_before_commands() {
    let first = "from toolr import command, command_group\n\ncommand_group(\"grp\", \"Grp\", \"Grp\")\n\n\ndef register():\n    @command(group=\"grp\")\n    def nested(ctx):\n        \"\"\"Nested.\"\"\"\n\n\n(g := command_group(\"w\", \"W\", \"W\"))\n";
    let second =
        "from toolr import command_group\n\nif True:\n    command_group(\"n\", \"N\", \"N\")\n";
    assert_eq!(
        build_error_for(&[("tools/m.py", first), ("tools/n.py", second)]),
        format!(
            "unsupported command declarations (3):\n  - tools.m::command_group(\"w\"): {GROUP_BINDING_LINE}\n  - tools.n::command_group(\"n\"): {GROUP_BINDING_LINE}\n  - tools.m::nested: `@command` must decorate a top-level `def`, which toolr reads without running it."
        )
    );
}

/// Every shape toolr can't read without running it: nested `def`s
/// (another `def`, an `if`, a class) and non-literal `group=`/names.
const UNSUPPORTED_PY: &str = r#"from toolr import command, command_group

GROUP = "grp"
NAME = "renamed"
g = command_group("grp", "Grp", "Grp")


def register():
    @g.command
    def nested(ctx):
        """Nested."""


register()


if True:

    @command(group="grp")
    def guarded(ctx):
        """Guarded."""


class Holder:
    @g.command
    def method(self, ctx):
        """Method."""


@command(group=GROUP)
def nonlit(ctx):
    """Non-literal group."""


@command(NAME, group="grp")
def renamed(ctx):
    """Non-literal name."""


@g.command(name=NAME)
def renamed_kw(ctx):
    """Non-literal name keyword."""


def other():
    @cli.command  # noqa: F821
    def ignored(ctx):
        pass
"#;

#[test]
fn unsupported_command_declarations_fail_the_build() {
    assert_eq!(
        build_error_for(&[("tools/m.py", UNSUPPORTED_PY)]),
        "unsupported command declarations (6):\n\
         \x20 - tools.m::nested: `@command` must decorate a top-level `def`, which toolr reads without running it.\n\
         \x20 - tools.m::guarded: `@command` must decorate a top-level `def`, which toolr reads without running it.\n\
         \x20 - tools.m::method: `@command` must decorate a top-level `def`, which toolr reads without running it.\n\
         \x20 - tools.m::nonlit: `group=` must be a string literal.\n\
         \x20 - tools.m::renamed: the command name must be a string literal.\n\
         \x20 - tools.m::renamed_kw: the command name must be a string literal."
    );
}

#[test]
fn stacked_command_decorators_fail_the_build() {
    let src = "import functools\nimport toolr\nfrom toolr import command as cmd\n\ng1 = toolr.command_group(\"g1\", \"G\", \"G\")\ng2 = toolr.command_group(\"g2\", \"G\", \"G\")\n\n\n@g1.command\n@g2.command\ndef both(ctx):\n    \"\"\"Both.\"\"\"\n\n\n@g1.command\n@cmd(group=\"g2\")\ndef mixed(ctx):\n    \"\"\"Mixed.\"\"\"\n\n\n@functools.lru_cache\n@g1.command\ndef cached(ctx):\n    \"\"\"Cached.\"\"\"\n";
    assert_eq!(
        build_error_for(&[("tools/m.py", src)]),
        "unsupported command declarations (2):\n\
         \x20 - tools.m::both: a function can carry only one command decorator.\n\
         \x20 - tools.m::mixed: a function can carry only one command decorator."
    );
}

/// A click `command` without `group=` and a local `def command` aren't toolr's.
#[test]
fn foreign_command_decorators_are_ignored() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/m.py",
        "from click import command\nfrom toolr import command_group\n\ng = command_group(\"grp\", \"G\", \"G\")\n\n\ndef make():\n    @command()\n    def cli():\n        \"\"\"Click.\"\"\"\n",
    );
    write(
        tmp.path(),
        "tools/n.py",
        "from toolr import *\n\n\ndef command(f):\n    return f\n\n\nclass Helper:\n    @command\n    def method(self):\n        \"\"\"Local.\"\"\"\n",
    );
    write(
        tmp.path(),
        "tools/o.py",
        "from toolr import *\n\n\n@command(group=\"grp\")\ndef star(ctx):\n    \"\"\"Star.\"\"\"\n",
    );
    let manifest = build_static_manifest_inner(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let names: Vec<_> = manifest
        .commands
        .iter()
        .map(|c| c.function.as_str())
        .collect();
    assert_eq!(names, ["star"]);
}

/// A `command` re-exported through other `tools` modules is still toolr's.
#[test]
fn reexported_command_is_followed() {
    let tmp = TempDir::new().unwrap();
    let doc = "    \"\"\"Doc.\"\"\"\n";
    let files = [
        (
            "tools/_common.py",
            "from toolr import command, command_group\n\ncommand_group(\"grp\", \"G\", \"G\")\n"
                .to_string(),
        ),
        (
            "tools/a.py",
            format!(
                "from ._common import command\n\n\n@command(group=\"grp\")\ndef relative(ctx):\n{doc}"
            ),
        ),
        (
            "tools/b.py",
            format!(
                "from tools._common import *\n\n\n@command(group=\"grp\")\ndef star(ctx):\n{doc}"
            ),
        ),
        (
            "tools/c.py",
            format!(
                "from toolr import *\nfrom click import command\n\n\ndef make():\n    @command()\n    def cli():\n    {doc}"
            ),
        ),
        (
            "tools/d.py",
            format!(
                "from tools.e import command\n\n\ndef make():\n    @command\n    def cycle():\n    {doc}"
            ),
        ),
        ("tools/e.py", "from tools.d import command\n".to_string()),
    ];
    for (name, contents) in &files {
        write(tmp.path(), name, contents);
    }
    let manifest = build_static_manifest_inner(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let names: Vec<_> = manifest
        .commands
        .iter()
        .map(|c| c.function.as_str())
        .collect();
    assert_eq!(names, ["relative", "star"]);
}

/// A group binding from an earlier file counts in a nested `def`, and the
/// check runs before the unknown-group one.
#[test]
fn nested_command_on_an_imported_binding_is_reported_first() {
    let common =
        "from toolr import command_group\n\ng = command_group(\"grp\", \"Grp\", \"Grp\")\n";
    let user = "import toolr\nfrom tools._common import g\n\n\ndef register():\n    @g.command\n    def inner(ctx):\n        \"\"\"Inner.\"\"\"\n\n    @toolr.command(group=\"nope\")\n    async def other(ctx):\n        \"\"\"Other.\"\"\"\n\n\n@toolr.command(group=\"typo\")\ndef typo(ctx):\n    \"\"\"Typo.\"\"\"\n";
    assert_eq!(
        build_error_for(&[("tools/_common.py", common), ("tools/user.py", user)]),
        "unsupported command declarations (2):\n\
         \x20 - tools.user::inner: `@command` must decorate a top-level `def`, which toolr reads without running it.\n\
         \x20 - tools.user::other: `@command` must decorate a top-level `def`, which toolr reads without running it."
    );
}

#[test]
fn unsupported_message_escapes_control_chars() {
    let msg = BuildError::UnsupportedCommands(vec![UnsupportedCommand {
        module: "tools.m\x1b".into(),
        function: "f\x07".into(),
        kind: UnsupportedCommandKind::NonLiteralGroup,
    }])
    .to_string();
    assert_eq!(
        msg,
        "unsupported command declarations (1):\n  - tools.m\\u{1b}::f\\u{7}: `group=` must be a string literal."
    );
}

/// Several bad references list one per line, in source order.
#[test]
fn unknown_group_refs_list_one_per_line() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/c.py",
        r#""""CI utilities."""
from toolr import command
@command(group="nope")
def first(ctx):
    """First."""
    pass

@command(group="nada")
def second(ctx):
    """Second."""
    pass
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    let lines: Vec<&str> = msg.lines().collect();
    assert_eq!(lines.len(), 3, "got: {msg}");
    assert!(
        lines[0].starts_with("unknown group references (2)"),
        "got: {msg}"
    );
    assert!(lines[1].contains("`nope`"), "got: {msg}");
    assert!(lines[2].contains("`nada`"), "got: {msg}");
}

/// The user's `[tool.toolr.argparse.*]` blocks in
/// `tools/pyproject.toml` graft children onto user-decorated
/// dispatcher commands inside `build_static_manifest`. Verifies the
/// dotted-name derivation that maps a `command_group("django")` +
/// `def django(...)` dispatcher onto pyproject's `parent = "django"`.
#[test]
fn build_static_manifest_grafts_argparse_children() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/dispatcher.py",
        r#""""Django dispatcher."""
group = command_group("django", "Django", "Django.")

@group.command
def django(ctx):
    """Dispatch to manage.py."""
    pass
"#,
    );
    write(
        tmp.path(),
        "apps/x/management/commands/migrate.py",
        "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n",
    );
    write(
        tmp.path(),
        "tools/pyproject.toml",
        r#"
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]

[[tool.toolr.argparse.django.attach]]
parent = "django"
"#,
    );

    let manifest = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let names: std::collections::BTreeSet<_> =
        manifest.commands.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains("django"), "names: {names:?}");
    assert!(names.contains("migrate"), "names: {names:?}");
    let migrate = manifest
        .commands
        .iter()
        .find(|c| c.name == "migrate")
        .unwrap();
    assert_eq!(migrate.group, "django");
    assert_eq!(migrate.dispatched_from.as_deref(), Some("argparse:django"),);
    let django = manifest
        .commands
        .iter()
        .find(|c| c.name == "django")
        .unwrap();
    assert_eq!(migrate.module, django.module);
    assert_eq!(migrate.function, django.function);
    assert!(
        django.is_dispatcher,
        "expected dispatcher flag set on django",
    );
}

/// A dispatcher command with both a real CLI flag (`cpu`) and a
/// `DispatchCommand`-annotated injection kwarg builds cleanly: the
/// CLI flag lands on the manifest, the injection kwarg is dropped
/// rather than rejected as "unsupported type `DispatchCommand`".
#[test]
fn dispatcher_with_real_flags_and_injection_kwarg_builds() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/dispatcher.py",
        r#""""Django dispatcher."""
from toolr.sources import DispatchCommand

group = command_group("django", "Django", "Django.")

@group.command
def django(ctx, *, cpu: str = "1", dispatched: DispatchCommand) -> int:
    """Dispatch."""
    return 0
"#,
    );
    let manifest = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let django = manifest
        .commands
        .iter()
        .find(|c| c.name == "django")
        .expect("django command present");
    let arg_names: Vec<&str> = django.arguments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        arg_names,
        vec!["cpu"],
        "expected dispatcher arguments to keep `cpu` and drop `dispatched`",
    );
}

/// Bare `@command` (no `group=` kwarg) is a build error pointing
/// at the missing kwarg.
#[test]
fn bare_direct_command_missing_group_errors() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/c.py",
        r#""""CI utilities."""
from toolr import command
command_group("ci", docstring=__doc__)

@command
def hello(ctx):
    """Hi."""
    pass
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("missing a `group=...` kwarg"), "got: {msg}");
}

/// `T | None` (no default) is accepted as a single zero-or-one
/// positional. This is the user's primary use case (e.g.
/// `def bump(ctx, new_version: str | None)`).
#[test]
fn accepts_single_zero_or_one_positional() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/v.py",
        r#""""Version utils."""
group = command_group("version", "Version", docstring=__doc__)

@group.command
def bump(ctx, new_version: str | None) -> None:
    """Bump.

    Args:
        new_version: Explicit version.
    """
"#,
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let bump = m.commands.iter().find(|c| c.name == "bump").unwrap();
    assert_eq!(bump.arguments.len(), 1);
    assert_eq!(bump.arguments[0].kind, ArgumentKind::Positional);
    assert!(matches!(
        bump.arguments[0].resolved_type,
        Some(SupportedType::Optional(_))
    ));
}

#[test]
fn rejects_multiple_zero_or_one_positionals() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/v.py",
        r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, a: str | None, b: str | None) -> None:
    """Bad.

    Args:
        a: first.
        b: second.
    """
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("two zero-or-one positionals"), "got: {msg}");
    assert!(msg.contains("`a`"), "got: {msg}");
    assert!(msg.contains("`b`"), "got: {msg}");
}

#[test]
fn rejects_required_positional_after_zero_or_one() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/v.py",
        r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, maybe: str | None, name: str) -> None:
    """Bad ordering.

    Args:
        maybe: optional.
        name: required.
    """
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("required positional `name` follows"),
        "got: {msg}"
    );
}

#[test]
fn rejects_zero_or_one_combined_with_var_positional() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/v.py",
        r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, maybe: str | None, *files: str) -> None:
    """Bad combo.

    Args:
        maybe: optional.
        files: trailing.
    """
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("cannot coexist with the variadic positional"),
        "got: {msg}"
    );
}

#[test]
fn rejects_command_name_passed_both_positionally_and_by_keyword() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/v.py",
        r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command("positional", name="keyword")
def f(ctx) -> None:
    """Ambiguous name."""
"#,
    );
    let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("conflicting command name") && msg.contains("v::f"),
        "got: {msg}"
    );
}

/// `T | None = None` (with a default) stays a `--flag` and doesn't
/// trip the zero-or-one validator. Regression guard.
#[test]
fn allows_optional_keyword_alongside_required_positional() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/v.py",
        r#""""OK."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, name: str, alias: str | None = None) -> None:
    """OK.

    Args:
        name: required.
        alias: optional flag.
    """
"#,
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let f = m.commands.iter().find(|c| c.name == "f").unwrap();
    // `alias` should be Optional (flag), not a positional.
    let kinds: Vec<ArgumentKind> = f.arguments.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        vec![ArgumentKind::Positional, ArgumentKind::Optional]
    );
}

/// GH #449: two unrelated modules each declaring a same-named enum
/// class (`Database`, with different members) must not clobber each
/// other's `allowed_values` in the manifest — each command's flag
/// keeps the choices from *its own* module's enum.
#[test]
fn colliding_bare_enum_names_across_modules_resolve_independently() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/module_a.py",
        r#""""Module A."""
import enum

group = command_group("a", "A", docstring=__doc__)

class Database(enum.StrEnum):
    PRIMARY = "primary"
    STANDBY = "standby"

@group.command
def cmd_a(ctx, *, database: Database = Database.PRIMARY) -> None:
    """Cmd A."""
"#,
    );
    write(
        tmp.path(),
        "tools/module_b.py",
        r#""""Module B."""
import enum

group = command_group("b", "B", docstring=__doc__)

class Database(enum.StrEnum):
    REPLICA = "replica"
    ARCHIVE = "archive"

@group.command
def cmd_b(ctx, *, database: Database = Database.REPLICA) -> None:
    """Cmd B."""
"#,
    );

    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let cmd_a = m.commands.iter().find(|c| c.name == "cmd-a").unwrap();
    let cmd_b = m.commands.iter().find(|c| c.name == "cmd-b").unwrap();
    let arg_a = cmd_a
        .arguments
        .iter()
        .find(|a| a.name == "database")
        .unwrap();
    let arg_b = cmd_b
        .arguments
        .iter()
        .find(|a| a.name == "database")
        .unwrap();

    assert_eq!(
        arg_a.allowed_values,
        vec!["primary".to_string(), "standby".to_string()]
    );
    assert_eq!(arg_a.default.as_deref(), Some("primary"));
    assert_eq!(
        arg_b.allowed_values,
        vec!["replica".to_string(), "archive".to_string()]
    );
    assert_eq!(arg_b.default.as_deref(), Some("replica"));
}

/// Three modules each declare an identically-shaped
/// `Environment(StrEnum)` (same members, same values) and a fourth
/// module imports the class from one of them rather than declaring
/// its own. The importing module previously hit the cross-module
/// collision guard — two-or-more cross-module defs with no local
/// match — and rejected the whole build with an unsupported-type
/// error, even though every candidate def serialises identically.
/// Building must succeed and the imported enum must still populate
/// `allowed_values`.
#[test]
fn imported_enum_with_identical_cross_module_defs_resolves() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/job.py",
        r#""""Job module — declares and uses Environment locally."""
import enum

group = command_group("job", "Job", docstring=__doc__)

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"

@group.command
def run(ctx, *, env: Environment = Environment.PRODUCTION) -> None:
    """Run."""
"#,
    );
    write(
        tmp.path(),
        "tools/helm_diff_pr_comment.py",
        r#""""Unrelated module with its own copy of the same enum shape."""
import enum

group = command_group("ci", "CI", docstring=__doc__)

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"

@group.command
def comment(ctx, *, env: Environment = Environment.PRODUCTION) -> None:
    """Comment."""
"#,
    );
    write(
        tmp.path(),
        "tools/metrics/_common.py",
        r#""""Shared metrics types."""
import enum

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"
"#,
    );
    write(
        tmp.path(),
        "tools/metrics/analyse.py",
        r#""""Metrics analysis — imports Environment from a sibling module."""
from ._common import Environment

group = command_group("metrics", "Metrics", docstring=__doc__)

@group.command
def analyse(ctx, *, env: Environment | None = None) -> None:
    """Analyse."""
"#,
    );

    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let analyse = m.commands.iter().find(|c| c.name == "analyse").unwrap();
    let arg = analyse.arguments.iter().find(|a| a.name == "env").unwrap();
    assert_eq!(
        arg.allowed_values,
        vec!["production".to_string(), "staging".to_string()]
    );
}

/// The general case an identical-members-only dedupe can't handle:
/// two *differently*-shaped same-named enum classes across
/// modules, resolved correctly via a genuine explicit import rather
/// than a lucky "only one candidate" guess. This is the case that
/// motivated the real import-following resolver.
#[test]
fn imported_enum_with_differently_shaped_collision_resolves_via_import() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/module_a.py",
        r#""""Module A."""
import enum

class Database(enum.StrEnum):
    PRIMARY = "primary"
    STANDBY = "standby"
"#,
    );
    write(
        tmp.path(),
        "tools/module_b.py",
        r#""""Module B."""
import enum

class Database(enum.StrEnum):
    REPLICA = "replica"
    ARCHIVE = "archive"
"#,
    );
    write(
        tmp.path(),
        "tools/module_c.py",
        r#""""Module C imports Database from module_b, not module_a."""
from .module_b import Database

group = command_group("c", "C", docstring=__doc__)

@group.command
def cmd_c(ctx, *, database: Database = Database.REPLICA) -> None:
    """Cmd C."""
"#,
    );

    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let cmd_c = m.commands.iter().find(|c| c.name == "cmd-c").unwrap();
    let arg = cmd_c
        .arguments
        .iter()
        .find(|a| a.name == "database")
        .unwrap();
    assert_eq!(
        arg.allowed_values,
        vec!["replica".to_string(), "archive".to_string()]
    );
    // The manifest's resolved_type carries the *declaring* module
    // (module_b), not the importing module (module_c) -- this is
    // what the Python runtime needs to lazily import the real
    // class at coercion time, independent of the annotation's own
    // module.
    match &arg.resolved_type {
        Some(crate::parser::types::SupportedType::Enum { module, .. }) => {
            assert_eq!(module, "tools.module_b");
        }
        other => panic!("expected Enum resolved_type, got {other:?}"),
    }
}

fn type_errors_for(files: &[(&str, &str)]) -> Vec<TypeResolutionError> {
    let tmp = TempDir::new().unwrap();
    for (name, contents) in files {
        write(tmp.path(), name, contents);
    }
    let err = build_static_manifest_inner(&tmp.path().join("tools"))
        .expect_err("expected the build to fail");
    let BuildError::UnsupportedTypes(errs) = err else {
        unreachable!("expected UnsupportedTypes")
    };
    errs
}

fn missing_docstrings_for(files: &[(&str, &str)]) -> Vec<MissingDocstring> {
    let tmp = TempDir::new().unwrap();
    for (name, contents) in files {
        write(tmp.path(), name, contents);
    }
    let err = build_static_manifest_inner(&tmp.path().join("tools"))
        .expect_err("expected the build to fail");
    let BuildError::MissingDocstrings(missing) = err else {
        unreachable!("expected MissingDocstrings, got {err}")
    };
    missing
}

/// Every docstring shape that leaves a command without a `--help`
/// summary line (#501).
const UNDOCUMENTED_PY: &str = r#"from toolr import Context, command_group

group = command_group("doc", "Doc test", description="Doc test.")


@group.command
def bare(ctx: Context) -> None:
    ctx.print("no docstring")


@group.command
def empty(ctx: Context) -> None:
    """"""


@group.command
def blank(ctx: Context) -> None:
    """   """


@group.command
def sections_only(ctx: Context, name: str) -> None:
    """
    Args:
        name: Who to greet.
    """


@group.command
def documented(ctx: Context) -> None:
    """Has a summary."""
"#;

#[test]
fn commands_without_a_docstring_summary_fail_the_build() {
    let missing = missing_docstrings_for(&[("tools/doc.py", UNDOCUMENTED_PY)]);
    let functions: Vec<&str> = missing.iter().map(|m| m.function.as_str()).collect();
    assert_eq!(functions, ["bare", "empty", "blank", "sections_only"]);
    assert!(
        missing.iter().all(|m| m.module == "tools.doc"),
        "{missing:?}"
    );
    let msg = BuildError::MissingDocstrings(missing).to_string();
    assert!(
        msg.starts_with("commands without a docstring (4):\n  - tools.doc::bare: "),
        "got: {msg}"
    );
}

#[test]
fn path_state_types_resolve_bare_in_containers_and_as_varargs() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/paths.py",
        r#"from toolr import Context, command_group
from toolr.types import DirectoryPath, ExecutablePath, FilePath, NewPath, WritableDirectoryPath

group = command_group("paths", "Paths", description="Paths.")


@group.command
def run(
    ctx: Context,
    config: FilePath,
    *rest: ExecutablePath,
    inputs: list[DirectoryPath],
    output: NewPath | None = None,
    scratch: WritableDirectoryPath | None = None,
) -> None:
    """Run.

    Args:
        config: Config file.
        rest: Tools to run.
        inputs: Input dirs.
        output: Output file.
        scratch: Scratch dir.
    """
"#,
    );
    let manifest = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let cmd = manifest.commands.iter().find(|c| c.name == "run").unwrap();
    let ty = |name: &str| {
        cmd.arguments
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no argument {name}"))
            .resolved_type
            .clone()
    };
    assert_eq!(ty("config"), Some(SupportedType::FilePath));
    assert_eq!(ty("rest"), Some(SupportedType::ExecutablePath));
    assert_eq!(
        ty("inputs"),
        Some(SupportedType::List(Box::new(SupportedType::DirectoryPath)))
    );
    assert_eq!(
        ty("output"),
        Some(SupportedType::Optional(Box::new(SupportedType::NewPath)))
    );
    assert_eq!(
        ty("scratch"),
        Some(SupportedType::Optional(Box::new(
            SupportedType::WritableDirectoryPath
        )))
    );
}

fn assert_builds(src: &str) {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "tools/kw.py", src);
    build_static_manifest(&tmp.path().join("tools")).expect("expected the build to succeed");
}

fn unknown_keyword(keyword: &str, suggestion: Option<&str>) -> UnsupportedType {
    UnsupportedType::UnknownArgKeyword {
        keyword: keyword.to_string(),
        suggestion: suggestion.map(str::to_string),
    }
}

/// The exact shape from issue #500.
const ISSUE_500_KW_PY: &str = r#"from pathlib import Path
from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, config: Annotated[Path, arg(path_must_exist=True)]) -> None:
    """Read a config."""
    ctx.print(f"got {config}")
"#;

#[test]
fn unknown_arg_keyword_path_must_exist_fails_build_pointing_at_resolved_path() {
    let errs = type_errors_for(&[("tools/kw.py", ISSUE_500_KW_PY)]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    let err = &errs[0];
    assert_eq!(err.module, "tools.kw");
    assert_eq!(err.function, "read");
    assert_eq!(err.argument, "config");
    assert_eq!(err.reason, unknown_keyword("path_must_exist", None));
    let msg = BuildError::UnsupportedTypes(errs).to_string();
    assert!(
        msg.starts_with("invalid parameter declarations (1):"),
        "got: {msg}"
    );
    assert!(
        msg.contains(
            "unknown `arg()` keyword `path_must_exist`; use `toolr.types.ResolvedPath` instead"
        ),
        "got: {msg}"
    );
}

#[test]
fn unknown_arg_keyword_via_module_level_alias_is_reported() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        r#"from pathlib import Path
from typing import Annotated

from toolr import Context, arg, command_group

ConfigPath = Annotated[Path, arg(must_be_file=True)]

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, config: ConfigPath) -> None:
    """Read a config."""
"#,
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].argument, "config");
    assert_eq!(errs[0].reason, unknown_keyword("must_be_file", None));
    assert!(
        errs[0].to_string().ends_with(
            "unknown `arg()` keyword `must_be_file`; use `toolr.types.FilePath` instead"
        )
    );
}

#[test]
fn unknown_keyword_in_toolr_dot_arg_attribute_form_is_reported() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        r#"from pathlib import Path
from typing import Annotated

import toolr

group = toolr.command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: toolr.Context, config: Annotated[Path, toolr.arg(path_must_be_dir=True)]) -> None:
    """Read a config."""
"#,
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].reason, unknown_keyword("path_must_be_dir", None));
}

#[test]
fn misspelt_active_arg_keyword_suggests_the_real_one() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, *, name: Annotated[str, arg(metvar="NAME")] = "x") -> None:
    """Read."""
"#,
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].reason, unknown_keyword("metvar", Some("metavar")));
    assert!(
        errs[0]
            .to_string()
            .ends_with("unknown `arg()` keyword `metvar` (did you mean `metavar`?)")
    );
}

#[test]
fn every_bad_arg_keyword_in_the_build_is_reported_together() {
    let errs = type_errors_for(&[
        ("tools/kw.py", ISSUE_500_KW_PY),
        (
            "tools/other.py",
            r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("other", "Other", description="Other.")


@group.command
def run(ctx: Context, *, name: Annotated[str, arg(foo=1)] = "x") -> None:
    """Run."""
"#,
        ),
    ]);
    assert_eq!(errs.len(), 2, "{errs:?}");
    let reasons: Vec<&UnsupportedType> = errs.iter().map(|e| &e.reason).collect();
    assert!(reasons.contains(&&unknown_keyword("path_must_exist", None)));
    assert!(reasons.contains(&&unknown_keyword("foo", None)));
    let msg = BuildError::UnsupportedTypes(errs).to_string();
    assert!(
        msg.starts_with("invalid parameter declarations (2):"),
        "got: {msg}"
    );
    assert!(msg.contains("unknown `arg()` keyword `foo`"), "got: {msg}");
    assert!(!msg.contains("`foo` (did you mean"), "got: {msg}");
}

#[test]
fn positional_argument_to_arg_is_reported() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, name: Annotated[str, arg("x")]) -> None:
    """Read."""
"#,
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].reason, UnsupportedType::PositionalArgArgument);
    assert!(
        errs[0]
            .to_string()
            .ends_with("`arg()` takes keyword arguments only")
    );
}

#[test]
fn removed_arg_keywords_fail_the_build_with_a_replacement() {
    for keyword in ["required", "action", "choices", "nargs", "group"] {
        let errs = type_errors_for(&[(
            "tools/kw.py",
            &format!(
                r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, *, name: Annotated[str, arg({keyword}=None)] = "x") -> None:
    """Read."""
"#
            ),
        )]);
        assert_eq!(errs.len(), 1, "{keyword}: {errs:?}");
        let message = errs[0].to_string();
        assert!(
            message.contains(&format!("unknown `arg()` keyword `{keyword}`; use "))
                || message.contains("the action comes from the type"),
            "{keyword}: {message}"
        );
    }
}

#[test]
fn every_accepted_arg_keyword_builds() {
    for keyword in crate::parser::types::ACTIVE_ARG_KEYWORDS {
        assert_builds(&format!(
            r#"from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, *, name: Annotated[str, arg({keyword}=None)] = "x") -> None:
    """Read."""
"#
        ));
    }
}

#[test]
fn non_toolr_call_inside_annotated_is_not_checked() {
    assert_builds(
        r#"from typing import Annotated

from toolr import Context, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


def other(**kwargs):
    return kwargs


@group.command
def read(ctx: Context, *, name: Annotated[str, other(foo=1)] = "x") -> None:
    """Read."""
"#,
    );
}

#[test]
fn splatted_arg_arguments_are_not_checked() {
    assert_builds(
        r#"from typing import Annotated

from toolr import Context, arg, command_group

KWARGS = {"metavar": "NAME"}
ARGS = ()

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, *, name: Annotated[str, arg(*ARGS, **KWARGS)] = "x") -> None:
    """Read."""
"#,
    );
}

fn kw_module(imports: &str, prelude: &str, signature: &str) -> String {
    format!(
        r#"from pathlib import Path
from typing import Annotated, Optional

{imports}

{prelude}

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, {signature}) -> None:
    """Read a config."""
"#
    )
}

const TOOLR_IMPORTS: &str = "from toolr import Context, arg, command_group";

fn assert_path_must_exist_flagged(src: &str) {
    let errs = type_errors_for(&[("tools/kw.py", src)]);
    let flagged: Vec<&TypeResolutionError> = errs
        .iter()
        .filter(|e| e.reason == unknown_keyword("path_must_exist", None))
        .collect();
    assert_eq!(flagged.len(), 1, "{errs:?}");
    assert_eq!(flagged[0].function, "read");
}

#[test]
fn unknown_arg_keyword_in_annotated_or_none_with_none_default_is_reported() {
    assert_path_must_exist_flagged(&kw_module(
        TOOLR_IMPORTS,
        "",
        "config: Annotated[Path, arg(path_must_exist=True)] | None = None",
    ));
}

#[test]
fn unknown_arg_keyword_in_optional_annotated_is_reported() {
    assert_path_must_exist_flagged(&kw_module(
        TOOLR_IMPORTS,
        "",
        "config: Optional[Annotated[Path, arg(path_must_exist=True)]] = None",
    ));
}

#[test]
fn unknown_arg_keyword_in_list_of_annotated_is_reported() {
    assert_path_must_exist_flagged(&kw_module(
        TOOLR_IMPORTS,
        "",
        "configs: list[Annotated[Path, arg(path_must_exist=True)]]",
    ));
}

#[test]
fn unknown_arg_keyword_in_alias_or_none_is_reported() {
    assert_path_must_exist_flagged(&kw_module(
        TOOLR_IMPORTS,
        "ConfigPath = Annotated[Path, arg(path_must_exist=True)]",
        "config: ConfigPath | None = None",
    ));
}

#[test]
fn unknown_arg_keyword_through_alias_chain_is_reported() {
    assert_path_must_exist_flagged(&kw_module(
        TOOLR_IMPORTS,
        "ConfigPath = Annotated[Path, arg(path_must_exist=True)]\nSettingsPath = ConfigPath",
        "config: SettingsPath",
    ));
}

#[test]
fn cyclic_alias_does_not_hang_the_arg_check() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        &kw_module(TOOLR_IMPORTS, "A = B\nB = A", "config: A"),
    )]);
    assert!(
        errs.iter()
            .all(|e| !matches!(e.reason, UnsupportedType::UnknownArgKeyword { .. })),
        "{errs:?}"
    );
}

#[test]
fn renamed_toolr_arg_import_is_checked() {
    assert_path_must_exist_flagged(&kw_module(
        "from toolr import Context, command_group\nfrom toolr import arg as a",
        "",
        "config: Annotated[Path, a(path_must_exist=True)]",
    ));
}

#[test]
fn toolr_module_alias_and_submodule_arg_calls_are_checked() {
    assert_path_must_exist_flagged(&kw_module(
        "import toolr as t\nfrom toolr import Context, command_group",
        "",
        "config: Annotated[Path, t.arg(path_must_exist=True)]",
    ));
    assert_path_must_exist_flagged(&kw_module(
        "import toolr.utils\nfrom toolr import Context, command_group",
        "",
        "config: Annotated[Path, toolr.utils.arg(path_must_exist=True)]",
    ));
}

#[test]
fn foreign_arg_attribute_call_is_not_checked() {
    assert_builds(&kw_module(
        "import mylib\nfrom toolr import Context, command_group",
        "",
        r#"*, name: Annotated[str, mylib.arg(foo=1)] = "x""#,
    ));
}

#[test]
fn foreign_bare_arg_import_is_not_checked() {
    assert_builds(&kw_module(
        "from mylib import arg\nfrom toolr import Context, command_group",
        "",
        r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
    ));
}

#[test]
fn argparse_style_keywords_get_a_targeted_hint() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        &kw_module(
            TOOLR_IMPORTS,
            "",
            r#"*, name: Annotated[str, arg(help="Name.", type=str, default="x")] = "x""#,
        ),
    )]);
    assert_eq!(errs.len(), 3, "{errs:?}");
    let msg = BuildError::UnsupportedTypes(errs).to_string();
    assert!(!msg.contains("did you mean"), "got: {msg}");
    for needle in [
        "unknown `arg()` keyword `help`; help text comes from the docstring's `Args:` section",
        "unknown `arg()` keyword `type`; the type comes from the annotation",
        "unknown `arg()` keyword `default`; the default comes from the parameter's default value",
    ] {
        assert!(msg.contains(needle), "missing {needle:?} in: {msg}");
    }
}

#[test]
fn local_arg_function_is_not_checked() {
    assert_builds(&kw_module(
        "from toolr import Context, command_group",
        "def arg(**kw):\n    return kw",
        r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
    ));
}

#[test]
fn local_arg_assignment_is_not_checked() {
    assert_builds(&kw_module(
        "from toolr import Context, command_group",
        "arg = dict",
        r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
    ));
}

#[test]
fn local_arg_annotated_assignment_is_not_checked() {
    assert_builds(&kw_module(
        "from toolr import Context, command_group",
        "arg: type = dict",
        r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
    ));
}

#[test]
fn local_arg_class_is_not_checked() {
    assert_builds(&kw_module(
        "from toolr import Context, command_group",
        "class arg:\n    def __init__(self, **kw):\n        self.kw = kw",
        r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
    ));
}

fn assert_tree_builds(files: &[(&str, &str)]) {
    let tmp = TempDir::new().unwrap();
    for (name, contents) in files {
        write(tmp.path(), name, contents);
    }
    build_static_manifest(&tmp.path().join("tools")).expect("expected the build to succeed");
}

#[test]
fn imported_alias_is_checked_against_its_defining_modules_imports() {
    assert_tree_builds(&[
        (
            "tools/other.py",
            "from typing import Annotated\n\nfrom mylib import arg\n\nTagged = Annotated[str, arg(foo=1)]\n",
        ),
        (
            "tools/kw.py",
            &kw_module(
                "from toolr import Context, arg, command_group\nfrom tools.other import Tagged",
                "",
                "name: Tagged",
            ),
        ),
    ]);
}

#[test]
fn alias_the_using_module_never_imported_is_not_expanded() {
    assert_tree_builds(&[
        (
            "tools/other.py",
            "from typing import Annotated\n\nfrom toolr import arg\n\nName = Annotated[str, arg(foo=1)]\n",
        ),
        (
            "tools/kw.py",
            &kw_module(
                "from toolr import Context, command_group\nfrom mylib import Name",
                "",
                "name: list[Name] | None = None",
            ),
        ),
    ]);
}

#[test]
fn alias_imported_from_a_sibling_module_is_checked() {
    let common = "from pathlib import Path\nfrom typing import Annotated\n\nfrom toolr import arg\n\nConfigPath = Annotated[Path, arg(path_must_exist=True)]\n";
    for import in [
        "from tools.common import ConfigPath",
        "from .common import ConfigPath",
    ] {
        let kw = kw_module(
            &format!("from toolr import Context, command_group\n{import}"),
            "",
            "config: ConfigPath | None = None",
        );
        let errs = type_errors_for(&[("tools/common.py", common), ("tools/kw.py", &kw)]);
        assert_eq!(errs.len(), 1, "{import}: {errs:?}");
        assert_eq!(
            errs[0].reason,
            unknown_keyword("path_must_exist", None),
            "{import}"
        );
    }
}

#[test]
fn alias_pulled_in_by_a_star_import_is_checked() {
    let common = "from pathlib import Path\nfrom typing import Annotated\n\nfrom toolr import arg\n\nConfigPath = Annotated[Path, arg(path_must_exist=True)]\n";
    for import in ["from tools.common import *", "from .common import *"] {
        let kw = kw_module(
            &format!("from toolr import Context, command_group\n{import}"),
            "",
            "config: ConfigPath | None = None",
        );
        let errs = type_errors_for(&[("tools/common.py", common), ("tools/kw.py", &kw)]);
        let flagged = errs
            .iter()
            .filter(|e| e.reason == unknown_keyword("path_must_exist", None))
            .count();
        assert_eq!(flagged, 1, "{import}: {errs:?}");
    }
}

#[test]
fn local_arg_def_after_toolr_import_is_not_checked() {
    assert_builds(&kw_module(
        TOOLR_IMPORTS,
        "def arg(**kw):\n    return kw",
        r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
    ));
}

#[test]
fn toolr_import_after_local_arg_def_is_checked() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        &kw_module(
            "from toolr import Context, command_group",
            "def arg(**kw):\n    return kw\n\n\nfrom toolr import arg",
            r#"*, name: Annotated[str, arg(foo=1)] = "x""#,
        ),
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].reason, unknown_keyword("foo", None));
}

#[test]
fn alias_behind_a_star_import_from_an_unparsed_module_is_not_expanded() {
    assert_tree_builds(&[
        (
            "tools/other.py",
            "from typing import Annotated\n\nfrom toolr import arg\n\nTagged = Annotated[str, arg(foo=1)]\n",
        ),
        (
            "tools/kw.py",
            &kw_module(
                "from toolr import Context, command_group\nfrom mylib import *",
                "",
                "name: Tagged",
            ),
        ),
    ]);
}

#[test]
fn bare_arg_behind_a_toolr_star_import_is_checked() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        &kw_module(
            "from toolr import *",
            "",
            "config: Annotated[Path, arg(path_must_exist=True)]",
        ),
    )]);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].reason, unknown_keyword("path_must_exist", None));
}

#[test]
fn non_arg_calls_inside_annotated_are_not_checked() {
    for call in [
        "make().arg(foo=1)",
        "mylib.other(foo=1)",
        "FACTORIES[0](foo=1)",
    ] {
        assert_builds(&kw_module(
            "from toolr import Context, command_group\nimport mylib\nfrom mylib import FACTORIES, make",
            "",
            &format!(r#"*, name: Annotated[str, {call}] = "x""#),
        ));
    }
}

/// #545: a wrapped `Args:` entry must keep its continuation lines.
#[test]
fn wrapped_args_entry_keeps_continuation_lines() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/demo.py",
        r#""""Demo commands."""
from toolr import command

from toolr import Context, command, command_group

command_group("demo", docstring=__doc__)


@command("wrap", group="demo")
def wrap(ctx: Context, *, name: str = "x", loud: bool = False) -> None:
    """Show how wrapped Args entries render.

    Args:
        name: First line of the name help, which is long enough that it has to
            continue on a second line that should also be shown.
        loud: Single-line help for comparison.
    """
"#,
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let wrap = m.commands.iter().find(|c| c.name == "wrap").unwrap();
    let help = |n: &str| &wrap.arguments.iter().find(|a| a.name == n).unwrap().help;
    assert_eq!(
        help("name"),
        "First line of the name help, which is long enough that it has to \
         continue on a second line that should also be shown."
    );
    assert_eq!(help("loud"), "Single-line help for comparison.");
}

fn bool_flag_error(signature: &str) -> (BoolFlagError, String) {
    bool_flag_error_in(&[("tools/kw.py", &kw_module(TOOLR_IMPORTS, "", signature))])
}

fn bool_flag_error_in(files: &[(&str, &str)]) -> (BoolFlagError, String) {
    let errs = type_errors_for(files);
    assert_eq!(errs.len(), 1, "{errs:?}");
    let UnsupportedType::BoolFlag { error, .. } = &errs[0].reason else {
        panic!("expected BoolFlag, got {:?}", errs[0].reason)
    };
    (error.clone(), errs[0].to_string())
}

#[test]
fn bool_true_default_rejected() {
    let (error, msg) = bool_flag_error("colour: bool = True");
    assert_eq!(
        error,
        BoolFlagError::DefaultTrue {
            suggested: "no_colour".into()
        }
    );
    assert!(
        msg.contains("Write `no_colour: bool = False` (CLI: `--no-colour`)"),
        "got: {msg}"
    );
}

#[test]
fn annotated_bool_true_default_rejected() {
    let (error, _) = bool_flag_error("colour: Annotated[bool, arg(env=\"C\")] = True");
    assert_eq!(
        error,
        BoolFlagError::DefaultTrue {
            suggested: "no_colour".into()
        }
    );
}

#[test]
fn no_prefixed_true_default_suggests_positive_name() {
    let (error, msg) = bool_flag_error("no_cache: bool = True");
    assert_eq!(
        error,
        BoolFlagError::DefaultTrue {
            suggested: "cache".into()
        }
    );
    assert!(msg.contains("`cache: bool = False`"), "got: {msg}");
}

#[test]
fn kwonly_bool_without_default_rejected() {
    let (error, _) = bool_flag_error("*, verbose: bool");
    assert_eq!(error, BoolFlagError::NotFalseDefault);
}

#[test]
fn none_default_suggests_false_not_negated_name() {
    let (error, msg) = bool_flag_error("*, verbose: bool = None");
    assert_eq!(error, BoolFlagError::NotFalseDefault);
    assert!(msg.contains("`verbose: bool = False`"), "{msg}");
    assert!(!msg.contains("no_verbose"), "{msg}");
}

#[test]
fn imported_constant_default_suggests_literal_false() {
    let (error, msg) = bool_flag_error_in(&[
        ("tools/defaults.py", "OFF = False\n"),
        (
            "tools/kw.py",
            &kw_module(
                &format!("{TOOLR_IMPORTS}\nfrom .defaults import OFF"),
                "",
                "*, verbose: bool = OFF",
            ),
        ),
    ]);
    assert_eq!(error, BoolFlagError::NotFalseDefault);
    assert!(msg.contains("literal `False`"), "{msg}");
    assert!(!msg.contains("no_verbose"), "{msg}");
}

#[test]
fn positional_bool_rejected() {
    let (error, _) = bool_flag_error("flag: bool");
    assert_eq!(error, BoolFlagError::NotKeyword);
}

#[test]
fn var_positional_bool_rejected() {
    let (error, _) = bool_flag_error("*flags: bool");
    assert_eq!(error, BoolFlagError::NotKeyword);
}

#[test]
fn optional_bool_rejected() {
    let (error, _) = bool_flag_error("colour: bool | None = None");
    assert_eq!(error, BoolFlagError::NotPlainFlag);
}

#[test]
fn typing_optional_bool_rejected() {
    let (error, _) = bool_flag_error("colour: Optional[bool] = None");
    assert_eq!(error, BoolFlagError::NotPlainFlag);
}

#[test]
fn list_of_bool_rejected() {
    let (error, _) = bool_flag_error("*, flags: list[bool] = []");
    assert_eq!(error, BoolFlagError::NotPlainFlag);
}

#[test]
fn tuple_of_bool_rejected() {
    let (error, _) = bool_flag_error("*, flags: tuple[bool, bool] = (False, False)");
    assert_eq!(error, BoolFlagError::NotPlainFlag);
}

#[test]
fn all_bool_violations_reported_together() {
    let errs = type_errors_for(&[(
        "tools/kw.py",
        &kw_module(
            TOOLR_IMPORTS,
            "",
            "flag: bool, *, colour: bool = True, verbose: bool",
        ),
    )]);
    assert_eq!(errs.len(), 3, "{errs:?}");
}

#[test]
fn bool_alias_with_false_default_becomes_flag() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/kw.py",
        &kw_module(
            TOOLR_IMPORTS,
            "Verbose = Annotated[bool, arg(env=\"V\")]",
            "verbose: Verbose = False",
        ),
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let read = m.commands.iter().find(|c| c.name == "read").unwrap();
    let arg = read.arguments.iter().find(|a| a.name == "verbose").unwrap();
    assert_eq!(arg.kind, ArgumentKind::Flag);
    assert_eq!(arg.default, Some("false".into()));
}

#[test]
fn plain_false_default_still_builds() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/kw.py",
        &kw_module(
            TOOLR_IMPORTS,
            "",
            "verbose: bool = False, *, dry_run: bool = False",
        ),
    );
    let m = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let read = m.commands.iter().find(|c| c.name == "read").unwrap();
    for name in ["verbose", "dry_run"] {
        let arg = read.arguments.iter().find(|a| a.name == name).unwrap();
        assert_eq!(arg.kind, ArgumentKind::Flag, "{name}");
    }
}

/// argparse `store_false` sources never pass through the native bool rule.
#[test]
fn argparse_store_false_with_true_default_still_builds() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "tools/dispatcher.py",
        r#""""Django dispatcher."""
group = command_group("django", "Django", "Django.")

@group.command
def django(ctx):
    """Dispatch to manage.py."""
    pass
"#,
    );
    write(
        tmp.path(),
        "apps/x/management/commands/runserver.py",
        "def add_arguments(self, parser):\n    parser.add_argument('--no-colour', dest='colour', action='store_false', default=True)\n",
    );
    write(
        tmp.path(),
        "tools/pyproject.toml",
        r#"
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]

[[tool.toolr.argparse.django.attach]]
parent = "django"
"#,
    );

    let manifest = build_static_manifest(&tmp.path().join("tools"))
        .unwrap()
        .manifest;
    let runserver = manifest
        .commands
        .iter()
        .find(|c| c.name == "runserver")
        .unwrap();
    let arg = runserver
        .arguments
        .iter()
        .find(|a| a.long_flag.as_deref() == Some("--no-colour"))
        .unwrap();
    assert_eq!(arg.kind, ArgumentKind::Flag);
    assert_eq!(arg.default.as_deref(), Some("true"));
}
