use super::*;

use std::collections::HashMap;

fn pids(mut edges: Vec<(u32, u32)>) -> Vec<u32> {
    edges.sort_unstable();
    edges.into_iter().map(|(pid, _)| pid).collect()
}

#[test]
fn descendants_finds_nested_children() {
    let table = [(10, 1), (11, 10), (12, 11), (13, 1)];
    let found = descendants(10, &table, |_, _| true);
    assert_eq!(pids(found.clone()), vec![11, 12]);
    assert!(found.contains(&(12, 11)), "{found:?}");
}

#[test]
fn descendants_ignores_unrelated() {
    assert!(descendants(13, &[(10, 1), (11, 10), (13, 1)], |_, _| true).is_empty());
}

#[test]
fn descendants_survives_cycles() {
    // pid 0 is its own parent on macOS; a stale snapshot can also hold a loop.
    let found = descendants(5, &[(0, 0), (6, 5), (7, 6), (6, 7), (5, 7)], |_, _| true);
    assert_eq!(pids(found), vec![6, 7]);
}

#[test]
fn stale_ppid_pointing_at_a_reused_pid_is_refused() {
    // 12 names 11 as its parent but predates it: its real parent died and 11 reused the pid.
    // 13 is 12's own child, so it goes too; 14 is a genuine grandchild.
    let created = HashMap::from([(10, 100), (11, 150), (12, 50), (13, 60), (14, 160)]);
    let table = [(11, 10), (12, 11), (13, 12), (14, 11)];
    let adopts = not_older_than_parent(|pid| created.get(&pid).copied());
    assert_eq!(pids(descendants(10, &table, adopts)), vec![11, 14]);
}

#[test]
fn unknown_creation_time_is_refused() {
    let adopts = not_older_than_parent(|pid| (pid == 10).then_some(100));
    assert!(descendants(10, &[(11, 10)], adopts).is_empty());
}

#[test]
fn parse_stat_ppid_skips_parens_in_the_command_name() {
    assert_eq!(parse_stat_ppid("42 (a) b) S 7 42 42 0"), Some(7));
    assert_eq!(parse_stat_ppid("42 (sleep) S 1 42 42 0"), Some(1));
    assert_eq!(parse_stat_ppid("42 (sleep) S"), None);
    assert_eq!(parse_stat_ppid("garbage"), None);
}

/// `OpenProcess` refuses pid 0, the System Idle Process, so its creation time is unknown.
#[cfg(windows)]
#[test]
fn creation_time_needs_an_openable_process() {
    assert!(creation_time(std::process::id()).is_some());
    assert_eq!(creation_time(0), None);
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn process_table_includes_self() {
    let me = std::process::id();
    let found = process_table()
        .iter()
        .any(|&(pid, ppid)| pid == me && ppid != 0);
    assert!(found, "{me} missing from the process table");
}
