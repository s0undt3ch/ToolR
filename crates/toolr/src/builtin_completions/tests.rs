use super::*;
use toolr_core::complete::{Candidate, serve_completions};

fn values(candidates: Vec<Candidate>) -> Vec<String> {
    candidates.into_iter().map(|c| c.value).collect()
}

#[test]
fn walk_group_skips_hidden_children() {
    let group = ClapCommand::new("grp")
        .subcommand(ClapCommand::new("shown"))
        .subcommand(ClapCommand::new("secret").hide(true));
    let (mut groups, mut commands) = (Vec::new(), Vec::new());
    walk_group(&group, None, &mut groups, &mut commands);
    let names: Vec<_> = commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["shown"]);
}

fn merged_empty_manifest() -> Manifest {
    let (groups, commands) = built_in_completion_entries();
    Manifest {
        groups,
        commands,
        ..Manifest::empty()
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
    assert_eq!(
        derive_argument(&append).unwrap().kind,
        ArgumentKind::Repeated
    );

    let set_true = Arg::new("force").long("force").action(ArgAction::SetTrue);
    assert_eq!(derive_argument(&set_true).unwrap().kind, ArgumentKind::Flag);

    let optional = Arg::new("prefix").long("prefix").action(ArgAction::Set);
    assert_eq!(
        derive_argument(&optional).unwrap().kind,
        ArgumentKind::Optional
    );

    // `help` and hidden args are dropped from completion output.
    let help = Arg::new("help").long("help").action(ArgAction::SetTrue);
    assert!(derive_argument(&help).is_none());
    let hidden = Arg::new("secret")
        .long("secret")
        .action(ArgAction::SetTrue)
        .hide(true);
    assert!(derive_argument(&hidden).is_none());
}

#[test]
fn derive_argument_maps_value_hints_to_path_types() {
    for (hint, want) in [
        (ValueHint::AnyPath, Some(SupportedType::Path)),
        (ValueHint::FilePath, Some(SupportedType::FilePath)),
        (ValueHint::DirPath, Some(SupportedType::DirectoryPath)),
        (
            ValueHint::ExecutablePath,
            Some(SupportedType::ExecutablePath),
        ),
        (ValueHint::Url, None),
    ] {
        let arg = Arg::new("p").long("p").value_hint(hint);
        assert_eq!(
            derive_argument(&arg).unwrap().resolved_type,
            want,
            "{hint:?}"
        );
    }
    let plain = Arg::new("name").long("name");
    assert_eq!(derive_argument(&plain).unwrap().resolved_type, None);
}

// --- Behavioural: the derived tree drives real completion output ---

#[test]
fn top_level_offers_self_and_project() {
    let m = merged_empty_manifest();
    let out = values(serve_completions(&m, &tokens(&[""])));
    assert!(out.contains(&"self".to_string()), "candidates: {out:?}");
    assert!(out.contains(&"project".to_string()), "candidates: {out:?}");
}

#[test]
fn self_offers_known_subcommands() {
    // Proves a `self` subcommand defined only in `cli.rs` surfaces
    // here without editing this file.
    let m = merged_empty_manifest();
    let out = values(serve_completions(&m, &tokens(&["self", ""])));
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
    let out = values(serve_completions(&m, &tokens(&["project", ""])));
    for expected in ["init", "venv", "manifest"] {
        assert!(
            out.contains(&expected.to_string()),
            "missing {expected} in {out:?}"
        );
    }
}

#[test]
fn project_venv_offers_run_path_shell_sync_lock_add_remove() {
    let m = merged_empty_manifest();
    let out = values(serve_completions(&m, &tokens(&["project", "venv", ""])));
    for expected in ["run", "path", "shell", "sync", "lock", "add", "remove"] {
        assert!(
            out.contains(&expected.to_string()),
            "missing {expected} under project venv, got: {out:?}"
        );
    }
}

#[test]
fn project_venv_sync_offers_its_own_flags() {
    let m = merged_empty_manifest();
    let out = values(serve_completions(
        &m,
        &tokens(&["project", "venv", "sync", "--"]),
    ));
    for expected in ["--force", "--if-project"] {
        assert!(
            out.contains(&expected.to_string()),
            "missing {expected} in project venv sync flags, got: {out:?}"
        );
    }
}

#[test]
fn self_cache_offers_list_and_prune() {
    let m = merged_empty_manifest();
    let out = values(serve_completions(&m, &tokens(&["self", "cache", ""])));
    assert_eq!(out, vec!["list".to_string(), "prune".to_string()]);
}

#[test]
fn self_completion_install_offers_shells() {
    // A leaf with an enum positional offers its possible values,
    // proving `allowed_values` is derived from clap's value_parser.
    let m = merged_empty_manifest();
    let out = values(serve_completions(
        &m,
        &tokens(&["self", "completion", "install", ""]),
    ));
    assert_eq!(out, vec!["bash", "fish", "powershell", "zsh"]);
}

#[test]
fn self_cache_prune_offers_known_flags() {
    let m = merged_empty_manifest();
    let out = values(serve_completions(
        &m,
        &tokens(&["self", "cache", "prune", "--"]),
    ));
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
    let out = values(serve_completions(
        &m,
        &tokens(&["project", "init", "--venv-location", ""]),
    ));
    assert_eq!(out, vec!["cache".to_string(), "in-tree".to_string()]);
}

// --- Derivation invariants ---

#[test]
fn hidden_internal_helpers_are_absent() {
    // The `__*` helpers are `.hide(true)` top-level subcommands and
    // must never appear as completion candidates.
    let m = merged_empty_manifest();
    let out = values(serve_completions(&m, &tokens(&[""])));
    for hidden in ["__complete", "__install-uv-now"] {
        assert!(
            !out.contains(&hidden.to_string()),
            "hidden helper {hidden} leaked into candidates: {out:?}"
        );
    }
}

#[test]
fn offerable_long_flags_are_sorted_and_exclude_help() {
    let flags = values(scan(&[""]).offerable_long_flags());
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

fn scan(tokens: &[&str]) -> RootScan {
    let owned: Vec<String> = tokens.iter().map(ToString::to_string).collect();
    scan_root_flags(&owned)
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn scan_strips_leading_root_flags() {
    let s = scan(&["--quiet", "--timestamps", "ci", ""]);
    assert_eq!(strs(&s.rest), ["ci", ""]);
    assert_eq!(strs(&s.seen), ["quiet", "timestamps"]);
    assert!(!s.dead_end);
}

#[test]
fn scan_in_progress_word_is_never_consumed() {
    let s = scan(&["--quiet", "--timeo"]);
    assert_eq!(strs(&s.rest), ["--timeo"]);
    assert_eq!(strs(&s.seen), ["quiet"]);
    assert!(!s.dead_end);
}

#[test]
fn scan_without_tokens_is_empty() {
    let s = scan(&[]);
    assert!(s.rest.is_empty() && s.seen.is_empty() && !s.dead_end);
}

#[test]
fn scan_resolves_aliases_and_shorts() {
    let s = scan(&["--ts", "-q", "--nots", "5", ""]);
    assert_eq!(
        strs(&s.seen),
        ["no-output-timeout-secs", "quiet", "timestamps"]
    );
    assert_eq!(strs(&s.rest), [""]);
    assert!(!s.dead_end);
    let s = scan(&["-h", "--nts", "--timeout", "2", ""]);
    assert_eq!(strs(&s.seen), ["help", "no-timestamps", "timeout-secs"]);
}

#[test]
fn scan_value_flag_takes_next_token() {
    let s = scan(&["--timeout-secs", "5", ""]);
    assert_eq!(strs(&s.rest), [""]);
    assert_eq!(strs(&s.seen), ["timeout-secs"]);
    assert!(!s.dead_end);
}

#[test]
fn scan_attached_value() {
    let s = scan(&["--timeout-secs=5", ""]);
    assert_eq!(strs(&s.rest), [""]);
    assert_eq!(strs(&s.seen), ["timeout-secs"]);
    assert!(!s.dead_end);
}

#[test]
fn scan_value_owed_is_dead_end() {
    assert!(scan(&["--timeout-secs", ""]).dead_end);
    assert!(scan(&["--quiet", "--timeout-secs", "--"]).dead_end);
}

#[test]
fn scan_bad_value_is_dead_end() {
    for line in [
        &["--timeout-secs", "abc", ""][..],
        &["--timeout-secs=", ""],
        &["--timeout-secs=abc", ""],
        &["--timeout-secs", "-5", ""],
        &["--timeout-secs", "--quiet", ""],
        &["--quiet=yes", ""],
    ] {
        assert!(scan(line).dead_end, "{line:?}");
    }
}

#[test]
fn scan_duplicate_is_dead_end() {
    for line in [
        &["--quiet", "--quiet", ""][..],
        &["--ts", "--timestamps", ""],
        &["--help", "-h", ""],
        &["--timeout-secs", "1", "--timeout", "2", ""],
        &["-qq", ""],
    ] {
        assert!(scan(line).dead_end, "{line:?}");
    }
}

#[test]
fn scan_conflict_is_dead_end() {
    for line in [
        &["--quiet", "--debug", ""][..],
        &["-d", "--quiet", ""],
        &["-qd", ""],
        &["--ts", "--nts", ""],
        &["--nts", "--timestamps", ""],
    ] {
        assert!(scan(line).dead_end, "{line:?}");
    }
}

#[test]
fn scan_stops_at_terminator() {
    let s = scan(&["--quiet", "--", ""]);
    assert_eq!(strs(&s.rest), ["--", ""]);
    assert_eq!(strs(&s.seen), ["quiet"]);
    assert!(!s.dead_end);
}

#[test]
fn scan_stops_at_first_word() {
    let s = scan(&["--quiet", "ci", "--debug", ""]);
    assert_eq!(strs(&s.rest), ["ci", "--debug", ""]);
    assert_eq!(strs(&s.seen), ["quiet"]);
    assert!(!s.dead_end);
}

#[test]
fn scan_stops_at_unknown_flag_or_short() {
    for line in [&["--bogus", "--quiet", ""][..], &["-qx", ""], &["-", ""]] {
        let s = scan(line);
        assert_eq!(strs(&s.rest), line, "{line:?}");
        assert!(s.seen.is_empty() && !s.dead_end, "{line:?}");
    }
}

#[test]
fn root_flag_conflicts_are_symmetric() {
    let specs = root_flag_specs();
    for a in &specs {
        for b in &specs {
            assert_eq!(
                a.conflicts.contains(&b.id),
                b.conflicts.contains(&a.id),
                "{} / {}",
                a.id,
                b.id
            );
        }
    }
}

#[test]
fn offerable_long_flags_skip_given_and_conflicting() {
    let after = |line: &[&str]| values(scan(line).offerable_long_flags());
    assert_eq!(
        after(&[""]),
        [
            "--debug",
            "--no-output-timeout-secs",
            "--no-timestamps",
            "--quiet",
            "--timeout-secs",
            "--timestamps"
        ]
    );
    assert_eq!(
        after(&["--quiet", ""]),
        [
            "--no-output-timeout-secs",
            "--no-timestamps",
            "--timeout-secs",
            "--timestamps"
        ]
    );
    assert_eq!(
        after(&["--timestamps", "--timeout-secs=1", ""]),
        ["--debug", "--no-output-timeout-secs", "--quiet"]
    );
}

#[test]
fn scan_accepts_attached_negative_values() {
    for line in [
        &["--timeout-secs=-1", ""][..],
        &["--nots=-1", ""],
        &["--timeout-secs=-1e3", ""],
    ] {
        let s = scan(line);
        assert!(!s.dead_end, "{line:?}");
        assert_eq!(strs(&s.rest), [""], "{line:?}");
    }
    assert!(scan(&["--timeout-secs=-x", ""]).dead_end);
}

#[test]
fn scan_version_ends_the_line() {
    for line in [
        &["--version", ""][..],
        &["-V", ""],
        &["-qV", ""],
        &["--quiet", "--version", "--"],
    ] {
        assert!(scan(line).dead_end, "{line:?}");
    }
}

#[test]
fn version_is_still_not_offered() {
    let flags = values(scan(&[""]).offerable_long_flags());
    assert!(!flags.contains(&"--version".to_string()), "{flags:?}");
}

#[test]
fn unknown_value_types_are_not_validated() {
    let float = Arg::new("f")
        .long("f")
        .value_parser(clap::value_parser!(f64));
    let string = Arg::new("s").long("s");
    let int = Arg::new("i")
        .long("i")
        .value_parser(clap::value_parser!(i64));
    assert_eq!(value_kind(&float), Some(ValueKind::Float));
    assert_eq!(value_kind(&string), None);
    assert_eq!(value_kind(&int), None);
    let spec = |value| RootFlagSpec {
        takes_value: true,
        value,
        ..RootFlagSpec::default()
    };
    assert!(accepts_value(&spec(Some(ValueKind::Float)), "1.5", false));
    assert!(
        !accepts_value(&spec(None), "1.5", false),
        "unknown type dead-ends"
    );
    assert!(!accepts_value(&spec(None), "1.5", true));
}
