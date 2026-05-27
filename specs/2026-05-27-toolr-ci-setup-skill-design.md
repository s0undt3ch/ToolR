# Toolr CI-setup agent skill

**Date:** 2026-05-27
**Status:** design

## Problem

Toolr ships a composite GitHub Action at the repository root
(`action.yml`, "Setup ToolR") that installs the CLI binary,
verifies its SLSA build provenance, caches both the binary and
the per-repo `tools/.venv`, and hands callers a `toolr` on PATH
ready to dispatch. The action is the canonical way to use toolr
in CI — yet none of toolr's documentation surfaces it. It is
referenced once in passing in `docs/project-config.md` (in the
context of the `TOOLR_VENV_LOCATION` override) and otherwise
absent from `docs/installation/`, the README install table, and
both existing agent skills.

The two existing skills cover authoring (`tools/*.py` editing)
and packaging (shipping a plugin wheel). Both are local-loop /
build-time concerns. Neither tells an AI agent how to put what
it just authored into CI, even though every nontrivial change
that involves the packaging skill explicitly recommends a
`toolr self build-manifest --check` gate "on every PR" without
showing how. Agents asked to "wire toolr into CI" today
reinvent the action's inputs from `action.yml` by reading
source, or — worse — generate a workflow that bypasses the
action and reproduces parts of its logic by hand (manual
`curl | sh`, manual cache keys, no attestation verify).

A narrow third skill — strictly about consuming the
`s0undt3ch/ToolR` action in a caller repository's workflows —
gives agents the canonical inputs/outputs, the recommended pin
form, two complete worked recipes, and the same drift-defense
infrastructure the existing skills already use. The skill
inherits the trigger-isolation discipline of authoring and
packaging: it activates on CI intent and stays inert otherwise.

## Goals

- An agent reading the skill can produce a working
  `.github/workflows/*.yml` that uses
  `s0undt3ch/ToolR@<sha>` to run a toolr command, or to gate
  `toolr self build-manifest --check`, without spelunking
  `action.yml`.
- The skill is strictly the action-consumption delta on
  general GitHub Actions knowledge. It does not re-teach
  workflow syntax, `runs-on`, matrices in the abstract, or
  GitHub's permissions model.
- The skill is in lockstep with `action.yml`. A change to the
  action's input or output surface cannot land without a
  corresponding update to `references/action.md` in the same
  PR, enforced by the existing `cargo xtask build-skill-refs
  --check` CI gate.
- The skill ships in-tree at `skills/toolr-ci-setup/` and is
  installable via `skillshare` alongside the existing two
  skills.
- The skill's trigger keeps it inert outside CI / GitHub
  Action contexts; authoring requests do not fire it,
  packaging requests do not fire it except where the
  packaging skill explicitly cross-links.
- Existing skills gain small "CI is a different problem"
  footers pointing to this skill, matching how authoring and
  packaging already cross-reference each other.
- The `docs/skills.md` install pattern simplifies from
  per-skill subpaths to a single `skillshare install
  s0undt3ch/toolr/skills` invocation with picker, so the
  installation block does not grow each time a skill is
  added.

## Non-goals

- Documenting toolr's own internal `.github/actions/*`
  sub-actions (`apply-release-patch`, `configure-git`,
  `setup-pre-commit`, `setup-virtualenv`, `throttle`). Those
  are private plumbing for toolr's release/CI pipeline, not
  a published consumer surface.
- Adding a separate `docs/installation/github-action.md` doc
  page. The skill is the load-bearing reference; whether
  toolr also adds a user-docs page for the action is a
  separate question, possibly a follow-up.
- Coverage of non-action install paths in CI (manual
  `curl | sh` fallback, the `mise-action` route, self-hosted
  runner image baking). Out of scope; the skill names the
  one canonical action and recommends it.
- Matrix-build pedagogy beyond a single multi-OS example in
  the "run a toolr command" recipe. Agents handle matrix
  syntax competently; the skill only demonstrates that the
  action works across Linux / macOS / Windows.
- Modifications to `action.yml` itself. The action is the
  source of truth; the skill consumes it.
- Version-bump or release work for toolr. The skill names
  `0.20.0` as the minimum supported version (matching the
  action's own floor) and uses it in examples.

## Design

### Skill layout

The skill lives at `skills/toolr-ci-setup/` and is distributed
via `skillshare`. It is loaded as a single document with no
runtime dependency on the other skills. Layout mirrors the
existing two skills exactly:

```text
skills/toolr-ci-setup/
├── README.md          # human-readable counterpart to the frontmatter
├── SKILL.md           # the loaded skill body
├── REVIEW.md          # ownership checklist for hand-written surfaces
├── references/
│   └── action.md      # GENERATED from action.yml; drift-checked
└── tests/
    └── triggers.yaml  # best-effort "should activate / should not" fixtures
```

### SKILL.md body sections, in order

1. **What this skill does, what it doesn't.** One paragraph
   each. The "doesn't" paragraph explicitly names: local
   `tools/` setup (authoring's job), wheel-building outside a
   CI gate (packaging's job), non-action install paths,
   toolr's internal sub-actions.
2. **The minimum viable workflow.** A complete copy-pasteable
   `.github/workflows/toolr.yml` (~12 lines) that runs a
   single `toolr <group> <cmd>` step on `ubuntu-latest`.
   Uses the SHA-pinned form with a `# v0.20.0` comment.
3. **Pinning policy.** SHA-pinned with a version comment is
   the recommended default. Tag form (`@v0.20.0`) is
   acceptable for prototypes. Floating-major (`@v0`) is
   explicitly discouraged pre-1.0 because toolr's pre-1.0
   contract permits breaking changes on minor bumps.
4. **Two canonical recipes.**
   - **Run a toolr command in CI** — the §2 workflow scaled
     to a `matrix.runs-on` of `ubuntu-latest`, `macos-latest`,
     `windows-latest`, demonstrating the action's
     cross-platform support. Cross-links back to
     `toolr-command-authoring` for "how do I author the
     command this workflow runs?".
   - **Gate plugin manifests with `--check`** — workflow
     using the action, then
     `toolr self build-manifest --source-dir src/<pkg>
     --package <pkg> --check`. Cross-links back to
     `toolr-command-packaging` for "how do I generate the
     manifest in the first place?".
5. **Inputs and outputs at a glance.** Short prose pointer to
   `references/action.md`. The skill body does NOT reproduce
   the input/output table; that lives in the generated
   reference so it cannot drift.
6. **Common failure modes.** Exactly three:
   - "minimum-version error from a pre-0.20.0 pin" — the
     action's `resolve-version` step rejects anything below
     `0.20.0`; recommend the user upgrade rather than skip
     the check.
   - "attestation verify failed on a fork without `gh`" —
     pass `skip-attestation: true` only knowingly; document
     that this disables the SLSA provenance gate.
   - "venv cache miss every run because `tools/pyproject.toml`
     isn't committed" — the venv cache key hashes
     `tools/pyproject.toml`, `tools/uv.lock`, and `uv.lock`;
     a missing lock file means the cache cannot stabilise.
7. **CI is a different problem footer.** Three- to four-line
   block at the bottom mirroring the existing skills'
   cross-link footers, pointing to authoring and packaging.

### Drift defense

The skill inherits the three-layer drift-defense model from
the authoring spec (see
`specs/archive/2026/2026-05-21-toolr-command-authoring-skill-design.md`).
Infrastructure is shared (`crates/xtask/`, the `REVIEW.md`
ownership pattern); only the source of truth differs.

#### Layer 1 — Prose teaches shape, not specifics

The hand-written `SKILL.md` explains the action in conceptual
terms ("the action installs the binary, verifies its SLSA
attestation, and materialises the tools venv";
"SHA-pinning protects you from upstream tampering";
"`--check` is your gate against drift") and points the agent
at `references/action.md` for the input/output surface.

Hand-written load-bearing surfaces are:

- The skill's frontmatter `description:` (the trigger).
- The opening "what this skill does / doesn't" paragraphs.
- The two complete recipe workflows (intentionally
  hand-written for clarity).
- The pinning-policy guidance.
- The common-failure-mode list.
- The closing cross-link footer.
- The cross-link blocks added to the existing skills'
  `SKILL.md` files.

These get the same `REVIEW.md` ownership treatment as the
existing skills.

#### Layer 2 — `references/action.md` is generated from `action.yml`

`cargo xtask build-skill-refs` (the xtask added by the
authoring spec) gains a third generator entry for this skill.
The new generator regenerates
`skills/toolr-ci-setup/references/action.md` from a single
source of truth: the repository-root `action.yml`.

Implementation outline:

- New module `crates/xtask/src/build_skill_refs/ci_setup.rs`.
  Exposes `pub fn action(root: &Path) -> Result<Generated>`.
- Reads `<root>/action.yml` and parses with a serde-compatible
  YAML crate (no YAML parser is currently in the workspace; the
  plan picks one — `serde_yml`, `serde_norway`, or another
  actively maintained option — and adds it as an `xtask`-only
  dev/runtime dependency, scoped to the xtask crate so the
  rest of the workspace is unaffected).
- Renders a deterministic markdown body containing:
    - The action `name` and `description`.
    - A table of inputs (name, default, description), in the
      declaration order from `action.yml`.
    - A table of outputs (name, description), in declaration
      order.
- Multi-line YAML descriptions collapse to a single line in
  table cells (newlines become spaces) so the rendered table
  remains readable.
- Empty defaults render as `_(empty)_` to make "no default"
  visually distinct from a literal empty string.
- The generator guarantees byte-identical output across runs
  against the same `action.yml`, matching the idempotency
  contract the existing two generators already satisfy.
- Registered in `build_skill_refs/mod.rs::run()` next to
  `authoring::commands`, `authoring::docstrings`, and
  `packaging::packaging`:

  ```rust
  let outputs: Vec<Generated> = vec![
      authoring::commands(&root)?,
      authoring::docstrings(&root)?,
      packaging::packaging(&root)?,
      ci_setup::action(&root)?,   // NEW
  ];
  ```

- No CLI surface changes. `cargo xtask build-skill-refs` and
  `cargo xtask build-skill-refs --check` continue to drive
  all generators uniformly. The existing CI `--check` job
  automatically covers the new file.

#### Layer 3 — `action.yml` is the canonical example, already in tree

The action itself **is** the worked example, already
maintained as load-bearing source code with full CI coverage
(the existing release workflow exercises the action on every
release). The skill's body does not duplicate it; it links
to it.

The skill ships two short *consumer-side* workflow snippets
(the two recipes), which are hand-written load-bearing
surfaces tracked in `REVIEW.md`. They are intentionally short
enough that human review is the right gate; a snapshot test
on workflow YAML would be brittle (the SHA in the pin form
rotates with every action release).

### Trigger description

The frontmatter `description:` must activate on CI-action
intent. Representative trigger phrases:

- "set up toolr in CI"
- "GitHub Actions for toolr"
- "use `s0undt3ch/ToolR` action"
- "wire `toolr self build-manifest --check` into CI"
- "cache toolr in CI"
- "verify SLSA attestation in CI"
- `uses: s0undt3ch/ToolR@…` literal in a workflow file

It must *not* activate on:

- Local authoring (covered by `toolr-command-authoring`).
- Wheel-building or manifest generation outside a CI gate
  (covered by `toolr-command-packaging`).
- Generic GitHub Actions questions in non-toolr projects.
- Toolr's own internal `.github/actions/*` sub-actions.
- The mise-plugin route (separate install path).

The trigger and the cross-link footers are the two surfaces
most likely to overlap with the existing two skills. Both
live in `REVIEW.md` with explicit ownership.

### Cross-links from existing skills

Both existing skills gain small footers pointing to
`toolr-ci-setup`, mirroring how they already cross-reference
each other:

- `skills/toolr-command-authoring/SKILL.md` — gains a "CI
  for these commands" footer (3–4 lines) at the bottom,
  alongside the existing "Packaging is a different problem"
  footer.
- `skills/toolr-command-packaging/SKILL.md` — the existing
  line "Run it on every PR. A prek hook is a good local
  complement." gains a sibling sentence pointing to
  `toolr-ci-setup`. No structural change to the section.

Both additions are hand-written load-bearing surfaces and
get a checklist entry in each skill's `REVIEW.md`.

### `docs/skills.md` updates

Two changes, both small:

1. **Skills table** — add a third row for `toolr-ci-setup`.
2. **Installation block** — replace the current per-skill
   pattern:

   ```sh
   skillshare install s0undt3ch/toolr/skills/toolr-command-authoring
   skillshare install s0undt3ch/toolr/skills/toolr-command-packaging
   ```

   with the parent-path picker pattern:

   ```sh
   # Pick which skills to install (interactive)
   skillshare install s0undt3ch/toolr/skills

   # Or install everything non-interactively
   skillshare install s0undt3ch/toolr/skills --all

   # Or pick by name (e.g. just CI setup)
   skillshare install s0undt3ch/toolr/skills -s toolr-ci-setup
   ```

   This covers humans (interactive picker), scripts (`--all`),
   and CI (`-s <name>` for a specific skill) in one place,
   and removes the per-skill maintenance burden when future
   skills are added.
3. **References-stay-correct bullet list** — add a bullet
   noting `toolr-ci-setup/references/action.md` is rebuilt
   from `action.yml` by `cargo xtask build-skill-refs`.

No changes to the surrounding "Managing installed skills" or
"How the references stay correct" headers themselves.

## Relationship to the existing skills

The three skills are independent at runtime — different
triggers, different references, different problems. They
share:

- The `crates/xtask/` host crate and the `build-skill-refs`
  command (CI setup becomes the third registered generator).
- The `REVIEW.md` checklist pattern for hand-written
  load-bearing surfaces.
- The `docs/skills.md` top-level docs page (cross-links to
  all three).
- The `skillshare` distribution channel.

No skill imports content from another. The cross-link footers
are the only inter-skill references and are hand-written
load-bearing surfaces per skill, owned in each skill's
`REVIEW.md`.

## Removals

None substantive. The `docs/skills.md` Installation block is
rewritten in place (per-skill subpaths → parent-path picker
pattern) but no content disappears that was load-bearing
elsewhere.

## Documentation

- `skills/toolr-ci-setup/README.md` — human-readable
  counterpart to the frontmatter, mirroring the existing
  skills' README structure.
- `skills/toolr-ci-setup/REVIEW.md` — checklist for the
  hand-written load-bearing surfaces (trigger, what
  this/doesn't, recipe workflows, pinning policy, failure
  modes, cross-link footer, plus the cross-link inserts in
  the other two skills).
- `docs/skills.md` — updated table, updated install block,
  updated references-stay-correct list (see above).
- `UNRELEASED.md` — note the third skill, the third
  `build-skill-refs` generator entry, and the
  `docs/skills.md` install-pattern change.

## Testing strategy

Inherits the test families from the two existing specs.
Skill-specific additions:

### Generator idempotency

The existing `crates/xtask/tests/idempotency.rs` already runs
`cargo xtask build-skill-refs` twice and diffs every output.
Registering `ci_setup::action` in `run()` brings the new
output under that test automatically — no new test needed.

### Coverage — every input/output appears in the table

`crates/xtask/tests/coverage.rs` already has an analogous
assertion for `authoring::commands` (every name in
`toolr.__all__` appears in `references/commands.md`). Add a
sibling block: parse `action.yml`, iterate its declared
inputs and outputs, assert each appears in the rendered
`references/action.md`. Catches the "added an input, forgot
to regenerate" case before CI's `--check` gate.

### Fixture-style render test

One new unit test in `ci_setup.rs`: feed a small embedded
YAML string (a stripped-down action with one input and one
output) into the renderer and assert the markdown matches an
inline expected string. Protects the table format against
silent regressions from a future YAML-parser swap or a
refactor of the renderer.

### `--check` gate covers the new file

The existing CI workflow already runs
`cargo xtask build-skill-refs --check`. Registering the new
generator makes that gate cover `references/action.md` with
no workflow change.

### Trigger sanity (best-effort)

Same fixture pattern as the existing skills:
`skills/toolr-ci-setup/tests/triggers.yaml` carries a small
set of "should activate" and "should not activate" intents
("set up toolr in CI" — activates; "add a toolr command" —
does not). Best-effort guardrail; the host skill harness is
the final arbiter.

## Open questions

1. **Whether `docs/installation/github-action.md` lands in
   the same PR.** This spec deliberately scopes to the
   skill. A user-facing docs page covering the same
   action would reuse the generated `references/action.md`
   content but is a separate audience. Defer to the plan or
   to a follow-up; do not block this skill on it.
2. **`prek` hook entry for `toolr self build-manifest
   --check`.** The packaging skill recommends a pre-commit
   hook; whether toolr ships a canonical prek-hook entry
   (separate from the CI gate) is a plan-level call. If we
   ship one, the CI-setup skill's `--check` recipe gains a
   "local complement: prek hook" cross-link. If we don't,
   the skill just names prek in prose.
3. **Versioned skill releases.** `skillshare update` lets
   end users update skills; the toolr release process does
   not currently tag skill changes independently of toolr's
   own version. Whether the action-surface skill needs
   tighter version coupling to the action's `0.20.0` floor
   than the existing two skills have to their respective
   sources is worth thinking about at plan time. The
   current `--check` CI gate is sufficient for in-tree
   correctness; the user-facing question is "if I'm on
   toolr 0.19, will the skill mislead me?" The skill body
   already states `0.20.0` as the floor, matching the
   action's own enforcement.
4. **Sub-action discoverability.** Out of scope per
   non-goals, but the existence of
   `.github/actions/setup-virtualenv` (etc.) is visible to
   anyone reading the toolr repo. Whether the CI-setup
   skill mentions them in a single sentence ("toolr's own
   internal action library — not a published consumer
   surface") to forestall the question is a plan-level call.
