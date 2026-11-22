use super::misplaced_root_flag_hint;
use crate::cli;
use toolr_core::manifest::Manifest;

fn hint(args: &[&str]) -> Option<String> {
    let argv: Vec<String> = std::iter::once("toolr")
        .chain(args.iter().copied())
        .map(String::from)
        .collect();
    let root = cli::build_command(&Manifest::empty());
    let err = root.clone().try_get_matches_from(&argv).err()?;
    misplaced_root_flag_hint(&err, &root, &crate::invocation::RootArgv::new(argv))
}

#[test]
fn a_misplaced_root_flag_gets_an_example_built_from_the_matched_command() {
    let ends = |args: &[&str], tail: &str| {
        let h = hint(args).unwrap_or_default();
        assert!(h.ends_with(tail), "{args:?}: {h}");
    };
    ends(
        &["project", "venv", "sync", "-q"],
        "`toolr -q project venv sync ...`",
    );
    ends(
        &["project", "venv", "sync", "--force", "--timeout-secs", "5"],
        "`toolr --timeout-secs 5 project venv sync ...`",
    );
    ends(
        &["project", "venv", "sync", "--timeout=5"],
        "`toolr --timeout=5 project venv sync ...`",
    );
    // A missing or unsafe value gets the placeholder.
    ends(
        &["project", "venv", "sync", "--timeout-secs"],
        "`toolr --timeout-secs <SECONDS> project venv sync ...`",
    );
    ends(
        &["project", "venv", "sync", "--timeout-secs", "a b"],
        "`toolr --timeout-secs <SECONDS> project venv sync ...`",
    );
    // A spaced argument elsewhere never reaches the example.
    ends(
        &["project", "venv", "add", "a b", "-q"],
        "`toolr -q project venv add ...`",
    );
    // A cluster names only its root letter.
    ends(
        &["project", "venv", "sync", "-qx"],
        "`toolr -q project venv sync ...`",
    );
    ends(
        &["project", "venv", "sync", "-dev"],
        "`toolr -d project venv sync ...`",
    );
    ends(
        &["project", "venv", "sync", "-q", "-d"],
        "`toolr -q project venv sync ...`",
    );
    // Root flags typed before the command stand as a literal ellipsis.
    ends(
        &["--timeout-secs", "5", "project", "venv", "sync", "-q"],
        "`toolr \u{2026} -q project venv sync ...`",
    );
    ends(
        &["-q", "project", "venv", "sync", "-d"],
        "`toolr \u{2026} -d project venv sync ...`",
    );
}

#[test]
fn other_errors_get_no_hint() {
    assert_eq!(hint(&["project", "venv", "sync", "--nope"]), None);
    assert_eq!(hint(&["project", "venv", "sync", "-x"]), None);
    assert_eq!(hint(&["project", "venv", "sync", "-h"]), None);
    assert_eq!(hint(&["project", "venv", "nope"]), None);
    assert_eq!(hint(&["-q", "project", "venv", "sync"]), None);
}
