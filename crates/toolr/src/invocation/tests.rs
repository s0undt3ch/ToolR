use super::*;

fn argv(args: &[&str]) -> RootArgv {
    RootArgv::new(
        std::iter::once("toolr")
            .chain(args.iter().copied())
            .map(String::from)
            .collect(),
    )
}

#[test]
fn no_root_option_that_takes_a_value_has_a_short_flag() {
    for arg in crate::cli::root_command().get_arguments() {
        assert!(
            !(arg.get_action().takes_values() && arg.get_short().is_some()),
            "`{}` takes a value and has a short flag; RootArgv::new, `cli::root_flag_spelling` and \
             the completion scan read whole words and must learn short clusters (`-qt 5`) and \
             attached values (`-t5`) first",
            arg.get_id()
        );
    }
}

#[test]
fn nothing_after_the_separator_is_classified() {
    for args in [&["--", "self"][..], &["--", "-V"], &["--", "__complete"]] {
        let a = argv(args);
        assert!(!a.skips_manifest_build(), "{args:?}");
        assert!(!a.is_tab_completion() && !a.wants_version(), "{args:?}");
    }
    assert_eq!(argv(&["-q", "--", "self"]).first_word(), Some("--"));
}

#[test]
fn root_option_values_are_not_the_command() {
    for args in [
        &["--timeout-secs", "5", "self", "--help"][..],
        &["--timeout", "5", "--nots", "7", "self"][..],
        &["--no-output-timeout-secs", "5", "-d", "self"][..],
        &["--timeout-secs=5", "self"][..],
    ] {
        let a = argv(args);
        assert_eq!(a.first_word(), Some("self"), "{args:?}");
        assert!(
            a.skips_manifest_build() && a.has_leading_root_flags(),
            "{args:?}"
        );
    }
    assert_eq!(argv(&["--timeout-secs"]).first_word(), None);
    assert!(!argv(&["self"]).has_leading_root_flags());
}

#[test]
fn every_built_in_skips_the_build_and_names_no_user_command() {
    for name in ["project", "self", "__complete", "__install-uv-now"] {
        let a = argv(&[name, "x"]);
        assert!(
            a.skips_manifest_build() && !a.names_user_command(),
            "{name}"
        );
    }
}

#[test]
fn help_bare_toolr_and_user_commands_build_the_manifest() {
    for args in [
        &["--help"][..],
        &["-h"],
        &[],
        &["init", "hello"],
        &["--debug", "django", "migrate"],
        &["rel", "bump", "--version", "1.2"],
        &["rel", "-qV"],
    ] {
        assert!(!argv(args).skips_manifest_build(), "{args:?}");
    }
    assert!(argv(&["init", "hello"]).names_user_command());
    assert!(!argv(&["-q"]).names_user_command());
}

#[test]
fn version_counts_only_among_the_root_flags() {
    for args in [
        &["--version"][..],
        &["-V"],
        &["-q", "-V"],
        &["-qV"],
        &["-V", "ci"],
    ] {
        assert!(argv(args).skips_manifest_build(), "{args:?}");
    }
}

#[test]
fn tab_completion_is_the_first_command_word() {
    let a = argv(&["-q", "__complete", "/tmp", "--", "ci", ""]);
    assert!(a.is_tab_completion());
    assert_eq!(a.complete_tokens(), ["--", "ci", ""]);
    assert!(!argv(&["project", "__complete"]).is_tab_completion());
    assert!(argv(&["__complete"]).complete_tokens().is_empty());
}

#[test]
fn long_help_wins_and_nothing_after_the_separator_counts() {
    assert_eq!(
        argv(&["ci", "-h", "--help"]).help_mode(),
        Some(HelpMode::Long)
    );
    assert_eq!(argv(&["ci", "-h"]).help_mode(), Some(HelpMode::Short));
    assert_eq!(argv(&["run", "--", "pytest", "--help"]).help_mode(), None);
    assert_eq!(argv(&["ci"]).help_mode(), None);
}

#[test]
fn root_flag_clusters_and_the_separator_classify_the_gate() {
    let a = argv(&["-qv", "greet"]);
    assert!(a.is_quiet() && a.names_user_command() && a.help_mode().is_none());
    let a = argv(&["-qh"]);
    assert!(a.is_quiet() && a.is_bare());
    assert_eq!(a.help_mode(), Some(HelpMode::Short));
    assert_eq!(argv(&["-qh", "greet"]).help_mode(), Some(HelpMode::Short));
    let a = argv(&["greet", "--", "--help"]);
    assert!(a.names_user_command() && a.help_mode().is_none());
    // A user command's own short cluster is not toolr's help.
    assert_eq!(argv(&["greet", "-xh"]).help_mode(), None);
    // `--` is kept as the command word, so the gate still asks for trust.
    let a = argv(&["--", "greet"]);
    assert!(a.names_user_command() && !a.is_bare() && a.help_mode().is_none());
    assert!(!argv(&["--quiet"]).names_user_command());
}

#[test]
fn command_path_walks_from_the_first_command_word() {
    let root = crate::cli::build_command(&toolr_core::manifest::Manifest::empty());
    let a = argv(&["-q", "project", "venv", "sync", "--force"]);
    let (cmd, names) = a.command_path(&root);
    assert_eq!(names, ["project", "venv", "sync"]);
    assert_eq!(cmd.get_name(), "sync");
    let a = argv(&["--timeout-secs", "5"]);
    let (cmd, names) = a.command_path(&root);
    assert!(names.is_empty());
    assert_eq!(cmd.get_name(), "toolr");
}

#[test]
fn the_root_is_looked_up_once_and_handed_over_owned() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("tools")).unwrap();
    std::fs::write(tmp.path().join("tools/pyproject.toml"), "").unwrap();
    let run = Invocation::new(argv(&[]), tmp.path().to_path_buf());
    let found = run.root().unwrap().to_path_buf();
    std::fs::remove_dir_all(tmp.path().join("tools")).unwrap();
    // The cached answer stands even though the project is gone now.
    assert_eq!(run.root().unwrap(), found);
    let (cwd, root) = run.into_cwd_and_root();
    assert_eq!((cwd.as_path(), root.unwrap()), (tmp.path(), found));

    let none = tempfile::TempDir::new().unwrap();
    let run = Invocation::new(argv(&[]), none.path().to_path_buf());
    assert!(run.into_cwd_and_root().1.is_err());
}
