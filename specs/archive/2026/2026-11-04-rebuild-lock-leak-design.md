# Rebuild lock leak: design

## Problem

Manifest rebuilds take an advisory lock at
`<cache>/toolr/locks/<blake3 of the canonical manifest path>.lock`
(`crates/toolr-core/src/manifest/lock.rs`). Lock files are never deleted, and
`toolr self cache prune` skips `locks/`. Every distinct project path leaves a
file behind for good.

Two sources feed it:

1. **Tests.** Two unit tests in `crates/toolr/src/bootstrap.rs`
   (`ensure_manifest_fresh_rebuilds_a_missing_manifest`,
   `bootstrap_writes_a_missing_manifest_only_when_persisting`) rebuild a
   manifest in a fresh `TempDir` inside the test process, so they lock in the
   real cache. The temp path differs every run, so each run adds two files. A
   developer cache held about 22,500 of them.
2. **Users.** Every repo path a user runs toolr in (CI checkouts, throwaway
   clones, deleted worktrees) leaves a lock that nothing removes.

## Goals

- No test in the workspace writes to the real toolr cache.
- `toolr self cache prune` removes rebuild locks that no kept cache entry
  needs, including the existing backlog.

## Non-goals

- Changing where locks live or how they are taken.
- Moving the other toolr crate test env helpers (`project.rs`,
  `execute_build.rs`) onto a shared module.

## Design

### Tests

The two bootstrap tests point `XDG_CACHE_HOME` at a `TempDir` for their
duration. They use a module-local guard in the `bootstrap.rs` test module that
takes a module lock, sets the var, and restores the previous value on drop. This
follows the `with_env_var` pattern in `crates/toolr/src/project.rs`. It is safe
for the same reason: std serialises `std::env` access, and no test reads the env
through C. No other toolr unit test sets `XDG_CACHE_HOME`. A test elsewhere that
rebuilds at the same moment may take its lock in the temp dir. That only costs a
lock in a throwaway dir.

### Prune

Lock names are a one-way hash, so prune can't tell which project a lock belongs
to. Prune works from the other side and builds a **keep-set**. For every cache
entry it keeps, it computes the lock path that entry's repo would use. The repo
path comes from `meta.repo_path`, and the manifest is
`<repo_path>/tools/.toolr-manifest.json`. Prune deletes every `*.lock` file in
`locks/` that is not in the keep-set.

- `manifest/lock.rs` exposes `pub fn lock_path_for_repo(cache_root: &Path,
  repo_root: &Path) -> PathBuf`. It wraps the private `lock_path_in`, so prune
  and rebuilds share one canonicalisation.
- A kept entry whose `tools/` dir no longer exists can't canonicalise. It
  hashes the raw path and matches nothing, which is correct: that repo can't
  rebuild.
- Before deleting, prune takes the lock with a non-blocking `try_lock`. A
  held lock means a rebuild is running, so prune skips that file.
- **Race.** A process can open a lock file just before prune unlinks it. A
  second process then creates a fresh file and both rebuild. That only happens
  for a repo with no kept cache entry, and the cost is one duplicate rebuild,
  which is the same as the unlocked fallback the code already has. Accepted.
  The "Never deleted" comment on `lock_path_in` gets updated to say this.
- **Dry run.** `--dry-run` reports what prune would remove and deletes
  nothing, like every other prune target.
- **Output.** Lock files are empty, so prune reports them as a count
  (`removed N rebuild locks`), not one line each. Locks don't add to the
  freed-bytes total.
- Prune leaves `locks/` itself in place.

### Release note

User-visible: `toolr self cache prune` now also removes leftover manifest
rebuild locks. Queue it with `toolr release-notes add`.

## Testing

- **Acceptance (tests).** A marker file, then every workspace test binary run on
  its own, then `find <real cache> -newer marker`. It must find nothing. This
  per-binary sweep is the check that found the leak.
- **Prune unit tests** (`crates/toolr-core/src/cache/`):
    - a lock for a kept entry survives;
    - an unrelated lock is removed;
    - a lock held by another handle survives;
    - dry run removes nothing;
    - a missing `locks/` dir is a no-op.
- **Prune integration test** (`crates/toolr/tests/self_cache_prune.rs`): with a
  sandboxed `XDG_CACHE_HOME`, a stray lock goes, a kept repo's lock stays, and
  the count shows in the output.
