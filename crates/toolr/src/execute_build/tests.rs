use super::*;
use clap::{Arg, ArgAction};
use toolr_core::manifest::{Argument, ArgumentKind, Command, Origin};

fn cmd_hello_with_name_arg() -> Command {
    Command {
        name: "hello".into(),
        group: "ci".into(),
        module: "tools.ci".into(),
        function: "hello".into(),
        summary: "".into(),
        description: "".into(),
        arguments: vec![Argument {
            name: "name".into(),
            kind: ArgumentKind::Optional,
            help: "".into(),
            default: Some("world".into()),
            type_annotation: None,
            resolved_type: None,
            allowed_values: vec![],
            metadata: toolr_core::manifest::ArgMetadata::default(),
            long_flag: None,
        }],
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

fn parse(value: &str) -> ArgMatches {
    clap::Command::new("hello")
        .arg(Arg::new("name").long("name").default_value("world"))
        .get_matches_from(["hello", "--name", value])
}

#[test]
fn build_spec_copies_static_fields() {
    let cmd = cmd_hello_with_name_arg();
    let matches = parse("Alice");
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.group, "ci");
    assert_eq!(spec.command, "hello");
    assert_eq!(spec.module, "tools.ci");
    assert_eq!(spec.function, "hello");
    assert_eq!(spec.context.repo_root, "/repo");
}

#[test]
fn build_spec_runs_the_relative_path_warning_wrapper() {
    // Drives `warn_relative_paths` (the `eprintln!` wrapper, not just
    // the pure `relative_path_warning`): a relative Path arg plus a
    // repo_root that differs from the cwd makes the note fire. The note only goes to stderr; we just
    // assert the spec still builds.
    let cmd = cmd_with(vec![arg_of(
        "file",
        ArgumentKind::Positional,
        SupportedType::Path,
    )]);
    let matches = path_matches(&["t", "rel/x.py"]);
    let spec = build_spec(
        &cmd,
        &matches,
        &bases("/nonexistent-repo-xyz"),
        &OutputOptions::default(),
    )
    .unwrap();
    assert!(spec.args.contains_key("file"));
}

#[test]
fn build_spec_extracts_optional_arg_value() {
    let cmd = cmd_hello_with_name_arg();
    let matches = parse("Alice");
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("name"), Some(&Value::String("Alice".into())));
}

#[test]
fn build_spec_extracts_flag_as_bool() {
    let cmd = Command {
        name: "switch".into(),
        group: "ci".into(),
        module: "tools.ci".into(),
        function: "switch".into(),
        summary: "".into(),
        description: "".into(),
        arguments: vec![Argument {
            name: "force".into(),
            kind: ArgumentKind::Flag,
            help: "".into(),
            default: None,
            type_annotation: None,
            resolved_type: None,
            allowed_values: vec![],
            metadata: toolr_core::manifest::ArgMetadata::default(),
            long_flag: None,
        }],
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    };
    let matches = clap::Command::new("switch")
        .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
        .get_matches_from(["switch", "--force"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("force"), Some(&Value::Bool(true)));
}

/// Helper that returns an `Argument` with the given name + kind +
/// resolved type, defaults everywhere else. Used by the type-router
/// tests so each test reads as "given this resolved type, do we
/// route to the right `get_one`/`get_many` arm?".
fn arg_of(name: &str, kind: ArgumentKind, ty: SupportedType) -> Argument {
    Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: Some(ty),
        allowed_values: vec![],
        metadata: toolr_core::manifest::ArgMetadata::default(),
        long_flag: None,
    }
}

fn cmd_with(args: Vec<Argument>) -> Command {
    Command {
        name: "test".into(),
        group: "g".into(),
        module: "tools.g".into(),
        function: "test".into(),
        summary: String::new(),
        description: String::new(),
        arguments: args,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

/// Bases for tests that don't touch the filesystem: the cwd is never
/// the repo root.
pub(super) fn bases(repo_root: &'static str) -> PathBases<'static> {
    PathBases {
        cwd: Path::new("/elsewhere"),
        repo_root: Path::new(repo_root),
    }
}

/// A repo with `sub/dir` to run from.
struct Layout {
    repo: tempfile::TempDir,
    cwd: std::path::PathBuf,
}

impl Layout {
    fn new() -> Self {
        let repo = tempfile::TempDir::new().unwrap();
        let cwd = repo.path().join("sub").join("dir");
        std::fs::create_dir_all(&cwd).unwrap();
        Self { repo, cwd }
    }

    fn write(&self, rel: &str) -> std::path::PathBuf {
        let path = self.repo.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "x").unwrap();
        path
    }

    fn bases(&self) -> PathBases<'_> {
        PathBases {
            cwd: &self.cwd,
            repo_root: self.repo.path(),
        }
    }
}

fn deferred_cmd(ty: SupportedType, default: &str) -> Command {
    let mut arg = arg_of("cfg", ArgumentKind::Optional, ty);
    arg.default = Some(default.into());
    cmd_with(vec![arg])
}

fn user_matches(cmd: &Command, argv: &[&str]) -> ArgMatches {
    crate::cli::build_user_command(cmd)
        .try_get_matches_from(argv)
        .unwrap()
}

fn spec_args(
    cmd: &Command,
    argv: &[&str],
    bases: &PathBases,
) -> anyhow::Result<BTreeMap<String, Value>> {
    let matches = user_matches(cmd, argv);
    build_spec(cmd, &matches, bases, &OutputOptions::default()).map(|spec| spec.args)
}

fn path_value(path: &Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

#[test]
fn default_resolves_against_repo_root() {
    let layout = Layout::new();
    let cfg = layout.write("tools/pyproject.toml");
    let cmd = deferred_cmd(SupportedType::FilePath, "tools/pyproject.toml");
    let args = spec_args(&cmd, &["test"], &layout.bases()).unwrap();
    assert_eq!(args["cfg"], path_value(&dunce::canonicalize(cfg).unwrap()));
}

#[test]
fn typed_value_for_deferred_arg_resolves_against_cwd() {
    let layout = Layout::new();
    layout.write("x.toml");
    let typed = layout.write("sub/dir/x.toml");
    let cmd = deferred_cmd(SupportedType::FilePath, "tools/pyproject.toml");
    let args = spec_args(&cmd, &["test", "--cfg", "x.toml"], &layout.bases()).unwrap();
    assert_eq!(
        args["cfg"],
        path_value(&dunce::canonicalize(typed).unwrap())
    );
}

#[test]
fn typed_missing_value_for_deferred_arg_errors_like_clap() {
    let layout = Layout::new();
    let cmd = deferred_cmd(SupportedType::FilePath, "tools/pyproject.toml");
    let err = spec_args(&cmd, &["test", "--cfg", "nope.toml"], &layout.bases()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid value 'nope.toml' for '--cfg': path does not exist: nope.toml"
    );
}

#[test]
fn missing_default_errors_naming_repo_root() {
    let layout = Layout::new();
    let cmd = deferred_cmd(SupportedType::FilePath, "tools/missing.toml");
    let err = spec_args(&cmd, &["test"], &layout.bases()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "default 'tools/missing.toml' for '--cfg' (resolved from the repo root {}): \
             path does not exist: tools/missing.toml",
            layout.repo.path().display()
        )
    );
}

#[test]
fn dot_directory_default_resolves_to_repo_root() {
    let layout = Layout::new();
    let cmd = deferred_cmd(
        SupportedType::Optional(Box::new(SupportedType::DirectoryPath)),
        ".",
    );
    let args = spec_args(&cmd, &["test"], &layout.bases()).unwrap();
    assert_eq!(
        args["cfg"],
        path_value(&dunce::canonicalize(layout.repo.path()).unwrap())
    );
}

/// A deferred arg bound to `env_name` (clap's `.env()`, via the
/// manifest metadata), parsed with that variable set and no flag typed.
fn env_spec(
    layout: &Layout,
    env_name: &str,
    value: &str,
) -> anyhow::Result<BTreeMap<String, Value>> {
    let mut cmd = deferred_cmd(SupportedType::FilePath, "tools/pyproject.toml");
    cmd.arguments[0].metadata.env = Some(env_name.into());
    // SAFETY: the variable name is unique to the calling test.
    unsafe { std::env::set_var(env_name, value) };
    let matches = user_matches(&cmd, &["test"]);
    unsafe { std::env::remove_var(env_name) };
    build_spec(&cmd, &matches, &layout.bases(), &OutputOptions::default()).map(|s| s.args)
}

#[test]
fn env_value_for_deferred_arg_resolves_against_cwd() {
    let layout = Layout::new();
    let local = layout.write("sub/dir/local.toml");
    let args = env_spec(&layout, "TOOLR_TEST_DEFERRED_ENV_OK", "local.toml").unwrap();
    assert_eq!(
        args["cfg"],
        path_value(&dunce::canonicalize(local).unwrap())
    );
}

#[test]
fn env_value_error_names_the_env_var_not_a_default() {
    let layout = Layout::new();
    let err = env_spec(&layout, "TOOLR_TEST_DEFERRED_ENV_BAD", "nope.toml").unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid value 'nope.toml' for '--cfg' (from TOOLR_TEST_DEFERRED_ENV_BAD): \
         path does not exist: nope.toml"
    );
}

/// `cli` swaps a deferred arg to an unchecked `PathBuf` parser and
/// `extract_scalar` reads it back as one; both ask `deferred_path`. Pins that pairing for every path type,
/// bare and `Optional`, typed and defaulted, so a divergence fails here
/// rather than as a clap type-mismatch panic at run time.
#[test]
fn every_path_type_parses_and_extracts_a_relative_default() {
    use toolr_core::parser::types::SupportedTypeKind;
    let layout = Layout::new();
    let path_types = SupportedTypeKind::ALL
        .iter()
        .map(|kind| kind.representative())
        .filter(SupportedType::is_path);
    for inner in path_types {
        for ty in [
            inner.clone(),
            SupportedType::Optional(Box::new(inner.clone())),
        ] {
            let cmd = deferred_cmd(ty.clone(), "missing/out");
            assert_eq!(
                deferred_path(&cmd.arguments[0]).is_some(),
                inner != SupportedType::Path,
                "{ty:?}"
            );
            for argv in [&["test"][..], &["test", "--cfg", "missing/out"]] {
                // Parsing never checks the disk; extraction must not
                // panic on the stored type, whatever it decides.
                let matches = user_matches(&cmd, argv);
                let _ = build_spec(&cmd, &matches, &layout.bases(), &OutputOptions::default());
            }
        }
    }
}

#[test]
fn absolute_path_default_joins_repo_root() {
    let layout = Layout::new();
    let cmd = deferred_cmd(SupportedType::AbsolutePath, "build/out");
    let args = spec_args(&cmd, &["test"], &layout.bases()).unwrap();
    assert_eq!(
        args["cfg"],
        path_value(&layout.repo.path().join("build/out"))
    );
}

#[test]
fn relative_path_warning_ignores_deferred_args() {
    let layout = Layout::new();
    let cmd = deferred_cmd(SupportedType::FilePath, "tools/pyproject.toml");
    let matches = user_matches(&cmd, &["test", "--cfg", "x.toml"]);
    assert!(relative_path_warning(&cmd, &matches, layout.repo.path(), &layout.cwd).is_none());
}

#[test]
fn pack_child_args_resolves_deferred_default_from_repo_root() {
    let layout = Layout::new();
    let cfg = layout.write("tools/pyproject.toml");
    let cmd = deferred_cmd(SupportedType::FilePath, "tools/pyproject.toml");
    let matches = user_matches(&cmd, &["test"]);
    let packed = pack_child_args(&cmd, &matches, &layout.bases()).unwrap();
    assert_eq!(
        packed.args["cfg"],
        path_value(&dunce::canonicalize(cfg).unwrap())
    );
}

/// Parse `argv` against a single `file` arg with a `PathBuf` value-parser,
/// mirroring how the real CLI builder wires a `SupportedType::Path` arg.
fn path_matches(argv: &[&str]) -> ArgMatches {
    clap::Command::new("t")
        .arg(Arg::new("file").value_parser(clap::value_parser!(std::path::PathBuf)))
        .try_get_matches_from(argv)
        .expect("parse")
}

#[test]
fn warns_on_relative_path_arg_from_subdir() {
    let cmd = cmd_with(vec![arg_of(
        "file",
        ArgumentKind::Positional,
        SupportedType::Path,
    )]);
    let matches = path_matches(&["t", "rel/x.py"]);
    let msg = relative_path_warning(&cmd, &matches, Path::new("/repo"), Path::new("/repo/sub"));
    assert!(
        msg.is_some_and(|m| m.contains("repo root")),
        "expected a warning"
    );
}

#[test]
fn no_warn_when_cwd_equals_repo_root() {
    let cmd = cmd_with(vec![arg_of(
        "file",
        ArgumentKind::Positional,
        SupportedType::Path,
    )]);
    let matches = path_matches(&["t", "rel/x.py"]);
    assert!(
        relative_path_warning(&cmd, &matches, Path::new("/repo"), Path::new("/repo")).is_none()
    );
}

#[test]
fn no_warn_for_absolute_path_arg() {
    let cmd = cmd_with(vec![arg_of(
        "file",
        ArgumentKind::Positional,
        SupportedType::Path,
    )]);
    // `/abs/x.py` is NOT absolute on Windows (no drive letter), so use a
    // platform-appropriate absolute path for the "is_absolute" check.
    let abs = if cfg!(windows) {
        "C:\\abs\\x.py"
    } else {
        "/abs/x.py"
    };
    let matches = path_matches(&["t", abs]);
    assert!(
        relative_path_warning(&cmd, &matches, Path::new("/repo"), Path::new("/repo/sub")).is_none()
    );
}

#[test]
fn no_warn_for_non_path_arg_even_if_value_looks_like_a_path() {
    // A `str` arg whose value merely looks path-like must NOT warn —
    // the check is type-driven, not heuristic.
    let cmd = cmd_with(vec![arg_of(
        "file",
        ArgumentKind::Positional,
        SupportedType::Str,
    )]);
    let matches = clap::Command::new("t")
        .arg(Arg::new("file"))
        .try_get_matches_from(["t", "rel/x.py"])
        .expect("parse");
    assert!(
        relative_path_warning(&cmd, &matches, Path::new("/repo"), Path::new("/repo/sub")).is_none()
    );
}

#[test]
fn build_spec_extracts_count_arg() {
    let cmd = cmd_with(vec![arg_of(
        "verbose",
        ArgumentKind::Count,
        SupportedType::Int,
    )]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("verbose").short('v').action(ArgAction::Count))
        .get_matches_from(["test", "-vvv"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("verbose"), Some(&Value::Number(3.into())));
}

#[test]
fn build_spec_extracts_int_scalar() {
    let cmd = cmd_with(vec![arg_of(
        "n",
        ArgumentKind::Optional,
        SupportedType::Int,
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("n")
                .long("n")
                .value_parser(clap::value_parser!(i64))
                .default_value("42"),
        )
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("n"), Some(&Value::Number(42.into())));
}

#[test]
fn build_spec_extracts_float_scalar() {
    let cmd = cmd_with(vec![arg_of(
        "ratio",
        ArgumentKind::Optional,
        SupportedType::Float,
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("ratio")
                .long("ratio")
                .value_parser(clap::value_parser!(f64))
                .default_value("1.5"),
        )
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    match spec.args.get("ratio") {
        Some(Value::Number(n)) => assert_eq!(n.as_f64(), Some(1.5)),
        other => panic!("expected Float number, got {other:?}"),
    }
}

#[test]
fn build_spec_extracts_bool_scalar_via_optional() {
    let cmd = cmd_with(vec![arg_of(
        "dry",
        ArgumentKind::Optional,
        SupportedType::Bool,
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("dry")
                .long("dry")
                .value_parser(clap::value_parser!(bool))
                .default_value("false"),
        )
        .get_matches_from(["test", "--dry", "true"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("dry"), Some(&Value::Bool(true)));
}

#[test]
fn build_spec_extracts_path_scalar_as_string() {
    let cmd = cmd_with(vec![arg_of(
        "file",
        ArgumentKind::Optional,
        SupportedType::Path,
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("file")
                .long("file")
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .default_value("/tmp/x"),
        )
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("file"), Some(&Value::String("/tmp/x".into())));
}

#[test]
fn build_spec_extracts_optional_inner_type_via_unwrap_optional() {
    // `Optional<Int>` should resolve to the Int arm of `extract_scalar`.
    let cmd = cmd_with(vec![arg_of(
        "maybe_n",
        ArgumentKind::Optional,
        SupportedType::Optional(Box::new(SupportedType::Int)),
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("maybe_n")
                .long("maybe-n")
                .value_parser(clap::value_parser!(i64))
                .default_value("7"),
        )
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("maybe_n"), Some(&Value::Number(7.into())));
}

#[test]
fn build_spec_extracts_repeated_int_as_json_array() {
    let cmd = cmd_with(vec![arg_of(
        "ports",
        ArgumentKind::Repeated,
        SupportedType::List(Box::new(SupportedType::Int)),
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("ports")
                .long("port")
                .action(ArgAction::Append)
                .value_parser(clap::value_parser!(i64)),
        )
        .get_matches_from(["test", "--port", "80", "--port", "443"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(
        spec.args.get("ports"),
        Some(&Value::Array(vec![
            Value::Number(80.into()),
            Value::Number(443.into())
        ])),
    );
}

#[test]
fn build_spec_omits_optional_repeated_when_absent() {
    // Regression for #435: `list[T] | None` is promoted to
    // `Repeated`, but an omitted flag must stay absent from the
    // wire spec (not `[]`) so the Python function's own `= None`
    // default runs.
    let cmd = cmd_with(vec![arg_of(
        "items",
        ArgumentKind::Repeated,
        SupportedType::Optional(Box::new(SupportedType::List(Box::new(SupportedType::Str)))),
    )]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("items").long("items").action(ArgAction::Append))
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("items"), None);
}

#[test]
fn build_spec_extracts_optional_repeated_when_present() {
    let cmd = cmd_with(vec![arg_of(
        "items",
        ArgumentKind::Repeated,
        SupportedType::Optional(Box::new(SupportedType::List(Box::new(SupportedType::Str)))),
    )]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("items").long("items").action(ArgAction::Append))
        .get_matches_from(["test", "--items", "a", "--items", "b"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(
        spec.args.get("items"),
        Some(&Value::Array(vec![
            Value::String("a".into()),
            Value::String("b".into()),
        ])),
    );
}

#[test]
fn build_spec_extracts_repeated_path_as_json_strings() {
    let cmd = cmd_with(vec![arg_of(
        "files",
        ArgumentKind::Repeated,
        SupportedType::List(Box::new(SupportedType::Path)),
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("files")
                .long("file")
                .action(ArgAction::Append)
                .value_parser(clap::value_parser!(std::path::PathBuf)),
        )
        .get_matches_from(["test", "--file", "/a", "--file", "/b"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(
        spec.args.get("files"),
        Some(&Value::Array(vec![
            Value::String("/a".into()),
            Value::String("/b".into()),
        ])),
    );
}

#[test]
fn build_spec_extracts_tuple_arg_with_num_args() {
    // Heterogeneous Tuple routes through `extract_many` even though
    // the kind is Positional — verify the `is_tuple` early branch.
    let cmd = cmd_with(vec![arg_of(
        "pair",
        ArgumentKind::Positional,
        SupportedType::Tuple(vec![SupportedType::Str, SupportedType::Int]),
    )]);
    let matches = clap::Command::new("test")
        .arg(
            Arg::new("pair")
                .num_args(2)
                .value_parser(clap::value_parser!(String)),
        )
        .get_matches_from(["test", "name", "42"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(
        spec.args.get("pair"),
        Some(&Value::Array(vec![
            Value::String("name".into()),
            Value::String("42".into()),
        ])),
    );
}

#[test]
fn build_spec_omits_optional_tuple_when_absent() {
    // Regression for #435 (tuple half): `tuple[...] | None` resolves
    // as `Optional(Tuple(_))`, which the `is_tuple` early-return
    // above would otherwise route straight to `extract_many` and
    // send `[]` for an omitted flag — failing the tuple's own
    // fixed-arity schema instead of letting `= None` run.
    let cmd = cmd_with(vec![arg_of(
        "pair",
        ArgumentKind::Repeated,
        SupportedType::Optional(Box::new(SupportedType::Tuple(vec![
            SupportedType::Str,
            SupportedType::Int,
        ]))),
    )]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("pair").long("pair").num_args(2))
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(spec.args.get("pair"), None);
}

#[test]
fn build_spec_missing_optional_value_does_not_appear_in_args_map() {
    // No clap value present → extract_scalar returns None → key is
    // absent from the args map. Pinning this protects the Python
    // side, which uses `args.get(name)` and relies on absence
    // (rather than null) for "user didn't pass this".
    let cmd = cmd_with(vec![arg_of(
        "absent",
        ArgumentKind::Optional,
        SupportedType::Str,
    )]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("absent").long("absent"))
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert!(!spec.args.contains_key("absent"));
}

#[test]
fn build_spec_missing_fixed_arity_keyword_does_not_appear_in_args_map() {
    // A keyword-style FixedArity arg is `required(false)` with no
    // default. If omitted, `extract_value` must return `None` —
    // not `Some([])` — otherwise `DispatchCommand.argv` would emit
    // a bare `--pair` with zero of its required 2 values.
    let mut arg = arg_of("pair", ArgumentKind::FixedArity, SupportedType::Str);
    arg.long_flag = Some("--pair".to_string());
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(2));
    let cmd = cmd_with(vec![arg]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("pair").long("pair").num_args(2).required(false))
        .get_matches_from(["test"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert!(!spec.args.contains_key("pair"));
}

#[test]
fn build_spec_extracts_fixed_arity_keyword_when_present() {
    let mut arg = arg_of("pair", ArgumentKind::FixedArity, SupportedType::Str);
    arg.long_flag = Some("--pair".to_string());
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(2));
    let cmd = cmd_with(vec![arg]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("pair").long("pair").num_args(2).required(false))
        .get_matches_from(["test", "--pair", "a", "b"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(
        spec.args.get("pair"),
        Some(&Value::Array(vec![
            Value::String("a".into()),
            Value::String("b".into()),
        ])),
    );
}

#[test]
fn build_spec_extracts_fixed_arity_positional_when_present() {
    let mut arg = arg_of("files", ArgumentKind::FixedArity, SupportedType::Str);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(3));
    let cmd = cmd_with(vec![arg]);
    let matches = clap::Command::new("test")
        .arg(Arg::new("files").num_args(3).required(true))
        .get_matches_from(["test", "x", "y", "z"]);
    let spec = build_spec(&cmd, &matches, &bases("/repo"), &OutputOptions::default()).unwrap();
    assert_eq!(
        spec.args.get("files"),
        Some(&Value::Array(vec![
            Value::String("x".into()),
            Value::String("y".into()),
            Value::String("z".into()),
        ])),
    );
}

#[test]
fn output_options_default_values_match_python_runner_expectations() {
    // The runner side defaults `verbosity` to "normal" and
    // `log_level` to "INFO" when not overridden. Document those
    // strings here so a Rust-side rename gets caught by a unit
    // test instead of a CLI smoke run.
    let opts = OutputOptions::default();
    assert_eq!(opts.verbosity.as_str(), "normal");
    assert_eq!(Verbosity::Quiet.as_str(), "quiet");
    assert_eq!(Verbosity::Verbose.as_str(), "verbose");
    assert!(!opts.timestamps);
    assert_eq!(opts.log_level, "INFO");
    assert!(opts.default_timeout_secs.is_none());
    assert!(opts.default_no_output_timeout_secs.is_none());
}

use toolr_core::parser::SupportedType;
use toolr_core::parser::types::SupportedTypeKind;

fn path_arg(ty: SupportedType, kind: ArgumentKind) -> Argument {
    Argument {
        name: "p".into(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: Some(ty),
        allowed_values: vec![],
        metadata: Default::default(),
        long_flag: None,
    }
}

#[test]
fn every_path_kind_is_read_back_as_a_string_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().to_str().unwrap().to_string();
    let file = tmp.path().join("f.exe");
    std::fs::write(&file, "x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let file = file.to_str().unwrap().to_string();
    let new = tmp.path().join("new").to_str().unwrap().to_string();
    for kind in SupportedTypeKind::ALL {
        let ty = kind.representative();
        if !ty.is_path() {
            continue;
        }
        let value = match ty {
            SupportedType::NewPath => new.clone(),
            SupportedType::FilePath | SupportedType::ExecutablePath => file.clone(),
            _ => dir.clone(),
        };
        let arg = path_arg(ty.clone(), ArgumentKind::Optional);
        let clap_cmd = clap::Command::new("t").arg(crate::value_parsers::apply_value_parser(
            Arg::new("p").long("p"),
            &ty,
        ));
        let matches = clap_cmd.try_get_matches_from(["t", "--p", &value]).unwrap();
        let got = extract_scalar(&arg, &matches, &bases("/repo")).unwrap();
        assert!(
            matches!(got, Some(Value::String(_))),
            "{kind:?} got {got:?}"
        );
    }
}

#[test]
fn list_of_directory_paths_is_read_back_as_strings() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().to_str().unwrap();
    let ty = SupportedType::List(Box::new(SupportedType::DirectoryPath));
    let arg = path_arg(ty.clone(), ArgumentKind::Repeated);
    let clap_cmd = clap::Command::new("t").arg(
        crate::value_parsers::apply_value_parser(Arg::new("p").long("p"), &ty)
            .action(ArgAction::Append),
    );
    let matches = clap_cmd
        .try_get_matches_from(["t", "--p", dir, "--p", dir])
        .unwrap();
    assert_eq!(extract_many(&arg, &matches).len(), 2);
}

mod dispatched_pack_tests {
    //! `pack_child_args` packs a matched dispatched-leaf into a
    //! `PackedChild`. These tests pin the on-the-wire encoding (which
    //! reuses `extract_value`, so flags stay bools and missing
    //! arguments stay absent rather than null) — `build_dispatch_spec`
    //! and the Python runner read the resulting `args` map and must keep
    //! getting the same JSON shape `build_spec` would have produced.
    use super::super::*;
    use super::bases;
    use clap::{Arg, ArgAction};
    use toolr_core::manifest::{Argument, ArgumentKind, Command, Origin};

    fn migrate_cmd() -> Command {
        Command {
            name: "migrate".into(),
            group: "django".into(),
            module: "tools.dispatcher".into(),
            function: "django".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![Argument {
                name: "check".into(),
                kind: ArgumentKind::Flag,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                metadata: Default::default(),
                long_flag: None,
            }],
            origin: Origin::Static,
            dispatched_from: Some("argparse:django".into()),
            is_dispatcher: false,
        }
    }

    #[test]
    fn pack_child_args_extracts_flag_as_bool() {
        let clap_cmd = clap::Command::new("migrate")
            .arg(Arg::new("check").long("check").action(ArgAction::SetTrue));
        let matches = clap_cmd
            .try_get_matches_from(vec!["migrate", "--check"])
            .unwrap();
        let cmd = migrate_cmd();
        let packed = pack_child_args(&cmd, &matches, &bases("/repo")).unwrap();
        assert_eq!(packed.name, "migrate");
        assert_eq!(packed.args.get("check"), Some(&Value::Bool(true)));
        // Schema round-trips intact for the Python runner to consume.
        assert_eq!(
            packed.schema.dispatched_from.as_deref(),
            Some("argparse:django")
        );
        assert_eq!(packed.schema.function, "django");
    }

    #[test]
    fn pack_child_args_unset_flag_records_false_value() {
        // Flags route through `get_flag`, which always yields a bool,
        // so an unset flag still appears in the map as `false`. This
        // matches `build_spec`'s encoding — `extract_value` always
        // returns Some for `ArgumentKind::Flag`.
        let clap_cmd = clap::Command::new("migrate")
            .arg(Arg::new("check").long("check").action(ArgAction::SetTrue));
        let matches = clap_cmd.try_get_matches_from(vec!["migrate"]).unwrap();
        let packed = pack_child_args(&migrate_cmd(), &matches, &bases("/repo")).unwrap();
        assert_eq!(packed.args.get("check"), Some(&Value::Bool(false)));
    }

    #[test]
    fn build_dispatch_spec_routes_to_parent_function() {
        // The grafted leaf carries the parent dispatcher's module +
        // function (set by `graft_children`). `build_dispatch_spec`
        // copies those into the spec so the runner imports the parent.
        let leaf = migrate_cmd();
        let packed = pack_child_args(
            &leaf,
            &clap::Command::new("migrate")
                .arg(Arg::new("check").long("check").action(ArgAction::SetTrue))
                .try_get_matches_from(vec!["migrate", "--check"])
                .unwrap(),
            &bases("/repo"),
        )
        .unwrap();
        // Parent dispatcher has no own args in this scenario — use a
        // copy of the leaf shape but with an empty arguments list so
        // `build_dispatch_spec` won't try to extract a `check` arg
        // from the parent matches.
        let parent = Command {
            arguments: vec![],
            ..migrate_cmd()
        };
        let parent_matches = clap::Command::new("django")
            .try_get_matches_from(vec!["django"])
            .unwrap();
        let spec = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            &bases("/repo"),
            &OutputOptions::default(),
        )
        .unwrap();
        assert_eq!(spec.module, "tools.dispatcher");
        assert_eq!(spec.function, "django");
        let dispatch = spec.dispatch.expect("dispatch present");
        assert_eq!(dispatch.command, "migrate");
        assert_eq!(dispatch.command_args.get("check"), Some(&Value::Bool(true)));
        assert_eq!(dispatch.schema.name, "migrate");
        assert_eq!(dispatch.schema.arguments.len(), 1);
        assert_eq!(dispatch.schema.arguments[0].name, "check");
        assert_eq!(dispatch.schema.arguments[0].kind, "flag");
    }

    #[test]
    fn build_dispatch_spec_extracts_parent_kwargs() {
        // When the parent dispatcher has its own args (e.g. `--cpu`),
        // those are extracted from `parent_matches` into `spec.args` —
        // distinct from the leaf args carried under `dispatch`.
        let parent = Command {
            arguments: vec![Argument {
                name: "cpu".into(),
                kind: ArgumentKind::Optional,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                metadata: Default::default(),
                long_flag: None,
            }],
            ..migrate_cmd()
        };
        let parent_matches = clap::Command::new("django")
            .arg(Arg::new("cpu").long("cpu"))
            .try_get_matches_from(vec!["django", "--cpu", "5000m"])
            .unwrap();
        let leaf_matches = clap::Command::new("migrate")
            .arg(Arg::new("check").long("check").action(ArgAction::SetTrue))
            .try_get_matches_from(vec!["migrate"])
            .unwrap();
        let packed = pack_child_args(&migrate_cmd(), &leaf_matches, &bases("/repo")).unwrap();
        let spec = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            &bases("/repo"),
            &OutputOptions::default(),
        )
        .unwrap();
        assert_eq!(spec.args.get("cpu"), Some(&Value::String("5000m".into())));
        // Dispatch payload still carries the leaf's args, independent of parent kwargs.
        let dispatch = spec.dispatch.expect("dispatch present");
        assert_eq!(
            dispatch.command_args.get("check"),
            Some(&Value::Bool(false))
        );
    }

    /// A leaf whose only argument is a `VarPositional` (mirrors
    /// `company_ids` in toolr#445's repro, or native `*args`) — the
    /// shape that lets clap's `trailing_var_arg` silently swallow an
    /// unrecognized flag as a positional value.
    fn leaf_with_var_positional(name: &str) -> Command {
        Command {
            arguments: vec![Argument {
                name: "company_ids".into(),
                kind: ArgumentKind::VarPositional,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                metadata: Default::default(),
                long_flag: None,
            }],
            ..Command {
                name: name.into(),
                arguments: vec![],
                ..migrate_cmd()
            }
        }
    }

    fn dry_run_dispatcher() -> Command {
        Command {
            name: "job".into(),
            group: "jenkins".into(),
            arguments: vec![Argument {
                name: "dry_run".into(),
                kind: ArgumentKind::Flag,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                metadata: Default::default(),
                long_flag: None,
            }],
            ..migrate_cmd()
        }
    }

    #[test]
    fn build_dispatch_spec_rejects_dispatcher_flag_leaked_into_leaf_var_positional() {
        // Regression for toolr#445: `--dry-run` typed after the leaf name
        // (`toolr jenkins job wrapup_company 12345 --dry-run`) must not
        // silently forward to the wrapped command.
        let leaf = leaf_with_var_positional("wrapup_company");
        let leaf_matches = clap::Command::new("wrapup_company")
            .arg(
                Arg::new("company_ids")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true),
            )
            .try_get_matches_from(vec!["wrapup_company", "12345", "--dry-run"])
            .unwrap();
        let packed = pack_child_args(&leaf, &leaf_matches, &bases("/repo")).unwrap();
        let parent = dry_run_dispatcher();
        let parent_matches = clap::Command::new("job")
            .arg(
                Arg::new("dry_run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            )
            .try_get_matches_from(vec!["job"])
            .unwrap();
        let err = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            &bases("/repo"),
            &OutputOptions::default(),
        )
        .expect_err("leaked --dry-run must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("--dry-run"),
            "expected flag name in error, got: {msg}"
        );
        assert!(
            msg.contains("jenkins job"),
            "expected dispatcher path in error, got: {msg}"
        );
        assert!(
            msg.contains("wrapup_company"),
            "expected leaf name in error, got: {msg}"
        );
    }

    #[test]
    fn build_dispatch_spec_accepts_dry_run_placed_before_the_leaf() {
        // The correctly-ordered invocation must keep working: `--dry-run`
        // is consumed by the parent's own clap `Arg`, so it never lands
        // in the leaf's `VarPositional` bucket at all.
        let leaf = leaf_with_var_positional("wrapup_company");
        let leaf_matches = clap::Command::new("wrapup_company")
            .arg(
                Arg::new("company_ids")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true),
            )
            .try_get_matches_from(vec!["wrapup_company", "12345"])
            .unwrap();
        let packed = pack_child_args(&leaf, &leaf_matches, &bases("/repo")).unwrap();
        let parent = dry_run_dispatcher();
        let parent_matches = clap::Command::new("job")
            .arg(
                Arg::new("dry_run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            )
            .try_get_matches_from(vec!["job", "--dry-run"])
            .unwrap();
        let spec = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            &bases("/repo"),
            &OutputOptions::default(),
        )
        .unwrap();
        assert_eq!(spec.args.get("dry_run"), Some(&Value::Bool(true)));
    }

    #[test]
    fn build_dispatch_spec_does_not_flag_a_literal_positional_value() {
        // A positional value that isn't one of the parent's own flag
        // spellings must pass through untouched — the check is an exact
        // match against the parent's declared flags, not a blanket
        // "looks like a flag" heuristic.
        let leaf = leaf_with_var_positional("wrapup_company");
        let leaf_matches = clap::Command::new("wrapup_company")
            .arg(
                Arg::new("company_ids")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true),
            )
            .try_get_matches_from(vec!["wrapup_company", "--stuck-company"])
            .unwrap();
        let packed = pack_child_args(&leaf, &leaf_matches, &bases("/repo")).unwrap();
        let parent = dry_run_dispatcher();
        let parent_matches = clap::Command::new("job")
            .arg(
                Arg::new("dry_run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue),
            )
            .try_get_matches_from(vec!["job"])
            .unwrap();
        let spec = build_dispatch_spec(
            &parent,
            &parent_matches,
            packed,
            &bases("/repo"),
            &OutputOptions::default(),
        )
        .unwrap();
        assert_eq!(
            spec.dispatch.unwrap().command_args.get("company_ids"),
            Some(&Value::Array(vec![Value::String("--stuck-company".into())]))
        );
    }

    #[test]
    fn argument_to_arg_schema_maps_all_kinds() {
        fn argument_named(name: &str, kind: ArgumentKind) -> Argument {
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
        let cases = [
            (ArgumentKind::Positional, "positional"),
            (ArgumentKind::Optional, "optional"),
            (ArgumentKind::Flag, "flag"),
            (ArgumentKind::Count, "flag"),
            (ArgumentKind::Repeated, "repeated"),
            (ArgumentKind::VarPositional, "positional"),
            (ArgumentKind::OptionalPositional, "positional"),
        ];
        for (kind, expected) in cases {
            let schema = argument_to_arg_schema(&argument_named("a", kind));
            assert_eq!(schema.kind, expected, "kind {kind:?}");
        }
    }

    #[test]
    fn argument_to_arg_schema_carries_choices_metavar_and_type_annotation() {
        let metadata = toolr_core::manifest::ArgMetadata {
            metavar: Some("PATH".into()),
            ..Default::default()
        };
        let arg = Argument {
            name: "out".into(),
            kind: ArgumentKind::Optional,
            help: "where to write".into(),
            default: Some("/tmp".into()),
            type_annotation: Some("str".into()),
            resolved_type: None,
            allowed_values: vec!["a".into(), "b".into()],
            metadata,
            long_flag: None,
        };
        let schema = argument_to_arg_schema(&arg);
        assert_eq!(schema.name, "out");
        assert_eq!(schema.help, "where to write");
        assert_eq!(schema.default.as_deref(), Some("/tmp"));
        assert_eq!(schema.metavar.as_deref(), Some("PATH"));
        assert_eq!(schema.type_annotation.as_deref(), Some("str"));
        assert_eq!(
            schema.choices.as_deref(),
            Some(&["a".to_string(), "b".to_string()][..])
        );
    }

    #[test]
    fn argument_to_arg_schema_carries_nargs_star_for_repeated() {
        fn repeated_with(nargs: Option<Nargs>) -> Argument {
            Argument {
                name: "customer-ids".into(),
                kind: ArgumentKind::Repeated,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                metadata: toolr_core::manifest::ArgMetadata {
                    nargs,
                    ..Default::default()
                },
                long_flag: None,
            }
        }
        assert_eq!(
            argument_to_arg_schema(&repeated_with(Some(Nargs::Star))).nargs,
            Some(serde_json::json!("*"))
        );
        assert_eq!(
            argument_to_arg_schema(&repeated_with(Some(Nargs::Plus))).nargs,
            Some(serde_json::json!("+"))
        );
        assert_eq!(argument_to_arg_schema(&repeated_with(None)).nargs, None);
    }

    fn bare_arg(name: &str, kind: ArgumentKind) -> Argument {
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
    fn argument_to_arg_schema_maps_fixed_arity_keyword_to_optional_with_int_nargs() {
        let mut arg = bare_arg("pair", ArgumentKind::FixedArity);
        arg.long_flag = Some("--pair".to_string());
        arg.metadata.nargs = Some(Nargs::Fixed(2));
        let schema = argument_to_arg_schema(&arg);
        assert_eq!(schema.kind, "optional");
        assert_eq!(schema.nargs, Some(serde_json::json!(2)));
    }

    #[test]
    fn argument_to_arg_schema_maps_fixed_arity_positional_to_positional_with_int_nargs() {
        let mut arg = bare_arg("files", ArgumentKind::FixedArity);
        arg.metadata.nargs = Some(Nargs::Fixed(3));
        let schema = argument_to_arg_schema(&arg);
        assert_eq!(schema.kind, "positional");
        assert_eq!(schema.nargs, Some(serde_json::json!(3)));
    }

    #[test]
    fn argument_to_arg_schema_maps_optional_positional_to_positional_with_question_nargs() {
        let arg = bare_arg("maybe", ArgumentKind::OptionalPositional);
        let schema = argument_to_arg_schema(&arg);
        assert_eq!(schema.kind, "positional");
        assert_eq!(schema.nargs, Some(serde_json::json!("?")));
    }

    #[test]
    fn pack_child_args_omits_unset_optional() {
        // Optional args with no clap value present should be absent
        // from the packed map (rather than `null`) — matches the
        // `build_spec_missing_optional_value_does_not_appear_in_args_map`
        // contract.
        let cmd = Command {
            arguments: vec![Argument {
                name: "label".into(),
                kind: ArgumentKind::Optional,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                metadata: Default::default(),
                long_flag: None,
            }],
            ..migrate_cmd()
        };
        let clap_cmd = clap::Command::new("migrate").arg(Arg::new("label").long("label"));
        let matches = clap_cmd.try_get_matches_from(vec!["migrate"]).unwrap();
        let packed = pack_child_args(&cmd, &matches, &bases("/repo")).unwrap();
        assert!(!packed.args.contains_key("label"));
    }
}

mod output_options_tests {
    use super::super::*;
    use toolr_core::manifest::Manifest;

    fn parse(args: &[&str]) -> ArgMatches {
        let manifest = Manifest::empty();
        let root = crate::cli::build_command(&manifest);
        let mut full: Vec<String> = vec!["toolr".into()];
        full.extend(args.iter().map(|s| (*s).to_string()));
        root.try_get_matches_from(full).unwrap()
    }

    #[test]
    fn output_options_defaults_when_no_flags_set() {
        let m = parse(&[]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.verbosity, OutputOptions::default().verbosity);
        assert_eq!(opts.log_level, OutputOptions::default().log_level);
        assert!(!opts.timestamps);
        assert!(opts.default_timeout_secs.is_none());
        assert!(opts.default_no_output_timeout_secs.is_none());
    }

    #[test]
    fn output_options_quiet_overrides_verbosity_and_log_level() {
        let m = parse(&["--quiet"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.verbosity, Verbosity::Quiet);
        assert_eq!(opts.log_level, "INFO");
    }

    #[test]
    fn output_options_debug_overrides_verbosity_and_log_level() {
        let m = parse(&["--debug"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.verbosity, Verbosity::Verbose);
        assert_eq!(opts.log_level, "DEBUG");
    }

    #[test]
    fn output_options_timestamps_flag_is_propagated() {
        let m = parse(&["--timestamps"]);
        let opts = output_options_from_matches(&m);
        assert!(opts.timestamps);
    }

    #[test]
    fn output_options_no_timestamps_gives_false() {
        let m = parse(&["--no-timestamps"]);
        let opts = output_options_from_matches(&m);
        assert!(!opts.timestamps);
    }

    #[test]
    fn output_options_propagates_timeout_secs() {
        let m = parse(&["--timeout-secs", "12.5"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.default_timeout_secs, Some(12.5));
    }

    #[test]
    fn output_options_propagates_no_output_timeout_secs() {
        let m = parse(&["--no-output-timeout-secs", "3"]);
        let opts = output_options_from_matches(&m);
        assert_eq!(opts.default_no_output_timeout_secs, Some(3.0));
    }
}
