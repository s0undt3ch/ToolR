# CLAUDE.md

Agent context for this repo. For human-contributor setup see [`CONTRIBUTING.md`](./CONTRIBUTING.md).

This file is canonical for agent behavior in this repo. Rules here override the
agent's default skills and memory when they conflict.

---

## Facts

### Workspace layout

Cargo workspace, four crates, one Python package:

- `crates/toolr-core/` — pure-Rust lib (parser, manifest, freshness, completion engine). No `pyo3`.
- `crates/toolr/` — the `toolr` binary (clap CLI, dispatch, subprocess control).
- `crates/toolr-py/` — `pyo3` extension + Python source at `crates/toolr-py/python/toolr/`.
  Ships as the `toolr-py` wheel.
- `crates/xtask/` — workspace automation; `cargo xtask <task>`.

Two wheels ship at the same version: `toolr` (binary-only) and `toolr-py` (Python + extension).

### Commands

All tooling is driven by [mise](https://mise.jdx.dev/) — never `brew install` Python/Rust/uv directly.

```sh
mise install                      # pinned tool versions from mise.toml
uv sync --all-extras --dev        # Python deps
prek install --install-hooks      # pre-commit hooks (prek, not pre-commit)

mise run test                     # umbrella: skill-refs drift gate + cargo test --workspace + pytest
cargo test -p toolr-core          # Rust unit
cargo test -p toolr --test '*'    # Rust integration (assert_cmd against the built binary)
uv run pytest                     # Python unit
uv run pytest -m distribution     # slow, opt-in; builds real wheels

cargo run -p toolr -- --help                                   # dogfood the dev binary
cargo run -p toolr -- self build-manifest toolr_example_plugin

prek run --all-files              # run every pre-commit hook
```

### Where things live

- Release notes queue: `UNRELEASED.md` (folded into `CHANGELOG.md` on release).
- **Specs (live):** `specs/<YYYY-MM-DD>-<topic>-design.md` and `<…>-plan.md`.
  Override the brainstorming-skill default of `docs/superpowers/specs/` — this repo uses top-level
  `specs/`. See `specs/README.md`.
- **Specs (archive):** `specs/archive/<year>/`. Move with `git mv` in the implementing PR (see
  *Archive specs as the last implementation step* below).
- Built-in completion entries: `crates/toolr/src/builtin_completions.rs` (derived from `cli::build_command`).
- Static manifest parser: `crates/toolr-core/src/parser/`.
- Tab-completion freshness: `crates/toolr-core/src/complete/freshness.rs`.
- Reference plugin / example: `examples/plugin-package/`.
- Pre-commit hooks (project-local): `.pre-commit-hooks/`.
- Skills shipped from this repo:
  `skills/toolr-ci-setup/`, `skills/toolr-command-authoring/`, `skills/toolr-command-packaging/`.

---

## Working rules

### Decisions (when)

- **Scale verification to PR scope.** Doc-only PR → `prek run --all-files` + `mkdocs build --strict`
  covers it. Touched Rust or Python? Run the full umbrella `mise run test`.
- **Monitor long `cargo test --workspace` runs.** They can stall. Poll output every 30–60s rather
  than fire-and-forget.
- **Brainstorming writes the design, then the plan.** `/superpowers:brainstorming` saves to
  `specs/<YYYY-MM-DD>-<topic>-design.md`; `/superpowers:writing-plans` saves to
  `specs/<YYYY-MM-DD>-<topic>-plan.md`. Never `docs/superpowers/specs/`.

### Actions (how)

- **Bump `RUNNER_SCHEMA_VERSION` and `SCHEMA_VERSION` together** when changing the runner JSON spec
  on either side:
    - `crates/toolr-core/src/execute/spec.rs::RUNNER_SCHEMA_VERSION`
    - `crates/toolr-py/python/toolr/_runner.py::SCHEMA_VERSION`

  Each carries a doc comment listing which changes require a bump. CI fails when they disagree.
- **Queue release notes in `UNRELEASED.md`. Never hand-edit `CHANGELOG.md`** — `git-cliff`
  regenerates it on release from Conventional Commits.
- **Regenerate doc snippets, don't hand-edit them.** `.pre-commit-hooks/regen-doc-snippets.py`
  captures `toolr` output into `docs/**/*.txt` from `docs/.fixtures/sample-repo/`.
- **Regenerate skill refs after public-surface changes.** `cargo xtask build-skill-refs --check`
  runs first in `mise run test` and CI. Regen with `cargo xtask build-skill-refs` and commit.
- **Stacked PRs use [git-spice](https://abhinav.github.io/git-spice/).** `git-spice branch create`
  and `git-spice branch submit --draft`. Don't `git checkout -b` + `git push` directly.
- **Conventional Commits** (`feat(cli): …`, `fix(parser): …`, `docs(internals): …`).
  `git-cliff` reads these on release.
- **No `--no-verify`** without a stated reason in the commit body. Pre-commit failures are signals.
- **Python tests: factory fixtures over bare helpers** for `tmp_path`-based setup. Keep test
  imports top-of-file.
- **Archive specs as the last implementation step.** When the implementing PR is otherwise ready,
  `git mv specs/<…>-design.md specs/archive/<year>/` (and the matching `-plan.md`). The archive
  move is the final commit before opening the PR — same PR as the implementation, not a follow-up.

### Off-limits (what not)

- **No cross-repo sharing infrastructure.** Declined. `toolr doctor` is the only live Option-C candidate.
