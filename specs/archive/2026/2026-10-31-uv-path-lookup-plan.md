# uv PATH Lookup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An empty or relative `PATH` entry no longer makes toolr run a `uv` or `python3` from the current directory
(F-35).

**Architecture:** A crate-private `absolute_search_path` helper in toolr-core filters `PATH` to absolute entries. The uv
lookup (`which_uv_in`) and the Python lookup (`resolve_python_in`) both go through it.

**Tech Stack:** Rust, `which` 8, `assert_cmd`.

**Spec:** [`specs/2026-10-31-uv-path-lookup-design.md`](./2026-10-31-uv-path-lookup-design.md)

## Global Constraints

- Keep only entries where `Path::is_absolute()` is true. No `~` expansion.
- `absolute_search_path` returns `None`, never `Some("")`, when no entry survives.
- No schema change.
- Integration tests that plant executables are `#[cfg(unix)]`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **All entries filtered out.** `PATH=":"` or `PATH="."` must not turn into `""`, which `which` reads as the cwd. Task
   1, `absolute_search_path_returns_none_when_nothing_is_absolute`, and Task 2's `PATH=""`-style cases.
2. **A trailing `:`**, the common accident. Task 2, `trailing_colon_does_not_run_repo_uv`.
3. **The planted uv must not even get `--version`.** Asserting only that `lock` didn't run would miss the probe. Task 2
   asserts the stub's argv log is empty.
4. **Python lookup with no tools venv.** Same bug class through `which`. Task 1,
   `resolve_python_skips_relative_path_entries`.
5. **Order kept.** The first absolute entry with a `uv` still wins. Task 1,
   `absolute_search_path_keeps_absolute_entries_in_order`.

---

### Task 1: Filter `PATH` in both lookups

**Files:**

- Create: `crates/toolr-core/src/path_search.rs`
- Modify: `crates/toolr-core/src/lib.rs` (add `mod path_search;`, private)
- Modify: `crates/toolr-core/src/uv/discover.rs` (`which_uv_in`; the `which_uv` doc comment)
- Modify: `crates/toolr-core/src/execute/python.rs` (`resolve_python_in`; the `resolve_python` doc comment)
- Test: `path_search.rs` `mod tests`, `discover.rs` `mod tests`, `python.rs` `mod tests`

**Interfaces:**

- Produces: `pub(crate) fn absolute_search_path(path: Option<OsString>) -> Option<OsString>` in
  `crate::path_search`.

- [ ] **Step 1: Write the failing tests.**
    - `path_search.rs` (build `PATH` strings with `std::env::join_paths` so they work on Windows too):
        - `absolute_search_path_drops_empty_and_relative_entries`: `[abs_a, "", ".", "bin", "~/bin", abs_b]` →
          `split_paths` of the result is exactly `[abs_a, abs_b]`.
        - `absolute_search_path_keeps_absolute_entries_in_order`: `[abs_b, abs_a]` → `[abs_b, abs_a]`.
        - `absolute_search_path_returns_none_when_nothing_is_absolute`: `Some("")`, `Some(".")`, `Some(":")` (unix
          only) → `None`.
        - `absolute_search_path_returns_none_for_unset_path`: `None` → `None`.
    - `discover.rs`: no new unit test. `which_uv_in` checks `is_file()` against the process cwd, which a unit test
      can't change safely, so Task 2 proves the uv side through the binary with `current_dir`.
    - `python.rs`: `resolve_python_skips_relative_path_entries` (`#[cfg(unix)]`): a `python3` stub that prints
      `Python 3.13.0` in `tmp/rel`, `resolve_python_in(None, Some(".".into()), &tmp.path().join("rel"))` →
      `Err(PythonError::NotFound)`. Before the fix `which_in` resolves `.` against the passed `cwd` and finds the stub.

- [ ] **Step 2: Run them and confirm they fail.**
      Write `absolute_search_path` as a stub returning `path` unchanged, then run
      `cargo test -p toolr-core --lib path_search:: uv::discover:: execute::python::` → the new `path_search` tests
      and `resolve_python_skips_relative_path_entries` fail, and the old ones pass.

- [ ] **Step 3: Implement.** Write `absolute_search_path` as the spec describes: `split_paths`, `filter(is_absolute)`,
      collect, `None` if empty, else `join_paths(..).ok()`. In `which_uv_in`, replace `std::env::split_paths(&path?)`
      with `std::env::split_paths(&absolute_search_path(path)?)`. In `resolve_python_in`, compute
      `let path = absolute_search_path(path);` once at the top and pass it to both `which_in` calls. Update both doc
      comments to say relative and empty entries are skipped.

- [ ] **Step 4: Run** the Step 2 command → PASS. Then `cargo clippy -p toolr-core --all-targets -- -D warnings` → clean.

- [ ] **Step 5: Commit.** `fix(uv): skip relative PATH entries when looking up uv and Python`

### Task 2: Prove it through the binary

**Files:**

- Create: `crates/toolr/tests/uv_path_relative_entries.rs` (`#![cfg(unix)]`)

**Interfaces:**

- Consumes: `common::VenvFixture` (`root`, `bin_dir`, `uv_argv()`), `common::toolr()`.

- [ ] **Step 1: Write the tests.** A factory `planted_repo() -> (VenvFixture, TempDir)`: a `VenvFixture`, its stub uv
      copied (with mode `0o755`) to `fx.root.join("uv")` and `fx.root.join("rel-bin").join("uv")`, then
      `fx.bin_dir/uv` removed so no absolute `PATH` entry has a uv. The `TempDir` is an empty data dir. A helper
      `run(fx, data, path: &str) -> Output` runs `common::toolr()` with `current_dir(&fx.root)`, `env("PATH", path)`,
      `env("XDG_DATA_HOME", data)`, `env_remove("TOOLR_AUTO_INSTALL_UV")`, args
      `["project", "venv", "lock", "--quiet"]`. Each test asserts: not success, stderr contains
      `TOOLR_AUTO_INSTALL_UV=1` (the existing no-uv hint), and `fx.uv_argv()` is empty.
    - `empty_path_entry_does_not_run_repo_uv`: `PATH` = `:/usr/bin:/bin`.
    - `trailing_colon_does_not_run_repo_uv`: `PATH` = `/usr/bin:/bin:`.
    - `dot_path_entry_does_not_run_repo_uv`: `PATH` = `.:/usr/bin:/bin`.
    - `relative_path_entry_does_not_run_repo_uv`: `PATH` = `rel-bin:/usr/bin:/bin`.

- [ ] **Step 2: Confirm they would have failed.** `git stash` Task 1's source changes (keep the new test file), run
      `cargo test -p toolr --test uv_path_relative_entries` → all four fail with a non-empty argv log. `git stash pop`.

- [ ] **Step 3: Run** `cargo test -p toolr --test uv_path_relative_entries` → PASS.

- [ ] **Step 4: Commit.** `test(uv): planted repo uv never runs through relative PATH entries`

### Task 3: Docs, release notes, verification, archive

- [ ] **Step 1:** Make the spec's `docs/installation/index.md` edits and add its `UNRELEASED.md` text verbatim.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green.
- [ ] **Step 3:** Commit `docs(uv): document that relative PATH entries are skipped`.
- [ ] **Step 4:** `git mv specs/2026-10-31-uv-path-lookup-design.md specs/2026-10-31-uv-path-lookup-plan.md
      specs/archive/2026/`; fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive uv path lookup design and plan`.
