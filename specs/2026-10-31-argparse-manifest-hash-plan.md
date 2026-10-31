# Argparse Manifest Hash Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Editing an argparse-scanned file or `tools/pyproject.toml` makes the manifest stale (F-16).

**Architecture:** `hash::compute_static_hash` hashes a list of static inputs (`tools/**/*.py`, `tools/pyproject.toml`,
argparse `scan_paths` matches). The scanner and the hash share `argparse::scan::expand_scan_paths`.

**Tech Stack:** Rust, `blake3`, `glob`, `toml`, `assert_cmd`.

**Spec:** [`specs/2026-10-31-argparse-manifest-hash-design.md`](./2026-10-31-argparse-manifest-hash-design.md)

## Global Constraints

- Stacks on `fix/symlinked-tools-files` (F-17): `list_python_files` already follows file symlinks. Don't change it
  here.
- Input keys are project-root-relative with `/` separators; sorted; deduplicated.
- Per-input hash bytes: `key || 0x00 || len(contents) as u64 LE || contents`.
- A config or glob error never fails the hash. It drops that block's scanned files; the pyproject bytes stay an input.
- No schema change.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Typo'd `scan_paths` glob.** Must not make `--help` and TAB fail on the hash. The build still reports it. Task 2,
   `static_hash_tolerates_bad_glob`.
2. **Invalid TOML in `tools/pyproject.toml`.** Same rule. Task 2, `static_hash_tolerates_invalid_pyproject`.
3. **A `scan_paths` glob matching files under `tools/`.** No double counting and no error. Task 2,
   `scanned_file_under_tools_is_hashed_once`.
4. **Repos without `tools/pyproject.toml`.** The hash is still computed from `*.py` alone. Task 2,
   `static_hash_without_pyproject`.
5. **Removing the block entirely.** Grafted commands must disappear without a manual rebuild. Task 1,
   `removing_scan_paths_drops_grafted_commands`.

---

### Task 1: Reproduce through the binary

**Files:**

- Test: `crates/toolr/tests/freshness_dispatch.rs`

**Interfaces:** none.

- [ ] **Step 1: Write the regression tests**, reproducing the spec's repro exactly. A helper
      `write_django_project(root: &Path)` writes `tools/pyproject.toml` (`[project]` plus the
      `[tool.toolr.argparse.django]` block with `scan_paths = ["apps/*/management/commands/*.py"]`, `django = true`,
      and `[[tool.toolr.argparse.django.attach]] parent = "django"`), `tools/dispatcher.py` (the docs' `django`
      dispatcher taking `*, dispatched: DispatchCommand`), and `apps/billing/management/commands/migrate.py` with a
      `"""Migrate the database."""` docstring and `--check`.
    - `grafted_command_help_reflects_scanned_file_edit`: `toolr django migrate --help` → stdout contains `--check`;
      append `--database`; run it again → stdout contains `--database`.
    - `removing_scan_paths_drops_grafted_commands`: `toolr django --help` → contains `migrate`; set `scan_paths` to
      `["nowhere/*.py"]` → `toolr django --help` succeeds and doesn't contain `migrate`.
- [ ] **Step 2: Run them.** `cargo test -p toolr --test freshness_dispatch grafted_ removing_scan_paths`.
      Expected: both FAIL (the old arguments and `migrate` persist).
      **Close criteria:** if both pass on an unmodified tree, the finding doesn't hold. Stop, commit nothing, and
      report F-16 closed with the test output. Otherwise keep the tests and go on.

### Task 2: Hash every static input

**Files:**

- Modify: `crates/toolr-core/src/argparse/scan.rs` (extract `expand_scan_paths` from `scan_block_paths`)
- Modify: `crates/toolr-core/src/hash.rs` (`StaticInput`, `static_inputs`, `compute_static_hash`; remove
  `hash_tools_dir`; tests)
- Modify: `crates/toolr-core/src/parser/build.rs` (`build_static_manifest_inner`)
- Modify: `crates/toolr-core/src/freshness/compare.rs`, `crates/toolr-core/src/freshness/tests.rs`
- Modify: `crates/toolr/tests/cli_smoke.rs`, `crates/toolr/tests/freshness_dispatch.rs` (callers of `hash_tools_dir`)

**Interfaces:**

- Produces:
    - `pub fn expand_scan_paths(root: &Path, scan_paths: &[String]) -> Result<Vec<PathBuf>, ScanError>`: matches that
      are files, sorted, deduplicated.
    - `pub(crate) struct StaticInput { pub key: String, pub path: PathBuf }`
    - `pub(crate) fn static_inputs(tools_dir: &Path) -> Vec<StaticInput>`
    - `pub fn compute_static_hash(tools_dir: &Path) -> anyhow::Result<String>`
  F-30 (`fix/tab-manifest-freshness`) builds its stat gate on `static_inputs`.
- [ ] **Step 1: Write the tests.**
    - `scan.rs`: `expand_scan_paths_matches_scan_block_paths`: the existing
      `scan_paths_expands_globs_and_skips_unparsable` fixture → returned paths are a superset of the scanned commands'
      source files, sorted.
    - `hash.rs`, with a helper `project(files: &[(&str, &str)]) -> TempDir` rooted at the project (not `tools/`):
        - `static_hash_changes_when_scanned_file_changes`: Task 1's layout; hash, edit `migrate.py`, hash → different.
        - `static_hash_changes_when_pyproject_changes`: edit only `tools/pyproject.toml` → different.
        - `static_hash_ignores_unmatched_files_outside_tools`: add `apps/billing/notes.txt` → same.
        - `static_hash_tolerates_bad_glob`: `scan_paths = ["apps/[/x.py"]` → `Ok`.
        - `static_hash_tolerates_invalid_pyproject`: `tools/pyproject.toml` = `"not = = toml"` → `Ok`.
        - `scanned_file_under_tools_is_hashed_once`: `scan_paths = ["tools/*.py"]` → `static_inputs` keys are unique.
        - `static_hash_without_pyproject`: only `tools/a.py` → `Ok`, and differs after editing `a.py`.
        - Port the existing `hash_tools_dir` tests to `compute_static_hash` (same assertions).
    - `freshness/tests.rs`: `returns_static_drift_when_scanned_file_changed_without_a_venv`: manifest built with
      `build_static_manifest`, edit the scanned file → `compare(Some(&m), &tools, None)` == `StaticDrift`.
- [ ] **Step 2: Run them.** `cargo test -p toolr-core --lib hash:: argparse::scan freshness::` → the drift tests FAIL
      (or don't compile until the new names exist).
- [ ] **Step 3: Implement.**
    - Move the glob loop out of `scan_block_paths` into `expand_scan_paths`; `scan_block_paths` calls it.
    - `static_inputs`: `tools/<rel>` from `list_python_files`; `tools/pyproject.toml` if it's a file; if it parses
      with `argparse::parse_blocks_from_pyproject`, each block's `expand_scan_paths(project_root, &block.scan_paths)`
      results, skipping blocks that return `Err`. Keys relative to `project_root` with `/`; sort; dedup by key.
    - `compute_static_hash`: the `hash_paths` loop over `static_inputs`, keyed by `key`.
    - Switch `build_static_manifest_inner` and `compare` to `compute_static_hash`. Remove `hash_tools_dir` and update
      its callers in `cli_smoke.rs` and `freshness_dispatch.rs`. Update the `Manifest::static_hash` doc comment
      (`manifest/model.rs`).
- [ ] **Step 4: Run** Step 2 → PASS. Then `cargo test -p toolr-core` and
      `cargo test -p toolr --test freshness_dispatch --test cli_smoke` → PASS, including Task 1's tests.
- [ ] **Step 5: Commit.** `fix(manifest): hash argparse sources and tools/pyproject.toml into static_hash`

### Task 3: Docs, release notes, verification, archive

**Files:**

- Modify: `docs/internals/manifest.md`
- Modify: `UNRELEASED.md`
- [ ] **Step 1:** `docs/internals/manifest.md`: the `static_hash` field bullet and the "Hashing details" bullet list
      the three input kinds and the key and byte scheme above. The "static layer is rebuilt when" paragraph names
      scanned files and `tools/pyproject.toml`. Keep F-17's wording about symlinked files.
- [ ] **Step 2:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 3:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green. Commit `docs(internals): list every static_hash input`.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-argparse-manifest-hash-design.md specs/2026-10-31-argparse-manifest-hash-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive argparse manifest hash design and plan`.
