# TAB Manifest Freshness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The freshness check rereads only static inputs whose size or mtime changed. TAB handles a corrupt manifest
without a panic path (F-30).

**Architecture:** The manifest stores a `FileStat` per static input. `static_hash` is derived from the per-file
digests. `compare` reuses a digest when the stat matches and the file is older than the manifest file itself (git's
racy rule).

**Tech Stack:** Rust, `blake3`, `serde`, `tempfile`, `assert_cmd`.

**Spec:** [`specs/archive/2026/2026-10-31-tab-manifest-freshness-design.md`](./2026-10-31-tab-manifest-freshness-design.md)

> **As implemented.** This plan predates F-16's final shape; the spec was updated before implementation and is
> authoritative. Differences:
>
> - The field is `input_records`: `files` plus the parsed `scan_paths`, so `tools/pyproject.toml` is read at most
>   once (twice when a `scan_paths` glob also matches it) and not at all when unchanged.
> - `FileStat` became `InputRecord`. Records also carry `ctime_ns`, `ino` and `dev`, and their `content` is
>   `Content::Digest | Content::Unreadable`; an unreadable record is never reused.
> - The racy bound is `InputRecords::snapshot_at_ns`, the filesystem clock taken before the snapshot, not the manifest
>   file's mtime, and `compare` keeps its signature. `InputRecords::snapshot_dev` limits reuse to inputs under the
>   project root on `tools/`'s device. Every persisting path builds through `hash::build_to_persist`.
> - `main.rs` skips its own manifest load for `__complete`.
> - Release notes go in `release-notes/`, not `UNRELEASED.md`.
> - The tests live in `hash.rs` (read-count tests via `take_reads`), `complete/tests.rs`, `manifest/tests.rs`,
>   `parser/build.rs` and `crates/toolr/tests/freshness_dispatch.rs`.

## Global Constraints

- Stacks on `fix/argparse-manifest-hash` (F-16): `hash::static_inputs` and `hash::compute_static_hash` exist.
- `Manifest.static_files` is `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. No `SCHEMA_VERSION` or
  `MANIFEST_SCHEMA_VERSION` bump.
- Reuse a digest only if `key`, `size` and `mtime_ns` match and `mtime_ns < written_at` (strict). `written_at` is the
  manifest file's mtime; `None` reuses nothing.
- Stat before read.
- TAB writes nothing to stderr and never writes the manifest.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Same size, same mtime, different bytes.** The racy case must be caught when the file's mtime isn't older than the
   manifest. Task 1, `racy_entry_is_reread`.
2. **Old manifests without `static_files`.** Must load, compare correctly and reuse nothing. Task 1,
   `manifest_without_static_files_loads` and Task 2, `compare_without_records_reads_everything`.
3. **Unchanged files really aren't read.** Otherwise the fix saves nothing. Task 1,
   `unchanged_stat_reuses_digest_without_reading` (unreadable-file trick).
4. **Corrupt manifest at TAB.** In-memory rebuild, no write, no panic. Task 2,
   `corrupt_manifest_at_tab_rebuilds_in_memory`.
5. **Inputs added by F-16.** `tools/pyproject.toml` and scanned files get records too. Task 1,
   `snapshot_covers_pyproject_and_scanned_files`.

---

### Task 1: Records, snapshot, record-based `static_hash`

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs` (`FileStat`, `Manifest.static_files`; update every `Manifest { … }`
  literal the compiler flags, e.g. `crates/toolr/src/main.rs::empty_manifest`)
- Modify: `crates/toolr-core/src/hash.rs` (`snapshot_static_inputs`, `static_hash_of`, `compute_static_hash`)
- Modify: `crates/toolr-core/src/parser/build.rs` (`build_static_manifest_inner` stamps `static_files`)
- Test: `hash.rs` `mod tests`, `crates/toolr-core/src/manifest/tests.rs`

**Interfaces:**

- Produces:
    - `pub struct FileStat { pub key: String, pub size: u64, pub mtime_ns: u64, pub blake3: String }` (`Serialize`,
      `Deserialize`, `Clone`, `Debug`, `PartialEq`, `Eq`)
    - `pub fn snapshot_static_inputs(tools_dir: &Path, previous: &[FileStat], written_at: Option<SystemTime>) ->
      anyhow::Result<Vec<FileStat>>`
    - `pub fn static_hash_of(files: &[FileStat]) -> String`
- Consumes: F-16's `static_inputs(tools_dir) -> Vec<StaticInput>`.
- [ ] **Step 1: Write the tests.**
    - `manifest/tests.rs`:
        - `manifest_without_static_files_loads`: a schema-2 JSON without the field → `static_files.is_empty()`.
        - `empty_static_files_not_serialised`: serialised JSON has no `static_files` key.
    - `hash.rs`:
        - `static_hash_of_snapshot_equals_compute_static_hash`.
        - `unchanged_stat_reuses_digest_without_reading` (`#[cfg(unix)]`, skip when running as root): snapshot `a`,
          `chmod 000 tools/a.py` (the mtime doesn't change), snapshot again with `previous = a` and `written_at =
          now + 1 s` → `Ok`, same digests.
        - `changed_size_rereads_file`: append a byte → the new record's `blake3` differs.
        - `racy_entry_is_reread`: snapshot; rewrite `a.py` with other bytes of the same length; restore its mtime with
          `File::set_modified`; snapshot with `written_at` == that mtime → digest differs.
        - `non_racy_entry_with_restored_mtime_is_reused`: same setup with `written_at` = mtime + 1 s → digest reused.
          This pins the rule's boundary.
        - `none_written_at_reuses_nothing`: same setup with `written_at = None` → digest differs.
        - `snapshot_covers_pyproject_and_scanned_files`: F-16's django layout → keys include `tools/pyproject.toml` and
          `apps/billing/management/commands/migrate.py`.
    - `parser/build.rs`: `built_manifest_carries_static_files`: `static_files` keys equal `static_inputs` keys and
      `static_hash == static_hash_of(&static_files)`.
- [ ] **Step 2: Run.** `cargo test -p toolr-core --lib hash:: manifest:: parser::build` → new tests FAIL / don't
      compile.
- [ ] **Step 3: Implement** the types and functions to the spec. `mtime_ns`: `metadata.modified()` → duration since
      `UNIX_EPOCH` in ns, saturating to `u64`, `0` on error. A record with `mtime_ns == 0` is never reused.
      `static_hash_of` feeds `key`, `0x00` and the 32 raw digest bytes, in key order. Update the
      `Manifest::static_hash` doc comment.
- [ ] **Step 4: Run** Step 2 → PASS; `cargo test -p toolr-core` → PASS.
- [ ] **Step 5: Commit.** `perf(manifest): record per-file stats and derive static_hash from file digests`

### Task 2: Gate `compare` on the records; fix TAB loading

**Files:**

- Modify: `crates/toolr-core/src/freshness/compare.rs` (`compare` signature and body)
- Modify: `crates/toolr-core/src/freshness/tests.rs`
- Modify: `crates/toolr-core/src/complete/freshness.rs` (`resolve_manifest_at_tab`)
- Modify: `crates/toolr-core/src/complete/tests.rs`
- Modify: `crates/toolr/src/bootstrap.rs` (`ensure_manifest_fresh` passes the manifest mtime)

**Interfaces:**

- Produces: `pub fn compare(cached: Option<&Manifest>, cached_written_at: Option<SystemTime>, tools_dir: &Path,
  venv_dir: Option<&Path>) -> Result<FreshnessVerdict>`.
- [ ] **Step 1: Write the tests.**
    - `freshness/tests.rs`:
        - `compare_fresh_without_reading_unchanged_files` (`#[cfg(unix)]`, skip as root): build, write the manifest,
          `chmod 000` a tools file, `compare(Some(&m), Some(manifest_mtime + 1 s), …)` → `Fresh`.
        - `compare_detects_racy_same_size_edit`: same-length edit with restored mtime and `written_at` == that mtime
          → `StaticDrift`.
        - `compare_without_records_reads_everything`: `m.static_files.clear()` → still `Fresh`; after an edit →
          `StaticDrift`.
        - Update existing calls to pass `None` for `cached_written_at`.
    - `complete/tests.rs`:
        - `corrupt_manifest_at_tab_rebuilds_in_memory`: manifest bytes `{"schema_ver`; `resolve_manifest_at_tab` →
          `Ok`, `from_cache == false`, the fresh manifest has the tree's group, and the file's bytes are unchanged.
- [ ] **Step 2: Run.** `cargo test -p toolr-core --lib freshness:: complete::` → new tests FAIL / don't compile.
- [ ] **Step 3: Implement.** `compare` replaces `compute_static_hash(tools_dir)` with
      `static_hash_of(&snapshot_static_inputs(tools_dir, &cached.static_files, cached_written_at)?)`.
      `resolve_manifest_at_tab` and `ensure_manifest_fresh` read `fs::metadata(&manifest_path).and_then(|m|
      m.modified()).ok()` next to `load_manifest` and pass it. In `resolve_manifest_at_tab`, replace `.ok()` with a
      `match` (any `Err` → `None`, no output) and replace the `.expect` with `match (verdict, cached)`. Add a comment
      that completion stderr goes to `/dev/null` in every shell script.
- [ ] **Step 4: Run** Step 2 → PASS. Then `cargo test -p toolr-core` and
      `cargo test -p toolr --test freshness_dispatch --test complete_smoke --test manifest_concurrency` → PASS.
- [ ] **Step 5: Commit.** `perf(freshness): reread only static inputs whose stat changed`

### Task 3: Docs, release notes, verification, archive

**Files:**

- Modify: `docs/internals/manifest.md`
- Modify: `UNRELEASED.md`
- [ ] **Step 1:** `docs/internals/manifest.md`: add `static_files` to the JSON example and the field list. In "Hashing
      details", explain that `static_hash` is derived from per-file digests, and describe the stat gate and the racy
      rule: a file whose mtime is not older than the manifest file is always reread.
- [ ] **Step 2:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 3:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green. Commit `docs(internals): document the static_files stat gate`.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-tab-manifest-freshness-design.md specs/2026-10-31-tab-manifest-freshness-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive tab manifest freshness design and plan`.
