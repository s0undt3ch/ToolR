# Toolr command-authoring agent skill

**Date:** 2026-05-21
**Status:** design

## Problem

AI coding agents that interact with toolr today have to rediscover
the command-authoring surface from `--help` text and source-code
spelunking on every session. That is expensive (latency + tokens),
unreliable (an agent confidently misuses a decorator name or invents
a `ctx` method that does not exist), and produces no compounding
value — the next session starts from scratch.

A shareable agent skill that bundles the authoring surface with
toolr — and ships in lockstep with API changes — gives agents a
single, version-pinned reference and gives toolr's UX a real "AI
agents are a first-class consumer" story. Installation is via
`skillshare` (or whatever skill-distribution mechanism the host
platform exposes), so users can opt in to the skill the same way
they opt in to any other.

The skill targets one audience: users adding `tools/*.py` files
in their own repo to extend toolr with project-specific commands.
This is the common case for toolr users and the natural starting
point for an AI-first authoring story.

A separate, narrower skill for packaging toolr commands as a
distributable Python plugin is specced alongside this one; see
"Related work" below.

## Goals

- An agent reading the skill can author a correct `tools/*.py`
  command without spelunking toolr's Python or Rust source.
- The skill is in lockstep with the public command-authoring
  surface. It is impossible to land a toolr change that mutates
  that surface without a corresponding skill update in the same
  PR.
- The skill ships in-tree with toolr, versioned alongside the
  API it documents, and is installable via `skillshare` from the
  toolr repository.
- The skill's trigger keeps it inert in projects that do not use
  toolr — no false-positive activations in unrelated repos.
- The skill anchors on existing toolr UX (`toolr project init`
  as the starting point) instead of duplicating scaffolding
  content that would rot.
- The drift-defense infrastructure introduced here
  (`cargo xtask build-skill-refs`, the `examples/` snapshot
  framework) is designed to scale to additional skills (such as
  the planned packaging skill) without re-architecture.

## Non-goals

- Packaging toolr commands as a distributable Python plugin.
  That is a separate, narrower skill with its own trigger and
  its own spec
  (`2026-05-21-toolr-command-packaging-skill-design.md`).
- Documenting toolr Rust internals or the manifest builder.
  Agents authoring commands do not need this; toolr maintainers
  do not read skills.
- A skill for agents *operating* (running, debugging) existing
  commands. Out of scope; possible follow-up if the surface ever
  warrants it.
- Cross-platform skill-format parity. We target Claude Code's
  skill format first. Copilot CLI and Gemini equivalents follow
  if and when the platform shapes settle. The drift-defense
  design does not preclude multi-format output from the same
  generators.

## Design

### Skill layout

The skill lives in-tree at `skills/toolr-command-authoring/`
and is distributed via `skillshare` from there. It is loaded as
a single document. A short pointer at the end of the skill
("if you're shipping these as a distributable package, see the
separate `toolr-command-packaging` skill") links to the
packaging skill but creates no cross-skill dependency in the
runtime contract.

The skill covers:

- `tools/` discovery and the project-root model.
- The decorator surface: `command_group`, `command`, parameter
  decorators, type-hint binding rules.
- `ctx` — what is on it, when mutation is safe, how child
  commands inherit context.
- Help text and grouping conventions.
- The manifest from the user's point of view (dispatch auto-
  rebuilds since the freshness work; fall back to
  `toolr project manifest rebuild` on older toolr).
- The local feedback loop: `toolr <group> <cmd> --help`,
  iterating on a `tools/*.py` file, what error messages look
  like.

### Drift defense: three layers

Drift is unavoidable in prose, so the design pushes load-bearing
content out of prose and into generated and tested artifacts.

#### Layer 1 — Prose teaches shape, not specifics

The skill's hand-written `.md` files explain the authoring model
in conceptual terms ("a command is a class with decorators",
"parameters bind via type hints", "ctx flows from parent to
child") and, for the actual surface (decorator names, `ctx`
methods, manifest fields), point the agent at `references/`.
The smaller the prose claim, the less can rot.

The hand-written load-bearing surfaces are:

- The skill's frontmatter `description:` (the trigger).
- The conceptual narrative of the skill body (the model, not
  the specifics).
- Cross-references between body prose and `references/` files.
- The closing pointer to the packaging skill.

Everything else is generated.

#### Layer 2 — `references/` is generated from toolr itself

A new `cargo xtask build-skill-refs` command regenerates files
under `skills/toolr-command-authoring/references/` from the
same sources that already drive `--help`, the manifest schema,
and existing toolr documentation. A CI check
(`cargo xtask build-skill-refs --check`, mirroring `--check`
on `build-manifest`) exits non-zero if the working tree is
dirty after running it.

Effect: a PR that mutates the public command-authoring surface
without regenerating refs cannot land. There is no path by
which the skill silently falls out of date.

For this skill the generated reference is `references/
commands.md` — decorator inventory, `ctx` surface, type-hint
binding rules.

The generator is designed so that adding a future skill (e.g.
packaging) just adds another generator entry; the `--check`
command iterates over every registered skill's references.
Each skill's references are gated on the code that drives
*that* skill's surface; cross-talk is zero.

#### Where the generator lives — `xtask`

`build-skill-refs` is a maintainer-only tool. It operates on
`skills/<name>/references/`, those directories only exist
inside the toolr repo, and end users never need to regenerate
skill references (they consume them via `skillshare`).
Shipping the command in the released `toolr` binary would add
dead weight: binary size, `--help` surface, and discoverability
noise for end users who can do nothing useful with it.

The generator therefore lives in a new workspace crate
`crates/xtask/`, following the established Rust `xtask`
pattern. The crate is **not published** and is not built into
release artifacts. A `.cargo/config.toml` alias makes
`cargo xtask <subcommand>` work directly. CI runs
`cargo xtask build-skill-refs --check`; the released `toolr`
binary is unaffected.

The xtask crate is also the precedent for future maintainer-
only tooling. `build-release-manifest.py` at the repo root is
a candidate for eventual migration, but that is out of scope
for this spec.

#### Generator architecture

`cargo xtask build-skill-refs` is a single Rust process. It
does not spawn a Python subprocess and does not require
toolr-py to be importable; it reads toolr-py's source files
lexically and parses them with `ruff_python_parser`, the
same crate toolr already uses for the static AST scan of
`tools/*.py`.

This is appropriate here because the surface we want to
document is statically declared:

- toolr's decorators (`command_group`, `command`, parameter
  decorators) are top-level `def` statements in known
  toolr-py modules.
- `Ctx` is a regular class with regular methods and
  properties.
- The information we want to render — name, parameter list
  including defaults and annotations, docstring — is exactly
  what AST gives us. No dynamic dispatch, no runtime-
  computed members, no need to resolve type annotations to
  values.

The dynamic-pattern concern that motivates a runtime
introspection subprocess for *user* `tools/*.py` does not
apply to toolr's own API surface.

**The full pipeline.**

1. **Locate source files.** A small constants block in the
   xtask lists the toolr-py source files that contain the
   public command-authoring surface (the decorator module,
   the `Ctx` class module). These are workspace-relative
   paths like `crates/toolr-py/python/toolr/...`.
2. **Parse with ruff.** Each file is read as text and parsed
   into a `ModModule` AST.
3. **Walk the AST.** Extract: top-level `def`s matching the
   public decorator names (parameters, defaults, annotations
   as source text, docstring); the `Ctx` class body's public
   methods and properties (same fields).
4. **Read the Rust-side type-resolution table** from xtask's
   `toolr-core` dependency. This knows how Python type-hint
   strings (which we have as source text from the AST) map
   to argparse binding behavior.
5. **Render `references/commands.md`** via a hand-written
   Rust template (literal `write!` macros, not a general-
   purpose templating library).

**Why source spelling rather than runtime introspection.**
`inspect.signature` round-trips through Python objects and
normalizes formatting. AST gives us the source text directly,
which is what we want for documentation — the rendered
reference matches what a reader of toolr-py would see in the
source.

**Coupling to toolr-py layout.** The constants block listing
which source files to scan creates a small coupling: if
toolr-py renames a file or moves the decorator surface,
xtask's path list must be updated in the same PR. A CI guard
asserts every listed path exists; a missing path fails the
build. This trades a runtime "import the package" approach
for a static "scan these files" approach, and the trade is
deliberate — the layout is stable, the failure mode is
loud, and the coupling lives in one named constant.

**Why not `build.rs`.** A `build.rs` would freeze
introspection at Rust build time, decoupling from the source
on disk at xtask invocation time. The xtask reads source at
run time so the reference always matches what is currently
checked in.

#### Idempotency

The generator must produce byte-identical output across
consecutive runs on the same source tree. Without this,
`--check` is testing the wrong thing — drift in the generator
itself would be indistinguishable from drift in the code.

Idempotency invariants the generator enforces:

- **Sorted containers throughout.** `BTreeMap`/`BTreeSet` in
  Rust; `sorted()` of `inspect.getmembers` in Python. No
  `HashMap`, no `set`, no reliance on Python dict insertion
  order even though it is well-defined.
- **No timestamps, version stamps, hostnames, paths, or
  random IDs in output.** A "do not edit" header is a static
  literal.
- **LF line endings, no trailing whitespace, exactly one
  trailing newline.**
- **ASCII byte-order sort everywhere.** No
  locale-dependent operations.
- **Hermetic generator.** No network, no system clock, no
  environment-variable reads beyond locating the Python
  interpreter.
- **Markdown rendered via literal `write!`** — no markdown
  library that might normalize formatting differently across
  versions.

The hand-written markdown template in the Rust driver is
itself a load-bearing surface (changing it regenerates every
references file with new formatting). Template edits go
through the same `REVIEW.md` checklist as the trigger
description and conceptual narrative.

#### Layer 3 — `examples/` is runnable and snapshot-tested

`skills/toolr-command-authoring/examples/` contains a working
`tools/` tree exercising the full surface — decorators, `ctx`,
type hints, multiple groups, nested subcommands. Each example
file is a real `tools/*.py` that toolr can introspect.

The examples are exercised by toolr's existing test harness:

- `toolr self build-manifest` is run against the examples tree;
  the result is diffed against a committed
  `toolr-manifest.json` fixture. Mismatch fails CI.
- A `--help` snapshot test runs `toolr --help` and `toolr
  <group> <cmd> --help` for each example command, diffed
  against committed text fixtures. Mismatch fails CI.

If a refactor breaks an example, the snapshot fails and the
author must either update the example (and snapshot) or back
out the breaking change. The skill cannot ship a broken
example.

### Anchoring on existing toolr commands

The skill consistently directs the agent to existing toolr UX
instead of duplicating it:

- "Run `toolr project init` to scaffold a project; here is how
  to extend what it produces" — not a reproduction of scaffold
  content.
- "Run `toolr <group> <cmd> --help` to verify shape; if your
  command does not appear, dispatch auto-rebuilds, or run
  `toolr project manifest rebuild` on older toolr" — not a
  reproduction of manifest internals.

This keeps the skill small, makes it self-correcting (if
`toolr project init` improves, the skill rides along for free),
and avoids reproducing content that would rot.

### Trigger description

The frontmatter `description:` is the only field that cannot be
auto-generated and is therefore the most drift-prone part of
the skill. It must:

- Activate on phrases that signal command authoring: "add a
  toolr command", "extend toolr", path mentions of `tools/`,
  decorator names like `@command_group`, `ctx.run`, etc.
- *Not* activate on generic Python CLI work, on running existing
  toolr commands, on debugging toolr's Rust runtime, in any
  project that does not use toolr, or on packaging-flavored
  intent (which belongs to the planned packaging skill).

The description and the conceptual narrative are the only
hand-written load-bearing surfaces. They get explicit ownership
and a short review checklist in `skills/toolr-command-
authoring/REVIEW.md`. Anyone updating the trigger or the
narrative runs through the checklist before landing.

## Related work

A separate **`toolr-command-packaging`** skill is specced in
[`2026-05-21-toolr-command-packaging-skill-design.md`](./
2026-05-21-toolr-command-packaging-skill-design.md). Its scope
is deliberately narrow: how to ship existing toolr commands
as a distributable Python plugin (generate
`toolr-manifest.json` via `toolr self build-manifest <pkg>`,
configure the build backend to include it in the wheel, wire
`toolr self build-manifest --check` as a CI gate). It shares
the drift-defense infrastructure introduced here
(`build-skill-refs`, the `examples/` snapshot framework) but
has independent triggers, independent references, and
independent ownership. The closing pointer in the authoring
skill links users to it, but the two skills have no runtime
dependency on each other.

## Removals

None. The skill is net-new.

## Documentation

- `skills/toolr-command-authoring/README.md` — human-readable
  counterpart to the frontmatter: what the skill is, who its
  audience is, the three drift-defense layers, how to regenerate
  it. Not loaded into agent context; meant for humans browsing
  the repo.
- `skills/toolr-command-authoring/REVIEW.md` — the review
  checklist for hand-written load-bearing surfaces (trigger
  description, conceptual narrative, cross-references). One
  page.
- `docs/skills.md` (new top-level docs page) — short page in
  the user-facing docs explaining toolr's skill story: what the
  authoring skill teaches, where it lives, how to install it
  via `skillshare`, and a forward reference to the planned
  packaging skill. Cross-links to the skill `README.md`.
- `UNRELEASED.md` — note the new skill, the new `crates/
  xtask/` workspace crate (and the precedent it sets for
  future maintainer-only tooling), and the new CI gates.

## Testing strategy

Four test families. The first enforces the generator's
idempotency contract; the next three map one-to-one to the
three drift defenses.

### Generator idempotency

The foundation everything else rests on. Without this,
`--check` is testing the wrong thing.

- **Rust-side idempotency.** An integration test in
  `crates/xtask/tests/` runs `cargo xtask build-skill-refs`
  twice against the workspace and asserts
  `references/commands.md` is byte-identical between the
  two runs.
- **Source-files-exist guard.** A unit test asserts every
  toolr-py source file listed in the xtask's "files to scan"
  constants block exists. A rename or move in toolr-py fails
  this guard before it can produce a stale reference.
- Both run in CI; both are fast.

### References generation — `cargo xtask build-skill-refs --check`

- Runs in CI on every PR.
- Exits non-zero if the working tree is dirty after running
  `cargo xtask build-skill-refs`, i.e. the committed
  `references/*.md` files do not match what the generator
  produces from the current code.
- Unit-tested with a fixture toolr project where a known
  decorator rename produces a known diff in `commands.md`.

### Example manifest — `toolr self build-manifest` snapshot diff

- `toolr self build-manifest` runs against `skills/toolr-
  command-authoring/examples/`; the result is diffed against
  the committed `toolr-manifest.json` fixture.
- Mismatch fails CI.
- Test lives alongside the existing manifest-builder tests in
  `crates/toolr-core/`.

### Example `--help` — text snapshot

- For each example command, run `toolr <group> <cmd> --help`
  and diff against the committed text fixture.
- Mismatch fails CI.
- Tests use the existing CLI integration test harness.

### Trigger sanity (best-effort)

- A small fixture set of "should activate" and "should not
  activate" intents lives in `skills/toolr-command-
  authoring/tests/triggers.yaml`.
- A lightweight matcher (substring / keyword) runs against the
  committed `description:` to catch regressions where a
  previous-working phrasing stops matching. The host skill
  harness is the real arbiter; this is a guardrail, not a
  contract.

## Open questions

1. **`skillshare` coverage.** Confirm exactly which host
   platforms `skillshare` distributes to today (Claude Code is
   given; Copilot CLI and Gemini are open). The skill format
   itself targets Claude Code first; the drift-defense
   generators are platform-agnostic and can emit additional
   formats later without re-architecture.
2. **The "files to scan" constants block.** The xtask needs
   to name the toolr-py source files that contain the
   command-authoring surface (decorator module(s), `Ctx`
   module). The exact paths and how to group them (one
   constant per surface vs. one big list) is plan-level.
   The CI guard that asserts each path exists is mandatory.
3. **`build-skill-refs --check` enforcement points.** CI gate
   is mandatory. Pre-commit hook is recommended for local dev
   loop. The plan will decide whether to ship a prek hook entry
   that invokes `cargo xtask build-skill-refs --check`
   alongside the existing ones.
4. **xtask subcommand naming.** `cargo xtask build-skill-refs`
   mirrors `toolr self build-manifest` in spelling. If more
   skill-generation work accumulates (the packaging skill
   alone guarantees this), a `cargo xtask skills <action>`
   group may be cleaner. Defer to the plan; the spec's
   commitment is that the command exists in `xtask` and scales
   to N skills, not its exact spelling.
