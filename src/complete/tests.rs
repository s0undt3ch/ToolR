use crate::complete::serve_completions;
use crate::manifest::{
    Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION,
};

fn fixture() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: "h".into(),
        dynamic_hash: String::new(),
        groups: vec![
            Group {
                name: "ci".into(),
                title: "CI utilities".into(),
                description: String::new(),
                origin: Origin::Static,
            },
            Group {
                name: "data".into(),
                title: "Data utilities".into(),
                description: String::new(),
                origin: Origin::Static,
            },
        ],
        commands: vec![
            Command {
                name: "hello".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "hello".into(),
                summary: "Say hello.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "name".into(),
                    kind: ArgumentKind::Optional,
                    help: "Who to greet".into(),
                    default: Some("\"world\"".into()),
                    type_annotation: Some("str".into()),
                    allowed_values: vec![],
                }],
                imports: vec![],
                origin: Origin::Static,
            },
            Command {
                name: "deploy".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "deploy".into(),
                summary: "Deploy something.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "env".into(),
                    kind: ArgumentKind::Optional,
                    help: "Target env".into(),
                    default: None,
                    type_annotation: Some("Literal".into()),
                    allowed_values: vec!["staging".into(), "production".into()],
                }],
                imports: vec![],
                origin: Origin::Static,
            },
            Command {
                name: "load".into(),
                group: "data".into(),
                module: "tools.data".into(),
                function: "load".into(),
                summary: "Load data.".into(),
                description: String::new(),
                arguments: vec![Argument {
                    name: "shape".into(),
                    kind: ArgumentKind::Positional,
                    help: "Shape".into(),
                    default: None,
                    type_annotation: Some("Literal".into()),
                    allowed_values: vec!["wide".into(), "tall".into()],
                }],
                imports: vec![],
                origin: Origin::Static,
            },
        ],
    }
}

fn tokens(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn empty_tokens_lists_all_groups() {
    let out = serve_completions(&fixture(), &tokens(&[""]));
    assert_eq!(out, vec!["ci".to_string(), "data".to_string()]);
}

#[test]
fn group_prefix_filters_groups() {
    let out = serve_completions(&fixture(), &tokens(&["c"]));
    assert_eq!(out, vec!["ci".to_string()]);
}

#[test]
fn after_group_lists_its_commands() {
    let out = serve_completions(&fixture(), &tokens(&["ci", ""]));
    assert_eq!(out, vec!["deploy".to_string(), "hello".to_string()]);
}

#[test]
fn command_prefix_filters_commands() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "h"]));
    assert_eq!(out, vec!["hello".to_string()]);
}

#[test]
fn flag_prefix_lists_argument_flags() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--"]));
    assert_eq!(out, vec!["--name".to_string()]);
}

#[test]
fn flag_value_completes_to_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", ""]));
    assert_eq!(out, vec!["production".to_string(), "staging".to_string()]);
}

#[test]
fn flag_value_partial_filters_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", "s"]));
    assert_eq!(out, vec!["staging".to_string()]);
}

#[test]
fn positional_value_completes_to_allowed_values() {
    let out = serve_completions(&fixture(), &tokens(&["data", "load", ""]));
    assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
}

#[test]
fn unknown_group_returns_no_completions() {
    let out = serve_completions(&fixture(), &tokens(&["nope", ""]));
    assert!(out.is_empty());
}

#[test]
fn flag_without_allowed_values_returns_empty() {
    // `--name` has no allowed_values → shell falls back to filename completion.
    let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--name", ""]));
    assert!(out.is_empty());
}

use crate::complete::{ResolvedManifest, resolve_manifest_at_tab};
use crate::manifest::{write_manifest};
use tempfile::TempDir;

fn make_tree(py_files: &[(&str, &str)]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("tools")).unwrap();
    for (name, contents) in py_files {
        let path = tmp.path().join("tools").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    tmp
}

#[test]
fn returns_fresh_manifest_when_no_cache_exists() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    let ResolvedManifest {
        manifest,
        from_cache,
        project_root,
    } = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!from_cache, "no cache file existed");
    assert_eq!(project_root, tmp.path().canonicalize().unwrap());
    assert!(manifest.groups.iter().any(|g| g.name == "ci"));
    assert!(manifest.commands.iter().any(|c| c.name == "hello"));
}

#[test]
fn returns_cached_manifest_when_hash_matches() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    // Build once and write to disk.
    let built = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &built).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(resolved.from_cache);
    assert_eq!(resolved.manifest, built);
}

#[test]
fn re_parses_when_cached_hash_is_stale() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    // Write a stale manifest with a bogus hash.
    let mut stale = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
    stale.static_hash = "deliberately-stale".into();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &stale).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache, "stale hash should trigger reparse");
    assert_ne!(resolved.manifest.static_hash, "deliberately-stale");
}

#[test]
fn preserves_dynamic_entries_from_cache_when_reparsing() {
    let tmp = make_tree(&[(
        "ci.py",
        "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
    )]);
    // Seed a manifest with a fake dynamic command and a stale static_hash
    // so the re-parse path runs.
    let mut seeded = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
    seeded.static_hash = "stale".into();
    seeded.commands.push(crate::manifest::Command {
        name: "from-plugin".into(),
        group: "dyn-group".into(),
        module: "third_party_pkg".into(),
        function: "from_plugin".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        imports: vec![],
        origin: Origin::Dynamic,
    });
    seeded.groups.push(crate::manifest::Group {
        name: "dyn-group".into(),
        title: "Dynamic group".into(),
        description: String::new(),
        origin: Origin::Dynamic,
    });
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    write_manifest(&manifest_path, &seeded).unwrap();

    let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
    assert!(!resolved.from_cache);
    // Static-layer entry survives.
    assert!(resolved.manifest.commands.iter().any(|c| c.name == "hello"));
    // Dynamic-layer entry from the cache is preserved through the reparse.
    assert!(
        resolved
            .manifest
            .commands
            .iter()
            .any(|c| c.name == "from-plugin" && matches!(c.origin, Origin::Dynamic))
    );
    assert!(
        resolved
            .manifest
            .groups
            .iter()
            .any(|g| g.name == "dyn-group" && matches!(g.origin, Origin::Dynamic))
    );
}

#[test]
fn errors_when_no_tools_dir_exists() {
    let tmp = TempDir::new().unwrap();
    let err = resolve_manifest_at_tab(tmp.path()).expect_err("no tools/");
    let msg = err.to_string();
    assert!(msg.contains("tools"), "expected hint about tools/, got: {msg}");
}

use crate::complete::{Shell, completion_script};

#[test]
fn bash_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Bash);
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("complete -F _toolr_complete toolr"));
}

#[test]
fn zsh_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Zsh);
    assert!(script.starts_with("#compdef toolr"));
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("compdef _toolr toolr"));
}

#[test]
fn fish_script_invokes_toolr_complete() {
    let script = completion_script(Shell::Fish);
    assert!(script.contains("toolr __complete"));
    assert!(script.contains("complete -c toolr"));
}
