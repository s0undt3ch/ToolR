# Repo presentation pass

**Date:** 2026-05-22
**Status:** Design (approved direction; not yet implemented)
**Related:** Closes the gap between the actual shipped state of toolr
(Rust front-end rewrite complete; static-manifest plugins; pure-Rust
authoring path) and the outward-facing surface (README, CONTRIBUTING,
docs/internals, specs/) that still reads like the pre-rewrite project.

## Problem

The 12 sub-plans of `specs/rust-front-end/01-roadmap.md` are all done.
Four further design+plan pairs shipped on top in May 2026:

- `2026-05-18-keep-bound-command-decorator-design.md` — rolled back the
  `@group.command` deprecation; the canonical single-file form is no
  longer deprecated.
- `2026-05-19-external-command-sources-*` — argparse AST scanner and
  the `DispatchCommand` runtime contract (PR #222).
- `2026-05-19-fill-the-gaps-*` — auto-rebuild on missing manifest,
  dispatcher-hosts-children CLI tree shape.
- `2026-05-21-dispatch-manifest-freshness-*` — replaced dist-info
  hashing with a `toolr-manifest.json`-targeted hash; removed
  entry-point plugin discovery entirely (PR #234).
- `2026-05-22-rust-build-manifest-*` — replaced `python -m toolr.build`
  with a pure-Rust implementation; deleted the Python `toolr.build`
  module (PR #235, HEAD merge).

The outward surface has not caught up with any of that. Concretely, a
new visitor lands at `README.md` and reads instructions that either
contradict the shipped code or describe a model that no longer exists.

### Concrete contradictions (the audit, condensed)

1. **README §Quickstart** — instructs `mkdir tools && touch
   tools/__init__.py`. The shipped scaffold is `toolr project init`,
   which deliberately produces a PEP 420 namespace package (no
   `__init__.py`). `docs/quickstart.md` and `docs/concepts.md` both
   say so. The README contradicts itself and the rest of the docs.
2. **README §Third-Party Commands** — describes registration via
   "Python entry points". Entry-point discovery was removed in
   dispatch-manifest-freshness; the model is now static
   `toolr-manifest.json` fragments shipped inside the wheel. The
   shipped `docs/third-party.md` documents the correct model.
3. **README §Advanced Usage** — links to
   `s0undt3ch.github.io/ToolR/usage/#advanced-topics`. That IA was
   replaced by Plan 11's restructure (Quickstart → Installation →
   Writing commands → …). Dead link.
4. **README §Testing and Security** — pitches Hypothesis fuzzing as a
   headline feature. It is a CI implementation detail. The
   genuinely-interesting story (sub-50ms `--help`, lazy uv-managed
   venv, pure-Rust manifest authoring, AST-driven argparse scanner,
   SLSA-attested releases) gets no air time.
5. **`docs/writing-commands/index.md:25-30`** — warns readers that
   `@group.command` is deprecated and emits a runtime warning. Per
   the 2026-05-18 keep-bound-command-decorator design, that
   deprecation was withdrawn; the form is canonical for single-file
   use. The remaining deprecation is only the bound subgroup method
   `parent.command_group("child", ...)`.
6. **`docs/writing-commands/known-bugs.md:24`** — closed-bugs list
   demonstrates nested groups with the still-deprecated method form
   (`docker.command_group("image")`), not the dotted-string form.
7. **`docs/internals/manifest.md:32-33` and `:184-187`** — describes
   `third_party_hash` as a blake3 over the venv's installed package
   set / dist-info mtimes. The shipped implementation hashes only the
   sorted set of `toolr-manifest.json` files under
   `<venv>/lib/python*/site-packages/*/`. Internally inconsistent
   too: line 33 says "venv's installed package set", line 184 says
   "metadata file mtime" — both descriptions are wrong, just
   wrong-in-different-directions.
8. **`docs/internals/manifest.md:142-156` (§Dynamic layer)** — step 3
   lists `importlib.metadata.entry_points(group="toolr.tools")` as a
   discovery step. Entry-point discovery was removed.
9. **`docs/.nav.yml`** — references `usage/`, `examples/`,
   `reference/toolr/`. None of those directories exist. `mkdocs.yml`
   has its own `nav:` block; the `.nav.yml` is dead.
10. **`tools/__init__.py`** (dogfood repo) — file exists with body
    `"""Tools package for ToolR."""`. Project docs explicitly say
    `tools/` is a PEP 420 namespace package and **no `__init__.py`
    is needed**. Dogfood repo contradicts its own documentation.
11. **`crates/toolr-core/src/dynamic/payload.rs:16`** — doc comment
    "Groups discovered by importing `tools.*` and via entry points."
    Entry points are gone.
12. **`specs/rust-front-end/` and four top-level 2026-05-* specs** —
    all closed work, but read like in-flight design docs. A future
    contributor opening `01-roadmap.md` has no signal that every
    sub-plan is merged and the document is historical.
13. **Auto-memory** — `MEMORY.md` index points at
    `toolr_rust_frontend_rewrite.md` and `project_toolr_django_path.md`.
    The former describes a multi-plan rewrite "in design phase" that
    is in fact complete. The latter cites `crates/toolr-django/`
    which never landed; the canonical reference plugin is now
    `examples/plugin-package/toolr_example_plugin/`.

## Goal

After this work lands:

- `README.md` leads with the actual differentiator (Rust binary
  front-end, sub-50ms interactive use, lazy uv-managed venv,
  static-manifest plugins, SLSA-attested releases) instead of a
  framework-comparison from 2024.
- `CONTRIBUTING.md` describes the three-crate Cargo workspace that
  exists today; references to `tests/cli/`, `tests/parser/`, and a
  Python-only test layout are gone.
- The six concrete documentation contradictions listed above are
  fixed in-place.
- `specs/` cleanly separates live design work (top level) from
  shipped post-mortem records (`specs/archive/<year>/`).
- `docs/.nav.yml`, `tools/__init__.py`, and the stale source comment
  in `dynamic/payload.rs` are removed.
- Auto-memory entries match current repo state.

A new visitor reading `README.md` builds a correct mental model
within the first screen.

## Non-goals

- **No runtime behavior changes.** No code paths change. No
  deprecations are removed. No new features ship. The diff is
  entirely prose, file moves, file deletions, and stale-comment
  cleanup.
- **No version bump.** The 1.0 release prep is a separate body of
  work (Option B in the brainstorming session). This work makes 1.0
  prep easier but does not start it.
- **No CHANGELOG manual edit.** `git-cliff` generates the changelog
  on release.
- **No new benchmarks committed.** PR-2 will measure once with
  `hyperfine` against a fresh checkout before headline numbers go in
  the README, but no benchmark harness is added to the repo.
- **No `rich-argparse`/rich-pin debt fix.** The `rich<14.3` pin in
  `crates/toolr-py/pyproject.toml` carries follow-up notes about
  `tests/context/test_prompt.py::test_prompt_password` patching
  `rich.console.getpass` (gone in rich 14.3+). That is its own
  follow-up.
- **No `__pycache__` hygiene work.** Stale local bytecode like
  `crates/toolr-py/python/toolr/__pycache__/build.cpython-314.pyc`
  is a working-tree artifact, not a commit issue; `.gitignore`
  already covers `__pycache__/`.

## Architecture

Three stacked PRs, in dependency order. Each PR is independently
shippable and reviewable; PR-1 ships value even if PR-2 stalls in
review.

```text
PR-1  Doc correctness fixes        ──┐  Smallest, hardest-to-argue-with.
      (six contradictions)           │  Mechanical edits; cite line numbers.
                                     ▼
PR-2  README + CONTRIBUTING rewrite ──┐  The big presentation lift.
                                     │  Prose; no behavior change.
                                     ▼
PR-3  specs/ archival + cleanup     ──┘  Housekeeping; file moves +
                                          dead-file deletions.
```

Stacked via `git-spice` (`gs branch create` on top of the previous
branch). PR-1 → main; PR-2 rebased on PR-1's merge; PR-3 rebased on
PR-2's merge.

### Why a stack and not one PR

PR-1 is undebatable correctness work that should ship even if review
on PR-2 takes a week. Bundling them blocks the contradiction fixes
behind prose review fatigue. PR-3 is the largest in *file count* (~30
file moves) but lowest in *reviewer cognitive load*; isolating it
lets reviewers approve on a skim.

### Why not a single PR with three commits

Stacked PRs let reviewers focus per-layer and let merge-conflicts (if
any) be resolved per-layer. A single PR with three commits forces a
full re-review of every commit on every push. Stacking is cheap with
git-spice already in use here.

## PR-1: Doc correctness fixes

Closes the six numbered contradictions where docs actively
misinform readers.

### Edits (file:line citations from current `main`)

| File | Edit |
|---|---|
| `docs/writing-commands/index.md:25-30` | Rewrite the "Migrating from the legacy decorators?" admonition. New text names only `parent.command_group("child", ...)` as deprecated. Drop the "every legacy call emits a runtime warning" line — the warning was removed for `@group.command`. Link to the migration guide stays. |
| `docs/writing-commands/known-bugs.md:24` | Replace the example "`docker.command_group("image")`" with the dotted form "`command_group("docker.image", ...)`". This is the *closed-bugs* section celebrating a feature; using the deprecated form as the demonstrating example is exactly the wrong signal. |
| `docs/internals/manifest.md:32-33` | Rewrite the `third_party_hash` bullet in §File shape. New text: "blake3 over the sorted set of `toolr-manifest.json` files under `<tools-venv>/lib/python*/site-packages/*/`, each entry hashed as path + content. Drives third-party-fragment rebuilds." |
| `docs/internals/manifest.md:142-156` (§Dynamic layer) | Remove step 3 (`importlib.metadata.entry_points(...)` walk). Renumber. Rewrite the surrounding prose: the dynamic layer's job is now exclusively to import `tools.*` modules so dynamically-registered commands are caught; third-party packages are picked up via static fragments, not via this layer. |
| `docs/internals/manifest.md:184-187` (§Hashing details) | Replace the dist-info-mtime description with the path+content blake3 over `toolr-manifest.json` files (matching the §File shape bullet). Keep the `static_hash` paragraph above as-is. |
| `crates/toolr-core/src/dynamic/payload.rs:16` | Source doc comment: remove "and via entry points". New text: "Groups discovered by importing `tools.*` modules." |

### Out of scope for PR-1

The historical "rich-argparse" comments in
`crates/toolr/src/markdown.rs:7` and `crates/toolr/src/cli.rs:8`
describe what the binary visually replaced. They are vestigial but
not actively misleading — they explain a design decision (mimicking
the old visual look). Decide their fate in PR-3.

### Verification

- `mkdocs build --strict` passes. (`mkdocs.yml` already sets
  `strict: true`; any broken cross-reference fails the build.)
- `cargo test --workspace` passes unchanged.
- Manual: render `docs/writing-commands/index.md` and
  `docs/internals/manifest.md` locally; eyeball.

### Size

~30 lines changed, ~10 lines deleted. Six files.

## PR-2: README + CONTRIBUTING rewrite

Both files rewritten end-to-end. The diff is large but the surface
area is small (two files).

### New `README.md` outline

```text
# ToolR — In-project CLI tooling, with a Rust front-end

[Hero: short demo. Either an ASCII transcript of
 `toolr project init` → first command, or a single asciinema cast.
 If hyperfine numbers are credible, surface one here:
   $ toolr --help          # NN ms
   $ toolr <group> --help  # NN ms
 If numbers are mediocre, drop the headline and lead with the model.]

## Why ToolR

- Sub-NN-ms `--help` and Tab completion. The CLI is a Rust binary;
  Python never boots for non-execute paths.
- Per-repo Python venv, materialised on first use by uv. No
  framework imports on the hot path; no system-Python dependency.
- Discover commands by writing functions in `tools/*.py` — no
  framework boilerplate beyond two decorators.
- Third-party command packages contribute via a static JSON
  manifest the toolr binary globs at startup. Zero Python import
  during discovery.
- SLSA-attested release archives. Install via curl|sh, pip, mise,
  PowerShell, or a release tarball.

## Two wheels, two roles

| Package    | Role                                                    | Where it lives                    |
|------------|---------------------------------------------------------|-----------------------------------|
| `toolr`    | The CLI binary you run from the shell.                  | On `$PATH`, installed once.       |
| `toolr-py` | The Python runtime your `tools/*.py` import.            | In your `tools/pyproject.toml`.   |

## Install

Five first-class install paths; pick the one that matches how the
rest of your tooling is installed.

### mise

mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise use --global toolr@latest

For projects that already pin tool versions via `.mise.toml`, this
is the most-natural fit — toolr's version becomes part of your
project's reproducible tool set. See [installation/mise](docs link).

### pip

pip install toolr      # the Rust CLI binary, installed by pip
pip install toolr-py   # the Python runtime your tools/*.py import

The `toolr` wheel ships only the binary; the `toolr-py` wheel
ships the Python `import toolr` surface. Most projects want both,
in different venvs — see "Two wheels, two roles" above.

### curl | sh (Linux + macOS)

curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh

Verifies the SLSA attestation when `gh` is on PATH. Pin a version
with `sh -s -- --version X.Y.Z`.

### PowerShell (Windows)

irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex

### GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
Windows) from <https://github.com/s0undt3ch/ToolR/releases>, verify
the `.sha256` sibling and the SLSA attestation, drop the binary
on `$PATH`. Useful in locked-down environments that audit binaries
before allowing them on a machine.

### Scaffold your repo

After the binary is on `$PATH`:

toolr project init                   # writes tools/{pyproject.toml,.gitignore,example.py}
toolr example hello                  # run the generated example
toolr self completion install bash   # or zsh / fish

For Windows / non-bash shells, swap the completion target. The full
install matrix (per-OS notes, attestation flags, prefix overrides)
lives in [docs/installation/](docs link).

## What you write

# tools/example.py
from toolr import Context, command_group

example = command_group("example", title="Example", description="...")

@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Say hello to <name>."""
    ctx.print(f"Hello, {name}!")

[Mirror docs/quickstart.md's canonical example. No
 `__init__.py`-creation step. Mention `toolr project init` did this
 for you.]

## Where to go next

- Quickstart: docs link
- Writing commands: docs link
- Third-party packages: docs link
- Internals (manifest layers, cache, freshness): docs link
- CLI reference: docs link

## Project status

ToolR is pre-1.0. The on-disk manifest is versioned
(`schema_version` in `tools/.toolr-manifest.json`) and the binary
refuses to load a higher version than it understands. The Python
import surface is `toolr.__all__`; anything not listed there is
implementation detail. Backwards-incompatible changes will be
explicit in the changelog.

## Contributing

See CONTRIBUTING.md.

## License

Apache-2.0.
```

### What `README.md` drops vs current

- The "next generation of python-tools-scripts" framing comparison.
  Keep it for the homepage of an old project; the README is for
  someone landing fresh. Move the lineage paragraph to
  `docs/concepts.md` if it adds value there.
- The `mkdir tools && touch tools/__init__.py` Quickstart. Replaced
  by `toolr project init`.
- The Testing and Security section pitching Hypothesis. Hypothesis
  remains in the test suite; it doesn't deserve top-billing.
- The Advanced Usage link to the dead `/usage/#advanced-topics`
  anchor.
- "Pronounced /ˈtuːlər/" pronunciation line. Keep it lower in the
  README or move to docs — not load-bearing in a first impression.

### Bench numbers — process

Before merging PR-2:

1. On a fresh checkout, run `hyperfine 'toolr --help' --warmup 3`
   and `hyperfine 'toolr <group> --help' --warmup 3`.
2. Record the numbers in the PR description, not just in the README.
   The PR description is the audit trail; the README is the polish.
3. If a number is `> 50 ms`, drop the "sub-50ms" framing entirely
   and lead with the model ("Rust binary front-end, Python at
   execute time only"). Honesty over marketing.

### New `CONTRIBUTING.md` outline

```text
# Contributing

[Friendly opener — one paragraph.]

## Repo layout

This is a Cargo workspace with three crates plus the Python source:

- `crates/toolr-core/` — pure-Rust library: parser, manifest,
  freshness, argparse scanner, completion engine, cache. No pyo3.
- `crates/toolr/` — the binary: clap CLI, dispatch, subprocess
  control.
- `crates/toolr-py/` — pyo3 dynlib + the Python source tree at
  `crates/toolr-py/python/toolr/`. Ships as the `toolr-py` wheel.

Two PyPI wheels at the same workspace version:

- `toolr` — `[bindings = "bin"]` maturin build; ships the Rust
  binary only.
- `toolr-py` — `[bindings = "pyo3"]` maturin build; ships the
  Python package plus the `_rust_utils` extension module.

A GH-Release archive of the binary ships alongside the wheels.

## Dev setup

Requires `mise`. Everything else (Python, Rust, uv, prek) installs
from `mise.toml`:

mise install
uv sync --all-extras --dev
prek install --install-hooks

To run the dev binary against the dogfood `tools/`:

cargo run -p toolr -- --help
cargo run -p toolr -- self build-manifest toolr-plugin-example

## Tests

- Python unit tests: `tests/` (pytest).
- Rust unit tests: `cargo test -p toolr-core`.
- Rust integration tests: `crates/toolr/tests/*.rs` (assert_cmd).
- Distribution lock-tests: `tests/distribution/` (opt-in, slow;
  build real wheels).

[Pre-commit, prek, conventional commits — keep current content,
 reword.]

## RUNNER_SCHEMA_VERSION ↔ SCHEMA_VERSION lock-step

[Keep current section as-is — it's good and accurate.]

## Filing bugs / asking for features

[Keep current content with minor tightening.]

## Commit hygiene

- Conventional Commits.
- No `Co-Authored-By:` footer on commits to this repo.
- Don't manually edit `CHANGELOG.md`; git-cliff generates it on
  release.

## License

Apache-2.0. Sign-off not required.
```

### What `CONTRIBUTING.md` drops vs current

- The `tests/cli/, tests/parser/, tests/registry/, ...` test
  organization tree. `tests/cli/` and `tests/parser/` no longer
  exist; CLI is in Rust, parser is in `crates/toolr-core/src/parser/`.
- The Python-only "Coding Standards" section that omits Rust style.
  Replace with cross-referencing `cargo fmt` + `cargo clippy` + the
  ruff/mypy configs already in `pyproject.toml`.
- The standalone Hypothesis section. One sentence in the Tests
  section is enough.

### Verification

- `mkdocs build --strict` (no doc page links into README/CONTRIBUTING
  by path; safe).
- Manual review against the actual `mise.toml`, `pyproject.toml`,
  `crates/*/Cargo.toml`, and the existing CI workflows
  (`.github/workflows/*.yml`).

### Size

~250 lines of prose churn. Two files. No code or test impact.

## PR-3: specs/ archival + cleanup

### Move shipped specs to `specs/archive/2026/`

```text
specs/
├── README.md                             [new — see below]
├── archive/
│   └── 2026/
│       ├── rust-front-end/               [the entire 16-file tree]
│       │   ├── 00-design.md
│       │   ├── 01-roadmap.md
│       │   ├── 02-plan-1-rust-skeleton.md
│       │   ├── ... (through 15-plan-12-workspace-split.md)
│       │   └── followups/
│       │       └── 2026-05-14-rich-argparse-dependency.md
│       ├── 2026-05-18-keep-bound-command-decorator-design.md
│       ├── 2026-05-19-external-command-sources-design.md
│       ├── 2026-05-19-external-command-sources-plan-a.md
│       ├── 2026-05-19-fill-the-gaps-design.md
│       ├── 2026-05-19-fill-the-gaps-plan.md
│       ├── 2026-05-21-dispatch-manifest-freshness-design.md
│       ├── 2026-05-21-dispatch-manifest-freshness-plan.md
│       ├── 2026-05-22-rust-build-manifest-design.md
│       └── 2026-05-22-rust-build-manifest-plan.md
└── 2026-05-22-repo-presentation-pass-design.md   [this file, stays
                                                   top-level until PR-3
                                                   itself ships]
```

`git mv` for each. Preserves history. After PR-3 lands, this design
doc moves into `archive/2026/` as its final step (or stays top-level
if more presentation-pass work follows; left as judgment call when
merging).

### Add `specs/README.md`

```markdown
# Specs

Design records for toolr — both live work and historical post-mortems.

## Where work lives

- **Top level (`specs/<date>-<topic>-design.md`)** — active design
  work and proposed-but-not-shipped features. Each design pairs with
  a `<date>-<topic>-plan.md` implementation plan once it leaves
  brainstorming.
- **`specs/archive/<year>/`** — shipped or abandoned designs.
  Archived files are immutable post-mortem records; do not edit
  them in place. If a shipped design needs revising, write a new
  design that supersedes it.

## How to start a new design

Open a brainstorming session:

    /superpowers:brainstorming

The session writes the design here once you approve it. The
implementation plan follows from `/superpowers:writing-plans`.

## How to archive

When the PR implementing a design merges to `main`:

    git mv specs/<date>-<topic>-design.md specs/archive/<year>/
    git mv specs/<date>-<topic>-plan.md specs/archive/<year>/

Land the move in the same PR (or as an immediate follow-up).
```

### Pre-move check

Before `git mv`, run:

```text
git grep -nE 'specs/(rust-front-end|2026-05-(18|19|21|22))' -- ':!specs/'
```

to catch any in-tree reference into the spec paths. Audit at design
time confirms zero pointers from `docs/`, code, or CI; only the
spec files themselves cross-link, and within-spec relative links
(`./00-design.md`, etc.) survive the move unchanged.

### Upward-relative links that break after the move

Three real markdown links in
`specs/rust-front-end/13-plan-11-docs-overhaul.md` reference
`../../docs/writing-commands/arguments.md`, `../../docs/quickstart.md`,
and `../../docs/project-config.md`. From the current location these
resolve into the repo's `docs/` tree. After moving to
`specs/archive/2026/rust-front-end/`, those `../../` paths would
resolve into `specs/archive/2026/docs/` — broken.

Two reasonable approaches; pick **A** for this work:

- **A — rewrite the three links during the move.** Change
  `../../docs/...` to `../../../../docs/...` (four levels up from the
  archived location). One-line edits in one file. No more wrong than
  the original, and the links stay clickable on github.com.
- **B — accept broken upward links in archived docs.** Document in
  `specs/README.md` that archived specs may contain stale relative
  links to live docs and that's expected for post-mortem records.
  Cheaper now, worse for the next reader.

Option B is tempting and wrong: the next session that opens an
archived design to understand prior reasoning will hit dead links
and lose context. Pay the four-line edit cost.

Other `../../`-prefixed strings under `specs/rust-front-end/`
(verified at design time: 14 occurrences in
`14-workspace-split-design.md` and `15-plan-12-workspace-split.md`)
are **literal content**, not markdown links — quoted examples of
`pyproject.toml` directives like `readme = "../../README.md"`,
inside fenced code blocks. They display the same text regardless of
the file's location; no edit needed.

Similarly `include_str!("../../../examples/plugin-package/...")` in
`2026-05-22-rust-build-manifest-plan.md` is Rust source illustrated
inside a code fence, resolved relative to the Rust file being
described, not relative to the spec file. Safe to move unchanged.

### Delete dead files

- `docs/.nav.yml` — dead. `mkdocs.yml` has its own `nav:` block;
  `mkdocs-awesome-nav` is installed but `.nav.yml` references
  `usage/index.md`, `examples/index.md`, `reference/toolr/` — none
  of which exist in `docs/`. Confirmed by `mkdocs build --strict`
  passing today *despite* the broken paths in `.nav.yml`, which
  means awesome-nav is ignoring it.
- `tools/__init__.py` — contradicts the documented PEP 420 model.
  Delete. Verify with `cargo test --workspace` that the static
  parser + dynamic introspect still work against the dogfood
  `tools/` directory.
- Decision on the `rich-argparse` historical comments
  (`crates/toolr/src/markdown.rs:7`, `crates/toolr/src/cli.rs:8`):
  delete the references in PR-3. They describe what was replaced,
  not what is. Anyone wanting that context can find it in the
  workspace-split design archive.

### Update auto-memory

Memory lives in
`/Users/pedro.algarvio/.claude-work/projects/-Users-pedro-algarvio-projects-me-toolr/memory/`
(outside the repo). Two edits at PR-3 merge time:

- **Retire `toolr_rust_frontend_rewrite.md`**: rewrite as a short
  pointer at `specs/archive/2026/rust-front-end/01-roadmap.md`
  noting "rewrite complete; document archived." Or remove and
  reduce `MEMORY.md` index to one line.
- **Correct `toolr_django_path.md`**: the canonical reference
  plugin is `examples/plugin-package/toolr_example_plugin/`. There
  is no `crates/toolr-django/`. Either rewrite the memory to point
  at `examples/plugin-package/` or remove it.

### Verification

- `cargo test --workspace` — guards the `tools/__init__.py` deletion
  and the source-comment edits.
- `mkdocs build --strict` — guards `.nav.yml` deletion and any
  cross-doc references that incidentally touched `specs/`.
- `git grep` (per pre-move check above) — guards the spec moves.
- `prek run --all-files` — markdown lint + codespell + typos on the
  new `specs/README.md`.

### Size

- ~30 file moves (no content change inside moved files).
- ~30 lines new (`specs/README.md`).
- 3 link rewrites in
  `specs/archive/2026/rust-front-end/13-plan-11-docs-overhaul.md`
  (per "Upward-relative links" section above).
- 2 file deletions (`docs/.nav.yml`, `tools/__init__.py`).
- ~6 line deletions (two source-comment locations; the
  `dynamic/payload.rs` comment edit is in PR-1, not PR-3).

## Risks and mitigations

- **README headline numbers underwhelm.** If `hyperfine` shows
  `toolr --help` at 80 ms on a real-world repo, the "sub-50ms"
  framing is dishonest. Mitigation: measure first; if the number is
  weak, lead with the *model* ("Rust binary, Python at execute time
  only") which is true regardless. Don't ship marketing the code
  can't back up.
- **Spec archival breaks deep links from outside the repo.** Anyone
  who linked `specs/rust-front-end/00-design.md` from a blog post
  or PR description now has a 404. Acceptable: the repo is pre-1.0,
  the spec docs are not contracts, no public docs link into
  `specs/`. GitHub's redirect for renamed files would help on the
  same repo; cross-repo links break. We accept this.
- **`tools/__init__.py` deletion subtly breaks something.** The
  static parser walks `tools/**/*.py` directly; the dynamic layer
  imports `tools.*` via `_import_tools_modules` after putting
  `tools/..` on `sys.path`, which works for PEP 420 namespace
  packages. Both paths already support the `__init__.py`-less
  layout (`toolr project init` produces it). Mitigation: full
  `cargo test --workspace` and `uv run pytest` before the PR-3
  merge. If anything fails, that's a real bug to fix, not a reason
  to keep the contradictory file.
- **PR-2's prose changes spawn endless bikeshedding.** Possible.
  Mitigation: PR-1 ships independent of PR-2's review timeline; the
  most-egregious contradictions are fixed before the prose pass even
  starts.
- **Stacked-PR overhead.** `git-spice` already in use here; the
  overhead is negligible. If git-spice becomes flaky for the
  contributor doing the work, fall back to one big PR with three
  clearly-titled commits and ask for per-commit review.

## What this does not do (recap)

- No 1.0 release prep (deprecation removals, public-surface lock,
  stability statement). Separate work.
- No new features (toolr-django demo, `toolr doctor`, cross-repo
  command sharing). Separate work.
- No version bump.
- No CHANGELOG manual edits.
- No `rich<14.3` pin fix or `getpass` test-patch debt remediation.
- No CI workflow changes. The existing
  `.github/workflows/*.yml` are correct for the post-rewrite shape.

## Implementation order summary

1. Write the implementation plan (`writing-plans` skill) covering
   PR-1, PR-2, PR-3 as three sequenced sections with task lists.
2. Execute PR-1: open, review, merge.
3. Rebase PR-2 on PR-1's merge. Measure benchmarks. Open, review,
   merge.
4. Rebase PR-3 on PR-2's merge. Run pre-move grep. Move files. Open,
   review, merge.
5. Update auto-memory entries as the last step of PR-3.
6. Move this design doc into `specs/archive/2026/` (judgment call —
   may stay top-level if a follow-up presentation pass is queued).

## Approval

Design direction approved in brainstorming session 2026-05-22.
Implementation plan pending.
