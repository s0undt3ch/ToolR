# Codebase audit — 2026-05-23

**Status:** Snapshot, post-presentation-pass + post-dead-code-cleanup.
**Author:** Claude (interactive session).
**Method:** Fresh-eyes pass after surface-level cleanup (PRs #237–#241) was already in flight. Citations as `path:line` everywhere. Line numbers reflect `main` at the time of writing (HEAD `d26e41b`).

## Summary

The project is in **genuinely good shape**. The presentation pass (PRs #237–#239) + the dead-code sweep (PR #241) + the preflight error fix (PR #240) cleaned the most visible surface. What remains isn't "broken" — it's a coherent codebase whose internal structure could be tightened in a few specific spots. There is **one real flake** (a hung test) that's been there the whole time; treat it as urgent.

## Good

- **Schema-versioning discipline.** `RUNNER_SCHEMA_VERSION` (`crates/toolr-core/src/execute/spec.rs`) ↔ `SCHEMA_VERSION` (`crates/toolr-py/python/toolr/_runner.py:49`) lock-step is CI-enforced via `crates/toolr-core/tests/schema_version_lockstep.rs`. The doc-comment on each constant lists which changes require a bump and which don't. The kind of cross-language guard most projects don't bother with; load-bearing here and clearly authored on purpose.
- **Pre-flight + post-mortem `deps_check`** (`crates/toolr-core/src/deps_check/`) — structurally clever. Filesystem-only probe before running, traceback-parsing intercept after, both producing the same user-actionable hint. Real UX win for the common "missing PyPI dep" case.
- **Two-layer manifest.** Static AST extraction in `crates/toolr-core/src/parser/`, dynamic introspection in `crates/toolr-core/src/dynamic/`. The split lets `--help` and Tab completion stay sub-50ms by reading the cached static layer, while still catching runtime-generated commands via the dynamic helper. The freshness logic (`crates/toolr-core/src/freshness/`) hashing only `toolr-manifest.json` files (not full dist-info) is genuinely smart — unrelated `uv pip install` doesn't invalidate the cache.
- **Crate boundaries are clean.** `toolr-core` has no `pyo3`; `toolr` is just the binary; `toolr-py` is the pyo3 dynlib + Python source. Each crate has one job.
- **`toolr bench` ships in-tree** (`tools/bench.py`). The project measures its own competitive position, against `invoke`, `doit`, `nox`, `duty`, `python-tools-scripts`. README's headline numbers come from a command anyone can run.
- **Public API discipline.** `toolr.__all__` in `crates/toolr-py/python/toolr/__init__.py` is a hard contract. The 14 Rust integration tests in `crates/toolr/tests/` exercise the real binary via `assert_cmd`, not internal symbols.
- **SLSA attestation + multiple install paths** with verification flags (`installation/install.sh`, `installation/install.ps1`). The release surface is mature for a pre-1.0 project.

## Bad

- **`crates/toolr-core/src/parser/types.rs` — 1376 lines, 38 top-level items.** Mixes `PathConstraints`, `SupportedType`, `UnsupportedType`, `TypeImports`, `SourcesImports`, arg-metadata extraction, literal extraction, and type resolution. Could plausibly split into `path_constraints.rs`, `supported_type.rs`, `imports.rs`, `arg_metadata.rs`, `resolve.rs`, `literal.rs`. The one file where I had to scroll-grep just to find what I was looking for.

- **`crates/toolr-core/src/command/command_test.rs::test_no_output_timeout`** (lines 631–692) — **the hang**. Spawns bare `python` (not `python3` or `$TOOLR_TEST_PYTHON`). When `python` isn't on PATH (`mise` installs as `python3.14.5`; bare `python` may not symlink), the spawn fails silently, the initial-output pipe never gets written, and `handle.join()` waits forever for a thread blocked on `read_pipe`. **The full `cargo test --workspace` hangs indefinitely on this test on any clean machine without the `python` symlink.** Real bug, fix priority: high.

    The sibling `test_command_timeout` (~line 620) likely has the same vulnerability.

    Suggested fix:
    1. Try `python3` first, then `python`, then skip with a clear message.
    2. Wrap `handle.join()` in a wall-clock guard (`Duration::from_secs(5)`) so a future regression of the same shape fails the test instead of hanging forever.

- **`crates/toolr/src/cli.rs` (836 lines) and `crates/toolr/src/execute_build.rs` (931 lines).** Both do multiple things — `cli.rs` mixes help styling, group-subtree construction, dispatcher injection; `execute_build.rs` mixes spec packing, output-options resolution, dispatch-spec building. Each could naturally split into 2–3 files.

- **Test discovery is fragmented.** Python tests at `tests/` (pytest), Rust integration tests at `crates/toolr/tests/*.rs`, Rust unit tests inline as `#[cfg(test)] mod tests` and `crates/toolr-core/src/.../tests.rs`. A new contributor doesn't have one obvious "run all tests" entry beyond `prek run --all-files`. The CONTRIBUTING table helps, but a single `mise tasks test` (or equivalent) that runs all of them would be welcome.

- **`crates/toolr-py/python/toolr/testing.py` ships in the production wheel** despite being a test-helper module (`CommandsTester`). Its only importer is `tests/conftest.py:10`. Two reasonable choices: (a) keep + document publicly as a testing API for users — projects writing toolr commands could use it to test their own command discovery — or (b) move into a dev-only location and stop shipping. Current state is half-committed.

## Ugly

- **`crates/toolr-core/src/parser/` overall** — `types.rs` (1376) + `build.rs` (687) + `signatures.rs` (674) + `commands.rs` (610) + `groups.rs` (341) + smaller files ≈ 3600 lines doing AST traversal. The split is by-thing-extracted (commands vs groups vs signatures vs types), not by-pipeline-stage — so navigating "what happens when the parser sees `@group.command def f(...)`" hops across all six files. Refactor candidate.

- **`crates/toolr-py/python/toolr/utils/_signature.py` (723 lines)** — the largest Python file. Concentrates `ArgumentAnnotation`, all the `arg(...)` kwarg validation, deprecation warnings, mutually-exclusive group handling. The deprecation surface (lines 270–280) is real ongoing work, not vestigial.

- **`tools/bench.py` (561 lines)** is big for a single benchmark script. Well-built, but hard to split sensibly without losing the "one script you can read" property. Acceptable as-is; flagged for future review.

- **12 GitHub workflows** (`ci.yml`, `release.yml`, `codeql.yml`, `dependency-review.yml`, `install-smoke.yml`, `scorecards.yml`, `sync-rolling-tags.yml`, plus 5 `_*.yml` reusable workflows). All load-bearing on inspection, but the surface is dense. No dead workflow flagged; just a maintenance-cost note.

## Bugs found during this audit

1. **`test_no_output_timeout` hang** — see "Bad" above. Treat as urgent.
2. **`UvError::user_message` not wired into production paths** — fixed in PR #241.
3. **Preflight error message reported as bare "No such file or directory"** — fixed in PR #240.
4. **22 Plan/Task references in source comments** pointing at archived rust-front-end plans — fixed in PR #241.

## Plans to improve (prioritized)

### Plan 1 — Fix the hung test (S, safe, independent)

Patch `crates/toolr-core/src/command/command_test.rs::test_no_output_timeout` (and the sibling `test_command_timeout` at ~line 620). Resolve python via `which("python3").or(which("python"))`, skip if neither found, wrap `handle.join()` in a 5s wall-clock guard so a future regression fails fast. ~30 min. Unblocks running `cargo test --workspace` on any clean machine.

### Plan 2 — Split `parser/types.rs` (M, careful, independent)

1376 lines → 5–6 focused modules. Move tests with the items they cover. Re-export from a `types/mod.rs` to keep the public path stable. No behaviour change. Worth doing because future type-system additions (e.g. supporting Pydantic models, `Optional[T]` aliases) will land in whichever file is the right home — currently any of them could be.

### Plan 3 — `toolr doctor` (M, design first)

The candidate the user kept on the table. Single command that surfaces: uv missing/old, tools venv stale or missing, `tools/pyproject.toml` missing, manifest drift detected, `$XDG_CACHE_HOME/toolr/` cache health. Structured exit codes so CI can `toolr doctor || exit 1`. Brainstorming session first to nail the surface, then a small spec, then implementation.

### Plan 4 — Decide on `toolr.testing` (S, decision then maybe code)

Make the call once: either (a) keep it in the production wheel + document it under "Testing your commands" in the docs, with `CommandsTester` as a stable supported API, or (b) move it out of the wheel into a dev-only path. Current state — ships in production but doc'd nowhere user-facing — is the worst of both worlds.

### Plan 5 — Single `mise task test` entry point (S, safe, independent)

Add a top-level `mise.toml` task that runs `cargo test --workspace` + `uv run pytest` + the distribution opt-in suite. CONTRIBUTING already lists the four test families; consolidating behind one command makes "run everything before you push" trivially scriptable.

## Would I have done this differently?

A few things I'd change in retrospect about the cleanup session that produced this audit:

- **Start the session with `cargo test --workspace` baseline.** I didn't. If I had, the `test_no_output_timeout` hang would have surfaced in the first 5 minutes, not after 4 hours of unrelated cleanup. The hang has been there the whole time and I never noticed until late in the session when I tried to verify my own changes. Every clean cargo build / test run masked it because I was running narrow `-p` subsets.

- **Treat the original audit more like this deep dive from the start.** The initial 5-task audit at the brainstorming stage was docs-focused. I missed the code-level dead code (`report_uv_error`, the `_path_is_used` placeholder, the file-level `#![allow(dead_code)]` blanket, the 22 Plan/Task references). All of that would have surfaced from a `git grep -E '#!?\[allow|placeholder|Plan [0-9]|Task [0-9]'` run that took 10 seconds. I had to be prodded into it.

- **Run clippy with `-W dead_code -W unused_imports`** alongside the default lints. Default clippy doesn't flag `#[allow(dead_code)]`-guarded items; running with the warning re-enabled would have surfaced everything cleaned in PR #241 before I had to grep for it.

- **Don't trust audit-shaped subagent dispatches blindly.** The Explore-subagent audit I ran returned a confident "no dead code; codebase is clean" report. I almost shipped that and stopped. Going back and doing it myself was the right call. For future audit-shaped work, dispatch a subagent only for *parallel-friendly grunt work*, not for the "is this thing actually well-built" call.

- **The "presentation pass first" call was right.** The repo-presentation work made everything that followed (the PR-2 README, the bench numbers, the user-facing UX bug fix) more coherent. If I'd started with internal-code dead-code instead, the surface would still have lied.

In one line: the *order* of cleanup was correct (presentation → internal), but the *first audit* was too thin — should have been this deep dive's shape, not the original five bullet points.

## Related artifacts

- Presentation pass design + plan: `specs/archive/2026/2026-05-22-repo-presentation-pass-design.md` (after archival).
- Preflight error fix: PR #240.
- Dead-code cleanup pass: PR #241.
- Original audit memory (now archived): `specs/archive/2026/rust-front-end/01-roadmap.md`.
