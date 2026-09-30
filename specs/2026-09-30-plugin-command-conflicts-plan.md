# Plugin command conflicts — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two plugins that define the same command disable that command with a `Conflict` warning, instead of
stopping toolr working.

**Architecture:** `merge_into_manifest` collects every plugin definition per `(group, name)` first, then emits.
A key with one definition merges as today. A key with two or more distinct packages is left out and gets one
`Conflict` warning. `ThirdPartyError::DuplicateCommand` goes, and the merge becomes infallible. Both the
`Conflict` and `Shadowed` messages link #522 so people can vote for config-driven resolution.

**Tech Stack:** Rust (`toolr-core`, `toolr` crates), `assert_cmd` integration tests, mkdocs.

**Spec:** `specs/2026-09-30-plugin-command-conflicts-design.md`. Read it before starting any task.

## Global Constraints

- No `SCHEMA_VERSION` bump in `crates/toolr-core/src/manifest/model.rs` (it's also the fragment load-rule bound).
- No change to the `PluginWarning` struct. Only a new `PluginWarningKind::Conflict` variant.
- No group pruning. A plugin group whose only command was disabled stays.
- The #522 URL is exactly `https://github.com/s0undt3ch/ToolR/issues/522`, defined once as a `const` in `merge.rs`.
- `Conflict` message, exact: `<cmd> is defined by more than one plugin (<pkg>, <pkg>, ...), so it is disabled.
  Uninstall all but one. Choosing a winner in config is tracked in https://github.com/s0undt3ch/ToolR/issues/522`
  (one line, single spaces).
- `Shadowed` message, exact: `tools/<file> defines <cmd>, hiding the one from <pkg>. Choosing a winner in config
  is tracked in https://github.com/s0undt3ch/ToolR/issues/522` (one line).
- Warning order: `Skipped`, then all `Shadowed` in discovery order, then all `Conflict` in key first-seen order.
- British English, Conventional Commits with `(#522)` in the subject. No `Co-Authored-By` trailer.
- Stage files by explicit path. Never `git add -A` (the untracked `audit/` dir is local-only).
- `cargo fmt` only on touched files, never `cargo fmt -p`.
- The PR references #522 (`Refs #522`). It doesn't close it.

## Review Focus

1. A local command plus two clashing plugins: local must win with two `Shadowed` warnings and no `Conflict`.
   Pinned in Task 1 (`local_command_beats_two_clashing_plugins`).
2. A fresh clone (no `.toolr-manifest.json`) with a clash: toolr must start, not exit 2. Pinned in Task 2
   (`conflicting_plugins_disable_only_that_command` asserts no cache exists before the first run).
3. A clash inside a nested group, and at top level: the message must spell the path as typed. Pinned in Task 1
   (`conflict_message_spells_nested_and_top_level_paths`).
4. Uninstalling one of two clashing plugins: the survivor must come back on the next run, with no leftover
   warning. Pinned in Task 2 (`uninstalling_one_conflicting_plugin_restores_the_command`).
5. The clashing command must be gone from completion, not only from `--help`. Pinned in Task 2 (same test as 2).

## Model assignment

Subagent-driven execution. One implementer and one reviewer per task, then a whole-branch review.

| Task | Implementer | Task reviewer | Why |
| --- | --- | --- | --- |
| 1. Conflict merge in `toolr-core` | `opus` | `fable` | Core behaviour change, ordering rules, an API change, 10 call-site edits. A subtle mistake here ships a wrong CLI. |
| 2. Binary-level tests | `sonnet` | `opus` | Test-only, follows an existing harness closely. The reviewer checks the tests really fail without Task 1. |
| 3. Docs, skill, `UNRELEASED.md`, skill refs | `sonnet` | `haiku` | Prose edits at named lines plus one generator run. The review is a mechanical check against the spec. |
| 4. Verify and archive specs | controller (this session) | — | Runs the full gate and moves two files. Nothing to delegate. |
| Whole-branch review | — | `fable` | Adversarial pass over the full diff before the PR. |

Tasks run in order: 2 needs Task 1's code, 3 quotes Task 1's messages.

---

### Task 1: Disable plugin-vs-plugin conflicts in the merge

**Files:**

- Modify: `crates/toolr-core/src/third_party/merge.rs` (whole file)
- Modify: `crates/toolr-core/src/third_party/mod.rs:24-47`
- Modify: `crates/toolr-core/src/third_party/parse.rs:42-52` (remove `DuplicateCommand`)
- Modify: `crates/toolr-core/src/manifest/model.rs:67-75` (add `Conflict`)
- Test: `crates/toolr-core/src/third_party/tests.rs`

**Interfaces:**

- Consumes: nothing new.
- Produces:
    - `pub fn merge_into_manifest(base: Manifest, fragments: Vec<(ManifestFragment, PathBuf)>) -> Manifest`
      (was `Result<Manifest, ThirdPartyError>`).
    - `PluginWarningKind::Conflict` (serialises as `"conflict"`).
    - The exact `Conflict` and `Shadowed` messages from Global Constraints. Task 2 and Task 3 quote them.
    - `ThirdPartyError::DuplicateCommand` no longer exists.
- [ ] **Step 1: Write the failing tests**

In `crates/toolr-core/src/third_party/tests.rs`:

Delete `third_party_error_duplicate_command_renders_both_packages` (around line 317) and
`merge_errors_on_third_party_to_third_party_collision` (around line 453).

Add these helpers next to `sample_fragment` (around line 384):

```rust
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
```

Add these tests where the deleted collision test was:

```rust
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
    let messages: Vec<_> = merged.plugin_warnings.iter().map(|w| w.message.as_str()).collect();
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
    assert_eq!(kinds, [PluginWarningKind::Shadowed, PluginWarningKind::Shadowed]);
}

#[test]
fn a_fragment_listing_one_command_twice_merges_it_once() {
    let merged = merge_into_manifest(
        empty_base(),
        from_files(vec![two_command_fragment("pkg_a", "deploy", "rollout", "rollout")]),
    );
    assert_eq!(command_names(&merged), [("deploy", "rollout")]);
    assert!(merged.plugin_warnings.is_empty(), "{:?}", merged.plugin_warnings);
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
```

Update the two exact `Shadowed` assertions:

```rust
// local_command_shadows_plugin_with_warning (around line 741)
assert_eq!(
    warning.message,
    format!("tools/ci.py defines ci lint, hiding the one from demo. {RESOLVE_TAIL}")
);

// shadow_message_spells_a_nested_group_with_spaces (around line 764)
assert_eq!(
    messages,
    [format!(
        "tools/docker/image.py defines docker image build, hiding the one from demo. {RESOLVE_TAIL}"
    )]
);
```

`messages` is a `Vec<&str>`, so compare with `messages, [expected.as_str()]` if the array of `String` doesn't
type-check.

Remove the `.unwrap()` after every remaining `merge_into_manifest(...)` call in this file (there are about 8).
Leave `.unwrap()` on `discover_and_merge(...)`, which still returns `Result`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p toolr-core third_party 2>&1 | tail -30`
Expected: compile errors. `PluginWarningKind::Conflict` doesn't exist, and `Result` has no `commands` field where
`.unwrap()` was removed. That's the failing state.

- [ ] **Step 3: Add the `Conflict` kind**

In `crates/toolr-core/src/manifest/model.rs`, `PluginWarningKind`, after `Shadowed`:

```rust
    /// A command defined by two or more plugins and no local command. It is disabled.
    Conflict,
```

- [ ] **Step 4: Remove `DuplicateCommand`**

In `crates/toolr-core/src/third_party/parse.rs`, delete the whole `DuplicateCommand { .. }` variant and its
`#[error(...)]` attribute (lines 42-52).

- [ ] **Step 5: Rewrite `merge.rs`**

Replace `crates/toolr-core/src/third_party/merge.rs` with:

```rust
//! Merge parsed third-party fragments into a project `Manifest`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::model::ManifestFragment;
use crate::manifest::{Command, Group, Manifest, Origin, PluginWarning, PluginWarningKind};

/// Where people vote for choosing a clash winner in configuration.
const RESOLVE_ISSUE_URL: &str = "https://github.com/s0undt3ch/ToolR/issues/522";

/// One plugin's definition of a command, held until every fragment is read.
struct Definition {
    package: String,
    path: PathBuf,
    command: Command,
}

/// Consume `fragments`, each paired with the file it was read from, merging their groups +
/// commands into `base`.
///
/// Conflict resolution:
/// - A group/command pair already present in `base` (from `tools/**/*.py`)
///   wins; the third-party entry is skipped and a `Shadowed` warning is
///   appended to `base.plugin_warnings`.
/// - A group/command pair declared by two or more third-party packages is
///   disabled: none of them is merged, and one `Conflict` warning names them all.
/// - Groups merge by `full_path()`: if a third-party fragment declares a
///   group already present in `base` or in a prior fragment, the existing
///   group's title/description are kept.
///
/// Warnings follow discovery order: every `Shadowed`, then every `Conflict`.
/// Merged entries are tagged `Origin::ThirdParty`, and commands lose any
/// argparse-dispatch flags, which only a local build may set.
pub fn merge_into_manifest(
    mut base: Manifest,
    fragments: Vec<(ManifestFragment, PathBuf)>,
) -> Manifest {
    // (group, command) → the local command's module, which wins and names the shadow warning.
    let local: HashMap<(String, String), String> = base
        .commands
        .iter()
        .map(|c| ((c.group.clone(), c.name.clone()), c.module.clone()))
        .collect();
    // A HashMap alone iterates in random order, so `keys` keeps first-seen order.
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut definitions: HashMap<(String, String), Vec<Definition>> = HashMap::new();

    let mut known_groups: HashSet<String> = base.groups.iter().map(Group::full_path).collect();

    for (fragment, path) in fragments {
        for mut fg in fragment.groups {
            if known_groups.insert(fg.full_path()) {
                fg.origin = Origin::ThirdParty;
                base.groups.push(fg);
            }
        }
        for fc in fragment.commands {
            let key = (fc.group.clone(), fc.name.clone());
            if let Some(module) = local.get(&key) {
                base.plugin_warnings.push(PluginWarning {
                    package: fragment.package.clone(),
                    path: path.clone(),
                    kind: PluginWarningKind::Shadowed,
                    message: shadow_message(module, &fc, &fragment.package),
                });
                continue;
            }
            let defs = definitions.entry(key.clone()).or_default();
            if defs.is_empty() {
                keys.push(key);
            }
            // Only a hand-edited fragment lists a command twice; keep its first copy.
            if defs.iter().any(|d| d.package == fragment.package) {
                continue;
            }
            defs.push(Definition {
                package: fragment.package.clone(),
                path: path.clone(),
                command: fc,
            });
        }
    }

    for key in keys {
        let mut defs = definitions.remove(&key).unwrap_or_default();
        if defs.len() > 1 {
            base.plugin_warnings.push(conflict_warning(&defs));
            continue;
        }
        let Some(Definition { mut command, .. }) = defs.pop() else {
            continue;
        };
        command.origin = Origin::ThirdParty;
        command.dispatched_from = None;
        command.is_dispatcher = false;
        base.commands.push(command);
    }

    base
}

/// The command as typed after `toolr`: `docker image build`, or `hello` at top level.
fn command_path(cmd: &Command) -> String {
    if cmd.group.is_empty() {
        cmd.name.clone()
    } else {
        format!("{} {}", cmd.group.replace('.', " "), cmd.name)
    }
}

/// `tools/<file> defines <group> <name>, hiding the one from <pkg>`. The file comes from the
/// module path alone, so a package module reads as `<pkg>.py` rather than `__init__.py`.
fn shadow_message(local_module: &str, plugin_cmd: &Command, package: &str) -> String {
    let file = format!("{}.py", local_module.replace('.', "/"));
    format!(
        "{file} defines {}, hiding the one from {package}. \
         Choosing a winner in config is tracked in {RESOLVE_ISSUE_URL}",
        command_path(plugin_cmd)
    )
}

/// One warning for a command that `defs` (two or more packages) all define.
fn conflict_warning(defs: &[Definition]) -> PluginWarning {
    let first = &defs[0];
    let packages: Vec<&str> = defs.iter().map(|d| d.package.as_str()).collect();
    PluginWarning {
        package: first.package.clone(),
        path: first.path.clone(),
        kind: PluginWarningKind::Conflict,
        message: format!(
            "{} is defined by more than one plugin ({}), so it is disabled. \
             Uninstall all but one. Choosing a winner in config is tracked in {RESOLVE_ISSUE_URL}",
            command_path(&first.command),
            packages.join(", ")
        ),
    }
}
```

- [ ] **Step 6: Update `discover_and_merge`**

In `crates/toolr-core/src/third_party/mod.rs`, replace the doc comment and the last line of `discover_and_merge`:

```rust
/// Glob for fragments under `tools_venv`, parse each, and merge them
/// into `base`. Returns the augmented manifest, with every skipped plugin,
/// then every shadowed plugin command, then every plugin-to-plugin conflict
/// recorded in `plugin_warnings`.
///
/// A plugin outside the load rule, or with an invalid argument, is skipped,
/// and a command two plugins define is disabled, so neither can break the CLI.
/// These still abort the whole merge:
/// - Malformed JSON in any fragment → `ThirdPartyError::Json`.
/// - Missing/invalid `toolr_schema_version` → `MissingVersion`.
pub fn discover_and_merge(
```

and at the end of the function:

```rust
    Ok(merge_into_manifest(base, fragments))
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p toolr-core third_party 2>&1 | tail -30`
Expected: all `third_party` tests pass, including the seven new ones.

Then: `cargo build -p toolr 2>&1 | tail -5`
Expected: builds. Nothing outside `toolr-core` calls `merge_into_manifest` or matches `DuplicateCommand`
(verified: `grep -rn DuplicateCommand crates` returns nothing after Step 4).

- [ ] **Step 8: Lint and format touched files**

Run: `cargo clippy -p toolr-core --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings.

Run:

```bash
rustfmt --edition 2021 \
  crates/toolr-core/src/third_party/{merge,mod,parse,tests}.rs \
  crates/toolr-core/src/manifest/model.rs
```

The workspace edition is 2021 (`Cargo.toml`). Then `git diff --stat` must list only these five files.

- [ ] **Step 9: Commit**

```bash
git add crates/toolr-core/src/third_party/merge.rs crates/toolr-core/src/third_party/mod.rs \
  crates/toolr-core/src/third_party/parse.rs crates/toolr-core/src/third_party/tests.rs \
  crates/toolr-core/src/manifest/model.rs
git diff --cached --name-only
git commit -m "feat(plugins): disable a command two plugins define instead of failing (#522)"
```

---

### Task 2: Prove the CLI survives a conflict

**Files:**

- Test: `crates/toolr/tests/plugin_warnings.rs`

**Interfaces:**

- Consumes: Task 1's `Conflict` message prefix `"<cmd> is defined by more than one plugin ("`, and the existing
  `Project` harness in this file (`Project::new`, `add_plugin`, `toolr`, `stderr`, `stdout`, `manifest`, `tools`)
  and `fragment(schema, package, group, parent, cmd)`.
- Produces: nothing.
- [ ] **Step 1: Write the tests**

Append to `crates/toolr/tests/plugin_warnings.rs`:

```rust
const CONFLICT_WARNING: &str =
    "toolr: warning: deploy rollout is defined by more than one plugin (a_plugin, b_plugin), so it is disabled.";

/// A fresh project (no cached manifest) where `a_plugin` and `b_plugin` both ship `deploy rollout`,
/// and `c_plugin` ships its own `extra run`.
fn project_with_conflict() -> Project {
    let p = Project::new();
    p.add_plugin("a_plugin", &fragment(2, "a_plugin", "deploy", None, "rollout"));
    p.add_plugin("b_plugin", &fragment(2, "b_plugin", "deploy", None, "rollout"));
    p.add_plugin("c_plugin", &fragment(2, "c_plugin", "extra", None, "run"));
    p
}

fn has_command(manifest: &serde_json::Value, group: &str, name: &str) -> bool {
    manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["group"] == group && c["name"] == name)
}

#[test]
fn conflicting_plugins_disable_only_that_command() {
    let p = project_with_conflict();
    assert!(
        !p.tools().join(".toolr-manifest.json").exists(),
        "the test needs a fresh project with no cache"
    );

    // Before the fix, the bootstrap build failed here and toolr exited 2.
    let out = p.toolr(&["--help"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert_eq!(stderr.matches(CONFLICT_WARNING).count(), 1, "stderr:\n{stderr}");
    assert!(stdout.contains("greet"), "stdout:\n{stdout}");
    assert!(stdout.contains("extra"), "stdout:\n{stdout}");

    // The local group and the unrelated plugin command still work. The fixture venv
    // can't import plugins, so plugin commands are checked through help and the manifest.
    p.toolr(&["greet", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
    let manifest: serde_json::Value = serde_json::from_str(&p.manifest()).unwrap();
    assert!(has_command(&manifest, "extra", "run"), "{manifest}");
    assert!(!has_command(&manifest, "deploy", "rollout"), "{manifest}");

    // Gone from the group's help and from completion.
    let deploy_help = p.toolr(&["deploy", "--help"]).output().unwrap();
    assert!(
        !String::from_utf8_lossy(&deploy_help.stdout).contains("rollout"),
        "{deploy_help:?}"
    );
    let cwd = p.tmp.path().to_string_lossy().to_string();
    let completions = p.stdout(&["__complete", &cwd, "deploy", ""]);
    assert!(!completions.contains("rollout"), "completions:\n{completions}");
    let top = p.stdout(&["__complete", &cwd, ""]);
    assert!(top.contains("extra"), "completions:\n{top}");
}

#[test]
fn uninstalling_one_conflicting_plugin_restores_the_command() {
    let p = project_with_conflict();
    let stderr = p.stderr(&["--help"]);
    assert!(stderr.contains(CONFLICT_WARNING), "stderr:\n{stderr}");

    std::fs::remove_dir_all(
        p.tools()
            .join(".venv/lib/python3.13/site-packages/a_plugin"),
    )
    .unwrap();

    let stderr = p.stderr(&["--help"]);
    assert!(!stderr.contains("is defined by more than one plugin"), "stderr:\n{stderr}");
    let manifest: serde_json::Value = serde_json::from_str(&p.manifest()).unwrap();
    assert!(has_command(&manifest, "deploy", "rollout"), "{manifest}");
    assert!(p.manifest().contains("b_plugin.commands"));
    p.toolr(&["deploy", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("rollout"));
}
```

If `p.tmp` isn't reachable from a free function in this file, it is: `completion_never_warns` already uses
`p.tmp.path()`. The `deploy --help` call in the first test uses `.output()`, not `p.stdout`, because a group with no
commands may exit non-zero (spec §3.1). Don't assert its status.

- [ ] **Step 2: Prove the tests fail without Task 1**

Task 1 is committed, so `crates/toolr-core` is clean. Temporarily put back the pre-Task-1 code, run, then restore:

```bash
TASK1=$(git log --format=%H -1 --grep='disable a command two plugins define')
git checkout "$TASK1~1" -- crates/toolr-core
cargo test -p toolr --test plugin_warnings conflict 2>&1 | tail -15
git checkout HEAD -- crates/toolr-core
git status --short crates/toolr-core
```

Expected: `conflicting_plugins_disable_only_that_command` fails on `out.status.success()` (exit 2). The
uninstall test fails too, on its first `stderr` call. The final `git status` prints nothing.

- [ ] **Step 3: Run the tests with Task 1 in place**

Run: `cargo test -p toolr --test plugin_warnings 2>&1 | tail -15`
Expected: every test in the file passes, including `shadowed_plugin_command_warns_then_recovers`, whose substring
assertions are unchanged.

- [ ] **Step 4: Format and commit**

Run: `rustfmt --edition 2021 crates/toolr/tests/plugin_warnings.rs`

```bash
git add crates/toolr/tests/plugin_warnings.rs
git diff --cached --name-only
git commit -m "test(plugins): cover a plugin conflict on a fresh project and its recovery (#522)"
```

---

### Task 3: Docs, skill, release notes, skill refs

**Files:**

- Modify: `docs/third-party.md:287-306` ("Command resolution")
- Modify: `skills/toolr-command-packaging/SKILL.md:88-94` ("Command resolution")
- Modify: `crates/toolr-core/src/manifest/model.rs:51` (the `plugin_warnings` doc comment)
- Regenerate: `skills/toolr-command-packaging/references/packaging.md` (via `cargo xtask build-skill-refs`)
- Modify: `UNRELEASED.md` (the "Plugin commands behave like local commands" section, lines 143-178)

**Interfaces:**

- Consumes: the exact `Conflict` and `Shadowed` messages from Global Constraints.
- Produces: nothing.
- [ ] **Step 1: Rewrite "Command resolution" in `docs/third-party.md`**

Replace the section body (from `When multiple sources contribute commands` to the end of the "Between third-party
packages" bullet) with:

````markdown
When multiple sources contribute commands with the same name:

- **Project commands** (defined in your `tools/`) always win over a
  plugin command with the same group and name. The plugin command is
  hidden, and toolr warns about it on every run
  (`toolr: warning: ... hiding the one from <pkg>`), so a plugin release
  can't silently change what a local command does.
- **Between third-party packages:** when two or more plugins define the
  same command and your `tools/` doesn't, toolr can't know which one you
  want, so it disables that command. Everything else keeps working. The
  warning names every plugin:

  ```text
  toolr: warning: deploy rollout is defined by more than one plugin (toolr_a, toolr_b), so it is disabled. Uninstall all but one. Choosing a winner in config is tracked in https://github.com/s0undt3ch/ToolR/issues/522
  ```

  Uninstall all but one of them to get the command back. If a group only
  held that command, it stays in `--help` but is empty.
- **Group augmentation:** to add commands to a group of the host repo,
  the plugin declares that group itself (`command_group("ci", ...)`, with
  the same full path). A command in a group the plugin doesn't declare
  fails the build. The host's title and description win over the
  plugin's. Groups are matched by their full path, so a plugin's
  `docker.image` and a local `ci.image` stay separate.

Choosing the winner in configuration isn't supported yet. If you need it,
vote on [#522](https://github.com/s0undt3ch/ToolR/issues/522).
````

Keep the "Group augmentation" bullet's wording exactly as it is today. Only its position changes.

- [ ] **Step 2: Update the packaging skill's prose**

In `skills/toolr-command-packaging/SKILL.md`, replace the "Command resolution" paragraph with:

```markdown
When multiple sources contribute commands with the same name: a
project's own `tools/` commands always win over a plugin's, with a
warning on every run; a plugin can add commands to an existing group
rather than creating a duplicate; and a command that two or more plugins
define is disabled with a warning naming them all, while the rest of the
CLI keeps working. Choosing the winner in configuration isn't supported.
```

No URLs in skill prose. `build-skill-refs --check` rejects bare or linked `github.com/s0undt3ch/ToolR` URLs in
skills.

- [ ] **Step 3: Update the `plugin_warnings` doc comment and regenerate skill refs**

In `crates/toolr-core/src/manifest/model.rs`, change

```rust
    /// Plugins skipped or shadowed by the last third-party merge, warned about on every run.
```

to

```rust
    /// Plugins skipped, shadowed or in conflict at the last third-party merge, warned about on every run.
```

Run: `cargo xtask build-skill-refs && cargo xtask build-skill-refs --check`
Expected: the first run rewrites `skills/toolr-command-packaging/references/packaging.md` line ~108. The second
exits 0.

- [ ] **Step 4: Queue the release note**

In `UNRELEASED.md`, at the end of the "Plugin commands behave like local commands" section (after the paragraph that
ends `tracked in [#522](https://github.com/s0undt3ch/ToolR/issues/522).`), add:

```markdown

Two plugins that define the same command no longer stop toolr from working. Before, the manifest build failed, and
on a fresh clone or a CI runner no command ran, local ones included. Now that command is disabled, and a warning on
every run names every plugin that defines it (`toolr: warning: deploy rollout is defined by more than one plugin
(toolr_a, toolr_b), so it is disabled. ...`). Uninstall all but one to get it back. Both this warning and the
shadowing one now link [#522](https://github.com/s0undt3ch/ToolR/issues/522), where you can vote for choosing the
winner in configuration.
```

- [ ] **Step 5: Verify the docs build and hooks**

Run: `uv run mkdocs build --strict 2>&1 | tail -5`
Expected: builds with no warnings.

Run:

```bash
prek run --files docs/third-party.md skills/toolr-command-packaging/SKILL.md \
  skills/toolr-command-packaging/references/packaging.md UNRELEASED.md \
  crates/toolr-core/src/manifest/model.rs
```

Expected: every hook passes.

- [ ] **Step 6: Commit**

```bash
git add docs/third-party.md skills/toolr-command-packaging/SKILL.md \
  skills/toolr-command-packaging/references/packaging.md UNRELEASED.md \
  crates/toolr-core/src/manifest/model.rs
git diff --cached --name-only
git commit -m "docs(plugins): document disabled plugin command conflicts (#522)"
```

---

### Task 4: Full verification and spec archive

Run by the controller, not a subagent.

- [ ] **Step 1: Full test gate**

Run in the background, polling the output every 30-60s (it can stall): `mise run test`
Expected: the skill-refs drift gate, `cargo test --workspace` and `pytest` all pass.

- [ ] **Step 2: All hooks**

Run: `prek run --all-files`
Expected: every hook passes.

- [ ] **Step 3: Leak check**

Run: `git grep -in paddle -- crates docs skills specs UNRELEASED.md; git grep -n '/Users/' -- crates docs skills specs UNRELEASED.md`
Expected: no output.

- [ ] **Step 4: Archive the specs (final commit)**

```bash
git mv specs/2026-09-30-plugin-command-conflicts-design.md specs/archive/2026/
git mv specs/2026-09-30-plugin-command-conflicts-plan.md specs/archive/2026/
git diff --cached --name-only
git commit -m "docs(specs): archive the plugin command conflicts design and plan (#522)"
```

- [ ] **Step 5: Whole-branch review**

Dispatch a `fable` reviewer over `git diff main...HEAD` with the spec, before opening the PR.
