# Fill the gaps — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the two follow-up fixes deferred from Plan A — auto-rebuild on missing manifest, and routing the dispatcher's own CLI flags through the grafted-child invocation path.

**Architecture:** Two independent stacks inside one PR. Stack A adds a pre-clap bootstrap step in `main.rs` that detects a missing `tools/.toolr-manifest.json` and runs `rebuild_manifest_full`, skipping for built-ins / completion via cheap argv inspection. Stack B adds an `is_dispatcher: bool` field on `Command`, set at graft time by `argparse::run_for_project`, read by `cli.rs::build_group_subtree` to hoist grafted children into the dispatcher as subcommands (instead of treating them as siblings of the dispatcher inside the parent group). `dispatch.rs` widens its path-to-command lookup to handle 3-segment paths.

**Tech Stack:** Rust (toolr / toolr-core), pyo3-free dispatch wire (JSON spec), pytest E2E.

**Read first:** `specs/2026-05-19-fill-the-gaps-design.md` is the spec; this plan is the *how*. Plan A's commits on `feat/argparse-scanner` are the baseline.

---

## File map

### New Rust files

- `crates/toolr/src/bootstrap.rs` — argv inspector + manifest-bootstrap entry point. Re-exported via `mod bootstrap;` in `main.rs`.

### Modified Rust files

- `crates/toolr/src/main.rs` — `mod bootstrap;` declaration; `run()` calls `bootstrap::ensure_manifest_present_or_bootstrap(&cwd, &argv)` before `load_or_empty`.
- `crates/toolr-core/src/manifest/model.rs` — add `is_dispatcher: bool` to `Command`.
- `crates/toolr-core/src/argparse/mod.rs` — new `GraftResult { children_by_parent, dispatchers }`; `run_for_project` returns it.
- `crates/toolr-core/src/parser/build.rs` — consume `GraftResult.dispatchers` and flip the flag on each named parent. Factor the dotted-name derivation into `fn dotted_name(cmd: &Command) -> String`.
- `crates/toolr/src/cli.rs` — `build_group_subtree` reshape + new `build_dispatcher_command` helper.
- `crates/toolr/src/dispatch.rs` — wider path-to-command lookup that tries most-specific-group-first.

### Modified test files

- `crates/toolr-core/src/manifest/tests.rs` — `is_dispatcher` round-trip tests (mirror existing `dispatched_from` pair).
- `crates/toolr-core/src/argparse/mod.rs` `#[cfg(test)]` — extend `run_for_project_returns_grafted_children` to assert `dispatchers` set.
- `crates/toolr-core/src/parser/build.rs` `#[cfg(test)]` — extend `build_static_manifest_grafts_argparse_children` to assert `dispatcher.is_dispatcher == true`.
- `crates/toolr/src/cli.rs` `#[cfg(test)]` — 3 new clap-tree-shape tests.
- `tests/sources/test_e2e.py` — unskip `test_e2e_auto_rebuild_runs_argparse`; add new `test_e2e_dispatcher_outer_flags`.

### Sites to update for the new field

All places that construct a `Command { ... }` literal need `is_dispatcher: false,` appended. These are the same 8 sites Task 7 of Plan A already updated for `dispatched_from`:

- `crates/toolr-core/src/manifest/model.rs` (test fixtures)
- `crates/toolr-core/src/manifest/tests.rs`
- `crates/toolr-core/src/parser/commands.rs`
- `crates/toolr-core/src/third_party/merge.rs`
- `crates/toolr-core/src/third_party/tests.rs`
- `crates/toolr-core/src/dynamic/tests.rs`
- `crates/toolr-core/src/dynamic/merge.rs`
- `crates/toolr-core/src/complete/tests.rs`
- `crates/toolr/src/execute_build.rs`

Use `rg -n 'Command \{' crates/ tests/` to confirm the list at the start of Task 4.

---

## Task index

- Stack A — auto-rebuild on missing manifest:
  - Task 1: `should_skip_auto_rebuild` argv inspector + tests
  - Task 2: `ensure_manifest_present_or_bootstrap` + wire into `main.rs::run`
  - Task 3: Unskip `test_e2e_auto_rebuild_runs_argparse`
- Stack B — dispatcher hosts grafted children:
  - Task 4: `is_dispatcher: bool` field on `Command`
  - Task 5: `argparse::run_for_project` returns `GraftResult`; `build_static_manifest_inner` flips the flag
  - Task 6: `cli.rs::build_group_subtree` reshape + `build_dispatcher_command`
  - Task 7: `dispatch.rs` widens path-to-command lookup
  - Task 8: New E2E test exercising dispatcher outer flags

---

## Stack A — Auto-rebuild on missing manifest

### Task 1: `should_skip_auto_rebuild` argv inspector

**Files:**

- Create: `crates/toolr/src/bootstrap.rs`

The bootstrap module is empty for now — just the argv inspector and its tests. The wire-up into `main.rs::run` lands in Task 2.

- [ ] **Step 1: Write the failing tests**

Create `crates/toolr/src/bootstrap.rs`:

```rust
//! Pre-clap bootstrap: detect missing `tools/.toolr-manifest.json`
//! and run a full rebuild before clap parses the user's command.
//!
//! See `specs/2026-05-19-fill-the-gaps-design.md` (gap 1) for the
//! decision logic.

pub(crate) fn should_skip_auto_rebuild(argv: &[String]) -> bool {
    todo!("Task 1")
}

#[cfg(test)]
mod tests {
    use super::should_skip_auto_rebuild;

    fn args(parts: &[&str]) -> Vec<String> {
        std::iter::once("toolr")
            .chain(parts.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn skips_for_long_help_flag() {
        assert!(should_skip_auto_rebuild(&args(&["--help"])));
    }

    #[test]
    fn skips_for_short_help_flag() {
        assert!(should_skip_auto_rebuild(&args(&["-h"])));
    }

    #[test]
    fn skips_for_long_version_flag() {
        assert!(should_skip_auto_rebuild(&args(&["--version"])));
    }

    #[test]
    fn skips_for_short_version_flag() {
        assert!(should_skip_auto_rebuild(&args(&["-V"])));
    }

    #[test]
    fn skips_for_bare_toolr() {
        assert!(should_skip_auto_rebuild(&args(&[])));
    }

    #[test]
    fn skips_for_tab_completion() {
        assert!(should_skip_auto_rebuild(&args(&["__complete", "/tmp", "..."])));
    }

    #[test]
    fn skips_for_project_subcommands() {
        assert!(should_skip_auto_rebuild(&args(&["project", "manifest", "rebuild"])));
    }

    #[test]
    fn skips_for_self_subcommands() {
        assert!(should_skip_auto_rebuild(&args(&["self", "cache", "list"])));
    }

    #[test]
    fn skips_for_init() {
        assert!(should_skip_auto_rebuild(&args(&["init"])));
    }

    #[test]
    fn fires_for_user_command() {
        assert!(!should_skip_auto_rebuild(&args(&["jenkins", "job", "migrate"])));
    }

    #[test]
    fn fires_with_leading_global_flag() {
        assert!(!should_skip_auto_rebuild(&args(&["--debug", "django", "migrate"])));
    }
}
```

Then declare the module from `crates/toolr/src/main.rs`. Add `mod bootstrap;` next to the existing `mod cli;` etc. block (around `main.rs:1-10`).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p toolr --quiet bootstrap
```

Expected: 11 tests panicking on `todo!("Task 1")`.

- [ ] **Step 3: Implement `should_skip_auto_rebuild`**

Replace the `todo!()` body with:

```rust
pub(crate) fn should_skip_auto_rebuild(argv: &[String]) -> bool {
    const BUILTINS: &[&str] = &["__complete", "project", "self", "init"];
    const HELP_FLAGS: &[&str] = &["--help", "--version", "-h", "-V"];

    // Any help/version flag anywhere in argv → skip.
    if argv.iter().skip(1).any(|a| HELP_FLAGS.contains(&a.as_str())) {
        return true;
    }
    // First positional (= first arg after `toolr` that doesn't start with `-`).
    let first_positional = argv.iter().skip(1).find(|a| !a.starts_with('-'));
    match first_positional {
        None => true,                                // `toolr` alone
        Some(name) => BUILTINS.contains(&name.as_str()),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p toolr --quiet bootstrap
```

Expected: 11 passed.

- [ ] **Step 5: Run clippy**

```bash
cargo clippy --workspace --tests -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr/src/bootstrap.rs crates/toolr/src/main.rs
git commit -m "bootstrap: add should_skip_auto_rebuild argv inspector"
```

---

### Task 2: `ensure_manifest_present_or_bootstrap` + wire into `main.rs::run`

**Files:**

- Modify: `crates/toolr/src/bootstrap.rs`
- Modify: `crates/toolr/src/main.rs:27-36` (the `run` function)

- [ ] **Step 1: Read the existing seam**

```bash
sed -n '27,40p' crates/toolr/src/main.rs
```

The current shape:

```rust
fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    maybe_emit_cache_hint_from_argv();
    let manifest = load_or_empty(&cwd);
    let mut command = cli::build_command(&manifest);
    let matches = command.clone().get_matches();
    dispatch::dispatch(&matches, &manifest, &mut command)
}
```

The new bootstrap call lands between `cwd` and `load_or_empty`. It takes `&cwd` and `argv`.

- [ ] **Step 2: Add the function (no test yet — covered by E2E in Task 3)**

Append to `crates/toolr/src/bootstrap.rs`:

```rust
use std::path::Path;

use toolr_core::discovery::discover_project_root;
use toolr_core::dynamic::rebuild_manifest_full;
use toolr_core::venv::resolve_venv_path;

/// Bootstrap step that runs before clap parses the user's command.
///
/// When the manifest is missing AND `tools/pyproject.toml` exists AND
/// argv doesn't look like a built-in / help / completion call, run a
/// full `rebuild_manifest_full` so the user's command can succeed on
/// a fresh clone. Errors propagate so `main.rs` can print them and
/// exit non-zero — we intentionally do NOT fall through to an empty
/// manifest, since that's the buggy old behaviour this task fixes.
pub(crate) fn ensure_manifest_present_or_bootstrap(
    cwd: &Path,
    argv: &[String],
) -> anyhow::Result<()> {
    let Ok(root) = discover_project_root(cwd) else {
        return Ok(());
    };
    let tools = root.join("tools");
    if !tools.join("pyproject.toml").is_file() {
        return Ok(());
    }
    if tools.join(".toolr-manifest.json").is_file() {
        return Ok(());
    }
    if should_skip_auto_rebuild(argv) {
        return Ok(());
    }

    let resolved = match resolve_venv_path(&root) {
        Ok(r) => r,
        // Venv not yet set up — let the normal execute path surface
        // the diagnostic. Same fallback `ensure_dynamic_layer_fresh`
        // uses.
        Err(_) => return Ok(()),
    };
    if !resolved.python.is_file() {
        return Ok(());
    }

    eprintln!("toolr: manifest missing; building (first-time setup)...");
    rebuild_manifest_full(&root, &resolved.python, &resolved.venv_dir)?;
    Ok(())
}
```

- [ ] **Step 3: Wire into `main.rs::run`**

Modify `crates/toolr/src/main.rs:27-36` to:

```rust
fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let argv: Vec<String> = std::env::args().collect();
    maybe_emit_cache_hint_from_argv();
    bootstrap::ensure_manifest_present_or_bootstrap(&cwd, &argv)?;
    let manifest = load_or_empty(&cwd);
    let mut command = cli::build_command(&manifest);
    let matches = command.clone().get_matches();
    dispatch::dispatch(&matches, &manifest, &mut command)
}
```

(`maybe_emit_cache_hint_from_argv` already collects argv internally — leaving it alone keeps the diff minimal.)

- [ ] **Step 4: Verify the workspace still builds and existing tests pass**

```bash
cargo build --workspace --tests
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --tests -- -D warnings
```

Expected: all green. No new tests yet — Task 3's E2E covers behaviour.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/bootstrap.rs crates/toolr/src/main.rs
git commit -m "bootstrap: auto-rebuild manifest when missing for user commands"
```

---

### Task 3: Unskip the existing E2E test

**Files:**

- Modify: `tests/sources/test_e2e.py` (remove the `@pytest.mark.skip` decorator on `test_e2e_auto_rebuild_runs_argparse`)

- [ ] **Step 1: Find and remove the skip**

```bash
rg -n 'pytest.mark.skip' tests/sources/test_e2e.py
```

You'll see a `@pytest.mark.skip(reason="Auto-rebuild on missing manifest is not implemented...")` decorator immediately above `def test_e2e_auto_rebuild_runs_argparse(...)`. Delete the decorator and its multi-line `reason=` block.

- [ ] **Step 2: Rebuild the toolr binary into the venv**

The E2E test invokes `.venv/bin/toolr`. Drop in the latest build:

```bash
cargo build --release -p toolr
cp target/release/toolr .venv/bin/toolr
.venv/bin/toolr --version
```

Expected: prints `toolr 0.11.1` (or whatever the current crate version is).

- [ ] **Step 3: Run the unskipped test**

```bash
uv run pytest tests/sources/test_e2e.py::test_e2e_auto_rebuild_runs_argparse -v
```

Expected: PASS. The manifest didn't exist before the invocation; toolr bootstrapped it; the dispatch path then ran and the sidecar got written.

- [ ] **Step 4: Run the full E2E module to confirm no regression**

```bash
uv run pytest tests/sources/ -v
```

Expected: all green (4 happy-path E2E + the rest of the sources unit tests). No skipped tests in `test_e2e.py` after this task.

- [ ] **Step 5: Commit**

```bash
git add tests/sources/test_e2e.py
git commit -m "tests: unskip auto-rebuild E2E now that bootstrap is wired in"
```

---

## Stack B — Dispatcher hosts grafted children

### Task 4: `is_dispatcher: bool` field on `Command`

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs`
- Modify: `crates/toolr-core/src/manifest/tests.rs`
- Modify: every `Command { ... }` literal site (see file map; ~9 files)

- [ ] **Step 1: List the literal sites**

```bash
rg -n 'Command \{' crates/ tests/
```

Skim each hit and confirm it constructs `toolr_core::manifest::Command` (not `clap::Command` or `assert_cmd::Command` — those don't need updating). Make a checklist.

- [ ] **Step 2: Write failing round-trip tests**

Append to the existing `dispatched_from_tests` mod in `crates/toolr-core/src/manifest/tests.rs` (or create a parallel `is_dispatcher_tests` mod next to it):

```rust
#[cfg(test)]
mod is_dispatcher_tests {
    use super::*;

    fn cmd_with(is_dispatcher: bool) -> Command {
        Command {
            name: "job".into(),
            group: "jenkins".into(),
            module: "tools.jenkins".into(),
            function: "job".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher,
        }
    }

    #[test]
    fn command_serializes_is_dispatcher_when_true() {
        let json = serde_json::to_string(&cmd_with(true)).unwrap();
        assert!(json.contains(r#""is_dispatcher":true"#));
    }

    #[test]
    fn command_omits_is_dispatcher_when_false() {
        let json = serde_json::to_string(&cmd_with(false)).unwrap();
        assert!(!json.contains("is_dispatcher"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail (compile error)**

```bash
cargo test -p toolr-core --quiet is_dispatcher
```

Expected: compile error — `Command` has no field `is_dispatcher`.

- [ ] **Step 4: Add the field to `Command`**

In `crates/toolr-core/src/manifest/model.rs`, add to the `Command` struct definition (immediately after `dispatched_from`):

```rust
    /// True when this command hosts grafted children as its own
    /// subcommands. Set by `argparse::run_for_project` on the parent
    /// dispatcher entry whenever a `[[tool.toolr.argparse.*.attach]]`
    /// directs children at it. Read by the CLI builder to decide
    /// whether to build the command as a flat leaf or as a parent
    /// that owns children.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_dispatcher: bool,
```

- [ ] **Step 5: Update every literal site**

For each `Command { ... }` literal in your Step 1 checklist, append `is_dispatcher: false,` to the struct fields. Search for compile failures to find any you missed:

```bash
cargo build --workspace --tests 2>&1 | grep -E "error\[E\d+\]"
```

If any error mentions "missing field `is_dispatcher`", fix that site and rebuild.

- [ ] **Step 6: Run all tests and verify both new tests pass**

```bash
cargo test -p toolr-core --quiet -- --test-threads=1
cargo build --workspace --tests
cargo clippy --workspace --tests -- -D warnings
```

Expected: all green. New `is_dispatcher_tests::command_serializes_is_dispatcher_when_true` and `command_omits_is_dispatcher_when_false` pass.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-core/ crates/toolr/
git commit -m "manifest: add optional is_dispatcher field on Command"
```

---

### Task 5: `argparse::run_for_project` returns `GraftResult`; builder flips the flag

**Files:**

- Modify: `crates/toolr-core/src/argparse/mod.rs`
- Modify: `crates/toolr-core/src/parser/build.rs`

**Background:** Today `run_for_project` returns `HashMap<String, Vec<Command>>` (`{parent_dotted_name -> children}`). The caller appends children to the manifest. To set `is_dispatcher` on each parent, we need to also return the set of parent dotted names. We also factor the dotted-name derivation in `build_static_manifest_inner` (currently a closure) into a free function so the flag-flip pass can reuse it.

- [ ] **Step 1: Extend the existing `run_for_project_returns_grafted_children` test**

In `crates/toolr-core/src/argparse/mod.rs` `#[cfg(test)] mod tests`, modify the existing test to assert the new return shape:

```rust
#[test]
fn run_for_project_returns_grafted_children() {
    // ... existing fixture setup (tempdir, tools/, apps/, pyproject.toml) ...

    let result = run_for_project(project.path(), &parents).unwrap();

    let django_children = result.children_by_parent.get("django").unwrap();
    assert_eq!(django_children.len(), 1);
    assert_eq!(django_children[0].name, "sync");
    assert_eq!(
        django_children[0].dispatched_from.as_deref(),
        Some("argparse:django"),
    );

    // New: `django` received grafted children, so it's in `dispatchers`.
    assert!(result.dispatchers.contains("django"));
}
```

(Keep the existing fixture; only the assertions change.)

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p toolr-core --quiet argparse::tests::run_for_project
```

Expected: compile error — `HashMap<String, Vec<Command>>` doesn't have `children_by_parent` / `dispatchers`.

- [ ] **Step 3: Add `GraftResult` and reshape `run_for_project`**

In `crates/toolr-core/src/argparse/mod.rs`, add at module level (near the existing `ArgparseError`):

```rust
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct GraftResult {
    /// `{parent_dotted_name -> [grafted child Command]}`.
    pub children_by_parent: HashMap<String, Vec<Command>>,
    /// Dotted names of parents that received at least one grafted
    /// child. The caller flips `is_dispatcher = true` on each.
    pub dispatchers: HashSet<String>,
}
```

Reshape the function signature and body:

```rust
pub fn run_for_project(
    project_root: &Path,
    parents: &HashMap<String, (String, String)>,
) -> Result<GraftResult, ArgparseError> {
    let pyproject = project_root.join("tools").join("pyproject.toml");
    if !pyproject.exists() {
        return Ok(GraftResult::default());
    }
    let blocks = config::parse_blocks_from_pyproject(&pyproject)?;
    if blocks.is_empty() {
        return Ok(GraftResult::default());
    }
    attach::validate_attachments(&blocks, parents)?;

    let mut out: HashMap<String, Vec<Command>> = HashMap::new();
    let mut dispatchers: HashSet<String> = HashSet::new();
    for block in &blocks {
        let scanned: Vec<scan::ScannedCommand> = scan::scan_block_paths(project_root, &block.scan_paths)?
            .into_iter()
            .map(|s| scan::with_common_args(s, &block.common_args))
            .collect();
        let grafted = attach::graft_children(block, &scanned, parents)?;
        for (parent, children) in grafted {
            if !children.is_empty() {
                dispatchers.insert(parent.clone());
            }
            out.entry(parent).or_default().extend(children);
        }
    }
    attach::validate_no_collisions(&out)?;
    Ok(GraftResult {
        children_by_parent: out,
        dispatchers,
    })
}
```

- [ ] **Step 4: Update `build_static_manifest_inner` to consume the new shape**

Locate `crates/toolr-core/src/parser/build.rs::build_static_manifest_inner`. The existing call site looks roughly like:

```rust
let grafted = crate::argparse::run_for_project(project_root, &parents)
    .map_err(BuildError::Argparse)?;
for (_parent, mut children) in grafted {
    manifest.commands.append(&mut children);
}
```

Replace with:

```rust
let grafted = crate::argparse::run_for_project(project_root, &parents)
    .map_err(BuildError::Argparse)?;

// Splice grafted children into the manifest.
for (_parent, mut children) in grafted.children_by_parent {
    manifest.commands.append(&mut children);
}

// Flip the dispatcher flag on each parent that received children.
for cmd in manifest.commands.iter_mut() {
    if grafted.dispatchers.contains(&dotted_name(cmd)) {
        cmd.is_dispatcher = true;
    }
}
```

`dotted_name(cmd: &Command) -> String` is a new private helper. Add it near the top of `build.rs` (or at the bottom, matching the file's existing helper-placement convention). The body is the same derivation already used inline when populating `parents`:

```rust
fn dotted_name(cmd: &Command) -> String {
    let leaf = cmd.group.rsplit('.').next().unwrap_or(cmd.group.as_str());
    if !cmd.group.is_empty() && cmd.name == leaf {
        cmd.group.clone()
    } else if cmd.group.is_empty() {
        cmd.name.clone()
    } else {
        format!("{}.{}", cmd.group, cmd.name)
    }
}
```

Then replace the inline closure that builds the `parents` map with a call to this helper.

- [ ] **Step 5: Extend the existing `build_static_manifest_grafts_argparse_children` test**

Find the assertion block at the end of the test in `crates/toolr-core/src/parser/build.rs`. Append:

```rust
let django = manifest.commands.iter().find(|c| c.name == "django").unwrap();
assert!(django.is_dispatcher, "expected dispatcher flag set on django");
```

- [ ] **Step 6: Run everything**

```bash
cargo test -p toolr-core --quiet -- --test-threads=1
cargo build --workspace --tests
cargo clippy --workspace --tests -- -D warnings
```

Expected: all green. The two extended tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-core/src/argparse/mod.rs crates/toolr-core/src/parser/build.rs
git commit -m "argparse: mark dispatcher commands via GraftResult.dispatchers"
```

---

### Task 6: `cli.rs::build_group_subtree` reshape + `build_dispatcher_command`

**Files:**

- Modify: `crates/toolr/src/cli.rs:40` (`build_group_subtree`)
- Modify: `crates/toolr/src/cli.rs` — add `build_dispatcher_command` helper
- Modify: `crates/toolr/src/cli.rs` `#[cfg(test)]` — add 3 new tests

- [ ] **Step 1: Write the failing tests**

If `crates/toolr/src/cli.rs` already has a `#[cfg(test)] mod tests {…}` block, extend it. Otherwise add one at the bottom of the file:

```rust
#[cfg(test)]
mod cli_tree_tests {
    use super::*;
    use toolr_core::manifest::{Argument, ArgumentKind, Command, Group, Manifest, Origin};

    fn empty_arg(name: &str, kind: ArgumentKind) -> Argument {
        Argument {
            name: name.into(),
            kind,
            help: String::new(),
            default: None,
            type_annotation: None,
            resolved_type: None,
            allowed_values: vec![],
            path_constraints: None,
            metadata: Default::default(),
        }
    }

    fn dispatcher(name: &str, group: &str, args: Vec<Argument>) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{name}"),
            function: name.into(),
            summary: String::new(),
            description: String::new(),
            arguments: args,
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: true,
        }
    }

    fn child(name: &str, group: &str, dispatcher_module: &str, dispatcher_fn: &str) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: dispatcher_module.into(),
            function: dispatcher_fn.into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: Some(format!("argparse:{group}")),
            is_dispatcher: false,
        }
    }

    fn normal_leaf(name: &str, group: &str) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{name}"),
            function: name.into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }
    }

    fn group(name: &str) -> Group {
        Group {
            name: name.into(),
            title: name.into(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }
    }

    fn build_for(manifest: Manifest) -> clap::Command {
        let groups: Vec<&Group> = manifest.groups.iter().collect();
        let mut children_map: std::collections::HashMap<Option<String>, Vec<&Group>> =
            std::collections::HashMap::new();
        for g in &groups {
            children_map
                .entry(g.parent.clone())
                .or_default()
                .push(g);
        }
        let top: Vec<&Group> = manifest.groups.iter().filter(|g| g.parent.is_none()).collect();
        // Pick the first top-level group — tests only build one group at a time.
        build_group_subtree(top[0], &manifest, &children_map)
    }

    #[test]
    fn dispatcher_hosts_two_grafted_children() {
        let dispatcher_cmd = dispatcher(
            "job",
            "jenkins",
            vec![empty_arg("cpu", ArgumentKind::Optional)],
        );
        let migrate = child("migrate", "jenkins", "tools.job", "job");
        let runserver = child("runserver", "jenkins", "tools.job", "job");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: vec![group("jenkins")],
            commands: vec![dispatcher_cmd, migrate, runserver],
        };

        let jenkins = build_for(manifest);

        // `jenkins` (the group) has exactly one subcommand: `job`. Children
        // are NOT siblings of `job` at the group level.
        let group_subs: Vec<&str> = jenkins.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(group_subs, vec!["job"]);

        let job = jenkins.find_subcommand("job").expect("job under jenkins");
        let job_subs: Vec<&str> = job.get_subcommands().map(|c| c.get_name()).collect();
        let mut sorted = job_subs.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["migrate", "runserver"]);
    }

    #[test]
    fn two_dispatchers_in_one_group_each_host_their_own_children() {
        let build_cmd = dispatcher("build", "docker", vec![]);
        let image_cmd = dispatcher("image", "docker", vec![]);
        let build_child = child("compile", "docker", "tools.build", "build");
        let image_child = child("push", "docker", "tools.image", "image");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: vec![group("docker")],
            commands: vec![build_cmd, image_cmd, build_child, image_child],
        };

        let docker = build_for(manifest);
        let build_sub = docker.find_subcommand("build").unwrap();
        let image_sub = docker.find_subcommand("image").unwrap();

        let build_subs: Vec<&str> = build_sub.get_subcommands().map(|c| c.get_name()).collect();
        let image_subs: Vec<&str> = image_sub.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(build_subs, vec!["compile"]);
        assert_eq!(image_subs, vec!["push"]);
    }

    #[test]
    fn dispatcher_and_normal_leaf_coexist_in_one_group() {
        let dispatcher_cmd = dispatcher("job", "jenkins", vec![]);
        let migrate = child("migrate", "jenkins", "tools.job", "job");
        let status = normal_leaf("status", "jenkins");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: vec![group("jenkins")],
            commands: vec![dispatcher_cmd, migrate, status],
        };

        let jenkins = build_for(manifest);
        let mut group_subs: Vec<&str> = jenkins.get_subcommands().map(|c| c.get_name()).collect();
        group_subs.sort();
        assert_eq!(group_subs, vec!["job", "status"]);

        let job = jenkins.find_subcommand("job").unwrap();
        let job_subs: Vec<&str> = job.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(job_subs, vec!["migrate"]);

        let status_sub = jenkins.find_subcommand("status").unwrap();
        assert_eq!(status_sub.get_subcommands().count(), 0);
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p toolr --quiet cli_tree_tests
```

Expected: the tests fail because the current `build_group_subtree` hoists children as siblings of `job`, not as subcommands of `job`.

- [ ] **Step 3: Reshape `build_group_subtree`**

Locate `crates/toolr/src/cli.rs:40-59`. Replace the body with:

```rust
/// Compute the dotted name a dispatcher is addressable by from the
/// CLI. Mirrors `toolr_core::parser::build::dotted_name`: a command
/// whose `name` matches the leaf segment of its `group` is addressable
/// as the group path itself; otherwise it's `"<group>.<name>"` (or
/// just `name` when the group is empty). `graft_children` sets each
/// grafted child's `group` field to this dotted name (the
/// `attachment.parent`), so we use the same value to look children up.
fn dispatcher_dotted_name(cmd: &toolr_core::manifest::Command) -> String {
    let leaf = cmd.group.rsplit('.').next().unwrap_or(cmd.group.as_str());
    if !cmd.group.is_empty() && cmd.name == leaf {
        cmd.group.clone()
    } else if cmd.group.is_empty() {
        cmd.name.clone()
    } else {
        format!("{}.{}", cmd.group, cmd.name)
    }
}

fn build_group_subtree(
    group: &Group,
    manifest: &Manifest,
    children: &HashMap<Option<String>, Vec<&Group>>,
) -> Command {
    let full_path = group.full_path();
    let mut g = Command::new(group.name.clone()).about(group.title.clone());
    if !group.description.is_empty() {
        g = g.long_about(group.description.clone());
    }

    // Grafted children's `group` field stores the dispatcher's dotted
    // name (the `[[attach]] parent` value), NOT the parent group's
    // name. For each non-grafted command in this group, decide whether
    // it's a dispatcher and, if so, look up its grafted children at
    // its own dotted name. When the dispatcher's name matches the
    // group's leaf (e.g. `command_group("django")` + `def django(...)`)
    // its dotted name equals the group path itself, and the children
    // are hoisted directly onto the group (so users type
    // `toolr django migrate`, not `toolr django django migrate`).
    let group_leaf = full_path.rsplit('.').next().unwrap_or(full_path.as_str());
    for cmd in manifest
        .commands
        .iter()
        .filter(|c| c.group == full_path && c.dispatched_from.is_none())
    {
        if cmd.is_dispatcher {
            let dotted = dispatcher_dotted_name(cmd);
            let dispatched_children: Vec<&toolr_core::manifest::Command> = manifest
                .commands
                .iter()
                .filter(|child| child.group == dotted && child.dispatched_from.is_some())
                .collect();
            if cmd.name == group_leaf {
                // Hoist branch: children become direct subcommands of
                // the group; the dispatcher itself disappears as a
                // redundant CLI hop.
                for child in &dispatched_children {
                    g = g.subcommand(build_user_command(child));
                }
            } else {
                g = g.subcommand(build_dispatcher_command(cmd, &dispatched_children));
            }
        } else {
            g = g.subcommand(build_user_command(cmd));
        }
    }

    if let Some(child_groups) = children.get(&Some(full_path)) {
        for child in child_groups {
            g = g.subcommand(build_group_subtree(child, manifest, children));
        }
    }
    g
}

fn build_dispatcher_command(
    dispatcher: &toolr_core::manifest::Command,
    children: &[&toolr_core::manifest::Command],
) -> Command {
    let mut c = build_user_command(dispatcher).subcommand_required(true);
    for child in children {
        c = c.subcommand(build_user_command(child));
    }
    c
}
```

`HashMap` may already be imported at the top of `cli.rs`; if not, add `use std::collections::HashMap;`. Same for the `toolr_core::manifest::Command` alias — adjust to whatever the existing code uses.

**Important correction vs. the original plan draft:** an earlier revision of this section bucketed grafted children by `c.group == full_path` (the parent group's name). That's wrong — `argparse::graft_children` stores each child with `group = attachment.parent` (the full **dispatcher** dotted name). The fix above looks up children at the dispatcher's `dispatcher_dotted_name(cmd)` value instead, and adds the hoist branch for the `name == group_leaf` case. The corresponding `dispatch.rs` lookup (Task 7) was likewise fixed to find the dispatcher manifest entry by `(module, function) + is_dispatcher` rather than by name guessing.

- [ ] **Step 4: Run tests**

```bash
cargo test -p toolr --quiet cli_tree_tests
cargo build --workspace --tests
cargo clippy --workspace --tests -- -D warnings
```

Expected: 3 new tests pass; everything else still green.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/cli.rs
git commit -m "cli: hoist grafted children into dispatcher subcommands"
```

---

### Task 7: `dispatch.rs` widens path-to-command lookup

**Files:**

- Modify: `crates/toolr/src/dispatch.rs:65-71` (the existing find-cmd block)

- [ ] **Step 1: Read the existing seam**

```bash
sed -n '50,90p' crates/toolr/src/dispatch.rs
```

Note the variables `path`, `leaf_name`, `group_full_path`. The existing lookup:

```rust
let cmd = manifest
    .commands
    .iter()
    .find(|c| c.group == group_full_path && c.name == leaf_name)
    .ok_or_else(|| {
        anyhow::anyhow!("unknown command: {} {leaf_name}", path[..path.len() - 1].join(" "))
    })?;
```

After Task 6 the user can type `toolr jenkins job migrate` (3 segments). `group_full_path` would be `"jenkins.job"` but `migrate.group == "jenkins"`. The current lookup fails.

- [ ] **Step 2: Write a unit test in dispatch.rs**

If `dispatch.rs` has a `#[cfg(test)] mod tests`, append; otherwise add one. The unit test exercises the path-to-command-lookup logic. Since that logic isn't currently factored out, factor it into a free function first:

```rust
fn find_command_for_path<'a>(
    manifest: &'a Manifest,
    path: &[String],
) -> Option<&'a toolr_core::manifest::Command> {
    let leaf_name = path.last()?;
    let candidates: Vec<String> = if path.len() >= 2 {
        vec![
            path[..path.len() - 1].join("."),
            path[..path.len() - 2].join("."),
        ]
    } else {
        vec![String::new()]
    };
    // Try the most-specific group first; first match wins.
    candidates.iter().find_map(|group| {
        manifest
            .commands
            .iter()
            .find(|c| &c.group == group && &c.name == leaf_name)
    })
}
```

Then add tests (still in dispatch.rs):

```rust
#[cfg(test)]
mod path_lookup_tests {
    use super::*;
    use toolr_core::manifest::{Argument, Command, Manifest, Origin};

    fn cmd(name: &str, group: &str) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{name}"),
            function: name.into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }
    }

    fn manifest_with(commands: Vec<Command>) -> Manifest {
        Manifest {
            schema_version: 1,
            static_hash: String::new(),
            dynamic_hash: String::new(),
            groups: vec![],
            commands,
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
        let m = manifest_with(vec![
            cmd("build", "docker"),
            cmd("build", "docker.image"),
        ]);
        let c = find_command_for_path(&m, &parts(&["docker", "image", "build"])).unwrap();
        assert_eq!(c.group, "docker.image");
    }

    #[test]
    fn returns_none_when_no_command_matches() {
        let m = manifest_with(vec![cmd("migrate", "jenkins")]);
        assert!(find_command_for_path(&m, &parts(&["unknown"])).is_none());
    }
}
```

- [ ] **Step 3: Run to verify the new tests fail**

```bash
cargo test -p toolr --quiet path_lookup_tests
```

Expected: compile error or missing-function error on `find_command_for_path`.

- [ ] **Step 4: Add the helper at module level in dispatch.rs**

Paste the `find_command_for_path` function from Step 2 into `crates/toolr/src/dispatch.rs` (top of the file alongside other helpers, or anywhere appropriate).

- [ ] **Step 5: Replace the existing find-cmd block**

The block currently around `dispatch.rs:65-71`. Replace:

```rust
let cmd = manifest
    .commands
    .iter()
    .find(|c| c.group == group_full_path && c.name == leaf_name)
    .ok_or_else(|| {
        anyhow::anyhow!("unknown command: {} {leaf_name}", path[..path.len() - 1].join(" "))
    })?;
```

with:

```rust
let cmd = find_command_for_path(manifest, &path).ok_or_else(|| {
    anyhow::anyhow!("unknown command: {}", path.join(" "))
})?;
```

- [ ] **Step 6: Run all tests**

```bash
cargo test -p toolr --quiet -- --test-threads=1
cargo build --workspace --tests
cargo clippy --workspace --tests -- -D warnings
```

Expected: all green. The 4 new `path_lookup_tests` pass; the existing E2E happy-path test (with 2-segment path) still passes.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr/src/dispatch.rs
git commit -m "dispatch: widen path lookup to handle 3-segment dispatcher paths"
```

---

### Task 8: E2E test exercising dispatcher outer flags

**Files:**

- Modify: `tests/sources/test_e2e.py` — add `test_e2e_dispatcher_outer_flags`

- [ ] **Step 1: Rebuild the toolr binary**

After Tasks 4–7 land, the installed binary in `.venv/bin/toolr` is stale relative to the new tree shape. Refresh it:

```bash
cargo build --release -p toolr
cp target/release/toolr .venv/bin/toolr
.venv/bin/toolr --version
```

- [ ] **Step 2: Write the new test**

Append to `tests/sources/test_e2e.py`:

```python
def test_e2e_dispatcher_outer_flags(tmp_path: Path, toolr_bin: Path) -> None:
    """Dispatcher's --cpu/--ram flags are reachable on the child path.

    Invocation shape: `toolr jenkins job --cpu 5000m migrate --check`.
    The dispatcher writes both its own kwargs (`cpu`) and the
    DispatchCommand payload (`migrate`, `check=True`) to a sidecar.
    """
    tools_py = textwrap.dedent(
        """
        import json
        import os
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        group = command_group("jenkins", "Jenkins", description="Jenkins dispatcher")

        @group.command
        def job(
            ctx: Context,
            *,
            cpu: str = "1000m",
            ram: str = "4Gi",
            dispatched: DispatchCommand,
        ) -> int:
            payload = {
                "cpu": cpu,
                "ram": ram,
                "command": dispatched.command,
                "command_args": dispatched.command_args,
            }
            with open(os.environ["E2E_SIDECAR"], "w") as fh:
                json.dump(payload, fh)
            return 0
        """
    ).strip() + "\n"
    pyproject = textwrap.dedent(
        """
        [project]
        name = "demo-tools"
        version = "0"

        [tool.toolr]
        venv-location = "in-tree"

        [tool.toolr.argparse.django]
        scan_paths = ["apps/*/management/commands/*.py"]

        [[tool.toolr.argparse.django.attach]]
        parent = "jenkins"
        """
    ).strip() + "\n"
    project = _make_project(
        tmp_path,
        "dispatcher-flags",
        tools_py,
        pyproject,
        {
            "apps/x/management/commands/migrate.py":
                'def add_arguments(self, parser):\n    parser.add_argument("--check", action="store_true")\n',
        },
    )

    sidecar = tmp_path / "captured.json"
    env = {**os.environ, "TOOLR_TEST_PYTHON": sys.executable, "E2E_SIDECAR": str(sidecar)}

    subprocess.run(  # noqa: S603
        [str(toolr_bin), "project", "manifest", "rebuild"],
        check=True, cwd=project, env=env,
    )
    result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "jenkins", "job", "--cpu", "5000m", "migrate", "--check"],
        check=False, cwd=project, env=env, capture_output=True, text=True,
    )
    if result.returncode != 0:
        msg = (
            f"dispatcher-outer-flags dispatch failed (exit {result.returncode})\n"
            f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
        )
        raise AssertionError(msg)

    captured = json.loads(sidecar.read_text())
    assert captured["cpu"] == "5000m"
    assert captured["ram"] == "4Gi"  # default value preserved
    assert captured["command"] == "migrate"
    assert captured["command_args"]["check"] is True
```

This reuses the `_make_project` helper added during Plan A Task 19.

- [ ] **Step 3: Run the new test**

```bash
uv run pytest tests/sources/test_e2e.py::test_e2e_dispatcher_outer_flags -v
```

Expected: PASS.

- [ ] **Step 4: Run the full E2E module**

```bash
uv run pytest tests/sources/ -v
```

Expected: all green. 5 E2E tests now (4 from Plan A + 1 new).

- [ ] **Step 5: Commit**

```bash
git add tests/sources/test_e2e.py
git commit -m "tests: e2e dispatcher outer flags reachable via grafted child path"
```

---

## Wrap-up

After Task 8:

- [ ] **Full suite check:**

```bash
uv run pytest -q
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --tests -- -D warnings
uv run prek run --all-files
```

Expected: all green.

- [ ] **Push to PR #222:**

```bash
git-spice branch submit
```

`git-spice` will update the existing PR with the new commits.

- [ ] **Update memory:** Note in `MEMORY.md` that auto-rebuild on missing manifest now Just Works, and that dispatchers host their grafted children as subcommands (with `subcommand_required(true)`).

---

## Out of scope (handled by Plan A, the design, or future work)

- Widening `invoke_dispatcher` to support a dispatcher invoked without a child (`toolr <group> <dispatcher>` alone). Today clap rejects at parse time via `subcommand_required(true)` — clean failure mode.
- Auto-rebuild on a *corrupted* manifest (vs missing). Out of scope; current `load_or_empty` swallows non-`not-found` errors.
- Marking dispatchers from non-argparse sources. The mechanism (`is_dispatcher = true`) is source-agnostic; a future Jenkins plugin just sets the flag the same way.
