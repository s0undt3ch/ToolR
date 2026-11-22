//! Unit tests for `find_command_for_path`, the pure helper that
//! resolves a parsed subcommand path to its manifest entry.
//! Grafted children of a dispatcher live under a 3-segment path
//! like `toolr jenkins job migrate`, but `migrate` is stored at
//! `group == "jenkins"` (the dispatcher hop is invisible to the
//! manifest). These tests pin the most-specific-first preference
//! and the one-level fallback.
use super::*;
use toolr_core::manifest::{Command, Manifest, Origin};

fn cmd(name: &str, group: &str) -> Command {
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

fn manifest_with(commands: Vec<Command>) -> Manifest {
    Manifest {
        commands,
        ..Manifest::empty()
    }
}

fn parts(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn finds_two_segment_path_in_group() {
    let m = manifest_with(vec![cmd("migrate", "jenkins")]);
    let c = find_command_for_path(&m, &parts(&["jenkins", "migrate"])).unwrap();
    assert_eq!(c.name, "migrate");
}

#[test]
fn finds_three_segment_path_under_dispatcher() {
    // `migrate` lives at group=jenkins (the group), name=migrate;
    // the user typed `toolr jenkins job migrate` (3 segments).
    let m = manifest_with(vec![cmd("migrate", "jenkins")]);
    let c = find_command_for_path(&m, &parts(&["jenkins", "job", "migrate"])).unwrap();
    assert_eq!(c.name, "migrate");
}

#[test]
fn prefers_more_specific_group_when_both_exist() {
    // If both `docker.image.build` and `docker.build` exist, the
    // 3-segment path `docker image build` must resolve to the
    // nested one, not the top-level one.
    let m = manifest_with(vec![cmd("build", "docker"), cmd("build", "docker.image")]);
    let c = find_command_for_path(&m, &parts(&["docker", "image", "build"])).unwrap();
    assert_eq!(c.group, "docker.image");
}

#[test]
fn returns_none_when_no_command_matches() {
    let m = manifest_with(vec![cmd("migrate", "jenkins")]);
    assert!(find_command_for_path(&m, &parts(&["unknown"])).is_none());
}
