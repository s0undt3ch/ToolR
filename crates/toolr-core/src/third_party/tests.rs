use std::path::PathBuf;

use super::glob::glob_manifests;
use super::model::*;
use crate::manifest::{
    ArgMetadata, Argument, ArgumentKind, Command, FRAGMENT_SHAPE_SCHEMA, Group,
    MIN_READABLE_FRAGMENT_SCHEMA, Manifest, Nargs, Origin, PluginWarning, PluginWarningKind,
    SCHEMA_VERSION,
};
use crate::parser::SupportedType;
use crate::parser::types::SupportedTypeKind;
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
        toolr_schema_version: SCHEMA_VERSION,
        package: "my_pkg".into(),
        groups: vec![Group {
            name: "deploy".into(),
            title: "Deploy".into(),
            description: String::new(),
            parent: None,
            origin: Origin::ThirdParty,
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

use super::parse::{ParsedFragment, ThirdPartyError, parse_fragment};

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

fn loaded(parsed: ParsedFragment) -> ManifestFragment {
    match parsed {
        ParsedFragment::Loaded(fragment, _) => fragment,
        ParsedFragment::Skipped(w) => panic!("expected Loaded, got Skipped: {}", w.message),
    }
}

fn skipped(parsed: ParsedFragment) -> PluginWarning {
    match parsed {
        ParsedFragment::Skipped(w) => w,
        ParsedFragment::Loaded(f, _) => panic!("expected Skipped, got Loaded: {}", f.package),
    }
}

#[test]
fn fragment_at_minimum_schema_loads() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "demo",
        &format!(
            r#"{{
                "toolr_schema_version": {MIN_READABLE_FRAGMENT_SCHEMA},
                "package": "demo",
                "groups": [{{"name": "ci", "title": "CI", "origin": "third_party"}}],
                "commands": [{{
                    "name": "lint", "group": "ci",
                    "module": "demo.ci", "function": "lint",
                    "arguments": [], "origin": "third_party"
                }}]
            }}"#
        ),
    );
    let parsed = parse_fragment(&path).expect("should parse");
    let ParsedFragment::Loaded(frag, loaded_from) = parsed else {
        panic!("expected Loaded, got {parsed:?}");
    };
    assert_eq!(loaded_from, path);
    assert_eq!(frag.toolr_schema_version, MIN_READABLE_FRAGMENT_SCHEMA);
    assert_eq!(frag.package, "demo");
    assert_eq!(frag.groups.len(), 1);
    assert_eq!(frag.commands.len(), 1);
}

#[test]
fn newer_fragment_is_skipped_with_upgrade_hint() {
    let tmp = TempDir::new().unwrap();
    let newer = SCHEMA_VERSION + 1;
    let path = write_fragment(
        &tmp,
        "demo",
        &format!(r#"{{"toolr_schema_version": {newer}, "package": "demo"}}"#),
    );
    let warning = skipped(parse_fragment(&path).expect("a newer fragment is not an error"));
    assert_eq!(warning.kind, PluginWarningKind::Skipped);
    assert_eq!(warning.package, "demo");
    assert_eq!(warning.path, path);
    assert_eq!(
        warning.message,
        format!(
            "skipping plugin demo: needs toolr schema {}, this toolr supports {}. Upgrade toolr.",
            SCHEMA_VERSION + 1,
            SCHEMA_VERSION
        )
    );
}

#[test]
fn v1_fragment_is_skipped_with_rebuild_hint() {
    // The shape a real v1 fragment has: no `origin`, so a typed deserialise would fail. The
    // version check must run on the raw value first.
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "demo",
        r#"{
            "toolr_schema_version": 1,
            "package": "demo",
            "groups": [{"name": "ci", "title": "CI"}],
            "commands": [{"name": "lint", "group": "ci", "module": "demo.ci",
                          "function": "lint", "arguments": []}]
        }"#,
    );
    let warning = skipped(parse_fragment(&path).expect("a v1 fragment is not an error"));
    assert_eq!(warning.kind, PluginWarningKind::Skipped);
    assert_eq!(
        warning.message,
        "skipping plugin demo: built with toolr schema 1, this toolr needs >= 2. \
         Rebuild the plugin with toolr >= 0.34.0."
    );
}

#[test]
fn skipped_fragment_without_package_key_is_named_after_its_directory() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(&tmp, "nameless", r#"{"toolr_schema_version": 1}"#);
    let warning = skipped(parse_fragment(&path).unwrap());
    assert_eq!(warning.package, "nameless");
    assert!(
        warning.message.starts_with("skipping plugin nameless: "),
        "got: {}",
        warning.message
    );
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
fn parse_rejects_zero_or_below_version_as_missing() {
    // `toolr_schema_version` < 1 is treated as a malformed fragment: the
    // `>= 1` filter in `parse_fragment` rejects it as MissingVersion (schema
    // numbering starts at 1, so 0 never existed in the wild). Covers the
    // filter-reject path distinctly from the absent-key case above.
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "zero_pkg",
        r#"{"toolr_schema_version": 0, "package": "zero_pkg", "groups": [], "commands": []}"#,
    );
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
}

#[test]
fn parse_accepts_exactly_current_version() {
    // A fragment declaring the current SCHEMA_VERSION parses unchanged.
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "cur_pkg",
        &format!(
            r#"{{
                "toolr_schema_version": {SCHEMA_VERSION},
                "package": "cur_pkg",
                "groups": [],
                "commands": []
            }}"#
        ),
    );
    let frag = loaded(parse_fragment(&path).expect("current version should parse"));
    assert_eq!(frag.toolr_schema_version, SCHEMA_VERSION);
    assert_eq!(frag.package, "cur_pkg");
}

#[test]
fn parse_rejects_malformed_json() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(&tmp, "bad_pkg", "not valid json");
    let err = parse_fragment(&path).expect_err("should reject");
    assert!(matches!(err, ThirdPartyError::Json { .. }));
}

#[test]
fn parse_rejects_missing_file_with_io_error() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("no-such-file.json");
    let err = parse_fragment(&missing).expect_err("missing file should error");
    match err {
        ThirdPartyError::Io { path, source } => {
            assert_eq!(path, missing);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io error, got {other:?}"),
    }
}

#[test]
fn parse_treats_zero_version_as_missing() {
    // `toolr_schema_version: 0` fails the `>= 1` filter and falls through
    // to MissingVersion — same as if the key was absent. Pinning this
    // behaviour catches accidental drift in the version-extraction chain.
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "zero_pkg",
        r#"{"toolr_schema_version": 0, "package": "zero_pkg"}"#,
    );
    let err = parse_fragment(&path).expect_err("zero version should reject");
    assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
}

#[test]
fn parse_treats_non_integer_version_as_missing() {
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(
        &tmp,
        "str_pkg",
        r#"{"toolr_schema_version": "one", "package": "str_pkg"}"#,
    );
    let err = parse_fragment(&path).expect_err("string version should reject");
    assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
}

#[test]
fn parse_treats_non_object_root_as_missing_version() {
    // A top-level JSON array has no `toolr_schema_version` key at all.
    let tmp = TempDir::new().unwrap();
    let path = write_fragment(&tmp, "arr_pkg", "[]");
    let err = parse_fragment(&path).expect_err("array root should reject");
    assert!(matches!(err, ThirdPartyError::MissingVersion { .. }));
}

#[test]
fn third_party_error_io_renders_path_and_reason() {
    // `Display`-trait coverage for the `Io` arm — surfaces the path
    // and the underlying io::Error.
    let err = ThirdPartyError::Io {
        path: std::path::PathBuf::from("/tmp/x.json"),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "boom"),
    };
    let s = err.to_string();
    assert!(s.contains("/tmp/x.json"));
    assert!(s.contains("boom"));
}

use super::merge::merge_into_manifest;

fn empty_base() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
        groups: vec![],
        commands: vec![],
        plugin_warnings: Vec::new(),
    }
}

/// Pairs each fragment with the file it would have been read from.
fn from_files(fragments: Vec<ManifestFragment>) -> Vec<(ManifestFragment, PathBuf)> {
    fragments
        .into_iter()
        .map(|f| {
            let path = PathBuf::from(format!("site-packages/{}/toolr-manifest.json", f.package));
            (f, path)
        })
        .collect()
}

/// A fragment group for a dotted `full_path` such as `docker.image`.
fn fragment_group(full_path: &str) -> Group {
    let (parent, name) = match full_path.rsplit_once('.') {
        Some((parent, name)) => (Some(parent.to_string()), name),
        None => (None, full_path),
    };
    Group {
        name: name.into(),
        title: name.to_uppercase(),
        description: String::new(),
        parent,
        origin: Origin::ThirdParty,
    }
}

fn plain_argument(name: &str, kind: ArgumentKind) -> Argument {
    Argument {
        name: name.into(),
        kind,
        help: String::new(),
        default: None,
        type_annotation: None,
        resolved_type: None,
        allowed_values: vec![],
        metadata: ArgMetadata::default(),
        long_flag: None,
    }
}

fn sample_fragment(pkg: &str, group: &str, name: &str) -> ManifestFragment {
    ManifestFragment {
        toolr_schema_version: SCHEMA_VERSION,
        package: pkg.into(),
        groups: vec![fragment_group(group)],
        commands: vec![Command {
            name: name.into(),
            group: group.into(),
            module: format!("{pkg}.commands"),
            function: name.replace('-', "_"),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            origin: Origin::ThirdParty,
            dispatched_from: None,
            is_dispatcher: false,
        }],
    }
}

/// A fragment for `pkg` with two commands in one group.
fn two_command_fragment(pkg: &str, group: &str, first: &str, second: &str) -> ManifestFragment {
    let mut fragment = sample_fragment(pkg, group, first);
    fragment
        .commands
        .extend(sample_fragment(pkg, group, second).commands);
    fragment
}

const RESOLVE_TAIL: &str =
    "Choosing a winner in config is tracked in https://github.com/s0undt3ch/ToolR/issues/522";

fn command_names(manifest: &Manifest) -> Vec<(&str, &str)> {
    manifest
        .commands
        .iter()
        .map(|c| (c.group.as_str(), c.name.as_str()))
        .collect()
}

#[test]
fn merge_adds_groups_and_commands_from_fragments() {
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![sample_fragment("pkg_a", "deploy", "rollout")]),
    );
    assert_eq!(merged.groups.len(), 1);
    assert_eq!(merged.groups[0].name, "deploy");
    assert_eq!(merged.commands.len(), 1);
    assert_eq!(merged.commands[0].name, "rollout");
    assert_eq!(merged.groups[0].origin, Origin::ThirdParty);
    assert_eq!(merged.commands[0].origin, Origin::ThirdParty);
}

#[test]
fn merge_skips_third_party_command_when_local_already_defines_it() {
    let mut base = empty_base();
    base.groups.push(Group {
        name: "deploy".into(),
        title: "Deploy".into(),
        description: String::new(),
        parent: None,
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
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    });
    let merged = merge_into_manifest(
        base,
        from_files(vec![sample_fragment("pkg_a", "deploy", "rollout")]),
    );
    assert_eq!(merged.commands.len(), 1);
    assert_eq!(merged.commands[0].summary, "local");
    let kinds: Vec<_> = merged.plugin_warnings.iter().map(|w| w.kind).collect();
    assert_eq!(kinds, [PluginWarningKind::Shadowed]);
}

#[test]
fn merge_disables_a_command_two_plugins_define() {
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![
            two_command_fragment("pkg_a", "deploy", "rollout", "status"),
            sample_fragment("pkg_b", "deploy", "rollout"),
        ]),
    );
    assert_eq!(command_names(&merged), [("deploy", "status")]);
    assert_eq!(merged.plugin_warnings.len(), 1);
    let warning = &merged.plugin_warnings[0];
    assert_eq!(warning.kind, PluginWarningKind::Conflict);
    assert_eq!(warning.package, "pkg_a");
    assert_eq!(
        warning.path,
        PathBuf::from("site-packages/pkg_a/toolr-manifest.json")
    );
    assert_eq!(
        warning.message,
        format!(
            "deploy rollout is defined by more than one plugin (pkg_a, pkg_b), so it is disabled. \
             Uninstall all but one. {RESOLVE_TAIL}"
        )
    );
}

#[test]
fn a_conflict_names_every_plugin_in_discovery_order() {
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![
            sample_fragment("pkg_a", "deploy", "rollout"),
            sample_fragment("pkg_b", "deploy", "rollout"),
            sample_fragment("pkg_c", "deploy", "rollout"),
        ]),
    );
    assert!(merged.commands.is_empty());
    let messages: Vec<_> = merged
        .plugin_warnings
        .iter()
        .map(|w| w.message.as_str())
        .collect();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].starts_with(
            "deploy rollout is defined by more than one plugin (pkg_a, pkg_b, pkg_c), so it is disabled."
        ),
        "{messages:?}"
    );
}

#[test]
fn conflict_message_spells_nested_and_top_level_paths() {
    let mut top_a = sample_fragment("pkg_a", "", "hello");
    top_a.groups.clear();
    let mut top_b = sample_fragment("pkg_b", "", "hello");
    top_b.groups.clear();
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![
            sample_fragment("pkg_a", "docker.image", "build"),
            top_a,
            sample_fragment("pkg_b", "docker.image", "build"),
            top_b,
        ]),
    );
    let starts: Vec<_> = merged
        .plugin_warnings
        .iter()
        .map(|w| w.message.split(" is defined").next().unwrap())
        .collect();
    assert_eq!(starts, ["docker image build", "hello"]);
}

#[test]
fn local_command_beats_two_clashing_plugins() {
    let mut base = empty_base();
    base.groups.push(static_group("ci", None));
    base.commands.push(local_command("ci", "lint", "tools.ci"));
    let merged = merge_into_manifest(
        base,
        from_files(vec![
            sample_fragment("pkg_a", "ci", "lint"),
            sample_fragment("pkg_b", "ci", "lint"),
        ]),
    );
    assert_eq!(command_names(&merged), [("ci", "lint")]);
    assert_eq!(merged.commands[0].origin, Origin::Static);
    let kinds: Vec<_> = merged.plugin_warnings.iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        [PluginWarningKind::Shadowed, PluginWarningKind::Shadowed]
    );
}

#[test]
fn a_fragment_listing_one_command_twice_merges_it_once() {
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![two_command_fragment(
            "pkg_a", "deploy", "rollout", "rollout",
        )]),
    );
    assert_eq!(command_names(&merged), [("deploy", "rollout")]);
    assert!(
        merged.plugin_warnings.is_empty(),
        "{:?}",
        merged.plugin_warnings
    );
}

#[test]
fn a_group_emptied_by_a_conflict_is_kept() {
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![
            sample_fragment("pkg_a", "deploy", "rollout"),
            sample_fragment("pkg_b", "deploy", "rollout"),
        ]),
    );
    let paths: Vec<String> = merged.groups.iter().map(Group::full_path).collect();
    assert_eq!(paths, ["deploy"]);
    assert!(merged.commands.is_empty());
}

#[test]
fn warnings_run_skipped_then_shadowed_then_conflict() {
    let mut base = empty_base();
    base.groups.push(static_group("ci", None));
    base.commands.push(local_command("ci", "lint", "tools.ci"));
    let rollout = r#"[{"name": "rollout", "group": "deploy", "module": "m", "function": "f",
                       "arguments": [], "origin": "third_party"}]"#;
    let deploy = r#"[{"name": "deploy", "title": "Deploy", "origin": "third_party"}]"#;
    // Glob order is a_dup, b_dup, c_shadow, z_old. The conflict is seen first and the
    // skipped plugin last, so the asserted order is not just glob order.
    let a_dup = v2_fragment_json("a_dup", deploy, rollout);
    let b_dup = v2_fragment_json("b_dup", deploy, rollout);
    let c_shadow = v2_fragment_json(
        "c_shadow",
        "[]",
        r#"[{"name": "lint", "group": "ci", "module": "c_shadow.ci", "function": "lint",
             "arguments": [], "origin": "third_party"}]"#,
    );
    let z_old = r#"{"toolr_schema_version": 1, "package": "z_old"}"#;
    let tmp = setup_fake_venv(&[
        ("a_dup", &a_dup),
        ("b_dup", &b_dup),
        ("c_shadow", &c_shadow),
        ("z_old", z_old),
    ]);
    let merged = discover_and_merge(tmp.path(), base).unwrap();
    let kinds: Vec<_> = merged.plugin_warnings.iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        [
            PluginWarningKind::Skipped,
            PluginWarningKind::Shadowed,
            PluginWarningKind::Conflict,
        ]
    );
}

#[test]
fn repeated_tuple_argument_round_trips_through_merge() {
    let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
    let mut pair = plain_argument("pair", ArgumentKind::Repeated);
    pair.type_annotation = Some("tuple[str, int] | None".into());
    pair.resolved_type = Some(SupportedType::Optional(Box::new(SupportedType::Tuple(
        vec![SupportedType::Str, SupportedType::Int],
    ))));
    pair.metadata.metavar = Some("PAIR".into());
    frag.commands[0].arguments.push(pair.clone());

    let json = serde_json::to_string(&frag).unwrap();
    let back: ManifestFragment = serde_json::from_str(&json).unwrap();
    let merged = merge_into_manifest(empty_base(), from_files(vec![back]));
    assert_eq!(merged.commands[0].arguments, [pair]);
}

#[test]
fn merge_forces_third_party_origin_and_clears_dispatch_flags() {
    let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
    frag.groups[0].origin = Origin::Static;
    frag.commands[0].origin = Origin::Static;
    frag.commands[0].dispatched_from = Some("argparse:django".into());
    frag.commands[0].is_dispatcher = true;
    let merged = merge_into_manifest(empty_base(), from_files(vec![frag]));
    assert_eq!(merged.groups[0].origin, Origin::ThirdParty);
    let cmd = &merged.commands[0];
    assert_eq!(cmd.origin, Origin::ThirdParty);
    assert_eq!(cmd.dispatched_from, None);
    assert!(!cmd.is_dispatcher);
}

#[test]
fn argument_kind_propagates_through_merge() {
    let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
    frag.commands[0]
        .arguments
        .push(plain_argument("force", ArgumentKind::Flag));
    let merged = merge_into_manifest(empty_base(), from_files(vec![frag]));
    assert_eq!(merged.commands[0].arguments.len(), 1);
    assert_eq!(merged.commands[0].arguments[0].kind, ArgumentKind::Flag);
}

#[test]
fn fragment_min_schema_is_the_shape_floor_for_current_features() {
    let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
    let mut emails = plain_argument("emails", ArgumentKind::FixedArity);
    emails.resolved_type = Some(SupportedType::List(Box::new(SupportedType::Email)));
    emails.metadata.nargs = Some(Nargs::Fixed(2));
    frag.commands[0].arguments.push(emails);
    assert_eq!(frag.min_schema(), FRAGMENT_SHAPE_SCHEMA);
}

#[test]
fn fragment_min_schema_takes_the_newest_type_kind_used() {
    let mut frag = sample_fragment("pkg_a", "deploy", "rollout");
    let mut to = plain_argument("to", ArgumentKind::Optional);
    to.resolved_type = Some(SupportedType::Optional(Box::new(SupportedType::Email)));
    frag.commands[0].arguments.push(to);
    let since = |k: SupportedTypeKind| if k == SupportedTypeKind::Email { 7 } else { 2 };
    assert_eq!(frag.min_schema_with(&since), 7);

    let empty = ManifestFragment {
        commands: vec![],
        ..frag
    };
    assert_eq!(empty.min_schema_with(&since), FRAGMENT_SHAPE_SCHEMA);
}

use super::discover_and_merge;

#[test]
fn discover_and_merge_picks_up_all_valid_fragments() {
    let tmp = setup_fake_venv(&[
        (
            "pkg_a",
            r#"{
                "toolr_schema_version": 2,
                "package": "pkg_a",
                "groups": [{"name": "deploy", "title": "Deploy", "description": "", "origin": "third_party"}],
                "commands": [{
                    "name": "rollout", "group": "deploy",
                    "module": "pkg_a.commands", "function": "rollout",
                    "summary": "", "description": "",
                    "arguments": [], "origin": "third_party"
                }]
            }"#,
        ),
        (
            "pkg_b",
            r#"{
                "toolr_schema_version": 2,
                "package": "pkg_b",
                "groups": [{"name": "lint", "title": "Lint", "description": "", "origin": "third_party"}],
                "commands": [{
                    "name": "check", "group": "lint",
                    "module": "pkg_b.commands", "function": "check",
                    "summary": "", "description": "",
                    "arguments": [], "origin": "third_party"
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
        (
            "pkg_ok",
            r#"{"toolr_schema_version": 2, "package": "pkg_ok"}"#,
        ),
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

fn static_group(name: &str, parent: Option<&str>) -> Group {
    Group {
        name: name.into(),
        title: name.into(),
        description: String::new(),
        parent: parent.map(String::from),
        origin: Origin::Static,
    }
}

#[test]
fn merge_keeps_same_leaf_groups_under_different_parents() {
    let mut base = empty_base();
    base.groups.push(static_group("image", Some("ci")));
    let merged = merge_into_manifest(
        base,
        from_files(vec![sample_fragment("pkg_a", "docker.image", "build")]),
    );
    let paths: Vec<String> = merged.groups.iter().map(Group::full_path).collect();
    assert_eq!(paths, ["ci.image", "docker.image"]);
}

#[test]
fn merge_dedups_fragment_group_against_nested_base_group_by_full_path() {
    let mut base = empty_base();
    base.groups.push(static_group("image", Some("docker")));
    let merged = merge_into_manifest(
        base,
        from_files(vec![sample_fragment("pkg_a", "docker.image", "build")]),
    );
    assert_eq!(merged.groups.len(), 1);
}

#[test]
fn merge_keeps_the_host_title_when_a_fragment_group_collides() {
    let mut base = empty_base();
    let mut host = static_group("image", Some("docker"));
    host.title = "Host title".into();
    base.groups.push(host);
    let merged = merge_into_manifest(
        base,
        from_files(vec![sample_fragment("pkg_a", "docker.image", "build")]),
    );
    assert_eq!(merged.groups.len(), 1);
    assert_eq!(merged.groups[0].title, "Host title");
    assert_eq!(merged.groups[0].origin, Origin::Static);
}

/// A v2 fragment for `pkg` whose `commands` JSON is spliced in verbatim.
fn v2_fragment_json(pkg: &str, groups: &str, commands: &str) -> String {
    format!(
        r#"{{"toolr_schema_version": 2, "package": "{pkg}", "groups": {groups}, "commands": {commands}}}"#
    )
}

#[test]
fn bad_command_skips_the_whole_plugin() {
    let fragment = v2_fragment_json(
        "demo",
        r#"[{"name": "ci", "title": "CI", "origin": "third_party"}]"#,
        r#"[
            {"name": "lint", "group": "ci", "module": "demo.ci", "function": "lint",
             "arguments": [], "origin": "third_party"},
            {"name": "pair", "group": "ci", "module": "demo.ci", "function": "pair",
             "arguments": [{"name": "values", "kind": "fixed_arity"}],
             "origin": "third_party"}
        ]"#,
    );
    let tmp = setup_fake_venv(&[("demo", &fragment)]);
    let merged = discover_and_merge(tmp.path(), empty_base()).unwrap();
    assert!(merged.groups.is_empty(), "groups: {:?}", merged.groups);
    assert!(
        merged.commands.is_empty(),
        "commands: {:?}",
        merged.commands
    );
    assert_eq!(merged.plugin_warnings.len(), 1);
    let warning = &merged.plugin_warnings[0];
    assert_eq!(warning.kind, PluginWarningKind::Skipped);
    assert_eq!(warning.package, "demo");
    assert!(
        warning.message.starts_with("skipping plugin demo: ")
            && warning.message.contains("fixed_arity"),
        "got: {}",
        warning.message
    );
}

fn local_command(group: &str, name: &str, module: &str) -> Command {
    Command {
        name: name.into(),
        group: group.into(),
        module: module.into(),
        function: name.replace('-', "_"),
        summary: "local".into(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

#[test]
fn local_command_shadows_plugin_with_warning() {
    let mut base = empty_base();
    base.groups.push(static_group("ci", None));
    base.commands.push(local_command("ci", "lint", "tools.ci"));
    let fragment = v2_fragment_json(
        "demo",
        r#"[{"name": "ci", "title": "CI", "origin": "third_party"}]"#,
        r#"[{"name": "lint", "group": "ci", "module": "demo.ci", "function": "lint",
             "arguments": [], "origin": "third_party"}]"#,
    );
    let tmp = setup_fake_venv(&[("demo", &fragment)]);
    let merged = discover_and_merge(tmp.path(), base).unwrap();
    assert_eq!(merged.commands.len(), 1);
    assert_eq!(merged.commands[0].summary, "local");
    assert_eq!(merged.commands[0].origin, Origin::Static);
    assert_eq!(merged.plugin_warnings.len(), 1);
    let warning = &merged.plugin_warnings[0];
    assert_eq!(warning.kind, PluginWarningKind::Shadowed);
    assert_eq!(warning.package, "demo");
    assert!(
        warning.path.ends_with("demo/toolr-manifest.json"),
        "{:?}",
        warning.path
    );
    assert_eq!(
        warning.message,
        format!("tools/ci.py defines ci lint, hiding the one from demo. {RESOLVE_TAIL}")
    );
}

#[test]
fn shadow_message_spells_a_nested_group_with_spaces() {
    let mut base = empty_base();
    base.groups.push(static_group("docker", None));
    base.groups.push(static_group("image", Some("docker")));
    base.commands
        .push(local_command("docker.image", "build", "tools.docker.image"));
    let merged = merge_into_manifest(
        base,
        from_files(vec![sample_fragment("demo", "docker.image", "build")]),
    );
    let messages: Vec<_> = merged
        .plugin_warnings
        .iter()
        .map(|w| w.message.as_str())
        .collect();
    let expected = format!(
        "tools/docker/image.py defines docker image build, hiding the one from demo. {RESOLVE_TAIL}"
    );
    assert_eq!(messages, [expected.as_str()]);
}

#[test]
fn good_plugin_loads_next_to_a_skipped_one() {
    let good = v2_fragment_json(
        "pkg_good",
        r#"[{"name": "deploy", "title": "Deploy", "origin": "third_party"}]"#,
        r#"[{"name": "rollout", "group": "deploy", "module": "pkg_good.deploy",
             "function": "rollout", "arguments": [], "origin": "third_party"}]"#,
    );
    let old = r#"{
        "toolr_schema_version": 1,
        "package": "pkg_old",
        "groups": [{"name": "lint", "title": "Lint"}],
        "commands": [{"name": "check", "group": "lint", "module": "pkg_old.lint",
                      "function": "check", "arguments": []}]
    }"#;
    let tmp = setup_fake_venv(&[("pkg_good", &good), ("pkg_old", old)]);
    let merged = discover_and_merge(tmp.path(), empty_base()).unwrap();
    let paths: Vec<String> = merged.groups.iter().map(Group::full_path).collect();
    assert_eq!(paths, ["deploy"]);
    let commands: Vec<_> = merged.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(commands, ["rollout"]);
    assert_eq!(merged.plugin_warnings.len(), 1);
    assert_eq!(merged.plugin_warnings[0].kind, PluginWarningKind::Skipped);
    assert_eq!(merged.plugin_warnings[0].package, "pkg_old");
}

#[test]
fn skipped_warnings_come_before_shadowed_ones() {
    let mut base = empty_base();
    base.groups.push(static_group("ci", None));
    base.commands.push(local_command("ci", "lint", "tools.ci"));
    // `a_shadow` globs before `z_old`, so the order below is not just glob order.
    let shadow = v2_fragment_json(
        "a_shadow",
        "[]",
        r#"[{"name": "lint", "group": "ci", "module": "a_shadow.ci", "function": "lint",
             "arguments": [], "origin": "third_party"}]"#,
    );
    let old = r#"{"toolr_schema_version": 1, "package": "z_old"}"#;
    let tmp = setup_fake_venv(&[("a_shadow", &shadow), ("z_old", old)]);
    let merged = discover_and_merge(tmp.path(), base).unwrap();
    let kinds: Vec<_> = merged.plugin_warnings.iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        [PluginWarningKind::Skipped, PluginWarningKind::Shadowed]
    );
}

#[test]
fn plugin_warnings_round_trip_and_stay_off_an_empty_manifest() {
    let mut manifest = empty_base();
    let bare = serde_json::to_string(&manifest).unwrap();
    assert!(!bare.contains("plugin_warnings"), "got: {bare}");
    manifest.plugin_warnings.push(PluginWarning {
        package: "demo".into(),
        path: PathBuf::from("site-packages/demo/toolr-manifest.json"),
        kind: PluginWarningKind::Shadowed,
        message: "tools/ci.py defines ci lint, hiding the one from demo".into(),
    });
    let json = serde_json::to_string(&manifest).unwrap();
    assert!(json.contains(r#""kind":"shadowed""#), "got: {json}");
    let back: Manifest = serde_json::from_str(&json).unwrap();
    assert_eq!(back, manifest);
}

#[test]
fn conflict_plugin_warning_serialises_as_conflict_and_round_trips() {
    let warning = PluginWarning {
        package: "toolr_a".into(),
        path: PathBuf::from("site-packages/toolr_a/toolr-manifest.json"),
        kind: PluginWarningKind::Conflict,
        message: "deploy rollout is defined by more than one plugin (toolr_a, toolr_b)".into(),
    };
    let json = serde_json::to_string(&warning).unwrap();
    assert!(json.contains(r#""kind":"conflict""#), "got: {json}");
    let back: PluginWarning = serde_json::from_str(&json).unwrap();
    assert_eq!(back, warning);
}
