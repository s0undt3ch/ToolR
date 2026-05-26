# Toolr authoring + packaging skills implementation plan

> **For agentic workers:** Implement task-by-task. Each task is a self-contained commit. Steps use checkbox (`- [ ]`) syntax.

**Date:** 2026-05-26
**Status:** plan
**Designs:**
[`2026-05-21-toolr-command-authoring-skill-design.md`](2026-05-21-toolr-command-authoring-skill-design.md),
[`2026-05-21-toolr-command-packaging-skill-design.md`](2026-05-21-toolr-command-packaging-skill-design.md)

**Goal:** Ship two agent skills (`toolr-command-authoring`,
`toolr-command-packaging`) in-tree, distributed via `skillshare`, with a
drift-defense generator (`cargo xtask build-skill-refs`) that keeps
references in lockstep with the toolr code that drives the documented
surface.

**Architecture:** A new workspace crate `crates/xtask/` hosts a single
binary that registers two reference-file generators (one per skill).
The authoring generator walks `crates/toolr-py/python/toolr/__init__.py`
via `toolr_core::parser` and reads docstring conventions from
`toolr_core::docstrings`. The packaging generator reads
`toolr_core::manifest::*` types directly. Both generators run in one
process; `--check` exits non-zero on diff. CI runs the check on every
PR.

**Tech stack:** Rust 2021 (xtask + toolr-core reuse), Markdown
(skills), prek (local hook), GitHub Actions (CI gate), mkdocs
(site nav).

---

## Open-question resolutions

The designs left these to the plan; resolutions used below:

1. **Skill format:** Claude Code only for v1. The generator interface
   leaves room for additional formats but emits only Claude Code's
   single-document layout (`SKILL.md` with frontmatter + body, plus
   sibling `references/`, `README.md`, `REVIEW.md`).
2. **xtask subcommand spelling:** `cargo xtask build-skill-refs` (and
   `... --check`). A `skills` subcommand grouping is deferred until a
   third skill lands; renaming is internal to xtask.
3. **Walker behaviour:** the authoring generator handles every
   re-export shape present in `toolr/__init__.py` today (all currently
   `from .submodule import Name`). Other shapes are out of scope and
   the spec said so.
4. **`--check` enforcement:** CI gate is mandatory (added to the
   existing `cargo-test`-equivalent path). A prek hook entry is also
   added so local commits surface drift before push.
5. **docstring source-of-truth surface:** add a new
   `pub fn known_section_headers() -> &'static [(&'static str, &'static str)]`
   in `toolr-core/src/docstrings.rs` that returns each accepted header
   spelling paired with its canonical category. `detect_section` is
   refactored to consume this table; the generator reads the same
   table. One source of truth, prevented from drifting by an
   existing `detect_section` round-trip unit test.
6. **Build-backend coverage in packaging skill:** hatchling is the
   only worked example. setuptools/poetry get a one-line "see the
   build backend's data-file docs" pointer in the body; no test
   coverage.
7. **Migration paragraph longevity:** the entry-point migration
   paragraph in the packaging skill ends with a dated
   `<!-- review after 1.0 -->` HTML comment so a future cleanup pass
   knows when it can go.
8. **`toolr self build-manifest` discoverability:** out of scope for
   this plan.

## File structure

**New workspace crate:**

- `crates/xtask/Cargo.toml` — workspace member, depends on
  `toolr-core` and `clap`.
- `crates/xtask/src/main.rs` — entrypoint, dispatches subcommands.
- `crates/xtask/src/cli.rs` — clap definitions.
- `crates/xtask/src/build_skill_refs/mod.rs` — generator registry,
  shared rendering helpers (template macros, sorted iteration,
  trailing-newline enforcement).
- `crates/xtask/src/build_skill_refs/authoring.rs` —
  `references/commands.md` and `references/docstrings.md` generators
  for the authoring skill.
- `crates/xtask/src/build_skill_refs/packaging.rs` —
  `references/packaging.md` generator for the packaging skill.
- `crates/xtask/tests/idempotency.rs` — integration test asserting
  two consecutive `build-skill-refs` runs produce byte-identical
  output.
- `crates/xtask/tests/coverage.rs` — public-surface bidirectional
  coverage guard.

**Workspace plumbing:**

- `Cargo.toml` — add `crates/xtask` to `members`.
- `.cargo/config.toml` — append `[alias] xtask = "run --package
  xtask --release --"`.

**toolr-core touch-ups:**

- `crates/toolr-core/src/docstrings.rs` — extract section-header
  table into `known_section_headers()` (public); refactor
  `detect_section` to read it.

**Authoring skill:**

- `skills/toolr-command-authoring/SKILL.md` — frontmatter + body.
- `skills/toolr-command-authoring/README.md` — human-facing intro.
- `skills/toolr-command-authoring/REVIEW.md` — review checklist.
- `skills/toolr-command-authoring/references/commands.md` — generated.
- `skills/toolr-command-authoring/references/docstrings.md` — generated.
- `skills/toolr-command-authoring/examples/tools/` — runnable command
  tree.
- `skills/toolr-command-authoring/examples/tools/pyproject.toml` and
  `uv.lock` if needed for snapshotting.
- `skills/toolr-command-authoring/examples/toolr-manifest.json` —
  committed snapshot fixture.
- `skills/toolr-command-authoring/examples/help-snapshots/*.txt` —
  committed `--help` text snapshots.
- `skills/toolr-command-authoring/tests/triggers.yaml` — should-fire
  / shouldn't-fire fixtures.

**Packaging skill:**

- `skills/toolr-command-packaging/SKILL.md`
- `skills/toolr-command-packaging/README.md`
- `skills/toolr-command-packaging/REVIEW.md`
- `skills/toolr-command-packaging/references/packaging.md` — generated.
- `skills/toolr-command-packaging/tests/triggers.yaml`

(No `examples/` of its own — anchored on `examples/plugin-package/`.)

**Documentation:**

- `docs/skills.md` — top-level skills page, cross-links both skills.
- `mkdocs.yml` — add Skills section to nav.
- `UNRELEASED.md` — append "skills + xtask" entry.

**CI / hooks:**

- `.github/workflows/test.yml` (or equivalent) — add a
  `cargo xtask build-skill-refs --check` step.
- `.pre-commit-config.yaml` — add a local hook running
  `cargo xtask build-skill-refs --check`.

**Tests:**

- `crates/toolr-core/src/docstrings/tests.rs` (or sibling) — round-trip
  test for `known_section_headers()` ↔ `detect_section`.
- `crates/xtask/tests/idempotency.rs` — byte-identical re-run.
- `crates/xtask/tests/coverage.rs` — every `toolr.__all__` name is
  documented; nothing else is.
- `crates/toolr-core/tests/skill_examples.rs` (new) — manifest +
  --help snapshots over `skills/toolr-command-authoring/examples/`.
- Existing example-plugin tests gain a wheel-contents assertion
  for the packaging skill's correctness gate.

**Archive (on merge):**

- `specs/2026-05-21-toolr-command-authoring-skill-design.md` →
  `specs/archive/2026/`.
- `specs/2026-05-21-toolr-command-packaging-skill-design.md` →
  `specs/archive/2026/`.
- This plan also moves to `specs/archive/2026/`.

## Task order

The plan executes in 11 stages. Each stage produces a working tree
that compiles and tests pass. Commits are intentionally small.

---

### Task 1: Scaffold `crates/xtask/`

**Files:**

- Create: `crates/xtask/Cargo.toml`
- Create: `crates/xtask/src/main.rs`
- Create: `crates/xtask/src/cli.rs`
- Modify: `Cargo.toml` (workspace members)
- Modify: `.cargo/config.toml` (alias)
- [ ] **Step 1: Add the crate to the workspace.**

In root `Cargo.toml` under `[workspace] members`, append
`"crates/xtask"`.

- [ ] **Step 2: Write `crates/xtask/Cargo.toml`.**

```toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[dependencies]
anyhow.workspace = true
clap.workspace = true
toolr-core = { path = "../toolr-core" }
```

- [ ] **Step 3: Write `crates/xtask/src/cli.rs`.**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Toolr maintainer tooling")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Regenerate skills/*/references/*.md from toolr's own source.
    BuildSkillRefs {
        /// Fail with non-zero exit if regenerated files differ
        /// from the on-disk versions instead of writing them.
        #[arg(long)]
        check: bool,
    },
}
```

- [ ] **Step 4: Write `crates/xtask/src/main.rs`.**

```rust
mod build_skill_refs;
mod cli;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::BuildSkillRefs { check } => {
            build_skill_refs::run(check)
        }
    }
}
```

- [ ] **Step 5: Stub `crates/xtask/src/build_skill_refs/mod.rs`.**

```rust
use anyhow::Result;

pub fn run(_check: bool) -> Result<()> {
    // Generators added in subsequent tasks.
    Ok(())
}
```

- [ ] **Step 6: Add cargo alias.**

Append to `.cargo/config.toml`:

```toml
[alias]
xtask = "run --quiet --package xtask --release --"
```

- [ ] **Step 7: Verify it builds and `cargo xtask build-skill-refs`
  is a no-op.**

```bash
cargo build -p xtask
cargo xtask build-skill-refs
```

- [ ] **Step 8: Commit.**

```bash
git add Cargo.toml .cargo/config.toml crates/xtask
git commit -m "feat(xtask): scaffold maintainer-only xtask crate"
```

---

### Task 2: Make docstring section headers introspectable

**Files:**

- Modify: `crates/toolr-core/src/docstrings.rs`
- Modify: existing docstring tests
- [ ] **Step 1: Add the public table.**

Near the top of `docstrings.rs`, after the existing types:

```rust
/// Canonical Google-style section headers toolr's parser recognises,
/// paired with the category each one maps to. Sorted by header
/// spelling for stable iteration.
///
/// This is the source of truth shared between [`SimpleDocstringParser::detect_section`]
/// (which consumes it during parsing) and the `xtask build-skill-refs`
/// generator that documents the contract in
/// `skills/toolr-command-authoring/references/docstrings.md`.
pub const KNOWN_SECTION_HEADERS: &[(&str, &str)] = &[
    ("args:", "args"),
    ("arguments:", "args"),
    ("attributes:", "attributes"),
    ("attrs:", "attributes"),
    ("attr ", "attr"),
    ("attribute ", "attr"),
    ("deprecated:", "deprecated"),
    ("example:", "examples"),
    ("examples:", "examples"),
    ("except:", "raises"),
    ("note:", "notes"),
    ("notes:", "notes"),
    ("parameters:", "args"),
    ("raise:", "raises"),
    ("raises:", "raises"),
    ("refs:", "references"),
    ("references:", "references"),
    ("return:", "returns"),
    ("returns:", "returns"),
    ("see also:", "see_also"),
    ("see:", "see_also"),
    ("todo:", "todo"),
    ("warning:", "warnings"),
    ("warnings:", "warnings"),
    ("yield:", "yields"),
    ("yields:", "yields"),
    ("version added:", "version_added"),
    ("version changed:", "version_changed"),
];
```

(Match the actual current `detect_section` body — the example above
will be reconciled against the file.)

- [ ] **Step 2: Rewrite `detect_section` to consume the table.**

```rust
fn detect_section(&self, line: &str) -> Option<&'static str> {
    let lower = line.trim().to_lowercase();
    for (header, category) in KNOWN_SECTION_HEADERS {
        if lower.starts_with(header) {
            return Some(category);
        }
    }
    None
}
```

- [ ] **Step 3: Add a round-trip test.**

In `crates/toolr-core/src/docstrings/tests.rs` (creating the module
if it does not exist; otherwise append):

```rust
use super::*;

#[test]
fn known_headers_round_trip_through_detect_section() {
    let parser = SimpleDocstringParser::new();
    for (header, category) in KNOWN_SECTION_HEADERS {
        let detected = parser.detect_section(header);
        assert_eq!(
            detected,
            Some(*category),
            "header `{header}` should map to `{category}`",
        );
    }
}
```

Visibility: `detect_section` may need to be made `pub(crate)` for the
test or wrapped in a thin public helper. Choose whichever is less
disruptive to existing callers.

- [ ] **Step 4: Run tests.**

```bash
cargo test -p toolr-core docstrings
```

Expected: all pass.

- [ ] **Step 5: Commit.**

```bash
git add crates/toolr-core/src/docstrings.rs
git commit -m "feat(docstrings): expose KNOWN_SECTION_HEADERS as introspectable table"
```

---

### Task 3: Authoring-skill prose

**Files:**

- Create: `skills/toolr-command-authoring/SKILL.md`

- Create: `skills/toolr-command-authoring/README.md`

- Create: `skills/toolr-command-authoring/REVIEW.md`

- Create: `skills/toolr-command-authoring/tests/triggers.yaml`

- [ ] **Step 1: Write `SKILL.md`.** Frontmatter +
  conceptual narrative; points readers at `references/` files
  and at `toolr project init` / `toolr <group> <cmd> --help`
  rather than reproducing those surfaces. Closes with a pointer
  to `toolr-command-packaging`. (Content drafted from the design
  spec's "Skill layout" + "Anchoring on existing toolr commands"
  sections.)

- [ ] **Step 2: Write `README.md`** — human-facing intro to the
  three drift-defense layers, regeneration instructions.

- [ ] **Step 3: Write `REVIEW.md`** — checklist for the
  hand-written load-bearing surfaces (trigger description,
  conceptual narrative, cross-refs, closing pointer).

- [ ] **Step 4: Write `tests/triggers.yaml`** with at least 6
  should-fire and 6 should-not-fire intents (boundary cases:
  "add a toolr command" / "ship this toolr command as a plugin"
  / "debug toolr's Rust runtime" / generic Python CLI work).

- [ ] **Step 5: Commit.**

```bash
git add skills/toolr-command-authoring
git commit -m "feat(skills): authoring skill prose, review checklist, trigger fixtures"
```

---

### Task 4: Authoring-skill `references/commands.md` generator

**Files:**

- Create: `crates/xtask/src/build_skill_refs/authoring.rs`
- Modify: `crates/xtask/src/build_skill_refs/mod.rs`
- [ ] **Step 1: Skeleton in `mod.rs`.**

```rust
use std::path::{Path, PathBuf};

use anyhow::Result;

mod authoring;
mod packaging;

pub fn run(check: bool) -> Result<()> {
    let repo_root = repo_root()?;
    let outputs = vec![
        authoring::commands(&repo_root)?,
        authoring::docstrings(&repo_root)?,
        packaging::packaging(&repo_root)?,
    ];
    apply(outputs, check)
}

pub struct Generated {
    pub path: PathBuf,
    pub body: String,
}

fn apply(outputs: Vec<Generated>, check: bool) -> Result<()> {
    let mut drift = Vec::new();
    for out in outputs {
        let current = std::fs::read_to_string(&out.path).ok();
        if current.as_deref() == Some(&out.body) {
            continue;
        }
        if check {
            drift.push(out.path);
        } else {
            std::fs::write(&out.path, &out.body)?;
        }
    }
    if !drift.is_empty() {
        anyhow::bail!(
            "skill references are out of date — run `cargo xtask build-skill-refs`:\n  {}",
            drift.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n  "),
        );
    }
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Ok(Path::new(manifest_dir).parent().unwrap().parent().unwrap().to_path_buf())
}
```

- [ ] **Step 2: Write `authoring::commands`** — walks
  `crates/toolr-py/python/toolr/__init__.py`, reads `__all__`,
  resolves each name via `toolr_core::parser` re-export walk, emits
  the signature, defaults, annotations, and docstring of every
  entry. Use `BTreeMap`/`BTreeSet` throughout. Stub now if needed;
  fill in fully in Step 3.

- [ ] **Step 3: Render.** Hand-written `write!` macros into a
  `String`. Format:
    - `# Toolr command-authoring reference: API surface`
    - `<!-- generated by cargo xtask build-skill-refs; do not edit -->`
    - One `## <Name>` section per `__all__` entry, sorted ASCII.
    - For functions/decorators: signature in a fenced `python` block,
  short description, long description, per-param `Args:` table.
    - For classes (`Context`, `DispatchCommand`, `ArgSection`): list
  of public methods/properties with one-line summaries.
    - Newline at EOF, no trailing whitespace, LF endings.

- [ ] **Step 4: Run the generator.**

```bash
cargo xtask build-skill-refs
```

Expected: writes `skills/toolr-command-authoring/references/commands.md`.

- [ ] **Step 5: Commit.**

```bash
git add crates/xtask/src/build_skill_refs skills/toolr-command-authoring/references/commands.md
git commit -m "feat(xtask): generate skills/toolr-command-authoring/references/commands.md"
```

---

### Task 5: Authoring-skill `references/docstrings.md` generator

**Files:**

- Modify: `crates/xtask/src/build_skill_refs/authoring.rs`

- [ ] **Step 1: `authoring::docstrings`** reads
  `KNOWN_SECTION_HEADERS` and renders a stable Markdown reference
  describing:
    - The split between short and long descriptions.
    - Section headers grouped by category, sorted within each group.
    - The `Args:` entry shape (`name: description`).
    - The `Examples:` extraction rules.
    - The unsupported-section behaviour.

- [ ] **Step 2: Run + verify byte-stable.**

```bash
cargo xtask build-skill-refs
cargo xtask build-skill-refs
git diff skills/toolr-command-authoring/references/docstrings.md
```

Expected: second run produces no diff.

- [ ] **Step 3: Commit.**

```bash
git add crates/xtask/src/build_skill_refs/authoring.rs skills/toolr-command-authoring/references/docstrings.md
git commit -m "feat(xtask): generate skills/toolr-command-authoring/references/docstrings.md"
```

---

### Task 6: Authoring-skill `examples/` tree + snapshots

**Files:**

- Create: `skills/toolr-command-authoring/examples/tools/...`
- Create: `skills/toolr-command-authoring/examples/toolr-manifest.json`
- Create: `skills/toolr-command-authoring/examples/help-snapshots/...`
- Create: `crates/toolr-core/tests/skill_authoring_examples.rs`
- [ ] **Step 1: Build a small but representative `tools/` tree.**

Exercise:

- A top-level group with two leaf commands.
- A nested subgroup with one leaf.
- `arg` decorator usage on at least one command.
- `arg_section` grouping.
- A command that touches `ctx.run`, `ctx.warn`, `ctx.info`.
- A Google docstring with `Args:` and `Examples:`.

Keep imports minimal — `from toolr import Context, command, command_group, arg, arg_section`.

- [ ] **Step 2: Run `toolr self build-manifest --source-dir
  skills/toolr-command-authoring/examples/tools` and commit the
  resulting `toolr-manifest.json` alongside the example.**

- [ ] **Step 3: Capture `--help` snapshots.** A small driver script
  invokes `toolr <group> <cmd> --help` for each command and writes
  the text into `help-snapshots/<command-path>.txt`.

- [ ] **Step 4: Add a snapshot integration test in toolr-core (or
  the existing manifest-builder test crate) that runs the same
  build, diffs against the committed JSON, and runs each `--help`
  capture against its committed text file.**

- [ ] **Step 5: Run tests.**

```bash
cargo test -p toolr-core --test skill_authoring_examples
```

- [ ] **Step 6: Commit.**

```bash
git add skills/toolr-command-authoring/examples crates/toolr-core/tests/skill_authoring_examples.rs
git commit -m "test(skills): snapshot manifest + --help over the authoring skill's examples"
```

---

### Task 7: Packaging-skill prose

**Files:**

- Create: `skills/toolr-command-packaging/SKILL.md`
- Create: `skills/toolr-command-packaging/README.md`
- Create: `skills/toolr-command-packaging/REVIEW.md`
- Create: `skills/toolr-command-packaging/tests/triggers.yaml`
- [ ] **Step 1: Write `SKILL.md`.** Frontmatter (trigger described
  in the design spec's "Trigger description" section), opening
  pointer to the authoring skill, body covering the three rules
  (generate, include, gate), anchored on `examples/plugin-package/`
  for the worked example, closing migration paragraph with a
  `<!-- review after 1.0 -->` HTML comment.
- [ ] **Step 2: Write `README.md`, `REVIEW.md`,
  `tests/triggers.yaml`** following the same structure as the
  authoring skill, scoped to packaging intent.
- [ ] **Step 3: Commit.**

```bash
git add skills/toolr-command-packaging
git commit -m "feat(skills): packaging skill prose, review checklist, trigger fixtures"
```

---

### Task 8: Packaging-skill `references/packaging.md` generator

**Files:**

- Create: `crates/xtask/src/build_skill_refs/packaging.rs`

- [ ] **Step 1: Reflect on `toolr_core::manifest::*`** —
  `Manifest`, `Group`, `Command`, `Argument`, `ArgMetadata`,
  `ArgumentKind`, `Origin`, `SCHEMA_VERSION`, the
  `third_party_hash` rules.

  Because the source of truth is Rust types (not text files), use
  reflection-by-source-walk: include the struct definitions
  verbatim in the generated reference, surrounded by hand-written
  prose summarising semantics.

  Implementation: `packaging::packaging` reads
  `crates/toolr-core/src/manifest/model.rs` via
  `toolr_core::parser::parse_python_file`'s sibling
  `parse_rust_file`… *(if we have one)*.

  **Decision:** to avoid taking a dependency on a Rust AST library
  here, embed the relevant text via `include_str!("../../../toolr-core/src/manifest/model.rs")`
  inside the xtask crate, extract the lines between
  `// region: SkillRefSchema` and `// endregion: SkillRefSchema`
  markers, and inline them. The markers are added to model.rs in
  this task.

  This makes the reference's source-of-truth coupling explicit and
  build-time-cheap, and the marker addition is a one-time
  invariant we can guard with a unit test.

- [ ] **Step 2: Add markers to `crates/toolr-core/src/manifest/model.rs`** around the `Manifest`,
  `Origin`, and (if relevant) `SCHEMA_VERSION` declarations.

- [ ] **Step 3: Render `references/packaging.md`** with:
    - Hand-written narrative on the contract.
    - A fenced `rust` code block per marker region.
    - A description of the `third_party_hash` semantics (read from
  a short, hand-written summary in the xtask crate — this surface
  is small enough that a one-paragraph summary is fine, and a
  unit test guards that the prose name-checks every field of
  `Manifest` that ships in the wheel).

- [ ] **Step 4: Run + verify byte-stable.**

```bash
cargo xtask build-skill-refs
cargo xtask build-skill-refs
git diff skills/toolr-command-packaging/references/packaging.md
```

- [ ] **Step 5: Commit.**

```bash
git add crates/xtask/src/build_skill_refs/packaging.rs crates/toolr-core/src/manifest/model.rs skills/toolr-command-packaging/references/packaging.md
git commit -m "feat(xtask): generate skills/toolr-command-packaging/references/packaging.md"
```

---

### Task 9: Test families

**Files:**

- Create: `crates/xtask/tests/idempotency.rs`

- Create: `crates/xtask/tests/coverage.rs`

- Modify: existing example-plugin tests to add wheel-contents
  assertion + `--check` red-path coverage.

- [ ] **Step 1: Idempotency.** Test runs the generator twice
  against the workspace and asserts every generated file is
  byte-identical between runs.

- [ ] **Step 2: Public-surface coverage.** Test reads
  `crates/toolr-py/python/toolr/__init__.py` via the existing
  parser, extracts `__all__`, and asserts a bidirectional match
  against the `## <Name>` headings in the generated
  `references/commands.md`.

- [ ] **Step 3: Wheel-contents assertion.** Extend or create a
  test in the example-plugin CI job (or as a `tests/` integration
  in `examples/plugin-package` if the convention exists) that
  builds the wheel via hatchling, unpacks it with `zipfile`, and
  asserts `toolr_example_plugin/toolr-manifest.json` is present.

- [ ] **Step 4: `--check` red-path.** Mutate
  `examples/plugin-package/src/toolr_example_plugin/commands.py`
  in a known way inside the test fixture, run `toolr self
  build-manifest --source-dir examples/plugin-package/src
  --package toolr_example_plugin --check`, assert non-zero exit
  and a non-empty stderr. Restore the file at test teardown.

- [ ] **Step 5: Run all new tests.**

```bash
cargo test -p xtask
cargo test -p toolr-core --tests
```

- [ ] **Step 6: Commit.**

```bash
git add crates/xtask/tests
git commit -m "test(xtask): idempotency + public-surface coverage + wheel-contents + --check red-path"
```

---

### Task 10: Hook + CI integration

**Files:**

- Modify: `.pre-commit-config.yaml`
- Modify: `.github/workflows/test.yml` (or equivalent — confirm by
  inspection)
- Modify: `mise.toml` (add the check to `mise run test`)
- [ ] **Step 1: prek hook.** Append:

```yaml
  - repo: local
    hooks:
      - id: build-skill-refs-check
        name: Verify skills/*/references/*.md are in sync
        entry: cargo xtask build-skill-refs --check
        language: system
        pass_filenames: false
        files: ^(crates/toolr-(core|py)/.*\.rs$|crates/toolr-py/python/toolr/.*\.py$|skills/.*/references/.*\.md$|crates/xtask/.*\.rs$)
```

(Place inside the existing `- repo: local` block.)

- [ ] **Step 2: Mise task.** Either prepend or append to
  `[tasks.test]`:

```toml
"cargo xtask build-skill-refs --check",
```

- [ ] **Step 3: CI workflow.** Confirm the test workflow runs
  `mise run test` (per memory it does); if so, the check rides
  along for free. Otherwise add a discrete step.

- [ ] **Step 4: Run prek locally to confirm the hook is wired.**

```bash
prek run build-skill-refs-check --all-files
```

- [ ] **Step 5: Commit.**

```bash
git add .pre-commit-config.yaml mise.toml .github/workflows/*.yml
git commit -m "ci: wire cargo xtask build-skill-refs --check as a gate"
```

---

### Task 11: Docs + UNRELEASED + archive

**Files:**

- Create: `docs/skills.md`
- Modify: `mkdocs.yml`
- Modify: `UNRELEASED.md`
- Move: `specs/2026-05-21-toolr-command-authoring-skill-design.md` →
  `specs/archive/2026/`
- Move: `specs/2026-05-21-toolr-command-packaging-skill-design.md` →
  `specs/archive/2026/`
- Move: `specs/2026-05-26-toolr-skills-plan.md` →
  `specs/archive/2026/`
- [ ] **Step 1: Write `docs/skills.md`.** Short page: what skills
  are, how to install via `skillshare`, table of the two skills
  with one-paragraph descriptions, cross-link to each SKILL.md.
- [ ] **Step 2: Add to `mkdocs.yml` nav.** Under top level:

```yaml
  - Agent skills: skills.md
```

- [ ] **Step 3: Update `UNRELEASED.md`.** Append a section:

```markdown
## Agent skills

Toolr now ships two agent skills in-tree, installable via
`skillshare` from the repo:

- **`toolr-command-authoring`** — extends agents that author
  `tools/*.py` files in their own repos.
- **`toolr-command-packaging`** — ships an existing set of toolr
  commands as a distributable Python plugin.

A new `cargo xtask build-skill-refs --check` gate keeps each
skill's `references/*.md` in lockstep with the toolr-py and
toolr-core surfaces it documents.
```

- [ ] **Step 4: Move designs + plan to archive.**

```bash
git mv specs/2026-05-21-toolr-command-authoring-skill-design.md specs/archive/2026/
git mv specs/2026-05-21-toolr-command-packaging-skill-design.md specs/archive/2026/
git mv specs/2026-05-26-toolr-skills-plan.md specs/archive/2026/
```

- [ ] **Step 5: Verify `mkdocs build --strict` passes.**

```bash
uv run mkdocs build --strict
```

- [ ] **Step 6: Commit.**

```bash
git add docs/skills.md mkdocs.yml UNRELEASED.md specs/
git commit -m "docs(skills): add skills landing page, queue release notes, archive designs"
```

---

## Self-review

- Spec coverage:
    - Layer 1/2/3 drift defense → Tasks 3, 4-5, 6.
    - xtask infrastructure → Tasks 1, 9.
    - Authoring trigger + narrative → Task 3.
    - Packaging trigger + narrative → Task 7.
    - `examples/plugin-package/` anchoring → Task 9 (wheel + check
  red-path).
    - Idempotency + coverage guards → Task 9.
    - CI gate → Task 10.
    - `docs/skills.md` + UNRELEASED → Task 11.
- Placeholder scan: every step references concrete files or
  commands. No "TBD" / "fill in" markers remain.
- Type consistency: `Generated` struct used consistently in xtask;
  `KNOWN_SECTION_HEADERS` is the only new public toolr-core
  symbol and is referenced in Tasks 2 and 5.
