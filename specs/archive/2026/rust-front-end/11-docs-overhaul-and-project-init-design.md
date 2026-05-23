# Docs Overhaul + `toolr project init` — Design

- **Tracks:** [Rust Front-End Design](./00-design.md), [Roadmap](./01-roadmap.md)
- **Status:** Design committed; plan docs not yet drafted.
- **Purpose:** Bridge document scoping the follow-on work after the
  rust front-end rewrite (Plans 1–9) to (a) ship a `toolr project init`
  bootstrap command and (b) restructure the documentation site so it
  matches the new tool.

## Background

Plans 1–9 replaced the Python argparse CLI with a Rust binary front-end
plus a uv-managed tools venv, a static + dynamic manifest model, third-
party plugin discovery, missing-deps diagnostics, cache management, and
new distribution channels (`install.sh`, mise, GitHub release archives,
SLSA attestations). The existing `docs/` site predates all of this and
documents:

- The obsolete `python -m pip install toolr` lead.
- Argparse-era help output with `--timestamps` / `--timeout` /
  `--no-output-timeout-secs` flags that no longer exist. (Whether
  any of those should come back is tracked in
  [issue #191](https://github.com/s0undt3ch/ToolR/issues/191) and
  is out of scope for this design.)
- A `tools/__init__.py` scaffold step that is unnecessary (PEP 420
  namespace packages work — validated in design).
- Third-party packages only via the legacy entry-point mechanism.

It does not document:

- Install paths added by Plan 9 (`install.sh`, `install.ps1`, mise
  plugin, release archives, SLSA verification).
- New CLI surface: `toolr project {deps sync,venv path,venv shell,
  manifest rebuild}`, `toolr self {completion {print,install},
  cache {list,prune},build-manifest}`.
- The tools-venv model (`tools/pyproject.toml`, `tools/uv.lock`,
  in-tree vs cache layouts).
- The manifest model (`tools/.toolr-manifest.json`, static + dynamic
  layers, pre-commit hook).
- Missing-deps diagnostics (pre-flight, post-mortem,
  `TOOLR_NO_PREFLIGHT_DEPS`).
- The static-manifest convention for third-party packages
  (`toolr-manifest.json` + `toolr.build`).
- The cache layout and pruning model.

This document scopes both halves of the cleanup as a single coordinated
change.

## Goal

Two coupled deliverables:

1. **`toolr project init`** — a new built-in subcommand that scaffolds
   `tools/` in the current repo (writes `tools/pyproject.toml`,
   `tools/.gitignore`, an example `tools/example.py`), then runs the
   existing `ensure_venv_ready` path so the user goes from `init` to a
   runnable example in one command.
2. **Doc restructure** — rewrite the documentation site against the
   actual rust-front-end mental model, with the tools-author audience
   as the primary user. Add new sections covering the new surface and
   prune the obsolete content.

The init command lands first so the docs can reference real terminal
output from running it.

## Audience priority

1. **Tools-authors** (primary): developers adding `tools/` support to
   their own repo. Need authoring API, project configuration,
   command-running behaviour.
2. **End users of someone else's tools** (secondary): developers in a
   repo that already has `tools/`. Need install + run only.
3. **Third-party command package authors**: need the
   `toolr-manifest.json` convention and `toolr.build` API.
4. **Toolr contributors**: need the internals and the design specs
   under `specs/rust-front-end/`.

The lead reader is (1). Docs are ordered so (2) shares the
quickstart/install pages with (1) at the top, then the authoring
chapter dominates the middle, then (3) and (4) live in deeper sections.

## `toolr project init` — command design

### Surface

```text
toolr project init [--force] [--no-sync] [--venv-location {cache,in-tree}]
                   [--no-example] [--python <version>] [--quiet]
```

Runs against the current working directory; no path argument.
Recommendation in docs is "run from your repo root".

| Flag | Default | Behaviour |
|------|---------|-----------|
| `--force` | off | Overwrite an existing `tools/` directory. Default refuses if `tools/` exists with any contents. |
| `--no-sync` | off | Scaffold files only; skip the auto `uv sync`. |
| `--venv-location` | `cache` | `cache` (default, matches Plan 3) or `in-tree` (uses `tools/.venv/`). |
| `--no-example` | off | Skip generating `tools/example.py`. |
| `--python` | host's `>=major.minor` | `requires-python` value written into `tools/pyproject.toml`. |
| `--quiet` | off | Suppress informational output. |

### Files written

Inside `<cwd>/tools/`:

- **`pyproject.toml`** — `[project]` block with `name = "tools"`,
  `version = "0.0.0"`, the chosen `requires-python`, and
  `dependencies = ["toolr"]`. `[tool.toolr]` block with
  `venv-location` set to whatever the user picked.
- **`.gitignore`** — single line `.venv/` so in-tree users don't
  accidentally commit it.
- **`example.py`** (unless `--no-example`) — one `command_group("example",
  "Example commands")` with four `@group.command` functions:
  1. `hello(ctx, name: str = "world")` — simplest case, `ctx.print()`.
  2. `commit(ctx)` — `ctx.run("git rev-parse --short HEAD",
     capture_output=True)` to demonstrate subprocess + capture.
  3. `confirm(ctx)` — `ctx.prompt(...)` for interactive input;
     branches on the answer; uses `ctx.exit(code, msg)` for non-zero
     exit.
  4. `setlog(ctx, level: Literal["debug", "info", "warning"] = "info")`
     — demonstrates a `Literal[...]` type rendering as a choice in
     `--help`, plus showing how to switch on the value.

No `tools/__init__.py`. Validated against the dynamic introspect
helper (Plan 6) and the local-tools execute path — PEP 420 namespace
packages work end-to-end.

### Post-scaffold

Unless `--no-sync`, call into the existing
`_rust_utils::project::ensure_venv_ready` (which already drives
`uv sync` + `validate_venv` + `meta.json` write per Plans 3 + 8). No
new sync logic.

### Output (non-quiet, success case)

```text
toolr: scaffolded tools/ at /path/to/repo
toolr:   wrote tools/pyproject.toml
toolr:   wrote tools/.gitignore
toolr:   wrote tools/example.py
toolr: synced venv at /Users/.../cache/toolr/<repo-key>/venv using uv 0.6.0
toolr:
toolr: next steps:
toolr:   toolr example hello
toolr:   toolr example commit
toolr:   toolr self completion install <bash|zsh|fish>   # optional, for tab completion
```

### Error semantics

| Condition | Exit code | Behaviour |
|-----------|----------:|-----------|
| `tools/` exists and is non-empty without `--force` | 2 | Print `toolr: tools/ already exists at <path> (use --force to overwrite)`. No partial writes. |
| Mid-scaffold write failure | non-zero | Roll back any files written so far. Atomicity matters — no half-scaffold. |
| `uv sync` failure post-scaffold | propagate | Files stay on disk; user can rerun `toolr project deps sync`. |
| `uv` not available, `--no-sync` not set, sync would have run | propagate | Same as above. |

### Internal placement

- `src/bin/toolr/cli.rs` — register `init` as a fourth arm under the
  existing `project` subcommand (peer to `deps`, `venv`, `manifest`).
- `src/bin/toolr/project.rs` — add `project_init(matches)` arm in the
  existing dispatcher.
- `src/bin/toolr/init_templates.rs` (new) — embed the three template
  files via `include_str!`. Templates are parameterised on
  `venv-location` and `requires-python` via tiny string substitution.

## Documentation IA

### Top-level navigation

1. **Home** (`docs/index.md`) — what toolr is, who it's for, link to
   Quickstart. Short.
2. **Quickstart** (`docs/quickstart.md`) — single page: install binary
   → `toolr project init` → run example. Aims for "first command
   running in under 2 minutes".
3. **Installation** (`docs/installation/index.md`, rewrite) —
   install-channels reference: `install.sh`, PowerShell installer,
   mise, GitHub releases (with SLSA verification), pip (with the
   wheel-no-binary limitation noted). Requirements (uv, supported
   Pythons).
4. **How toolr is laid out** (`docs/concepts.md`, new) — orientation
   page. One page, no deep dives. Names + one-line descriptions: the
   `toolr` binary, the `tools/` directory (PEP 420), `tools/
   pyproject.toml`, the tools venv (uv-managed; in-tree vs cache),
   the manifest, tab completion, the per-repo cache. Each item links
   to its full page.
5. **Writing commands** (`docs/writing-commands/`, new chapter) —
   authoring guide. Sub-pages: defining groups & commands, type-driven
   argument parsing (positionals, optionals, flags, lists,
   `Literal`/Enum), docstrings (Google style), using `ctx` (print /
   run / prompt / exit / verbosity), `arg()` annotations & mutually
   exclusive groups, multi-file & nested groups. Examples are real
   files under `docs/writing-commands/files/` consumed via mkdocs-
   material `--8<--` snippets.
6. **Project configuration** (`docs/project-config.md`, new) — every
   key in `tools/pyproject.toml`: required `[project]` fields,
   `[tool.toolr]` options (`venv-location`, `editable-install`, …),
   `tools/uv.lock`, when to use in-tree vs cache.
7. **CLI reference** (`docs/cli.md`, new) — **single page with
   anchors** for every `toolr` subcommand documented uniformly (usage
   line, args, examples, behaviour notes): `project {init,deps sync,
   venv path,venv shell,manifest rebuild}`, `self {completion
   {print,install},cache {list,prune},build-manifest}`. Hidden
   subcommands (`__complete`, `__build-static-manifest`) get a short
   "Internal" section at the end.
8. **Third-party command packages** (`docs/third-party.md`, new) —
   plugin author guide. The static-manifest convention
   (`<pkg>/toolr-manifest.json`), `toolr.build` API + `python -m
   toolr.build` CLI, entry-point fallback (`toolr.commands` group via
   `importlib.metadata`), `toolr self build-manifest <package>`,
   `--check` for CI drift.
9. **Internals** (`docs/internals/`, new chapter) — manifest layers
   (static + dynamic), pre-commit hook
   (`.pre-commit-hooks.yaml`), missing-deps diagnostics (pre-flight +
   post-mortem), cache layout. Pointer to `specs/rust-front-end/` for
   contributors.
10. **API reference** (`docs/reference/`) — auto-generated, **scoped
    to the public surface only**: `toolr.Context`,
    `toolr.command_group`, `toolr.arg`, `toolr.testing`, `toolr.build`,
    `toolr.MANIFEST_SCHEMA_VERSION`. Private modules removed from the
    site.
11. **Contributing** + **Changelog** — existing symlinks; unchanged.

### File-level changes

| Current path | After | Action |
|---|---|---|
| `docs/index.md` | `docs/index.md` | Rewrite; trim to a landing page; narrow the README include. |
| (none) | `docs/quickstart.md` | Create. References real `toolr project init` output. |
| `docs/installation/index.md` | `docs/installation/index.md` | Rewrite; replace the `pip install toolr` lead with the install matrix; drop the obsolete argparse `toolr --help` block; add SLSA section. |
| (none) | `docs/concepts.md` | Create. ~300–400 words. |
| `docs/usage/index.md` (~220 lines) | split → `docs/writing-commands/{index,groups,arguments,docstrings,context,annotations,nesting}.md` | Move + restructure into focused sub-pages. |
| `docs/usage/files/*.py` | `docs/writing-commands/files/*.py` | Move; keep `--8<--` snippet pattern. |
| (none) | `docs/project-config.md` | Create. |
| (none) | `docs/cli.md` | Create. Single page with anchors. |
| (none) | `docs/third-party.md` | Create. Source material lives in `specs/rust-front-end/06-plan-5-static-third-party.md`. |
| (none) | `docs/internals/{manifest,cache,pre-commit,diagnostics}.md` | Create. Distilled from the corresponding plan docs. |
| `docs/reference/toolr/{_runner,_parser,_registry,_exc,_introspect}.md` | delete | Internal; not part of the user-facing API. |
| `docs/reference/toolr/utils/{_signature,_console,_docstrings,_imports,_logs,command}.md` | delete (except `_signature.arg` → `docs/reference/arg.md`) | Same. |
| `docs/reference/toolr/_context.md` | rename to `docs/reference/context.md` | Public. |
| `docs/reference/toolr/testing.md` | move to `docs/reference/testing.md` | Public. |
| `docs/reference/toolr/build.md` | move to `docs/reference/build.md` | Public. |
| `docs/examples/index.md` + `docs/examples/files/*.py` | fold into `docs/writing-commands/*` | Examples live alongside the concepts they demonstrate; net fewer top-level sections. |
| `docs/changelog.md`, `docs/contributing.md` | unchanged | Symlinks to repo-root files. |
| `mkdocs.yml` | rewrite nav | Match the new IA. `strict: true` already set. |

### Voice & style

- Stay in the existing terse-but-friendly register (current docs read
  well).
- Command output blocks are rendered from real runs against a known-
  good fixture project — no hand-edited approximations.
- Code samples are real files under `docs/.../files/` consumed via
  mkdocs-material `--8<--` snippets, so they remain runnable / type-
  checked.

### Captured terminal output (`.txt` snippets)

`.pre-commit-hooks/regen-doc-snippets.py` regenerates `.txt` captures by
running the real `toolr` binary against the doc fixture project. Lives
alongside the existing local hook scripts (`pin-github-actions.py`,
`ref-doc-stubs.py`). Each `.py`
example file gets matching `.txt` files in the same directory, e.g.:

```text
docs/writing-commands/files/example1.py
docs/writing-commands/files/example1-help.txt
docs/writing-commands/files/example1-run.txt
```

The doc page includes both the source and the captured output via
`--8<--`.

Drift detection:

- **In CI**: `python .pre-commit-hooks/regen-doc-snippets.py --check` runs in
  the existing CI workflow; fails if regenerating any snippet would
  change its on-disk contents.
- **Pre-commit hook**: same `--check` invocation, scoped via
  `files: ^docs/.*\.(py|txt|md)$|^tools/.*\.py$|^src/bin/toolr/` so
  the hook is a no-op on code-only commits that don't touch
  doc-rendered surface. Hook depends on `target/release/toolr`
  existing; falls back to `cargo build --release --bin toolr -q` if
  absent.

## Testing strategy

1. **`mkdocs build --strict`** — already enforced; with `strict: true`,
   broken internal links, missing snippet files, and undefined cross-
   references fail the build. Highest-leverage check.
2. **Init-command integration tests** in
   `tests/project_init.rs`:
   - Scaffold into a tmpdir → assert all expected files exist with
     expected content.
   - Refuse to scaffold over a non-empty `tools/` → assert exit code
        - error message.
   - `--force` overwrites → assert new content present.
   - Full `init` then `toolr example hello` against the result →
     assert stdout. Gated on a usable Python being available, matching
     the `running_a_user_command_invokes_python_runner` pattern from
     `tests/cli_smoke.rs`.
3. **Snippet drift check** — pre-commit hook + CI both run
   `.pre-commit-hooks/regen-doc-snippets.py --check`. Any drift fails the build.

## Decomposition

Two plans, tracked under `specs/rust-front-end/`, continuing the
existing numbering.

### Plan 10: `toolr project init` (Rust binary feature)

~6 tasks, all small. Lands first because Plan 11 needs real terminal
output from running `init`.

1. clap subcommand registration + `init_templates` module with
   `include_str!` of the three template files.
2. Scaffolding logic: read cwd, refuse if `tools/` exists, atomic
   file writes, roll back on partial failure.
3. `--no-sync` plumbing — by default, hand off to
   `ensure_venv_ready` for the auto-sync step.
4. Example file content: `hello` / `commit` / `confirm` / `setlog`
   (with `Literal[…]`).
5. Integration tests in `tests/project_init.rs`: scaffold + assert
   files + run example via real Python.
6. Roadmap update — Plan 10 → ✅.

### Plan 11: docs restructure (no Rust changes)

Larger but mechanical. Depends on Plan 10 being merged.

1. `mkdocs.yml` nav update + empty new-page skeletons so internal
   links resolve as content lands.
2. `.pre-commit-hooks/regen-doc-snippets.py` + the fixture project it runs
   against.
3. Quickstart page (uses real `toolr project init` output via
   captured `.txt`).
4. Installation page (rewrite; install matrix; SLSA section).
5. Concepts page ("How toolr is laid out").
6. Writing commands chapter — port + restructure into focused
   sub-pages; carry the existing examples forward; add new ones for
   `Literal[…]`, nested groups, etc.
7. Project configuration page.
8. Single CLI reference page with anchors for every subcommand.
9. Third-party command packages page.
10. Internals chapter (manifest, cache, pre-commit, diagnostics).
11. API reference cleanup: delete private-module `_*` pages; rename
    public-module pages out of the `_*` directory.
12. Move `docs/examples/` content into the writing-commands chapter;
    delete the old `docs/examples/` folder.
13. CI: enable / verify `mkdocs build --strict` + snippet drift
    check.
14. Pre-commit hook for snippet drift (scoped to relevant paths).
15. Roadmap update — Plan 11 → ✅.

### Roadmap entries to add

In `specs/rust-front-end/01-roadmap.md`, append two new entries to the
sub-plans table:

- **Plan 10: `toolr project init` bootstrap command**
- **Plan 11: Documentation overhaul**

With the dependency edge: Plan 11 depends on Plan 10.

## Done criteria

The combined work is complete when:

- `toolr project init` runs in an empty repo and produces a runnable
  example with no follow-up commands.
- `tools/__init__.py` is **not** generated by `init` — local
  `tools/` works as a PEP 420 namespace package end-to-end.
- All integration tests for `init` pass on Linux + macOS in CI.
- `mkdocs build --strict` passes; no broken links, no missing
  snippets.
- The new IA pages all exist and link to one another correctly.
- The pruned `docs/reference/` exposes only the public API surface.
- `.pre-commit-hooks/regen-doc-snippets.py --check` runs cleanly in CI and as
  a pre-commit hook.
- The roadmap shows Plan 10 and Plan 11 as ✅ Done.

## Open questions (resolved in design)

- Whether `tools/__init__.py` is needed → **no**, PEP 420 namespace
  package validated.
- Whether `init` takes a path argument → **no**, always cwd.
- Auto-sync after scaffold → **yes** by default, `--no-sync` opt-out.
- Example content depth → **four commands** showing `ctx.print`,
  `ctx.run` (capture), `ctx.prompt` + `ctx.exit`, and `Literal[…]`.
- CLI reference layout → **single page with anchors**.
- Snippet format → real `.py` source files + `.txt` captured output,
  both included via `--8<--`.
- Snippet drift enforcement → **pre-commit hook (scoped) + CI**.
- Plan tracking location → continue numbering under
  `specs/rust-front-end/`.
- Plan order → Plan 10 (init) first; Plan 11 (docs) depends on it.

## Open questions (deferred — surface to plan authors)

1. **mkdocstrings filtering** — currently every `python/toolr/**/*.py`
   module gets an auto-generated reference page. The simplest fix is
   to delete the unwanted `_*` pages and let `mkdocs --strict` enforce
   no broken links. A cleaner fix is to filter at generation via
   `mkdocstrings`'s `members:` / `filters:` config. Plan 11 picks one
   at implementation time.
2. **Snippet regen runtime cost** — ~20 toolr invocations against a
   fixture project. Estimated <2 s warm. If profiling shows the pre-
   commit hook is slow enough to be annoying, scope the hook to a
   smaller path set or drop the pre-commit step entirely and rely on
   CI only.
3. **Fixture project location** — does the regen-snippets fixture
   live in `docs/.fixtures/` (next to the docs that use it) or in
   `tests/fixtures/` (alongside other test fixtures)? Plan 11 picks
   one at implementation time.
