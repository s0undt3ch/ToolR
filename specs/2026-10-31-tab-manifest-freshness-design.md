# Skip rereading unchanged files in the freshness check

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-30 (low; mechanism confirmed, latency impact plausible).
- **Plan:** [`2026-10-31-tab-manifest-freshness-plan.md`](./2026-10-31-tab-manifest-freshness-plan.md)
- **Stacks on:** `fix/argparse-manifest-hash` (F-16, `specs/2026-10-31-argparse-manifest-hash-design.md`); merge that
  first.

This spec assumes F-16 has landed. `hash::static_inputs(tools_dir) -> Vec<StaticInput { key, path }>` lists every
static-build input (`tools/**/*.py`, `tools/pyproject.toml`, argparse `scan_paths` matches) under project-relative
keys. `hash::compute_static_hash(tools_dir)` hashes them, and `freshness::compare` and `build_static_manifest_inner`
call it.

## Problem

Every TAB calls `toolr __complete`, which runs `resolve_manifest_at_tab` (`crates/toolr-core/src/complete/freshness.rs`)
and then `freshness::compare`. `compare` computes the static hash, and that reads every input file in full and hashes
it with blake3. There is no mtime or size gate. The work per keystroke grows with the size of `tools/` (and, after
F-16, the argparse sources), and with filesystem latency on network mounts. `venv/sync.rs::check_freshness` already uses
an mtime gate for the venv.

Two smaller defects sit in the same function:

- `load_manifest(&manifest_path).ok()` maps every load error to "no cache". A corrupt manifest is the same as a missing
  one, so every TAB reparses `tools/` until a dispatch repairs the file.
- `cached.expect("Fresh verdict requires a cached manifest")` relies on an invariant of `compare` that only a comment
  states.

## Decision

### 1. Per-file stat records in the manifest

Add to `Manifest` (`crates/toolr-core/src/manifest/model.rs`):

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub static_files: Vec<FileStat>,

pub struct FileStat {
    pub key: String,     // StaticInput::key
    pub size: u64,
    pub mtime_ns: u64,   // since the Unix epoch; 0 when unknown or before 1970
    pub blake3: String,  // lowercase hex of the file's contents
}
```

`static_hash` becomes blake3 over the sorted records, each fed as `key || 0x00 || blake3 digest (32 bytes)`. It can
then be computed from the records, reading only the files whose stat changed.

### 2. Snapshot with reuse

`hash::snapshot_static_inputs(tools_dir: &Path, previous: &[FileStat], written_at: Option<SystemTime>) ->
anyhow::Result<Vec<FileStat>>`:

- For each input from `static_inputs`, take `fs::metadata` (which follows symlinks, as F-17 requires) **before**
  reading.
- Reuse the previous record's digest when its `key`, `size` and `mtime_ns` all match **and** `mtime_ns` is strictly
  older than `written_at`. Otherwise read the file and hash it. `written_at == None` means "reuse nothing".
- A file that vanishes after listing is skipped (`NotFound`), as now.

`hash::static_hash_of(files: &[FileStat]) -> String` combines the records. `compute_static_hash(tools_dir)` becomes
`static_hash_of(&snapshot_static_inputs(tools_dir, &[], None)?)`.

**The racy-mtime rule.** A file edited in the same timestamp tick as the snapshot can keep its size and mtime and still
change its bytes. Coarse filesystem clocks (FAT, some NFS) make this likely. Git solves this with its index: an entry
whose mtime is not older than the index file's own mtime is always re-read. toolr does the same. `written_at` is the
**manifest file's mtime**, read with `fs::metadata` by the caller. It comes from the same filesystem clock as the
inputs, so client/server clock skew doesn't matter. Taking the stat before the read covers the other side. If a file
changes between the stat and the read, the record keeps the old stat, so the next check sees a different stat and
rereads.

### 3. `compare` uses the snapshot

`compare(cached: Option<&Manifest>, cached_written_at: Option<SystemTime>, tools_dir: &Path, venv_dir: Option<&Path>)`
replaces the hash call with `static_hash_of(&snapshot_static_inputs(tools_dir, &cached.static_files,
cached_written_at)?)`. Its callers pass the manifest's mtime: `bootstrap::ensure_manifest_fresh`
(`crates/toolr/src/bootstrap.rs`) and `resolve_manifest_at_tab`. A manifest without `static_files` (older, or freshly
upgraded) reuses nothing and behaves as today.

`build_static_manifest_inner` stamps `static_files` from `snapshot_static_inputs(tools_dir, &[], None)` and sets
`static_hash` from it.

### 4. No rewrite just to refresh stat records

If content matches but stats differ (after a `touch`, or a checkout that rewrote identical bytes), `compare` returns
`Fresh`, and nothing rewrites the manifest. Those files are then reread on each check until the next real rebuild. That
covers only the touched files, and it keeps the rule that a fresh manifest is never written. Writes on the
`--help` path are F-27's subject (`fix/help-untrusted-manifest-write`).

Files that were racy at build time are reread the same way until the next rebuild. That is usually a handful of files
saved just before the build.

### 5. Corrupt manifest at TAB

`resolve_manifest_at_tab` matches on the load result. `NotFound` and any other error both mean "no cache", so TAB
rebuilds in memory as now. It prints nothing. The bash, zsh and fish scripts (`crates/toolr-core/src/complete/scripts/`)
run `toolr __complete … 2>/dev/null`, so a TAB-time warning would never be seen. The next dispatch already warns
("ignoring unreadable …; rebuilding") and rewrites the file in `ensure_manifest_fresh`. The `.expect` becomes a
`match (verdict, cached)` with no panic path.

### Rejected alternatives

- **A single fingerprint hash of all stats.** Any one change forces a reread of every file.
- **System clock instead of the manifest's mtime for the racy rule.** It breaks under NFS clock skew.
- **Store the records in a sidecar file in the cache directory.** That gives two files that must agree, and two writes
  per rebuild.
- **Bump `SCHEMA_VERSION`.** The field is additive with `serde(default)`. Older binaries ignore it, and the
  `toolr_version` check makes them rebuild anyway. `plugin_warnings` was added the same way in #520 without a bump. The
  model doc says to bump on breaking changes.

## Scope and compatibility

- Manifest: additive field `static_files`, no `SCHEMA_VERSION` bump. `MANIFEST_SCHEMA_VERSION` in
  `crates/toolr-py/python/toolr/_decorators.py` and `FRAGMENT_SHAPE_SCHEMA` stay put, so the lockstep test
  `crates/toolr-core/tests/manifest_schema_version_lockstep.rs` stays green. Plugin fragments never carry the field.
- `static_hash` changes value once more. Each manifest rebuilds once after upgrading, which the version check forces
  anyway.
- `compare` gains a parameter. Its in-tree callers and `freshness/tests.rs` are updated.
- The argparse glob from F-16 still runs on every check. Only file reads are saved.
- `fix/help-untrusted-manifest-write` (F-27) also edits `ensure_manifest_fresh`. Whichever branch merges second
  rebases.
- Docs: `docs/internals/manifest.md`. Add `static_files` to the field list. In "Hashing details", describe the
  record-based `static_hash` and the stat gate with its racy rule.
- `UNRELEASED.md`:

  > Tab completion and the start-up freshness check no longer reread every file under `tools/` each time. toolr now
  > keeps each file's size and modification time in the manifest and rereads only the files that changed, so
  > completion stays fast in large `tools/` trees and on network filesystems.
