use super::glob::glob_manifests;
use super::model::*;
use tempfile::TempDir;

fn setup_fake_venv(packages: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    std::fs::create_dir_all(&site).unwrap();
    for (pkg, contents) in packages {
        let pkg_dir = site.join(pkg);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("toolr-manifest.json"), contents).unwrap();
    }
    tmp
}

#[test]
fn fragment_round_trips_through_json() {
    let f = ManifestFragment {
        toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
        package: "my_pkg".into(),
        groups: vec![FragmentGroup {
            name: "deploy".into(),
            title: "Deploy".into(),
            description: String::new(),
        }],
        commands: vec![],
    };
    let json = serde_json::to_string_pretty(&f).unwrap();
    let back: ManifestFragment = serde_json::from_str(&json).unwrap();
    assert_eq!(f, back);
}

#[test]
fn glob_finds_only_toolr_manifest_files() {
    let tmp = setup_fake_venv(&[
        ("a_pkg", r#"{"toolr_schema_version": 1, "package": "a_pkg"}"#),
        ("b_pkg", r#"{"toolr_schema_version": 1, "package": "b_pkg"}"#),
    ]);
    // Drop a spurious file the glob must ignore.
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    std::fs::write(site.join("a_pkg").join("README"), "ignored").unwrap();

    let hits = glob_manifests(tmp.path()).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].ends_with("a_pkg/toolr-manifest.json"));
    assert!(hits[1].ends_with("b_pkg/toolr-manifest.json"));
}

#[test]
fn glob_returns_empty_when_no_site_packages() {
    let tmp = TempDir::new().unwrap();
    let hits = glob_manifests(tmp.path()).unwrap();
    assert!(hits.is_empty());
}

use super::parse::{ThirdPartyError, parse_fragment};

fn write_fragment(tmp: &TempDir, pkg: &str, contents: &str) -> std::path::PathBuf {
    let site = tmp
        .path()
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    let pkg_dir = site.join(pkg);
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let path = pkg_dir.join("toolr-manifest.json");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn parse_accepts_minimal_v1_fragment() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "my_pkg",
        r#"{
            "toolr_schema_version": 1,
            "package": "my_pkg",
            "groups": [],
            "commands": []
        }"#,
    );
    let frag = parse_fragment(&path).expect("should parse");
    assert_eq!(frag.toolr_schema_version, 1);
    assert_eq!(frag.package, "my_pkg");
}

#[test]
fn parse_rejects_missing_version_key() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "bad_pkg",
        r#"{"package": "bad_pkg", "groups": [], "commands": []}"#,
    );
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
}

#[test]
fn parse_rejects_unknown_future_version() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "future_pkg",
        r#"{"toolr_schema_version": 999, "package": "future_pkg"}"#,
    );
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(
        err,
        ThirdPartyError::UnknownVersion { version: 999, .. }
    ));
}

#[test]
fn parse_rejects_malformed_json() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(&tmp, "bad_pkg", "not valid json");
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(err, ThirdPartyError::Json { .. }));
}

use super::merge::merge_into_manifest;
use crate::manifest::{ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION};

fn empty_base() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        dynamic_hash: String::new(),
        groups: vec![],
        commands: vec![],
    }
}

fn sample_fragment(pkg: &str, group: &str, name: &str) -> ManifestFragment {
    ManifestFragment {
        toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
        package: pkg.into(),
        groups: vec![FragmentGroup {
            name: group.into(),
            title: group.to_uppercase(),
            description: String::new(),
        }],
        commands: vec![FragmentCommand {
            name: name.into(),
            group: group.into(),
            module: format!("{pkg}.commands"),
            function: name.replace('-', "_"),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
        }],
    }
}

#[test]
fn merge_adds_groups_and_commands_from_fragments() {
    let merged = merge_into_manifest(
        empty_base(),
        vec![sample_fragment("pkg_a", "deploy", "rollout")],
    )
    .unwrap();
    assert_eq!(merged.groups.len(), 1);
    assert_eq!(merged.groups[0].name, "deploy");
    assert_eq!(merged.commands.len(), 1);
    assert_eq!(merged.commands[0].name, "rollout");
    assert_eq!(merged.commands[0].origin, Origin::Static);
}

#[test]
fn merge_skips_third_party_command_when_local_already_defines_it() {
    let mut base = empty_base();
    base.groups.push(Group {
        name: "deploy".into(),
        title: "Deploy".into(),
        description: String::new(),
        origin: Origin::Static,
    });
    base.commands.push(Command {
        name: "rollout".into(),
        group: "deploy".into(),
        module: "tools.deploy".into(),
        function: "rollout".into(),
        summary: "local".into(),
        description: String::new(),
        arguments: vec![],
        imports: vec![],
        origin: Origin::Static,
    });
    let merged =
        merge_into_manifest(base, vec![sample_fragment("pkg_a", "deploy", "rollout")]).unwrap();
    assert_eq!(merged.commands.len(), 1);
    assert_eq!(merged.commands[0].summary, "local");
}

#[test]
fn merge_errors_on_third_party_to_third_party_collision() {
    let err = merge_into_manifest(
        empty_base(),
        vec![
            sample_fragment("pkg_a", "deploy", "rollout"),
            sample_fragment("pkg_b", "deploy", "rollout"),
        ],
    )
    .expect_err("should collide");
    let msg = err.to_string();
    assert!(msg.contains("pkg_a"), "got: {msg}");
    assert!(msg.contains("pkg_b"), "got: {msg}");
}

#[test]
fn argument_kind_propagates_through_merge() {
    let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
    frag.commands[0].arguments.push(FragmentArgument {
        name: "force".into(),
        kind: ArgumentKind::Flag,
        help: String::new(),
        default: None,
        type_annotation: None,
        allowed_values: vec![],
    });
    let merged = merge_into_manifest(empty_base(), vec![frag]).unwrap();
    assert_eq!(merged.commands[0].arguments.len(), 1);
    assert_eq!(merged.commands[0].arguments[0].kind, ArgumentKind::Flag);
}

use super::discover_and_merge;

#[test]
fn discover_and_merge_picks_up_all_valid_fragments() {
    let tmp = setup_fake_venv(&[
        (
            "pkg_a",
            r#"{
                "toolr_schema_version": 1,
                "package": "pkg_a",
                "groups": [{"name": "deploy", "title": "Deploy", "description": ""}],
                "commands": [{
                    "name": "rollout", "group": "deploy",
                    "module": "pkg_a.commands", "function": "rollout",
                    "summary": "", "description": "",
                    "arguments": [], "imports": []
                }]
            }"#,
        ),
        (
            "pkg_b",
            r#"{
                "toolr_schema_version": 1,
                "package": "pkg_b",
                "groups": [{"name": "lint", "title": "Lint", "description": ""}],
                "commands": [{
                    "name": "check", "group": "lint",
                    "module": "pkg_b.commands", "function": "check",
                    "summary": "", "description": "",
                    "arguments": [], "imports": []
                }]
            }"#,
        ),
    ]);
    let merged = discover_and_merge(tmp.path(), empty_base()).unwrap();
    let group_names: Vec<_> = merged.groups.iter().map(|g| g.name.clone()).collect();
    let command_names: Vec<_> = merged.commands.iter().map(|c| c.name.clone()).collect();
    assert!(group_names.contains(&"deploy".to_string()));
    assert!(group_names.contains(&"lint".to_string()));
    assert!(command_names.contains(&"rollout".to_string()));
    assert!(command_names.contains(&"check".to_string()));
}

#[test]
fn discover_and_merge_aborts_on_malformed_fragment() {
    let tmp = setup_fake_venv(&[
        ("pkg_ok", r#"{"toolr_schema_version": 1, "package": "pkg_ok"}"#),
        ("pkg_bad", "not valid json at all"),
    ]);
    let err = discover_and_merge(tmp.path(), empty_base()).expect_err("should abort");
    assert!(matches!(err, ThirdPartyError::Json { .. }));
}

#[test]
fn discover_and_merge_no_op_when_venv_has_no_fragments() {
    let tmp = TempDir::new().unwrap();
    // Create site-packages but no fragments.
    std::fs::create_dir_all(
        tmp.path()
            .join("lib")
            .join("python3.13")
            .join("site-packages"),
    )
    .unwrap();
    let merged = discover_and_merge(tmp.path(), empty_base()).unwrap();
    assert!(merged.groups.is_empty());
    assert!(merged.commands.is_empty());
}
