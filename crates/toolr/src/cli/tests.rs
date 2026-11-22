use super::*;
use toolr_core::manifest::{Argument, ArgumentKind, Command, Group, Manifest, Origin};

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

fn dispatcher(name: &str, group: &str, args: Vec<Argument>) -> Command {
    Command {
        name: name.into(),
        group: group.into(),
        module: format!("tools.{name}"),
        function: name.into(),
        summary: String::new(),
        description: String::new(),
        arguments: args,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: true,
    }
}

fn child(name: &str, group: &str, dispatcher_module: &str, dispatcher_fn: &str) -> Command {
    Command {
        name: name.into(),
        group: group.into(),
        module: dispatcher_module.into(),
        function: dispatcher_fn.into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::Static,
        dispatched_from: Some(format!("argparse:{group}")),
        is_dispatcher: false,
    }
}

fn normal_leaf(name: &str, group: &str) -> Command {
    Command {
        name: name.into(),
        group: group.into(),
        module: format!("tools.{name}"),
        function: name.into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

fn group(name: &str) -> Group {
    Group {
        name: name.into(),
        title: name.into(),
        description: String::new(),
        parent: None,
        origin: Origin::Static,
    }
}

fn build_for(manifest: Manifest) -> clap::Command {
    let groups: Vec<&Group> = manifest.groups.iter().collect();
    let mut children_map: std::collections::HashMap<Option<String>, Vec<&Group>> =
        std::collections::HashMap::new();
    for g in &groups {
        children_map.entry(g.parent.clone()).or_default().push(g);
    }
    let top: Vec<&Group> = manifest
        .groups
        .iter()
        .filter(|g| g.parent.is_none())
        .collect();
    build_group_subtree(top[0], &manifest, &children_map)
}

#[test]
fn dispatcher_hosts_two_grafted_children() {
    let dispatcher_cmd = dispatcher(
        "job",
        "jenkins",
        vec![empty_arg("cpu", ArgumentKind::Optional)],
    );
    // Grafted children live under the dispatcher's dotted_name
    // ("jenkins.job"), mirroring the argparse pipeline's output.
    let migrate = child("migrate", "jenkins.job", "tools.job", "job");
    let runserver = child("runserver", "jenkins.job", "tools.job", "job");

    let manifest = Manifest {
        groups: vec![group("jenkins")],
        commands: vec![dispatcher_cmd, migrate, runserver],
        ..Manifest::empty()
    };

    let jenkins = build_for(manifest);

    let group_subs: Vec<&str> = jenkins.get_subcommands().map(|c| c.get_name()).collect();
    assert_eq!(group_subs, vec!["job"]);

    let job = jenkins.find_subcommand("job").expect("job under jenkins");
    let mut job_subs: Vec<&str> = job.get_subcommands().map(|c| c.get_name()).collect();
    job_subs.sort();
    assert_eq!(job_subs, vec!["migrate", "runserver"]);
}

#[test]
fn two_dispatchers_in_one_group_each_host_their_own_children() {
    let build_cmd = dispatcher("build", "docker", vec![]);
    let image_cmd = dispatcher("image", "docker", vec![]);
    let build_child = child("compile", "docker.build", "tools.build", "build");
    let image_child = child("push", "docker.image", "tools.image", "image");

    let manifest = Manifest {
        groups: vec![group("docker")],
        commands: vec![build_cmd, image_cmd, build_child, image_child],
        ..Manifest::empty()
    };

    let docker = build_for(manifest);
    let build_sub = docker.find_subcommand("build").unwrap();
    let image_sub = docker.find_subcommand("image").unwrap();

    let build_subs: Vec<&str> = build_sub.get_subcommands().map(|c| c.get_name()).collect();
    let image_subs: Vec<&str> = image_sub.get_subcommands().map(|c| c.get_name()).collect();
    assert_eq!(build_subs, vec!["compile"]);
    assert_eq!(image_subs, vec!["push"]);
}

#[test]
fn dispatcher_and_normal_leaf_coexist_in_one_group() {
    let dispatcher_cmd = dispatcher("job", "jenkins", vec![]);
    let migrate = child("migrate", "jenkins.job", "tools.job", "job");
    let status = normal_leaf("status", "jenkins");

    let manifest = Manifest {
        groups: vec![group("jenkins")],
        commands: vec![dispatcher_cmd, migrate, status],
        ..Manifest::empty()
    };

    let jenkins = build_for(manifest);
    let mut group_subs: Vec<&str> = jenkins.get_subcommands().map(|c| c.get_name()).collect();
    group_subs.sort();
    assert_eq!(group_subs, vec!["job", "status"]);

    let job = jenkins.find_subcommand("job").unwrap();
    let job_subs: Vec<&str> = job.get_subcommands().map(|c| c.get_name()).collect();
    assert_eq!(job_subs, vec!["migrate"]);

    let status_sub = jenkins.find_subcommand("status").unwrap();
    assert_eq!(status_sub.get_subcommands().count(), 0);
}

#[test]
fn underscored_alias_lets_clap_accept_both_spellings() {
    // Mirrors the argparse scanner's output for
    // `add_argument('--skip_warm_cache', action='store_true')`:
    // the canonical CLI form is dashed (so help and shell
    // completion offer the dashed spelling), but the upstream
    // underscored form is registered as a hidden alias.
    let mut arg = empty_arg("skip_warm_cache", ArgumentKind::Flag);
    arg.metadata.aliases.push("--skip_warm_cache".into());
    arg.long_flag = Some("--skip_warm_cache".into());

    let mut cmd = normal_leaf("wrapup-company", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);

    // Dashed form parses.
    let dashed = clap_cmd
        .clone()
        .try_get_matches_from(vec!["wrapup-company", "--skip-warm-cache"])
        .expect("dashed form must parse");
    assert!(dashed.get_flag("skip_warm_cache"));

    // Underscored form (the upstream argparse spelling) parses too.
    let underscored = clap_cmd
        .clone()
        .try_get_matches_from(vec!["wrapup-company", "--skip_warm_cache"])
        .expect("underscored form must parse");
    assert!(underscored.get_flag("skip_warm_cache"));
}

#[test]
fn unresolvable_expr_default_is_not_applied_as_a_clap_default() {
    // Mirrors `param: Path = Path("~/projects")` — toolr-core can't
    // serialise the call expression to a literal, so it emits the
    // `<expr>` sentinel. That sentinel must never reach clap as a
    // real default_value (it would ship the literal string
    // "<expr>" whenever the flag is omitted); Python's own default
    // expression should run instead.
    let mut arg = empty_arg("root", ArgumentKind::Optional);
    arg.default = Some("<expr>".to_string());

    let mut cmd = normal_leaf("sync", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    let matches = clap_cmd
        .try_get_matches_from(vec!["sync"])
        .expect("omitting the flag must still parse");
    assert_eq!(matches.get_one::<String>("root"), None);
}

#[test]
fn relative_path_default_parses_without_touching_disk() {
    let mut arg = empty_arg("cfg", ArgumentKind::Optional);
    arg.default = Some("tools/pyproject.toml".to_string());
    arg.resolved_type = Some(toolr_core::parser::SupportedType::FilePath);
    let mut cmd = normal_leaf("hello", "demo");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    let defaults: Vec<_> = clap_cmd
        .get_arguments()
        .find(|a| a.get_id() == "cfg")
        .unwrap()
        .get_default_values()
        .to_vec();
    assert_eq!(defaults, ["tools/pyproject.toml"]);
    assert!(clap_cmd.try_get_matches_from(vec!["hello"]).is_ok());
}

#[test]
fn resolvable_literal_default_still_applies_for_typed_arguments() {
    // Contrast with the `<expr>` case above: a plain string literal
    // default (the actually-supported spelling, e.g.
    // `id: uuid.UUID = "12345678-1234-5678-1234-567812345678"`)
    // resolves cleanly in `literal_default()` and must still reach
    // clap as a real default — and pass the type's own value_parser.
    let mut arg = empty_arg("id", ArgumentKind::Optional);
    arg.default = Some("12345678-1234-5678-1234-567812345678".to_string());
    arg.resolved_type = Some(toolr_core::parser::SupportedType::Uuid);

    let mut cmd = normal_leaf("sync", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    let matches = clap_cmd
        .try_get_matches_from(vec!["sync"])
        .expect("omitting the flag must still parse using the default");
    assert_eq!(
        matches.get_one::<String>("id").map(String::as_str),
        Some("12345678-1234-5678-1234-567812345678")
    );
}

#[test]
fn repeated_flag_accepts_space_separated_values_and_repeat_invocations() {
    // Covers argparse `nargs="+"`/`"*"` (space-separated values in one
    // occurrence) alongside the pre-existing `action="append"` shape
    // (one value per occurrence, repeated). Both classify as
    // `ArgumentKind::Repeated`; the scanner sets `metadata.nargs` to
    // `Some(Star)`/`Some(Plus)` only for the nargs case.
    let mut arg = empty_arg("names", ArgumentKind::Repeated);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Star);
    let mut cmd = normal_leaf("greet", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    let space_separated = clap_cmd
        .clone()
        .try_get_matches_from(vec!["greet", "--names", "alice", "bob"])
        .expect("space-separated values must parse");
    assert_eq!(
        space_separated
            .get_many::<String>("names")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["alice", "bob"]
    );

    let repeated_flag = clap_cmd
        .try_get_matches_from(vec!["greet", "--names", "alice", "--names", "bob"])
        .expect("repeated flag invocations must parse");
    assert_eq!(
        repeated_flag
            .get_many::<String>("names")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["alice", "bob"]
    );
}

#[test]
fn plain_repeated_flag_beside_a_required_positional_does_not_swallow_it() {
    // Without `metadata.nargs` set (the `action="append"` shape, and
    // every pre-existing native `list[T]` keyword param), each
    // `--names` occurrence must still take exactly one value so a
    // trailing positional isn't swallowed by a widened `num_args`.
    let mut names_arg = empty_arg("names", ArgumentKind::Repeated);
    names_arg.long_flag = Some("--names".into());
    let env_arg = empty_arg("env", ArgumentKind::Positional);

    let mut cmd = normal_leaf("greet", "jenkins");
    cmd.arguments = vec![names_arg, env_arg];

    let clap_cmd = build_user_command(&cmd);
    let matches = clap_cmd
        .try_get_matches_from(vec!["greet", "--names", "a", "prod"])
        .expect("positional must still be reachable after a single-value --names");
    assert_eq!(
        matches
            .get_many::<String>("names")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["a"]
    );
    assert_eq!(
        matches.get_one::<String>("env").map(String::as_str),
        Some("prod")
    );
}

#[test]
fn var_positional_nargs_plus_rejects_zero_values() {
    // argparse `nargs="+"` on a positional (e.g. Django's
    // `manage.py test app1 app2`) demands at least one value, unlike
    // native `*args`/argparse `nargs="*"` where zero is valid.
    let mut labels_arg = empty_arg("labels", ArgumentKind::VarPositional);
    labels_arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Plus);

    let mut cmd = normal_leaf("test", "jenkins");
    cmd.arguments = vec![labels_arg];

    let clap_cmd = build_user_command(&cmd);
    clap_cmd
        .clone()
        .try_get_matches_from(vec!["test"])
        .expect_err("zero values must be rejected when nargs is Plus");

    let matches = clap_cmd
        .try_get_matches_from(vec!["test", "app1", "app2"])
        .expect("one or more values must parse");
    assert_eq!(
        matches
            .get_many::<String>("labels")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["app1", "app2"]
    );
}

#[test]
fn fixed_arity_keyword_gets_exact_num_args() {
    let mut arg = empty_arg("pair", ArgumentKind::FixedArity);
    arg.long_flag = Some("--pair".to_string());
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(2));

    let mut cmd = normal_leaf("greet", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    let matches = clap_cmd
        .try_get_matches_from(vec!["greet", "--pair", "a", "b"])
        .expect("exactly 2 values must parse");
    assert_eq!(
        matches
            .get_many::<String>("pair")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn fixed_arity_keyword_rejects_wrong_arity() {
    let mut arg = empty_arg("pair", ArgumentKind::FixedArity);
    arg.long_flag = Some("--pair".to_string());
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(2));

    let mut cmd = normal_leaf("greet", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    clap_cmd
        .try_get_matches_from(vec!["greet", "--pair", "a"])
        .expect_err("one value must be rejected when arity is 2");
}

#[test]
fn fixed_arity_positional_gets_exact_num_args_and_is_required() {
    let arg = empty_arg("files", ArgumentKind::FixedArity);
    let mut arg = arg;
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(3));

    let mut cmd = normal_leaf("copy", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    clap_cmd
        .clone()
        .try_get_matches_from(vec!["copy"])
        .expect_err("a required positional fixed-arity arg must reject zero values");

    let matches = clap_cmd
        .try_get_matches_from(vec!["copy", "x", "y", "z"])
        .expect("exactly 3 values must parse");
    assert_eq!(
        matches
            .get_many::<String>("files")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["x", "y", "z"]
    );
}

#[test]
fn optional_positional_is_not_required() {
    let arg = empty_arg("maybe", ArgumentKind::OptionalPositional);

    let mut cmd = normal_leaf("cmd", "jenkins");
    cmd.arguments = vec![arg];

    let clap_cmd = build_user_command(&cmd);
    clap_cmd
        .clone()
        .try_get_matches_from(vec!["cmd"])
        .expect("an optional positional must parse when omitted");

    let matches = clap_cmd
        .try_get_matches_from(vec!["cmd", "present"])
        .expect("an optional positional must still parse when given");
    assert_eq!(
        matches.get_one::<String>("maybe").map(String::as_str),
        Some("present")
    );
}

#[test]
fn dispatcher_with_matching_leaf_name_hoists_children_onto_group() {
    // command_group("django") + def django(...) → dotted_name == "django"
    // (matches the leaf), so the dispatcher's children appear
    // directly under the group, not under a redundant `django`
    // subcommand. User types `toolr django migrate`, not
    // `toolr django django migrate`.
    let dispatcher_cmd = dispatcher("django", "django", vec![]);
    let migrate = child("migrate", "django", "tools.dispatcher", "django");

    let manifest = Manifest {
        groups: vec![group("django")],
        commands: vec![dispatcher_cmd, migrate],
        ..Manifest::empty()
    };

    let django = build_for(manifest);
    let group_subs: Vec<&str> = django.get_subcommands().map(|c| c.get_name()).collect();
    assert_eq!(group_subs, vec!["migrate"]);
}

#[test]
fn user_groups_precede_built_ins_and_reserved_names_are_skipped() {
    let manifest = Manifest {
        groups: vec![group("ci"), group("self"), group("project")],
        commands: vec![
            normal_leaf("hello", "ci"),
            normal_leaf("hijack", "self"),
            normal_leaf("hijack", "project"),
        ],
        ..Manifest::empty()
    };

    let root = build_command(&manifest);
    let names: Vec<&str> = root.get_subcommands().map(|c| c.get_name()).collect();
    assert_eq!(
        names,
        vec!["ci", "project", "self", "__complete", "__install-uv-now",]
    );
    for reserved in ["project", "self"] {
        let built_in = root.find_subcommand(reserved).unwrap();
        assert!(built_in.find_subcommand("hijack").is_none());
    }
}

/// One `ci run` command whose parameters are named like toolr's root flags.
fn parse_ci_run(args: &[&str]) -> clap::ArgMatches {
    let mut run = normal_leaf("run", "ci");
    run.arguments = vec![
        empty_arg("quiet", ArgumentKind::Flag),
        empty_arg("debug", ArgumentKind::Flag),
    ];
    let manifest = Manifest {
        groups: vec![group("ci")],
        commands: vec![run],
        ..Manifest::empty()
    };
    build_command(&manifest)
        .try_get_matches_from(std::iter::once("toolr").chain(args.iter().copied()))
        .unwrap()
}

#[test]
fn parameters_named_like_root_flags_are_ordinary_arguments() {
    // The command's own flags set: toolr's stay false.
    let m = parse_ci_run(&["ci", "run", "--quiet", "--debug"]);
    let (_, ci) = m.subcommand().unwrap();
    let (_, run) = ci.subcommand().unwrap();
    assert!(run.get_flag("quiet") && run.get_flag("debug"));
    assert!(!m.get_flag("quiet") && !m.get_flag("debug"));
    // toolr's own flags set: the command's stay false.
    let m = parse_ci_run(&["-q", "ci", "run"]);
    let (_, ci) = m.subcommand().unwrap();
    let (_, run) = ci.subcommand().unwrap();
    assert!(m.get_flag("quiet"));
    assert!(!run.get_flag("quiet") && !run.get_flag("debug"));
}
