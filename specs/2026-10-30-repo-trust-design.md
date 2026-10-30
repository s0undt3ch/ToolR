# Repository trust

- **Status:** design agreed, not implemented
- **Origin:** whole-project review findings F-33 (the provenance gate is
  satisfied by a committed fake venv) and F-34 (a committed manifest is
  trusted verbatim).
- **Plan:** [`2026-10-30-repo-trust-plan.md`](./2026-10-30-repo-trust-plan.md)
- **Precondition:** F-01 (bash completion runs `$(...)` from repo values)
  must be fixed first or in the same release. Otherwise "an untrusted repo
  is inert" is false.

## Problem

toolr runs repo-controlled code in two ways:

1. `uv sync` against `tools/pyproject.toml`. uv honours the repo's
   `[tool.uv]` settings, sources and build backends.
2. Importing `tools/*.py` in the runner.

The only guard today is the interpreter provenance gate
(`crates/toolr-core/src/venv/provenance.rs::verify_interpreter`). It
can't stop a hostile repo:

- It runs after `uv sync` has already happened.
- A hostile repo doesn't need a fake interpreter. It puts its payload in
  `tools/*.py`.
- Its structural fallback (`validate_venv`) accepts a `#!/bin/sh` script
  plus an empty `site-packages/toolr/__init__.py`.
- It's skipped on purpose for repos without `tools/pyproject.toml`
  (`crates/toolr/src/dispatch.rs`, #588), which run `tools/*.py` on the
  host Python with no check at all.

## Decision

toolr refuses to run a repository's code until the user has trusted that
repository once. This follows `mise trust`.

- **Trust key: the canonical repository root path only.** It is the path
  `discover_project_root` returns, which is already canonicalised. No
  content hash: a `git pull` of hostile `tools/*.py` is outside what any
  key could cover, and hashing `pyproject.toml`/`uv.lock` would re-prompt
  on every dependency bump (five times in this repo in October 2026).
- **Trust is a one-way door.** Once trusted, repo code runs as the user and
  can do anything the user can, including editing the trust store. The docs
  say so.

### What is gated

| Path | Gated | Why |
|---|---|---|
| Running a user command, with or without `tools/pyproject.toml` | yes | imports `tools/*.py`; may `uv sync` |
| `toolr project venv sync`, `lock`, `add`, `remove`, `shell`, `run` | yes | `uv` against repo config, or runs venv binaries |
| `toolr project venv run --no-sync` | yes | runs venv binaries; bypasses `ensure_venv_ready`, so needs its own check |
| `toolr project init` | no | the user is authoring the repo; **records trust after writing its files and before its `uv sync`**, which goes through `ensure_venv_ready` and would otherwise be refused |
| `--help`, bare `toolr`, `toolr project manifest rebuild`, `toolr self build-manifest` | no | static parse only; nothing executes |
| `toolr __complete` (TAB) | no | static parse only, once F-01 is fixed |
| `toolr self …` cache commands, `toolr project venv path` | no | read toolr's own state or print a path |

The uv auto-install is not repo-controlled and keeps its own consent.

### Check points

`require_trust(repo_root: &Path, mode: TrustMode) -> Result<()>` in
toolr-core, called in exactly three places:

1. At the top of `toolr_core::project::ensure_venv_ready`, right after
   `discover_project_root`. This covers every `project venv` subcommand
   that syncs, and dispatch's resync.
2. In `crates/toolr/src/dispatch.rs`, before the spec is written and before
   the venv/host-Python split. This covers the no-`pyproject.toml` path.
3. In `crates/toolr/src/project.rs` `venv_run`'s `--no-sync` arm.

### Decision flow in `require_trust`

1. `TOOLR_TRUST_ALL=1` in the environment → allowed.
2. A trust record exists for the canonical root → allowed.
3. **Migration:** a `meta.json` under toolr's *cache root* (via
   `cache::enumerate_caches(toolr_cache_dir())`) has `repo_path` equal to
   the canonical root → write a trust record, then allowed. Never read a
   `meta.json` from inside the repo. A prior sync or dispatch is prior
   consent.
4. stdin is a terminal and `--quiet` was not passed → prompt. `y`/`yes`
   writes a record and allows; anything else, including bare Enter and EOF,
   refuses.
5. Otherwise → refuse.

### Prompt and error text

Prompt (stderr):

```text
toolr hasn't run code from this repository before:
  <canonical root>
Running its commands executes tools/*.py and syncs tools/pyproject.toml with uv.
Trust it? [y/N]
```

Refusal error:

```text
refusing to run code from an untrusted repository: <canonical root>
Run `toolr project trust` there once to allow it, or set TOOLR_TRUST_ALL=1 for automation.
```

The refusal exits non-zero. It must never match a benign marker in
`venv_sync_unattended_quiet_exit` (`crates/toolr/src/project.rs`), which
would turn it into exit 0 under `--quiet`.

### Commands

New subcommands go under existing reserved namespaces. A new top-level
`toolr trust` would reserve a group name and break any user who already has
a command group called `trust`.

- `toolr project trust [PATH]`: discover the root from `PATH` (default
  cwd) and record trust. Prints the root.
- `toolr project untrust [PATH]`: remove the record. Not an error when
  there's no record.
- `toolr self trusted [--prune]`: list trusted roots, marking ones whose
  path no longer exists. `--prune` removes those.

### Storage

- Directory: `$XDG_STATE_HOME/toolr/trust/` if `XDG_STATE_HOME` is set,
  otherwise `dirs::state_dir()/toolr/trust/`, otherwise
  `toolr_data_dir()/trust/`. (`dirs::state_dir()` is `None` on macOS and
  Windows; check this during implementation.) Add the helper as
  `toolr_state_dir()` beside `toolr_data_dir`/`toolr_cache_dir` in
  `crates/toolr-core/src/uv/mod.rs`.
- **Never the cache directory.** `self cache prune` and `rm -rf ~/.cache`
  must not drop trust.
- One file per repo: `<blake3 hex of the canonical root's bytes>.json`,
  containing `{"schema_version": 1, "repo_path": …, "trusted_at": …,
  "toolr_version": …}`.
- Write with tempfile + rename. No lock file is needed. Directory mode
  0700, file mode 0600 on Unix.
- Refuse to read or write a store path that resolves inside the repository
  being checked.

### The provenance gate goes

- Delete `crates/toolr-core/src/venv/provenance.rs` and its call in
  `dispatch.rs`.
- Stop writing `interpreter_path`/`interpreter_hash` in
  `project::finalize_sync`. Keep both fields on `Meta` with
  `#[serde(default)]` and mark them deprecated in the doc comment. No
  `meta.json` schema bump.
- Rewrite `crates/toolr/tests/untrusted_repo.rs` as trust tests. Keep its
  shape: a hostile repo whose code drops a sentinel; without trust, the
  sentinel never appears.

### CI and automation

- `TOOLR_TRUST_ALL=1` is the only bypass. No guessing from `CI=true`,
  because anything can set `CI`.
- The GitHub Action exports `TOOLR_TRUST_ALL=1` to `GITHUB_ENV`, beside
  `TOOLR_VENV_LOCATION`. The workflow has already checked out the code it
  is about to run.
- Contributors to a repo whose pre-commit hooks call toolr run
  `toolr project trust` once.

## Not covered (documented)

- A trusted repo that later pulls hostile `tools/*.py` or `[tool.uv]`
  changes.
- Editor and LSP integrations that import `tools/*.py` themselves.
- A compromised upstream package index used by a trusted repo.
- `toolr project init` in a directory that already contains someone else's
  files records trust for content the user didn't write.

## Release

- Breaking in practice: every repo without a toolr-built venv asks once.
  Minor bump (`0.X.0`).
- Conventional Commit type: `feat(security)`.
- `UNRELEASED.md`:

  > toolr now refuses to run a repository's commands or sync its tools venv
  > until you trust that repository once: run `toolr project trust` there,
  > or answer `y` at the prompt. Repositories where toolr already built a
  > venv are trusted automatically. In CI, set `TOOLR_TRUST_ALL=1`; the
  > `s0undt3ch/ToolR` GitHub Action sets it for you. `--help`, tab
  > completion and manifest builds never need trust. The interpreter
  > provenance check is removed; repository trust replaces it.

## Deferred

- Sharing trust between git worktrees of one repository.
- Passing `--no-config` to uv to ignore a repo's `[tool.uv]` settings
  (would break legitimate setups).
- Per-command trust.
