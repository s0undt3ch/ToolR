# Rebuild lock leak: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop tests writing rebuild locks into the real cache, and make `toolr self cache prune`
remove rebuild locks that no kept entry needs.

**Architecture:** A module-local env guard isolates the two bootstrap unit tests. A new
`toolr_core::cache::sweep_rebuild_locks` deletes `locks/*.lock` files that aren't in a keep-set
computed from kept entries' `meta.repo_path`. `self_cache_prune.rs` calls it on every prune path.

**Tech Stack:** Rust 2024, `std::fs::File::try_lock`, `blake3`, assert_cmd.

**Spec:** `specs/2026-11-04-rebuild-lock-leak-design.md`

## Global Constraints

- Branch `test/lock-leak`, stacked on `test/xdg-isolation` (#665). Worktree `.worktrees/lock-leak`.
- Stage files by path. Never `git add -A`. Never stage `audit/`.
- Conventional Commits. No Co-Authored-By trailer. British English.
- Format only the files you touch (`rustfmt --edition 2024 <file>`). Never run `cargo fmt` on a
  whole crate.
- 99% patch coverage on changed lines, `#[cfg(test)]` code in `src/` included. Use real tests, not
  mocks.
- Locks are never deleted while held. `locks/` itself is never removed.

## Review Focus

- A lock for a kept repo whose path is a symlink, or reached through one: prune must compute the
  same name the rebuild did, so the lock stays.
- `--all` with no cache entries but stray locks: the locks still go. Today this path returns early
  with "no cache entries".
- The classified path with nothing to prune but stray locks: the locks still go. Today this path
  returns early with "nothing to prune".
- A non-`.lock` file or a subdirectory in `locks/` is left alone.
- `--dry-run` deletes nothing and reports the count it would remove.

---

### Task 1: Isolate the bootstrap unit tests from the real cache

**Files:**

- Modify: `crates/toolr/src/bootstrap.rs` (test module, around lines 570–618)

**Interfaces:**

- Produces: nothing used by other tasks.
- [ ] **Step 1:** In the `bootstrap.rs` test module, add
      `fn temp_cache_home() -> (CacheHomeGuard, tempfile::TempDir)`. It takes a module
      `static Mutex<()>`, saves `XDG_CACHE_HOME`, sets it to a new `TempDir`, and restores the saved
      value when `CacheHomeGuard` drops. Model it on `with_env_var` in
      `crates/toolr/src/project.rs`, with the same `// SAFETY:` justification. Every line must run
      in these tests, so don't add an unused branch. If the restore needs both the `Some` and `None`
      arms, make sure the tests hit both, or restore with a single call that covers both.
- [ ] **Step 2:** Call it at the top of `ensure_manifest_fresh_rebuilds_a_missing_manifest` and
      `bootstrap_writes_a_missing_manifest_only_when_persisting`. Add an assertion that the temp
      cache now holds `toolr/locks/` with one `.lock` file, which proves the redirect took effect.
- [ ] **Step 3: Acceptance.** Run: `cargo test -p toolr --bin toolr bootstrap::`, then the
      per-binary sweep. That means touching a marker, running every workspace test executable from
      `cargo test --workspace --no-run --message-format=json` on its own, then
      `find <real cache>/toolr -newer marker -type f`. Expected: all tests pass, and `find` prints
      nothing.
- [ ] **Step 4: Commit** with `test(bootstrap): rebuild in a temp cache home, not the real one` and
      a `Release-Notes: skip` trailer.

### Task 2: Lock sweep in toolr-core

**Files:**

- Modify: `crates/toolr-core/src/manifest/lock.rs` (add `lock_path_for_repo`, update the "Never
  deleted" doc)
- Modify: `crates/toolr-core/src/manifest/mod.rs` (re-export)
- Create: `crates/toolr-core/src/cache/locks.rs`
- Modify: `crates/toolr-core/src/cache/mod.rs` (module plus re-exports)
- Test: `crates/toolr-core/src/cache/tests.rs`

**Interfaces:**

- Produces:
    - `pub fn lock_path_for_repo(cache_root: &Path, repo_root: &Path) -> PathBuf` returns
      `lock_path_in(cache_root, &repo_root.join("tools").join(".toolr-manifest.json"))`.
      `cache_root` is the toolr cache dir (the one holding `locks/`).
    - `pub struct LockSweep { pub removed: usize, pub held: usize }`, deriving
      `Debug, Default, PartialEq, Eq`.
    - `pub fn sweep_rebuild_locks(cache_root: &Path, keep_repos: &[&Path], dry_run: bool) -> std::io::Result<LockSweep>`

- [ ] **Step 1: Write the failing tests** in `cache/tests.rs`. Each builds a `TempDir` cache root.
    - `sweep_keeps_the_lock_of_a_kept_repo`: create a repo dir with `tools/`, create the lock at
      `lock_path_for_repo`, sweep with that repo kept. Assert the file exists and
      `LockSweep { removed: 0, held: 0 }`.
    - `sweep_removes_locks_no_kept_repo_needs`: two stray `locks/<hex>.lock` files and no kept
      repos. Assert both are gone and `removed == 2`.
    - `sweep_skips_a_lock_another_handle_holds`: open the stray lock and call `.lock()` on it, then
      sweep. Assert the file exists and `held == 1`.
    - `sweep_dry_run_removes_nothing`: a stray lock with `dry_run = true`. Assert the file exists
      and `removed == 1`.
    - `sweep_without_a_locks_dir_is_a_no_op`: returns `LockSweep::default()`.
    - `sweep_ignores_non_lock_entries`: `locks/notes.txt` and `locks/sub/` both remain.
    - `sweep_keeps_the_lock_of_a_repo_reached_through_a_symlink` (`#[cfg(unix)]`): the lock is
      created via the real path and the kept repo is the symlinked path. The lock stays.
- [ ] **Step 2:** Run `cargo test -p toolr-core cache::`. Expected: FAIL to compile, because the
      names are undefined.
- [ ] **Step 3: Implement** `lock_path_for_repo` and `sweep_rebuild_locks`. Steps for the sweep:
  1. Build the keep-set as a `HashSet` of `lock_path_for_repo` for each kept repo.
  2. If `read_dir(cache_root/locks)` fails with `NotFound`, return the default.
  3. Skip anything that isn't a file with a `lock` extension, and anything in the keep-set.
  4. Open the file read/write without truncating, then call `try_lock()`. If it fails with
     `WouldBlock`, count it as `held`.
  5. Otherwise count it as `removed`, and delete it while still holding the lock, unless this is a
     dry run.
  6. Propagate any other I/O error from opening, locking or deleting.

  Replace the "Never deleted" comment on `lock_path_in` with the race note from the spec.
- [ ] **Step 4:** Run `cargo test -p toolr-core cache:: manifest::`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(cache): sweep rebuild locks no cached repo needs`.

### Task 3: Wire the sweep into `toolr self cache prune`

**Files:**

- Modify: `crates/toolr/src/self_cache_prune.rs`
- Test: `crates/toolr/tests/self_cache_prune.rs`
- Create: a release note via `toolr release-notes add`

**Interfaces:**

- Consumes: `toolr_core::cache::{sweep_rebuild_locks, LockSweep}` and
  `toolr_core::manifest::lock_path_for_repo` from Task 2.
- [ ] **Step 1: Write the failing integration tests** in `self_cache_prune.rs`. Follow that file's
  existing pattern of a sandboxed `XDG_CACHE_HOME` with `HOME` removed.
    - `prune_removes_stray_rebuild_locks`: one fresh entry for an existing repo (with `tools/`), its
      lock, and one stray lock. Run `self cache prune`. The stray lock is gone, the kept repo's lock
      stays, and stdout has `removed 1 rebuild lock`.
    - `prune_dry_run_reports_stray_locks`: the same setup with `--dry-run`. Both locks stay, and
      stdout has `would remove 1 rebuild lock`.
    - `prune_all_with_no_entries_still_removes_locks`: no entries, one stray lock, run
      `--all --yes`. The lock is gone.
- [ ] **Step 2:** Run `cargo test -p toolr --test self_cache_prune`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.** In `run`, call `sweep_rebuild_locks` after the entry pruning on both paths:
    - `--all` keeps no repos.
    - The classified path keeps the `meta.repo_path` of `classification.keep`.

  Neither early return ("no cache entries" or "nothing to prune") may skip the sweep. When
  `removed > 0`, print `toolr: {removed|would remove} N rebuild lock{s}`. A sweep I/O error prints a
  `toolr: warning: …` line and does not fail the command, the same as `remove_entry`.
- [ ] **Step 4:** Run
      `cargo test -p toolr --test self_cache_prune && cargo test -p toolr --bin toolr`. Expected:
      PASS.
- [ ] **Step 5: Release note.** Run `toolr release-notes add --text "\`toolr self cache prune\` now
      also removes leftover manifest rebuild locks under the cache's \`locks/\` dir that no cached
      project still uses."`.
- [ ] **Step 6: Commit** with `feat(cache): prune leftover rebuild locks`, including the note file.

### Final: verify and archive

- [ ] Run `mise run test` and the per-binary marker sweep from Task 1. Both must be green and clean.
- [ ] Run `cargo llvm-cov` and then `cargo xtask patch-coverage --base test/xdg-isolation`.
      Expected: at least 99%.
- [ ] Run `git mv` on both spec files into `specs/archive/2026/`, then commit with
      `docs(specs): archive the rebuild lock leak design and plan`.
