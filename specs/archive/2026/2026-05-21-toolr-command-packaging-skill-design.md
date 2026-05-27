# Toolr command-packaging agent skill

**Date:** 2026-05-21
**Status:** design

## Problem

Plugin authors shipping toolr commands as distributable Python
packages need to do three toolr-specific things on top of
otherwise-normal Python packaging:

1. Generate `toolr-manifest.json` for the package via
   `toolr self build-manifest --source-dir <dir> --package
   <name>` (the pure-Rust build path that replaced the
   removed `toolr.build` Python module in PR #235).
2. Include that file in the built wheel so toolr's loader
   can discover it under `<venv>/lib/python*/site-packages/
   <package>/toolr-manifest.json`.
3. Wire `toolr self build-manifest ... --check` as a CI gate
   to prevent the committed manifest from drifting out of
   sync with the source.

Each of these is mechanical, but each is also a place where a
working set of commands silently produces a broken package:
manifest forgotten, manifest stale, manifest present but not
included in the wheel. AI coding agents asked to "ship this as
a plugin" today have no canonical reference for these
toolr-specific moves and routinely produce wheels that install
cleanly but expose no commands.

A narrow agent skill — strictly the toolr-specific delta on
regular Python packaging — gives agents the three rules above
and a worked example, without re-teaching pyproject.toml,
build backends, or PyPI publishing (which agents already
handle competently).

The skill targets one audience: developers packaging
already-written toolr commands as a distributable Python
plugin. It has no runtime dependency on the
`toolr-command-authoring` skill but shares the same drift-
defense infrastructure and is distributed via the same
mechanism (`skillshare` from the toolr repo).

## Goals

- An agent reading the skill can take an existing set of
  toolr commands and ship them as a working pip-installable
  plugin without spelunking toolr docs.
- The skill is strictly a delta on regular Python packaging.
  It does not re-teach what a build backend is, how to
  configure `pyproject.toml`, or how to publish to PyPI.
- The skill is in lockstep with the manifest schema and the
  plugin-loader semantics. A change to either cannot land
  without a corresponding update to this skill's
  `references/packaging.md` in the same PR.
- The skill ships in-tree at `skills/toolr-command-packaging/`
  and is installable via `skillshare`.
- The skill's trigger keeps it inert outside packaging
  contexts and outside toolr projects.
- The skill anchors on the existing `examples/plugin-package/`
  (added by PR #235 as the canonical reference plugin —
  hatchling-built, ships `toolr-manifest.json` in the wheel,
  exercised by CI) instead of reproducing scaffold content.

## Non-goals

- Authoring toolr commands. That is covered by
  `toolr-command-authoring`. The skill's body opens with a
  pointer back to that skill for users who haven't written
  commands yet, but the trigger does not activate on
  authoring intent.
- Generic Python packaging fundamentals (build backends,
  wheel layout, PyPI publishing, version pinning). Agents
  already handle these competently; re-teaching them would
  bloat the skill and drift against the broader Python
  ecosystem.
- Documenting the legacy `[project.entry-points.'toolr.
  commands']` mechanism beyond a single migration note. That
  mechanism is removed by the `dispatch_manifest_freshness`
  work landing on `main`; new plugins use `toolr-manifest.
  json` exclusively.
- Operating, debugging, or troubleshooting installed plugins
  at runtime. Out of scope; possible follow-up.

## Design

### Skill layout

The skill lives at `skills/toolr-command-packaging/` and is
distributed via `skillshare`. It is loaded as a single
document with a short opening pointer ("if you haven't written
toolr commands yet, see the `toolr-command-authoring` skill
first") and no runtime dependency on the authoring skill.

The skill covers, in order:

- The packaging contract: a toolr plugin is a Python package
  with a static `toolr-manifest.json` shipped at its
  installed-package root. toolr's loader globs every
  `<venv>/lib/python*/site-packages/<package>/
  toolr-manifest.json` at dispatch time (freshness work, PR
  #234); no entry-point registration is required or
  supported.
- Generating the manifest: `toolr self build-manifest
  --source-dir <dir> --package <name>` (pure-Rust path
  since PR #235; the older `toolr.build` Python module is
  gone). The command writes `toolr-manifest.json` into the
  package source tree.
- Including the manifest in the wheel: build-backend-
  specific configuration. Hatchling is the canonical worked
  example because that is what `examples/plugin-package/`
  uses (`[tool.hatch.build.targets.wheel] packages = [
  "src/<package>"]` — hatchling ships every file inside the
  named directory, including non-`.py` files like the
  committed `toolr-manifest.json`).
- Keeping the manifest in sync: `toolr self build-manifest
  --source-dir <dir> --package <name> --check` as a
  pre-commit hook and a CI gate.
- Verifying after install: how to confirm the manifest made
  it into the wheel (`python -c "import <pkg>; print(<pkg>
  .__path__)"` should contain `toolr-manifest.json`); how to
  confirm commands appear in `toolr --help`.
- Migration from entry-point plugins: one-paragraph note for
  authors of pre-PR-#234 plugins, consistent with the
  `UNRELEASED.md` migration text.

### Drift defense

The skill inherits the three-layer model from the authoring
skill (see
`specs/2026-05-21-toolr-command-authoring-skill-design.md`).
Infrastructure is shared (`crates/xtask/`, the `examples/`
snapshot framework); only the sources, generators, and
examples differ.

#### Layer 1 — Prose teaches shape, not specifics

Hand-written `.md` files explain the packaging contract in
conceptual terms ("your package ships a static JSON manifest
toolr discovers via glob at install time"; "the build
backend's job is to include that JSON in the wheel"; "the
`--check` flag is your CI gate against staleness") and point
the agent at `references/packaging.md` for the manifest
schema and loader semantics.

The hand-written load-bearing surfaces are:

- The skill's frontmatter `description:` (the trigger).
- The opening pointer back to the authoring skill.
- The conceptual narrative of the skill body.
- The closing migration note for entry-point plugins.
- Cross-references between body prose and
  `references/packaging.md`.

These get the same `REVIEW.md` ownership treatment as the
authoring skill.

#### Layer 2 — `references/packaging.md` is generated from toolr itself

`cargo xtask build-skill-refs` (the xtask added by the
authoring spec) gains a second generator entry for this
skill. The new generator regenerates
`skills/toolr-command-packaging/references/packaging.md`
from:

- The `Manifest` struct's serde metadata in
  `crates/toolr-core/src/manifest/` — field names, types,
  semantics, required vs optional, the `Origin` enum
  including `Origin::ThirdParty`.
- The `third_party_hash` computation rules from the freshness
  module added in `dispatch_manifest_freshness`.
- The plugin-loader glob behavior (which paths under
  `site-packages/` are scanned, how merging works, what
  happens on collision).

The packaging generator reads its source of truth directly
from xtask's `toolr-core` dependency — no file scan, no AST
parse. The manifest schema and plugin-loader semantics are
pure Rust types and constants, so the generator can use
them in-process. (The authoring skill is also a single
Rust process, scanning toolr-py source files via
`ruff_python_parser`; the two generators differ in their
source of truth — toolr-core types vs. toolr-py source text
— not in their process model.)

The same `--check` invocation handles both skills:
`cargo xtask build-skill-refs --check` iterates over every
registered generator, fails CI if any committed
`references/*.md` is out of date.

#### Layer 3 — `examples/plugin-package/` is the canonical example, already in tree

The canonical worked example **already exists** at the
repository root: `examples/plugin-package/` was added by
PR #235 as the reference third-party plugin. It is
hatchling-built, ships a committed `toolr-manifest.json`
under `src/toolr_example_plugin/`, is wired into CI to
build once and reuse via the wheelhouse, and is the
fixture the manifest-builder tests already snapshot
against. The packaging skill does **not** ship a separate
duplicate; it points agents at this directory by path.

This collapses the "skill example" and "test fixture"
into one artifact serving both purposes — the property we
wanted from the start, now realised. Concretely:

- The skill's body references `examples/plugin-package/`
  as the canonical reference (no `skills/toolr-command-
  packaging/examples/` of its own).
- Existing tests that snapshot the manifest against this
  example are reused as the skill's correctness gate. The
  same fixture file (`src/toolr_example_plugin/
  toolr-manifest.json`) is the source of truth.
- A new wheel-build test (if not already present in CI's
  example-plugin job) runs the build backend, unpacks the
  resulting wheel, and asserts `toolr-manifest.json` is at
  the package root inside the wheel. This catches the
  "manifest forgotten in build configuration" failure mode
  the skill is most explicitly designed to prevent.
- A new staleness test introduces a known modification to
  the example's source and asserts `toolr self
  build-manifest ... --check` exits non-zero.

If a refactor breaks the example, the snapshot fails and
the author must update the example or back out the
change. The skill cannot ship a broken example because the
example is not separately maintained.

### Anchoring on the canonical example

The skill consistently directs the agent at existing toolr
UX and at the canonical example plugin:

- "Run `toolr self build-manifest --source-dir <dir>
  --package <name>` to generate the manifest" — not a
  reproduction of manifest internals.
- "Look at `examples/plugin-package/` for a complete
  worked example: its `pyproject.toml` shows the canonical
  hatchling wheel-include configuration; CI runs it
  end-to-end" — not a reproduction of either.
- "Verify after install: `python -c 'import <pkg>; print
  (<pkg>.__path__)'` should contain `toolr-manifest.json`;
  `toolr --help` should list your commands" — not a
  reproduction of the loader.

This keeps the skill small and makes it self-correcting:
if `examples/plugin-package/` improves, the skill rides
along for free.

### Trigger description

The frontmatter `description:` must activate on packaging-
flavored intent:

- "ship toolr commands as a package"
- "publish a toolr plugin"
- "include `toolr-manifest.json` in the wheel"
- "toolr plugin pyproject.toml"
- legacy: "toolr.commands entry point" (for migration intent)

It must *not* activate on:

- Authoring (covered by the authoring skill, separate
  trigger).
- Generic Python packaging in non-toolr projects.
- Running, debugging, or troubleshooting installed plugins.
- Working with the manifest from the user side (covered by
  the authoring skill's body, which mentions the manifest in
  the context of `tools/`).

The trigger and the opening pointer to the authoring skill
are the two surfaces most likely to false-positive into
overlapping authoring territory. Both live in `REVIEW.md`
with explicit ownership.

## Relationship to the authoring skill

The two skills are independent at runtime — different
triggers, different references, different examples. They
share:

- The `crates/xtask/` host crate and the `build-skill-refs`
  command (the packaging skill is the second generator
  registered with the xtask).
- The `examples/` snapshot-test conventions and the testing
  harness.
- The `REVIEW.md` checklist pattern for hand-written
  load-bearing surfaces.
- The `docs/skills.md` top-level docs page (cross-links to
  both).

Neither skill imports content from the other. Pointers
between them are the only cross-references and they are
hand-written load-bearing surfaces per skill, owned in each
skill's `REVIEW.md`.

## Removals

None. The skill is net-new.

The migration of pre-`dispatch_manifest_freshness` plugins
away from the `[project.entry-points.'toolr.commands']`
mechanism is **documented in this skill** but **performed by
the freshness work**, not by this spec. The skill carries one
paragraph that mirrors the `UNRELEASED.md` migration note.

## Documentation

- `skills/toolr-command-packaging/README.md` — human-readable
  counterpart to the frontmatter, mirroring the authoring
  skill's README structure.
- `skills/toolr-command-packaging/REVIEW.md` — checklist for
  the hand-written load-bearing surfaces (trigger, opening
  pointer, conceptual narrative, closing migration note,
  cross-references).
- `docs/skills.md` (introduced by the authoring spec) — adds
  a section for the packaging skill and cross-links to it
  alongside the authoring skill.
- `UNRELEASED.md` — note the second skill, the second
  `build-skill-refs` generator entry, and the new CI gates
  (the example-wheel-build test, the staleness test).

## Testing strategy

Inherits the test families from the authoring spec.
Additional or differently-shaped tests for this skill:

### Generator idempotency

- **Idempotency.** The `cargo xtask build-skill-refs`
  integration test in `crates/xtask/tests/` (introduced by
  the authoring spec) extends its byte-identical assertion
  to cover `references/packaging.md` as well as
  `references/commands.md`. Running the xtask twice must
  produce byte-identical output for both files.

### References generation — `cargo xtask build-skill-refs --check`

Same command, now covers both skills. The packaging skill
contributes an additional unit test: a fixture change to the
`Manifest` struct's serde metadata produces a known diff in
`references/packaging.md`.

### Example manifest — `toolr self build-manifest` snapshot diff

`toolr self build-manifest --source-dir
examples/plugin-package/src --package toolr_example_plugin`
runs against the canonical example; the result is diffed
against the committed
`examples/plugin-package/src/toolr_example_plugin/
toolr-manifest.json`. Mismatch fails CI. The
manifest-builder tests already snapshot against this fixture
post-PR-#235; the packaging skill simply consumes the same
guarantee.

### Example wheel build

Hatchling runs against `examples/plugin-package/`. The
resulting wheel is unpacked and asserted to contain
`toolr-manifest.json` at the package root inside the wheel
(i.e. under `toolr_example_plugin/`). This test catches the
"manifest forgotten in build configuration" failure mode
that the skill is most explicitly designed to prevent. CI
already builds the example plugin once per run (the
`toolr-plugin-example` job added by PR #235); this test
adds the wheel-contents assertion on top of that existing
build artifact.

### Example staleness — `--check` red-path

The example is modified in a known way (a new command added
to source without regenerating the manifest) and `toolr
self build-manifest --source-dir ... --package ... --check`
is asserted to exit non-zero. This gives the prose's "wire
`--check` as a CI gate" recommendation teeth.

### Trigger sanity (best-effort)

Same fixture pattern as the authoring skill: a small set of
"should activate" / "should not activate" intents in
`skills/toolr-command-packaging/tests/triggers.yaml`. Best-
effort guardrail; the host skill harness remains the final
arbiter.

## Open questions

1. **Additional build backends in the skill body.** The
   canonical example uses hatchling (confirmed via
   `examples/plugin-package/pyproject.toml`). The skill
   body can mention setuptools and poetry with pointers to
   their canonical "include data file in wheel" docs, but
   only hatchling is exercised by tests. Defer the breadth
   question to the plan.
2. **Pre-commit hook integration.** The skill recommends
   `toolr self build-manifest ... --check` as a pre-commit
   hook. Whether toolr ships a `prek` hook entry for this
   (alongside its existing entries) or just documents the
   pattern is a plan-level call. The spec's commitment is
   that the recommendation is in the skill body and tested
   by the example.
3. **Migration note longevity.** The migration paragraph for
   pre-`dispatch_manifest_freshness` plugins is useful for
   the next few months but becomes deadweight a year out.
   Worth a dated note ("as of 2026-Q2; remove after
   1.0 release") or a follow-up tracking issue. Defer to
   the plan.
4. **`toolr self build-manifest` discoverability.** Plugin
   authors need to know this command exists before they
   reach the skill. The skill body opens by naming it; the
   `toolr project init` scaffolder for plugin projects (if
   such a thing exists or is planned) should also mention
   it. Out of scope for this skill but worth flagging.
