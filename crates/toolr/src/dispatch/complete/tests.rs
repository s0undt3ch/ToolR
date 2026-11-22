use super::*;

fn cand(value: &str) -> Candidate {
    Candidate::new(value.to_string(), String::new())
}

/// Scan `given` flags followed by the in-progress `rest`, then merge into `start`.
fn merged(given: &[&str], rest: &[&str], start: &[&str]) -> Vec<String> {
    merged_with(Matching::Prefix, given, rest, start)
}

fn merged_with(matching: Matching, given: &[&str], rest: &[&str], start: &[&str]) -> Vec<String> {
    let tokens: Vec<String> = given.iter().chain(rest).map(ToString::to_string).collect();
    let scan = crate::builtin_completions::scan_root_flags(&tokens);
    let mut candidates: Vec<Candidate> = start.iter().map(|v| cand(v)).collect();
    merge_root_flags(&scan, &mut candidates, matching);
    candidates.into_iter().map(|c| c.value).collect()
}

#[test]
fn matching_is_shell_only_for_the_exact_value() {
    assert_eq!(matching_from(Some("shell")), Matching::Shell);
    for value in [
        None,
        Some(""),
        Some("SHELL"),
        Some("prefix"),
        Some("banana"),
    ] {
        assert_eq!(matching_from(value), Matching::Prefix, "{value:?}");
    }
}

#[test]
fn merge_with_shell_matching_offers_every_root_flag_for_a_flag_word() {
    let all = [
        "--debug",
        "--help",
        "--no-output-timeout-secs",
        "--no-timestamps",
        "--quiet",
        "--timeout-secs",
        "--timestamps",
    ];
    assert_eq!(
        merged_with(Matching::Shell, &[], &["--ZZ"], &["--help"]),
        all
    );
    assert_eq!(
        merged_with(Matching::Prefix, &[], &["--ZZ"], &[]),
        Vec::<String>::new()
    );
    // Still only for a flag word at the root.
    assert_eq!(merged_with(Matching::Shell, &[], &["ZZ"], &["ci"]), ["ci"]);
}

#[test]
fn merge_offers_unseen_non_conflicting_flags() {
    assert_eq!(
        merged(&["--quiet"], &["--"], &["--help"]),
        [
            "--help",
            "--no-output-timeout-secs",
            "--no-timestamps",
            "--timeout-secs",
            "--timestamps"
        ]
    );
}

#[test]
fn merge_drops_help_once_given() {
    let got = merged(&["--help"], &["--"], &["--help"]);
    assert!(!got.contains(&"--help".to_string()), "{got:?}");
    assert!(got.contains(&"--quiet".to_string()), "{got:?}");
}

#[test]
fn merge_is_inert_after_a_committed_word() {
    assert_eq!(merged(&[], &["--", "--"], &["x"]), ["x"]);
    assert_eq!(merged(&["--quiet"], &["ci", "--"], &["--help"]), ["--help"]);
}

#[test]
fn merge_is_inert_for_non_flag_words_and_empty_rest() {
    assert_eq!(merged(&[], &["c"], &["ci"]), ["ci"]);
    assert_eq!(merged(&[], &[], &["ci"]), ["ci"]);
}
