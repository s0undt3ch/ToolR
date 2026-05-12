# ToolR Rust Front-End — Implementation Roadmap

- **Tracks:** [Rust Front-End Design](./00-design.md)
- **Status:** Spec committed; sub-plans not yet drafted.
- **Purpose:** Bridge document between the design and the work-in-flight.
  Future sessions read this first to discover the decomposition into
  sub-plans, dependencies between them, and the current state of each.

## How to use this document

**If you're a contributor (human or Claude session) opening this repo cold:**

1. Read this roadmap first. The status table tells you where every sub-plan
   stands.
2. Pick the next plan to act on. Options at any moment are:
    - Draft a plan doc for a `⬜ Not Started` entry whose dependencies are
      satisfied.
    - Implement a `📝 Drafted` plan (per its plan doc).
    - Continue a `🔧 In Progress` plan.
3. As you complete work, **update this roadmap** in the same commit (or PR):
    - Move a plan from `⬜` → `📝` when its plan doc is written and committed.
    - Move from `📝` → `🔧` when implementation starts.
    - Move from `🔧` → `✅` when the plan's implementation is merged into the
      design branch (or main, depending on the merge strategy).
    - Fill in the plan-doc path under each entry as it gets written.

**Status legend:**

- ⬜ Not Started
- 📝 Plan Drafted (plan doc written, implementation not yet begun)
- 🔧 In Progress (implementation underway)
- ✅ Done (implementation merged)

## Sub-plans

### Plan 1: Rust binary skeleton + static manifest layer

- **Status:** ✅ Done
- **Plan doc:** [02-plan-1-rust-skeleton.md](./02-plan-1-rust-skeleton.md)
- **Depends on:** —
- **Unblocks:** Plans 2, 4, 5
- **Produces:**
    - A `toolr` Rust binary that can be invoked from the shell.
    - Static AST parsing of `tools/**/*.py` via `ruff_python_parser`,
      including local enum / `typing.Literal[...]` resolution with a
      cross-file symbol table.
    - Manifest read/write at `tools/.toolr-manifest.json` with
      `schema_version`, `static_hash`, `dynamic_hash`, `groups`, `commands`.
    - clap-based CLI parsing that constructs subcommand structure from the
      loaded manifest.
    - Manifest path discovery (walk up from `cwd` looking for a `tools/`
      directory or configured root).
    - `toolr --help`, `toolr --version`, `toolr <user-group> --help`,
      `toolr <user-group> <command> --help` all functional.
    - At this point execution is **not** wired up: invoking a real user
      command exits with "execution backend not yet implemented." That's
      Plan 2's job.

### Plan 2: Python runner + execute model (S1)

- **Status:** ✅ Done
- **Plan doc:** [03-plan-2-runner-execute.md](./03-plan-2-runner-execute.md)
- **Depends on:** Plan 1
- **Unblocks:** Plans 3, 7
- **Produces:**
    - `toolr._runner` Python module (in the Python package): reads spec from
      `$TOOLR_SPEC_FILE`, decodes via `msgspec.json` with schema
      validation, imports the named module, reconstructs `Context`, calls the
      target function with parsed args.
    - Rust side: tempfile spec write (`tempfile::NamedTempFile`), subprocess
      spawn of `python -m toolr._runner` with inherited stdio and the
      `TOOLR_SPEC_FILE` env var.
    - Signal forwarding (SIGINT, SIGTERM) from Rust binary to Python
      subprocess.
    - Exit-code propagation.
    - End-to-end smoke test: invoking a real `tools/ci.py` command produces
      identical behavior to today's argparse-driven path.

### Plan 3: Tools venv + uv integration

- **Status:** ✅ Done
- **Plan doc:** [04-plan-3-venv-uv.md](./04-plan-3-venv-uv.md)
- **Depends on:** Plan 2
- **Unblocks:** Plans 6, 7, 8
- **Produces:**
    - uv discovery sequence: PATH check → `$XDG_DATA_HOME/toolr/bin/uv` →
      consented install prompt → refusal handling.
    - Tools venv resolution: default cache location
      `$XDG_CACHE_HOME/toolr/<repo-key>/venv/`, opt-in in-tree at
      `tools/.venv/` via `[tool.toolr] venv-location = "in-tree"`.
    - `tools/pyproject.toml` + `tools/uv.lock` recognition; `uv sync`
      invocation; venv freshness detection via lock mtime.
    - Validation that the `toolr` Python package is installed in the venv
      (refuses to operate otherwise).
    - Best-effort `editable-install = ["."]` post-sync hook.
    - `toolr project deps sync` (force a full uv sync).
    - `toolr project venv path` (print resolved venv path).
    - `toolr project venv shell` (spawn subshell with venv activated).

### Plan 4: Shell completion

- **Status:** 📝 Drafted
- **Plan doc:** [05-plan-4-completion.md](./05-plan-4-completion.md)
- **Depends on:** Plan 1
- **Unblocks:** —
- **Produces:**
    - Hidden `toolr __complete <cwd> <args>` endpoint that reads the
      manifest, prefix-matches against subcommands and arg values, and
      writes candidates to stdout.
    - Tab-time freshness check: hash `tools/**/*.py`, compare to
      `manifest.static_hash`. On mismatch, re-parse via
      `ruff_python_parser` on the fly (sub-50 ms) and serve those results;
      optionally write back the updated manifest asynchronously.
    - Value completion from local `Literal[...]` / `enum.Enum`
      definitions in `tools/`.
    - `toolr self completion install [shell]` writes completion scripts to
      the standard locations for `bash`, `zsh`, and `fish`.
    - `toolr self completion print [shell]` writes the script to stdout.

### Plan 5: Third-party static manifest convention + `toolr.build`

- **Status:** 📝 Drafted
- **Plan doc:** [06-plan-5-static-third-party.md](./06-plan-5-static-third-party.md)
- **Depends on:** Plan 1
- **Unblocks:** —
- **Produces:**
    - Rust side: glob discovery of
      `<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json` during
      manifest build; schema validation (mandatory
      `toolr_schema_version`); merge into the static manifest.
    - Schema-version migration framework so older manifest fragments can be
      transformed in-process when an older fragment meets a newer toolr
      binary.
    - `toolr.build` Python module that introspects a package's
      `command_group` / `@group.command` registry and emits
      `toolr-manifest.json` at the package root.
    - `python -m toolr.build <package-name>` CLI entrypoint.
    - `toolr self build-manifest <package-name>` Rust-side wrapper that
      locates a Python interpreter and runs the build helper.
    - `--check` mode for CI: regenerate in-memory and diff against the
      committed file, exit non-zero on drift.

### Plan 6: Dynamic manifest layer

- **Status:** 📝 Drafted
- **Plan doc:** [07-plan-6-dynamic-manifest.md](./07-plan-6-dynamic-manifest.md)
- **Depends on:** Plan 3
- **Unblocks:** —
- **Produces:**
    - Python introspection subprocess that imports `tools.*` modules in the
      tools venv, walks the registry, enumerates
      `importlib.metadata` entry points, dumps everything as JSON for the
      Rust side to merge into the manifest.
    - Dynamic-layer hash over the installed package set so toolr knows when
      to regenerate.
    - `toolr project manifest rebuild` command that runs both static and
      dynamic layers and writes the merged manifest.
    - Shipped `pre-commit-hooks.yaml` entry that runs
      `toolr project manifest rebuild` on changes under `tools/`.
    - Auto-rebuild of the dynamic layer at execute time when the dynamic
      hash is stale.

### Plan 7: Missing-dependency diagnostics

- **Status:** 📝 Drafted
- **Plan doc:** [08-plan-7-missing-deps.md](./08-plan-7-missing-deps.md)
- **Depends on:** Plan 2, Plan 3
- **Unblocks:** —
- **Produces:**
    - Pre-flight check before spawning Python: for every top-level import in
      the target command's source file (recorded by the static parser),
      probe `<tools-venv>/.../site-packages/<module>/__init__.py` or
      `<module>.py`. If any are missing, fail fast with the generic
      diagnostic message.
    - Post-mortem interception: when the Python subprocess exits with an
      `ImportError`, capture the message and append the same
      `toolr project deps sync` suggestion to the user-visible error.
      Preserve the original traceback.

### Plan 8: Cache management

- **Status:** 📝 Drafted
- **Plan doc:** [09-plan-8-cache.md](./09-plan-8-cache.md)
- **Depends on:** Plan 3
- **Unblocks:** —
- **Produces:**
    - `meta.json` written on venv creation with `repo_path`,
      `toolr_version`, `python_version`, `created_at`, `last_used_at`.
    - `last_used_at` mtime touch on every toolr invocation.
    - `toolr self cache list` — tabular output with origin repo, size,
      last use.
    - `toolr self cache prune` — remove orphans (repo no longer at recorded
      path) and stale (last_used_at older than configurable threshold,
      default 30 days).
    - `toolr self cache prune --all` — nuke everything.
    - Passive size-hint emitted on any invocation when cache exceeds the
      configured threshold or orphan count.

### Plan 9: Distribution + backwards compatibility

- **Status:** 📝 Drafted
- **Plan doc:** [10-plan-9-distribution.md](./10-plan-9-distribution.md)
- **Depends on:** Plans 1–8 substantially complete
- **Unblocks:** —
- **Produces:**
    - maturin build configuration for the Rust binary as a wheel-installed
      `bin` target — `pip install toolr` ships both the Python package and
      the Rust binary.
    - GitHub release archives:
      `toolr-<target-triple>.tar.gz` for each supported platform; checksums.
    - Cross-platform installer script (`curl ... | sh`).
    - Updated mise plugin at `toolr-mise/` that fetches the new binary
      archives.
    - `python -m toolr` deprecation shim: locates the `toolr` binary on
      PATH and execs it with the original argv, after printing a one-time
      deprecation note to stderr.
    - Smoke tests in CI for each install channel.

## Dependency graph

```text
                                 Plan 1
                       (Rust skeleton + static manifest)
                                   │
        ┌──────────────────────────┼──────────────────────────┐
        ▼                          ▼                          ▼
     Plan 2                      Plan 4                     Plan 5
   (Runner + execute)         (Completion)            (3p static + build)
        │
        ▼
     Plan 3
   (Venv + uv)
        │
        ├─────────────┬──────────────┐
        ▼             ▼              ▼
     Plan 6        Plan 7         Plan 8
   (Dynamic)     (Missing dep)    (Cache)

                          ▼ (everything above substantially done)

                        Plan 9
                  (Distribution + back-compat)
```

## Cross-cutting concerns (apply to every plan)

- **Tests first.** Every plan uses TDD: write the failing test, then the
  minimal code to pass, commit. The plan docs make this concrete.
- **Conventional commits.** Style follows existing repo history:
  `feat(scope): subject`, `fix(scope): subject`, `docs(scope): subject`.
- **Pre-commit hooks are enforced.** The repo has typos, codespell,
  markdownlint, ruff, mypy, clippy, and others wired up. Plans must
  produce code that passes these.
- **Backwards compatibility.** Existing `tools/*.py` files using
  `command_group()` and `@group.command` must work unchanged through all
  intermediate states.
- **No code in roadmap.** This file tracks state and decomposition only.
  All implementation guidance lives in each plan's plan doc.
