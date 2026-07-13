# Toolr CI-setup skill implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Date:** 2026-05-27
**Status:** plan
**Design:** [`2026-05-27-toolr-ci-setup-skill-design.md`](2026-05-27-toolr-ci-setup-skill-design.md)

**Goal:** Ship a third agent skill (`toolr-ci-setup`) at
`skills/toolr-ci-setup/`, with a generated `references/action.md`
driven from the repo-root `action.yml` via a new `ci_setup`
generator registered in the existing `cargo xtask
build-skill-refs` pipeline. Add small cross-link footers to the
two existing skills and consolidate the `docs/skills.md` install
block to the parent-path picker pattern.

**Architecture:** Reuse the existing `crates/xtask/` host crate.
Add one new module `crates/xtask/src/build_skill_refs/ci_setup.rs`
that reads `action.yml` via `serde_yml` (active fork of the
deprecated `serde_yaml`; added as an `xtask`-only dependency) and
renders a deterministic markdown body containing the action's
name/description and inputs/outputs tables. Register the new
generator in `mod.rs::run()`. Hand-write `SKILL.md`, `README.md`,
`REVIEW.md`, and a trigger fixture file; the generated
`references/action.md` is committed alongside.

**Tech stack:** Rust 2021 (xtask + new `serde_yml` dep), Markdown
(skill bodies), prek (local hook), GitHub Actions (CI gate via
existing `cargo xtask build-skill-refs --check`).

---

## Open-question resolutions

The spec left four questions to the plan; resolutions used below:

1. **`docs/installation/github-action.md` separate doc page** —
   not in this plan; explicitly deferred. The skill is the only
   consumer-facing surface for the action.
2. **`prek` hook entry for `toolr self build-manifest --check`** —
   not in this plan; the skill body recommends prek in prose but
   ships no new prek-hook entry.
3. **Versioned skill releases** — no special handling. The skill
   body names `0.20.0` as the floor (mirroring the action's
   enforced minimum). Future toolr releases that move the floor
   will update the skill body in the same PR.
4. **Sub-action discoverability** — addressed inline: the skill's
   "what this doesn't do" paragraph names them in one sentence to
   forestall the question, matching the spec's non-goal.

---

## File structure

Files created:

- `skills/toolr-ci-setup/SKILL.md`
- `skills/toolr-ci-setup/README.md`
- `skills/toolr-ci-setup/REVIEW.md`
- `skills/toolr-ci-setup/references/action.md` (generated)
- `skills/toolr-ci-setup/tests/triggers.yaml`
- `crates/xtask/src/build_skill_refs/ci_setup.rs`

Files modified:

- `crates/xtask/Cargo.toml` — add `serde_yml` + `indexmap` deps
- `crates/xtask/src/build_skill_refs/mod.rs` — register
  `ci_setup::action`
- `crates/xtask/tests/coverage.rs` — add bidirectional
  input/output coverage test
- `.rumdl.toml` — exclude the new generated references dir
- `skills/toolr-command-authoring/SKILL.md` — add "CI is a
  different problem" footer
- `skills/toolr-command-authoring/REVIEW.md` — add ownership
  entry for the new footer
- `skills/toolr-command-packaging/SKILL.md` — append cross-link
  sentence to the `--check` paragraph
- `skills/toolr-command-packaging/REVIEW.md` — add ownership
  entry for the new line
- `docs/skills.md` — table row, install block consolidation,
  references-stay-correct bullet
- `UNRELEASED.md` — note the new skill, generator, and
  install-pattern change

---

## Task 1: Add `serde_yml` + `indexmap` deps to xtask

**Files:**

- Modify: `crates/xtask/Cargo.toml`

- [ ] **Step 1: Add the new entries to `[dependencies]`**

Edit `crates/xtask/Cargo.toml`. After the existing `[dependencies]`
block, insert three new lines for `indexmap`, `serde`, and
`serde_yml`. Final block:

```toml
[dependencies]
anyhow.workspace = true
clap.workspace = true
indexmap = { version = "2", features = ["serde"] }
ruff_python_ast.workspace = true
ruff_python_parser.workspace = true
ruff_text_size.workspace = true
serde = { version = "1", features = ["derive"] }
serde_yml = "0.0.12"
toolr-core = { path = "../toolr-core" }
```

`serde_yml` is the actively maintained fork of `serde_yaml`
(archived upstream in 2024). It is API-compatible with the old
crate. `indexmap` preserves YAML declaration order so the
generated table is deterministic. Both are scoped to `xtask` so
the rest of the workspace is unaffected.

- [ ] **Step 2: Resolve the lockfile**

Run from the repo root:

```sh
cargo check --package xtask
```

Expected: succeeds. `Cargo.lock` updates with `serde_yml`,
`indexmap`, and their transitive deps.

- [ ] **Step 3: Commit**

```sh
git add crates/xtask/Cargo.toml Cargo.lock
git commit -m "xtask: add serde_yml + indexmap for upcoming action.yml generator"
```

---

## Task 2: Write the fixture-render unit test and module

**Files:**

- Create: `crates/xtask/src/build_skill_refs/ci_setup.rs`

- [ ] **Step 1: Create the new module file**

Create `crates/xtask/src/build_skill_refs/ci_setup.rs` with this
content:

```rust
//! Generator for the `toolr-ci-setup` skill's `references/action.md`.
//!
//! Source of truth is the repo-root `action.yml`. The generator
//! parses the action's `name`, `description`, `inputs`, and
//! `outputs` tables and renders them as markdown so an agent reading
//! the skill sees the action surface without having to read raw YAML.

use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::Generated;

const DO_NOT_EDIT: &str =
    "<!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->";

#[derive(Deserialize)]
struct Action {
    name: String,
    description: String,
    #[serde(default)]
    inputs: indexmap::IndexMap<String, Input>,
    #[serde(default)]
    outputs: indexmap::IndexMap<String, Output>,
}

#[derive(Deserialize)]
struct Input {
    #[serde(default)]
    description: String,
    #[serde(default)]
    default: Option<String>,
}

#[derive(Deserialize)]
struct Output {
    #[serde(default)]
    description: String,
}

pub fn action(repo_root: &Path) -> Result<Generated> {
    let yaml_path = repo_root.join("action.yml");
    let yaml = std::fs::read_to_string(&yaml_path)
        .with_context(|| format!("reading {}", yaml_path.display()))?;
    let body = render(&yaml).with_context(|| {
        format!("rendering action surface from {}", yaml_path.display())
    })?;
    Ok(Generated {
        path: repo_root.join("skills/toolr-ci-setup/references/action.md"),
        body,
    })
}

fn render(yaml: &str) -> Result<String> {
    let action: Action =
        serde_yml::from_str(yaml).context("parsing action.yml as YAML")?;
    let mut body = String::new();
    body.push_str("# `s0undt3ch/ToolR` action surface\n\n");
    body.push_str(DO_NOT_EDIT);
    body.push_str("\n\n");
    body.push_str("Generated from the repository-root `action.yml`.\n\n");

    writeln!(body, "## Name and description\n").unwrap();
    writeln!(
        body,
        "**{}** — {}\n",
        action.name,
        collapse(&action.description)
    )
    .unwrap();

    writeln!(body, "## Inputs\n").unwrap();
    if action.inputs.is_empty() {
        writeln!(body, "_(none declared)_\n").unwrap();
    } else {
        writeln!(body, "| Name | Default | Description |").unwrap();
        writeln!(body, "| ---- | ------- | ----------- |").unwrap();
        for (name, input) in &action.inputs {
            let default = match input.default.as_deref() {
                None | Some("") => "_(empty)_".to_string(),
                Some(value) => format!("`{value}`"),
            };
            writeln!(
                body,
                "| `{name}` | {default} | {} |",
                collapse(&input.description)
            )
            .unwrap();
        }
        body.push('\n');
    }

    writeln!(body, "## Outputs\n").unwrap();
    if action.outputs.is_empty() {
        writeln!(body, "_(none declared)_\n").unwrap();
    } else {
        writeln!(body, "| Name | Description |").unwrap();
        writeln!(body, "| ---- | ----------- |").unwrap();
        for (name, output) in &action.outputs {
            writeln!(
                body,
                "| `{name}` | {} |",
                collapse(&output.description)
            )
            .unwrap();
        }
        body.push('\n');
    }

    Ok(body)
}

/// Collapse a multi-line YAML scalar into a single line suitable for
/// a markdown table cell: newlines become spaces, runs of whitespace
/// collapse to one, the result is trimmed.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
name: "Setup ToolR"
description: |
  Install the ToolR Rust binary from a GitHub release and
  verify its SLSA build provenance.

inputs:
  version:
    description: |
      ToolR version to install.
    default: ""
  skip-attestation:
    description: "Skip gh attestation verify."
    default: "false"

outputs:
  version:
    description: Resolved ToolR version installed (no leading `v`).
"#;

    #[test]
    fn renders_name_description_inputs_and_outputs() {
        let out = render(FIXTURE).expect("render should succeed");
        let expected = "# `s0undt3ch/ToolR` action surface\n\n\
            <!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->\n\n\
            Generated from the repository-root `action.yml`.\n\n\
            ## Name and description\n\n\
            **Setup ToolR** — Install the ToolR Rust binary from a GitHub release and verify its SLSA build provenance.\n\n\
            ## Inputs\n\n\
            | Name | Default | Description |\n\
            | ---- | ------- | ----------- |\n\
            | `version` | _(empty)_ | ToolR version to install. |\n\
            | `skip-attestation` | `false` | Skip gh attestation verify. |\n\n\
            ## Outputs\n\n\
            | Name | Description |\n\
            | ---- | ----------- |\n\
            | `version` | Resolved ToolR version installed (no leading `v`). |\n\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn empty_default_renders_as_empty_marker() {
        let yaml = "name: t\ndescription: t\ninputs:\n  a:\n    description: x\n";
        let out = render(yaml).expect("render should succeed");
        assert!(out.contains("| `a` | _(empty)_ | x |"));
    }

    #[test]
    fn collapse_normalises_whitespace() {
        assert_eq!(collapse("foo\n  bar\n\tbaz"), "foo bar baz");
        assert_eq!(collapse("  trim me  "), "trim me");
        assert_eq!(collapse(""), "");
    }
}
```

- [ ] **Step 2: Run the new unit tests**

The module isn't yet registered in `mod.rs` (that's Task 3) so
`cargo test` won't include it. Add a temporary `mod ci_setup;`
line to `mod.rs` just to run the tests:

Edit `crates/xtask/src/build_skill_refs/mod.rs` and add
`mod ci_setup;` next to the existing module declarations. (Task 3
makes this permanent and uses it from `run`.)

Then run:

```sh
cargo test --package xtask --lib build_skill_refs::ci_setup
```

Expected: three tests pass —
`renders_name_description_inputs_and_outputs`,
`empty_default_renders_as_empty_marker`,
`collapse_normalises_whitespace`.

- [ ] **Step 3: Commit**

```sh
git add crates/xtask/src/build_skill_refs/ci_setup.rs crates/xtask/src/build_skill_refs/mod.rs
git commit -m "xtask: add ci_setup generator (renders action.md from action.yml)"
```

---

## Task 3: Register the new generator in `run()` and create the references dir

**Files:**

- Modify: `crates/xtask/src/build_skill_refs/mod.rs`
- Modify: `.rumdl.toml`
- [ ] **Step 1: Wire `ci_setup::action` into `run()`**

Edit `crates/xtask/src/build_skill_refs/mod.rs`. Confirm the module
declaration block lists `ci_setup`:

```rust
mod authoring;
mod ci_setup;
mod packaging;
```

Then in `pub fn run`, extend the `outputs` vector with the new
generator call. The vec becomes:

```rust
let outputs: Vec<Generated> = vec![
    authoring::commands(&root)?,
    authoring::docstrings(&root)?,
    packaging::packaging(&root)?,
    ci_setup::action(&root)?,
];
```

- [ ] **Step 2: Exclude the new generated dir from rumdl**

Edit `.rumdl.toml`. The `exclude` array under `[global]` already
lists the existing two skills' references dirs. Add the new one:

```toml
exclude = [
    ".git",
    ".venv",
    "node_modules",
    "target",
    "dist",
    "site",
    "CHANGELOG.md",
    "docs/reference",
    "skills/toolr-command-authoring/references",
    "skills/toolr-command-packaging/references",
    "skills/toolr-ci-setup/references",
    "specs/archive",
]
```

- [ ] **Step 3: Run the generator to create `references/action.md`**

```sh
cargo run --package xtask --quiet -- build-skill-refs
```

Expected: succeeds silently.
`skills/toolr-ci-setup/references/action.md` now exists on disk.

- [ ] **Step 4: Sanity-check the rendered file**

```sh
head -20 skills/toolr-ci-setup/references/action.md
```

Expected output begins with:

```text
# `s0undt3ch/ToolR` action surface

<!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->

Generated from the repository-root `action.yml`.

## Name and description

**Setup ToolR** — Install the ToolR Rust binary ...
```

- [ ] **Step 5: Confirm idempotency with `--check`**

```sh
cargo run --package xtask --quiet -- build-skill-refs --check
```

Expected: exits 0 silently. Failure means non-deterministic
iteration order in the renderer — investigate the generator.

- [ ] **Step 6: Commit registration + rumdl config + generated file together**

```sh
git add crates/xtask/src/build_skill_refs/mod.rs .rumdl.toml skills/toolr-ci-setup/references/action.md
git commit -m "xtask: register ci_setup generator; commit generated action.md"
```

---

## Task 4: Extend `coverage.rs` to assert bidirectional input/output coverage

**Files:**

- Modify: `crates/xtask/tests/coverage.rs`

- [ ] **Step 1: Add a coverage test for `references/action.md`**

Append the following to `crates/xtask/tests/coverage.rs`. The
existing `use std::collections::BTreeSet;` near the top is reused;
do not add a duplicate.

```rust
#[test]
fn references_action_md_covers_every_input_and_output() {
    let workspace = workspace_root();
    let action_yml = workspace.join("action.yml");
    let reference =
        workspace.join("skills/toolr-ci-setup/references/action.md");

    let yaml_source = fs::read_to_string(&action_yml)
        .unwrap_or_else(|e| panic!("reading {}: {e}", action_yml.display()));
    let reference_body = fs::read_to_string(&reference)
        .unwrap_or_else(|e| panic!("reading {}: {e}", reference.display()));

    let declared_inputs = extract_top_level_keys_under(&yaml_source, "inputs");
    let declared_outputs = extract_top_level_keys_under(&yaml_source, "outputs");

    for name in &declared_inputs {
        let needle = format!("| `{name}` |");
        assert!(
            reference_body.contains(&needle),
            "input '{name}' declared in action.yml but missing from references/action.md",
        );
    }
    for name in &declared_outputs {
        let needle = format!("| `{name}` |");
        assert!(
            reference_body.contains(&needle),
            "output '{name}' declared in action.yml but missing from references/action.md",
        );
    }

    let documented = extract_backticked_names_in_tables(&reference_body);
    let declared: BTreeSet<&String> = declared_inputs
        .iter()
        .chain(declared_outputs.iter())
        .collect();
    let documented_set: BTreeSet<&String> = documented.iter().collect();
    let extra: Vec<&&String> = documented_set.difference(&declared).collect();
    assert!(
        extra.is_empty(),
        "names documented in references/action.md but not in action.yml: {extra:?}\n\
         Regenerate with `cargo xtask build-skill-refs` after editing action.yml.",
    );
}

/// Lift the top-level keys under a YAML section like `inputs:` or
/// `outputs:`. Avoids a full YAML parse in the test crate; the
/// generator already parses YAML, this is just a check.
fn extract_top_level_keys_under(source: &str, section: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut in_section = false;
    let header = format!("{section}:");
    for line in source.lines() {
        if line.trim_end() == header {
            in_section = true;
            continue;
        }
        if in_section {
            // A non-indented non-empty line ends the section.
            if !line.starts_with(char::is_whitespace) && !line.is_empty() {
                in_section = false;
                continue;
            }
            // Indented `<name>:` with exactly two leading spaces is a
            // key declaration; deeper indents are nested fields.
            if let Some(rest) = line.strip_prefix("  ") {
                if !rest.starts_with(char::is_whitespace) {
                    if let Some(key) = rest.strip_suffix(':') {
                        keys.push(key.to_string());
                    }
                }
            }
        }
    }
    keys
}

/// Pull every `\`name\`` token that appears in the leftmost column of
/// a markdown table row. The renderer uses the form
/// `| \`<name>\` | ... |` for every input/output row.
fn extract_backticked_names_in_tables(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("| `") {
            if let Some(end) = rest.find('`') {
                names.push(rest[..end].to_string());
            }
        }
    }
    names
}
```

- [ ] **Step 2: Run the new test**

```sh
cargo test --package xtask --test coverage references_action_md_covers_every_input_and_output
```

Expected: PASS.

- [ ] **Step 3: Run the full xtask test suite**

```sh
cargo test --package xtask
```

Expected: every test passes (existing coverage, idempotency, and
the new coverage assertion).

- [ ] **Step 4: Commit**

```sh
git add crates/xtask/tests/coverage.rs
git commit -m "xtask/tests: assert action.yml inputs/outputs match references/action.md"
```

---

## Task 5: Write `skills/toolr-ci-setup/SKILL.md`

**Files:**

- Create: `skills/toolr-ci-setup/SKILL.md`

- [ ] **Step 1: Create the file**

Write the following content to
`skills/toolr-ci-setup/SKILL.md`. The outer fence below is
four backticks so the embedded triple-backtick code blocks render
verbatim.

````markdown
---
name: toolr-ci-setup
description: |
  Wire the `s0undt3ch/ToolR` GitHub Action into a caller
  repository's workflows. Use when setting up toolr in CI;
  when authoring `.github/workflows/*.yml` that runs a toolr
  command; when wiring `toolr self build-manifest --check`
  as a CI gate for a plugin repository; when picking the
  right pin form for `uses: s0undt3ch/ToolR@…`; or when
  debugging the action's minimum-version error, attestation
  verify failures, or persistent venv cache misses. Triggers
  on phrases like "set up toolr in CI", "GitHub Actions for
  toolr", "use the toolr action", "cache toolr in CI",
  "verify SLSA attestation in CI", and literal
  `uses: s0undt3ch/ToolR@` snippets. Stays inert on local
  authoring requests (covered by the `toolr-command-authoring`
  skill), on wheel-building outside a CI gate (covered by
  `toolr-command-packaging`), and on toolr's own internal
  `.github/actions/*` sub-actions.
---

# Setting up toolr in GitHub Actions

You are wiring the `s0undt3ch/ToolR` composite action into a
repository's CI. The action installs the toolr Rust binary, verifies
its SLSA build provenance, caches the binary and the per-repo
`tools/.venv`, and hands the next workflow step a `toolr` on PATH.
Your job is to produce (or modify) a `.github/workflows/*.yml` that
consumes the action correctly.

This skill teaches the **consumer side** of the action. The
authoritative input/output surface lives in
[`references/action.md`](references/action.md), regenerated from
`action.yml` on every release — read it when you need exact defaults
or argument shapes.

## What this skill covers

- Pinning `uses: s0undt3ch/ToolR@<sha>` correctly.
- A minimal one-step workflow that runs a toolr command.
- The two canonical recipes: running a toolr command in CI, and
  gating `toolr self build-manifest --check` for plugin repos.
- The three failure modes a typical caller hits first.

## What this skill does not cover

- Authoring or editing the `tools/*.py` commands the workflow runs —
  see the
  [`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring)
  skill.
- Building or shipping a toolr plugin wheel — see the
  [`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging)
  skill. This skill only covers the *CI gate* side
  (`--check`), not manifest generation itself.
- Non-action install paths in CI (manual `curl | sh` fallback,
  `mise-action`, self-hosted runner image baking). Use the action.
- Toolr's own internal `.github/actions/*` sub-actions
  (`apply-release-patch`, `configure-git`, `setup-pre-commit`,
  `setup-virtualenv`, `throttle`). Those are toolr's release
  plumbing, not a public consumer surface — do not call them from
  external repos.

## The minimum viable workflow

```yaml
name: toolr
on: [push, pull_request]
jobs:
  run:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: toolr <group> <cmd>
```

Replace `<sha>` with the full commit SHA of the release tag and
`<group> <cmd>` with the toolr command you want to run. That is
the whole surface for the common case — the action handles
attestation verification, caching, and venv setup.

## Pinning policy

Recommended default — **SHA-pinned with a version comment**:

```yaml
- uses: s0undt3ch/ToolR@a1b2c3d4e5f6...   # v0.20.0
```

This is the form GitHub's own security guidance recommends and the
form toolr itself uses for upstream actions (see `actions/cache` in
`action.yml`). The version comment lets a human reader (and
Dependabot) match the SHA to a release tag without resolving the ref.

Acceptable for prototypes — **tag-pinned**:

```yaml
- uses: s0undt3ch/ToolR@v0.20.0
```

Easier to write while iterating; trade reproducibility for readability.

**Do not use floating-major** (`@v0`) pre-1.0. Toolr's pre-1.0
contract permits breaking changes on minor bumps, so `@v0` is
effectively `latest` and may silently break a workflow.

The action enforces a minimum version of `0.20.0` (the first
binary-only release shape). Anything below that fails fast with a
clear error.

## Recipe 1 — Run a toolr command in CI

Single-OS form was shown above. For multi-OS coverage (use this when
your toolr commands shell out to platform-specific tooling or you
ship a plugin that must work cross-platform):

```yaml
name: toolr
on: [push, pull_request]
jobs:
  run:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: toolr <group> <cmd>
```

The action ships binaries for `x86_64`/`aarch64` Linux (glibc and
musl), `x86_64`/`aarch64` macOS, and `x86_64` Windows. The right
archive is selected automatically from `RUNNER_OS` + `uname -m`.

For the *authoring* side — how the `tools/<file>.py` defining the
command you're running is structured — see
[`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring).

## Recipe 2 — Gate plugin manifests with `--check`

When you ship toolr commands as a plugin wheel, the committed
`toolr-manifest.json` must match what `toolr self build-manifest`
would produce from the current source. The `--check` flag gives you
a non-zero exit on drift. Wire it as a CI gate so a stale manifest
cannot land:

```yaml
name: toolr-manifest
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: |
          toolr self build-manifest \
            --source-dir src/my_plugin \
            --package my_plugin \
            --check
```

Replace `src/my_plugin` with your plugin source directory and
`my_plugin` with the importable package name.

For the *generation* side — how to produce `toolr-manifest.json`
in the first place, what schema it follows, and how to include it
in the wheel — see
[`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging).

## Inputs and outputs at a glance

The full input/output surface (defaults, descriptions, what each
input controls) lives in
[`references/action.md`](references/action.md). It is regenerated
from `action.yml` on every release, so it cannot drift. Read it when
you need to override caching, point at a different release, or pass
extra `uv sync` flags.

## Common failure modes

- **`refusing to install toolr <ver> — minimum supported version is
  0.20.0`** — the action enforces a `0.20.0` floor because earlier
  releases shipped as a Python package and are no longer compatible.
  Upgrade your pin to `0.20.0` or later; do not try to work around
  the check.
- **`gh attestation verify` fails on a fork** — the action verifies
  the SLSA build provenance of every downloaded archive. On runners
  without `gh` available, set `skip-attestation: true` *only* if you
  understand you are turning off the supply-chain gate. Prefer
  installing `gh` (it's already present on GitHub-hosted runners) or
  pre-baking it into self-hosted runner images.
- **`tools/.venv` cache misses every run** — the venv cache key
  hashes `tools/pyproject.toml`, `tools/uv.lock`, and `uv.lock`. If
  none of those are committed (or if your `tools/` layout is
  non-standard), the key never stabilises. Commit the lock files
  alongside `tools/pyproject.toml`. Local complement to the CI gate:
  the `--check` recipe above works equally well as a prek hook in
  your `pre-commit` config.

## Authoring and packaging are different problems

If you haven't written the toolr commands yet, this skill cannot
help you produce them. Invoke
[`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring)
to write them, then come back here to wire the workflow. For
shipping commands as a distributable plugin, see
[`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging) —
this skill only owns the `--check` gate side.
````

- [ ] **Step 2: Verify the frontmatter parses**

```sh
python3 -c "import re; t=open('skills/toolr-ci-setup/SKILL.md').read(); m=re.match(r'^---\n(.*?)\n---\n', t, re.S); print('frontmatter present:', bool(m))"
```

Expected: `frontmatter present: True`.

- [ ] **Step 3: Commit**

```sh
git add skills/toolr-ci-setup/SKILL.md
git commit -m "skills/toolr-ci-setup: author SKILL.md body and trigger"
```

---

## Task 6: Write `skills/toolr-ci-setup/README.md`

**Files:**

- Create: `skills/toolr-ci-setup/README.md`

- [ ] **Step 1: Create the README**

Write the following to `skills/toolr-ci-setup/README.md` (outer
fence is four backticks to keep nested triple-backtick blocks intact):

````markdown
# `toolr-ci-setup` skill

Agent skill that teaches LLM coding assistants how to wire the
`s0undt3ch/ToolR` GitHub Action into a repository's CI. The skill
is loaded from `SKILL.md` and the sibling `references/action.md`;
this README is for humans browsing the repo.

## Audience

Authors wiring toolr into a GitHub Actions workflow. The skill
assumes the repository already (or will shortly) have `tools/`
scaffolded by `toolr project init`. It is *not* the right skill for:

- **Authoring** the `tools/*.py` commands the workflow runs — that
  is the separate `toolr-command-authoring` skill (sibling
  directory under `skills/`).
- **Packaging** toolr commands as a distributable plugin wheel —
  that is `toolr-command-packaging`. This skill only owns the CI
  gate (`toolr self build-manifest --check`), not manifest
  generation itself.
- **Operating** toolr at runtime, outside CI — out of scope.

## How drift is prevented

The skill follows the same three-layer drift defense as the
existing two skills:

1. **Prose teaches shape.** `SKILL.md` is hand-written and explains
   the action conceptually (what it installs, what it verifies,
   what it caches) and offers two complete recipe workflows. It
   points at `references/action.md` for the input/output surface
   so the prose itself stays small and stable.
2. **`references/action.md` is regenerated from `action.yml`.**
   `cargo xtask build-skill-refs` reads the repo-root `action.yml`
   and renders its name, description, inputs, and outputs as a
   markdown table. The `--check` variant runs in CI on every PR —
   a change to the action's surface that forgets to regenerate
   the reference cannot land.
3. **The action itself is the canonical worked example.** The
   action is already maintained as load-bearing code in this
   repository, exercised by the release workflow on every release.
   The skill points consumers at `s0undt3ch/ToolR@<sha>` rather
   than reproducing the action's logic.

## Regenerating the references

```sh
cargo xtask build-skill-refs            # write
cargo xtask build-skill-refs --check    # fail on drift
```

The check runs in CI and as a `prek` hook. If it fails locally,
run without `--check` and review the diff.

## Files

```text
.
├── SKILL.md                  # frontmatter + body (loaded)
├── README.md                 # this file — human-facing
├── REVIEW.md                 # checklist for hand-written surfaces
├── references/
│   └── action.md             # generated; do not edit
└── tests/
    └── triggers.yaml         # should-fire / shouldn't-fire fixtures
```

## Installation

The skill is distributed via `skillshare` from the toolr repo.
See the toolr docs (`docs/skills.md`) for the user-facing flow.
````

- [ ] **Step 2: Commit**

```sh
git add skills/toolr-ci-setup/README.md
git commit -m "skills/toolr-ci-setup: add README"
```

---

## Task 7: Write `skills/toolr-ci-setup/REVIEW.md`

**Files:**

- Create: `skills/toolr-ci-setup/REVIEW.md`

- [ ] **Step 1: Create the REVIEW checklist**

Write to `skills/toolr-ci-setup/REVIEW.md`:

```markdown
# Review checklist for `toolr-ci-setup`

The input/output reference is generated and guarded by the
`cargo xtask build-skill-refs --check` CI gate. A small set of
**hand-written load-bearing surfaces** is not guarded by the
generator and needs human review whenever it changes. Use this
checklist before landing edits to any of them.

## Hand-written surfaces

1. `SKILL.md` frontmatter `description:` — the trigger.
2. `SKILL.md` body — the conceptual narrative.
3. `SKILL.md` "minimum viable workflow" snippet — copy-pasteable
   YAML; verify it still works against the current action.
4. `SKILL.md` two recipe workflows — Recipe 1 (run a command) and
   Recipe 2 (`--check` gate).
5. `SKILL.md` pinning-policy guidance.
6. `SKILL.md` common-failure-modes list.
7. Cross-references from `SKILL.md` to `references/action.md`.
8. Closing cross-link footer pointing to the authoring and
   packaging skills.
9. `README.md` — human-facing intro.
10. `tests/triggers.yaml` — should-fire / shouldn't-fire fixtures.

## Checklist

When editing any of the surfaces above:

- [ ] **Trigger sanity.** Does the description still list at least
      two concrete CI-flavored phrases (e.g.
      `"set up toolr in CI"`, `"GitHub Actions for toolr"`, the
      literal `uses: s0undt3ch/ToolR@…`)?
- [ ] **No false-positive overlap.** Does the description still
      explicitly disclaim authoring intent (so
      `toolr-command-authoring` wins on `"add a toolr command"`)
      and packaging intent outside the `--check` gate (so
      `toolr-command-packaging` wins on `"ship as a plugin"`)?
- [ ] **Pinning policy intact.** Does the body still recommend
      SHA-pinned with version comment as the default and discourage
      floating-major pre-1.0?
- [ ] **Minimum version still 0.20.0.** Does the body still name
      `0.20.0` as the floor, matching the action's enforced
      minimum in `action.yml`?
- [ ] **Recipes still complete.** Do both recipe workflows still
      parse as valid YAML and reference the action by the
      placeholder SHA form so callers know to substitute?
- [ ] **No reference-content duplication.** Does the body still
      avoid restating the full inputs/outputs table? Those belong
      in `references/action.md` and are regenerated.
- [ ] **Cross-link footer.** Does the closing section still point
      to both the authoring and packaging skills, and are the
      links still valid?
- [ ] **`tests/triggers.yaml`.** Are the shouldn't-fire entries
      scoped to plausible-but-out-of-scope requests (authoring,
      packaging outside the `--check` gate, non-toolr GitHub
      Actions work) rather than nonsense inputs?
- [ ] **No regenerated content edited.** `references/action.md`
      is produced by `cargo xtask build-skill-refs`. If you find
      yourself editing it by hand, stop — the drift-defense
      contract is broken.

## When `action.yml` changes

The reference regenerates itself. The hand-written narrative may
still need updates if:

- A new input is added that materially changes the consumer
  experience (e.g. a new caching toggle, a new auth mode). Mention
  it in the relevant recipe.
- The minimum supported toolr version changes. Update the body's
  "0.20.0" floor and the action's enforcement message.
- A failure mode shifts (e.g. attestation behaviour changes).
  Update the common-failure-modes list.

Otherwise the narrative is independent of the input/output surface
— that's the point of the layered design.
```

- [ ] **Step 2: Commit**

```sh
git add skills/toolr-ci-setup/REVIEW.md
git commit -m "skills/toolr-ci-setup: add REVIEW checklist"
```

---

## Task 8: Write `skills/toolr-ci-setup/tests/triggers.yaml`

**Files:**

- Create: `skills/toolr-ci-setup/tests/triggers.yaml`

- [ ] **Step 1: Create the trigger fixture**

Write to `skills/toolr-ci-setup/tests/triggers.yaml`:

```yaml
# Trigger fixtures for the toolr-ci-setup skill.
#
# These are advisory: the host skill harness's intent matcher is
# the final arbiter. The "should activate" entries capture the
# canonical phrases an agent reading the description must continue
# to match; the "should not activate" entries cover plausible-
# but-out-of-scope requests where the trigger would be wrong.
#
# When the trigger description in SKILL.md changes, the
# should-activate set must still feel correct; the
# should-not-activate set must still feel safe.

should_activate:
  - "Set up toolr in our CI pipeline"
  - "Add a GitHub Actions workflow that runs `toolr ci pytest`"
  - "Use the `s0undt3ch/ToolR` action to install toolr"
  - "How do I pin `uses: s0undt3ch/ToolR@` in our workflow?"
  - "Cache toolr's tools venv between CI runs"
  - "Verify the SLSA attestation when installing toolr in CI"
  - "Wire `toolr self build-manifest --check` as a CI gate"
  - "The toolr action is failing with a minimum-version error"

should_not_activate:
  # Authoring is a separate skill.
  - "Add a toolr command for running migrations"
  - "Extend toolr with a `db reset` subcommand"
  - "Help me write the docstring for this toolr command"

  # Plugin generation (outside the --check gate) is packaging.
  - "Generate `toolr-manifest.json` for our plugin"
  - "Configure hatchling to include the manifest in the wheel"
  - "Publish our toolr plugin to PyPI"

  # Generic GitHub Actions work, unrelated to toolr.
  - "Add a workflow that runs ruff on every PR"
  - "Cache `node_modules` in CI"
  - "Set up a Docker build matrix in Actions"

  # Toolr's own internal sub-actions are not a consumer surface.
  - "Configure `setup-virtualenv` inside toolr's own CI"
  - "Modify `apply-release-patch` for toolr's release workflow"

  # Non-action install paths.
  - "Install toolr on a self-hosted runner via `curl | sh`"
  - "Use the mise-action to set up toolr in CI"
```

- [ ] **Step 2: Verify it parses as YAML**

```sh
python3 -c "import yaml; yaml.safe_load(open('skills/toolr-ci-setup/tests/triggers.yaml'))"
```

Expected: no output (clean exit).

- [ ] **Step 3: Commit**

```sh
git add skills/toolr-ci-setup/tests/triggers.yaml
git commit -m "skills/toolr-ci-setup: add trigger fixture"
```

---

## Task 9: Add cross-link footer to `toolr-command-authoring`

**Files:**

- Modify: `skills/toolr-command-authoring/SKILL.md`
- Modify: `skills/toolr-command-authoring/REVIEW.md`
- [ ] **Step 1: Append a new section to SKILL.md**

Open `skills/toolr-command-authoring/SKILL.md`. The file currently
ends with the "Packaging is a different problem" section (the
last section). After its closing paragraph, append:

```markdown

## CI is a different problem

If the user wants to **run** these commands in GitHub Actions
(a caller workflow that installs toolr, sets up the venv, and
runs `toolr <group> <cmd>`), that is the
[`toolr-ci-setup`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-ci-setup)
skill's job. This skill does not cover the `s0undt3ch/ToolR`
action, pinning policy, or CI cache shapes — invoke the
CI-setup skill for that work.
```

- [ ] **Step 2: Update REVIEW.md to track the new surface**

Edit `skills/toolr-command-authoring/REVIEW.md`. Under the
"Hand-written surfaces" numbered list, the list currently ends at
item 6 (`tests/triggers.yaml`). Insert a new item 4 between the
existing "The closing pointer to the `toolr-command-packaging`
skill." (item 4) and "`README.md`" — renumbering subsequent items
as needed. Final list shape:

```markdown
1. `SKILL.md` frontmatter `description:` — the trigger.
2. `SKILL.md` body — the conceptual narrative.
3. Cross-references from `SKILL.md` to `references/*.md`.
4. The closing pointer to the `toolr-command-packaging` skill.
5. The closing pointer to the `toolr-ci-setup` skill.
6. `README.md` — human-facing intro (lower stakes but still
   hand-written).
7. `tests/triggers.yaml` — should-fire / shouldn't-fire fixtures.
```

Under "Checklist", add the following new bullet right after the
existing "Closing packaging pointer" item:

```markdown
- [ ] **Closing CI-setup pointer.** Does the closing section still
      send CI-flavored intent to the CI-setup skill, and is the
      link still valid?
```

- [ ] **Step 3: Commit**

```sh
git add skills/toolr-command-authoring/SKILL.md skills/toolr-command-authoring/REVIEW.md
git commit -m "skills/toolr-command-authoring: cross-link to toolr-ci-setup"
```

---

## Task 10: Cross-link `toolr-command-packaging` to the new skill

**Files:**

- Modify: `skills/toolr-command-packaging/SKILL.md`
- Modify: `skills/toolr-command-packaging/REVIEW.md`
- [ ] **Step 1: Add a sibling sentence to rule 3 in SKILL.md**

Open `skills/toolr-command-packaging/SKILL.md`. In "The three
rules", rule 3 currently reads:

```markdown
3. **Wire `--check` as a CI gate.** `toolr self build-manifest
   --source-dir <pkg-src> --package <pkg-name> --check` exits
   non-zero when the committed manifest doesn't match what the
   builder would produce from the current source. Run it on every
   PR. A prek hook is a good local complement.
```

Replace it with:

```markdown
3. **Wire `--check` as a CI gate.** `toolr self build-manifest
   --source-dir <pkg-src> --package <pkg-name> --check` exits
   non-zero when the committed manifest doesn't match what the
   builder would produce from the current source. Run it on every
   PR. A prek hook is a good local complement. The
   [`toolr-ci-setup`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-ci-setup)
   skill shows the canonical workflow.
```

- [ ] **Step 2: Update REVIEW.md**

Edit `skills/toolr-command-packaging/REVIEW.md`. Under
"Hand-written surfaces", append a new item 9:

```markdown
9. The inline pointer to the `toolr-ci-setup` skill inside rule 3.
```

Under "Checklist", add this new bullet after the existing
"Closing authoring pointer" item:

```markdown
- [ ] **CI-setup pointer in rule 3.** Does rule 3 still link out
      to the CI-setup skill alongside the prek-hook mention, and
      is the link still valid?
```

- [ ] **Step 3: Commit**

```sh
git add skills/toolr-command-packaging/SKILL.md skills/toolr-command-packaging/REVIEW.md
git commit -m "skills/toolr-command-packaging: cross-link to toolr-ci-setup from rule 3"
```

---

## Task 11: Update `docs/skills.md`

**Files:**

- Modify: `docs/skills.md`

- [ ] **Step 1: Add a row to the Skills table**

In `docs/skills.md`, locate the existing Skills table (two rows
for `toolr-command-authoring` and `toolr-command-packaging`).
After the packaging row, insert:

```markdown
| **`toolr-ci-setup`** | Wiring `s0undt3ch/ToolR` into a caller repo's GitHub Actions workflow. | The action's inputs and outputs, recommended pin form, two canonical recipes (run a command; gate `--check`), common failure modes. |
```

- [ ] **Step 2: Rewrite the Installation block**

The current Installation section reads:

````markdown
## Installation

```sh
# from any directory
skillshare install s0undt3ch/toolr/skills/toolr-command-authoring
skillshare install s0undt3ch/toolr/skills/toolr-command-packaging
```

Substitute your platform's skill-install command if you're not on
`skillshare`; the layout (`SKILL.md` + sibling `references/`) is
Claude Code-compatible and the references files are plain Markdown
that any platform can ingest.
````

Replace it with:

````markdown
## Installation

Skillshare lets you install from the parent path and pick what
you want, so the install command does not grow as the skill set
evolves:

```sh
# Pick which skills to install (interactive)
skillshare install s0undt3ch/toolr/skills

# Or install everything non-interactively
skillshare install s0undt3ch/toolr/skills --all

# Or pick by name (e.g. just CI setup)
skillshare install s0undt3ch/toolr/skills -s toolr-ci-setup
```

Substitute your platform's skill-install command if you're not on
`skillshare`; the layout (`SKILL.md` + sibling `references/`) is
Claude Code-compatible and the references files are plain Markdown
that any platform can ingest.
````

- [ ] **Step 3: Add a bullet to the references-stay-correct list**

The "How the references stay correct" list has three bullets
currently. After the last existing bullet (the `packaging.md`
one), append:

```markdown
- `toolr-ci-setup/references/action.md` is rebuilt from the
  repository-root `action.yml`, so the inputs/outputs surface the
  skill points agents at cannot drift from what the action
  actually accepts.
```

- [ ] **Step 4: Verify mkdocs builds the page with `--strict`**

```sh
toolr docs build --strict
```

If `toolr docs` is not available in this checkout, fall back to:

```sh
uv run --project tools mkdocs build --strict
```

Expected: clean build. If `--strict` reports a broken link or
anchor, fix the affected path or anchor in `docs/skills.md`.

- [ ] **Step 5: Commit**

```sh
git add docs/skills.md
git commit -m "docs/skills: list toolr-ci-setup and consolidate install block"
```

---

## Task 12: Update `UNRELEASED.md`

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Find the right section**

Read `UNRELEASED.md` to locate the appropriate section heading
(typically "Added", "Documentation", or a dedicated "Skills"
subsection if one exists).

- [ ] **Step 2: Add three bullets**

Under the most appropriate heading, insert:

```markdown
- New `toolr-ci-setup` agent skill at `skills/toolr-ci-setup/`,
  installable via `skillshare`. Covers the `s0undt3ch/ToolR`
  GitHub Action: pinning policy, two canonical recipes (run a
  toolr command; gate `toolr self build-manifest --check`), and
  the common failure modes a caller hits first.
- `cargo xtask build-skill-refs` gains a third generator
  (`ci_setup::action`) that rebuilds
  `skills/toolr-ci-setup/references/action.md` from the
  repository-root `action.yml`. The existing `--check` CI gate
  automatically covers the new file.
- `docs/skills.md` install instructions now use the `skillshare`
  parent-path picker pattern (`skillshare install
  s0undt3ch/toolr/skills`) instead of one command per skill, so
  the install block does not grow with each new skill.
```

Match the existing bullet style if the file uses a different
format (numbered, scoped, etc.).

- [ ] **Step 3: Commit**

```sh
git add UNRELEASED.md
git commit -m "UNRELEASED: note toolr-ci-setup skill and install-pattern change"
```

---

## Task 13: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full xtask test suite**

```sh
cargo test --package xtask
```

Expected: every test passes (existing coverage + idempotency +
the new `references_action_md_covers_every_input_and_output`).

- [ ] **Step 2: Confirm the generator output is up to date**

```sh
cargo run --package xtask --quiet -- build-skill-refs --check
```

Expected: exits 0 silently. If it fails, a hand edit slipped into
one of the generated `references/*.md` files (or the generator's
output changed) — run without `--check` and review the diff.

- [ ] **Step 3: Run prek across the whole repo**

```sh
prek run --all-files
```

Expected: every hook passes. `rumdl` and `typos` are the most
likely to flag prose issues; fix any reported problems by editing
the source markdown.

- [ ] **Step 4: Build the docs site with `--strict`**

```sh
toolr docs build --strict
```

(Or `uv run --project tools mkdocs build --strict` if
`toolr docs` is not available.)

Expected: clean build with no warnings or broken links.

- [ ] **Step 5: Smoke-check the skill via `skillshare audit`**

If `skillshare` is on PATH, run:

```sh
skillshare audit skills/toolr-ci-setup
```

Expected: no findings; the new skill passes the same audit the
others do. If `skillshare audit` isn't available, skip this step
— the trigger fixture and the REVIEW checklist are the
load-bearing local gates.

- [ ] **Step 6: Final review commit if any fixes landed in steps 1–5**

```sh
git status
```

If there are uncommitted fixes:

```sh
git add -A
git commit -m "skills/toolr-ci-setup: verification fixes"
```

If `git status` is clean, this step is a no-op.
