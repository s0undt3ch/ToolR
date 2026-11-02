# Skip rereading unchanged files in the freshness check

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-30 (low; mechanism confirmed, latency impact plausible).
- **Plan:** [`2026-10-31-tab-manifest-freshness-plan.md`](./2026-10-31-tab-manifest-freshness-plan.md)
- **Stacks on:** `fix/argparse-manifest-hash` (F-16, `specs/archive/2026/2026-10-31-argparse-manifest-hash-design.md`);
  merge that first.

F-16 made `static_hash` cover every input of the static build. `hash::static_inputs(tools_dir) ->
anyhow::Result<Vec<StaticInput { key, path, module }>>` lists `tools/**/*.py` (`module: true`), `tools/pyproject.toml`
and every file its `[tool.toolr.argparse.*]` `scan_paths` match, under project-relative keys.
`hash::compute_static_hash(tools_dir)` hashes them through `hash_paths`. A module that can't be read fails the hash; any
other unreadable input is hashed as `key || 0x01 || <error kind>`. `freshness::compare` and
`build_static_manifest_inner` call it.

## Problem

Every TAB calls `toolr __complete`, which runs `resolve_manifest_at_tab` (`crates/toolr-core/src/complete/freshness.rs`)
and then `freshness::compare`. Every other invocation runs `compare` through `bootstrap::ensure_manifest_fresh`.
`compare` computes the static hash: it reads `tools/pyproject.toml` twice (once to parse the `scan_paths`, once to hash
it) and every other input once, in full, with no mtime or size gate. After F-16 that includes the argparse sources. On a
Django project whose `scan_paths` match 6,000 files (23 MB), TAB went from 0.06–0.10 s to 1.2–1.5 s and `django --help`
from 0.3 s to 1.6 s. `venv/sync.rs::check_freshness` already gates the venv on mtimes.

Two smaller defects sit in `resolve_manifest_at_tab`:

- `load_manifest(&manifest_path).ok()` maps every load error to "no cache". A corrupt manifest is the same as a missing
  one, so every TAB reparses `tools/` until a dispatch repairs the file.
- `cached.expect("Fresh verdict requires a cached manifest")` relies on an invariant of `compare` that only a comment
  states.

## Decision

### 1. Per-file records in the manifest

Add one field to `Manifest` (`crates/toolr-core/src/manifest/model.rs`):

```rust
#[serde(default, skip_serializing_if = "InputRecords::is_empty")]
pub input_records: InputRecords,

pub struct InputRecords {
    pub files: Vec<FileStat>,         // one per static input, sorted by key
    pub scan_paths: Vec<Vec<String>>, // each argparse block's `scan_paths`, as parsed from tools/pyproject.toml
}

pub struct FileStat {
    pub key: String,      // StaticInput::key
    pub size: u64,
    pub mtime_ns: u64,    // since the Unix epoch; 0 = never reuse
    pub ctime_ns: u64,    // unix only, else 0
    pub ino: u64,         // unix only, else 0
    pub blake3: String,   // lowercase hex of the contents; empty when unreadable
    pub unreadable: Option<String>, // the read error's kind, for a non-module input
}
```

`static_hash` becomes blake3 over the records in key order, each fed as `len(key) as u64 LE || key || 0x00 || digest (32
bytes)`, or `len(key) as u64 LE || key || 0x01 || <error kind>` for an unreadable input. It can then be computed from
the records, reading only the files whose stat changed.

### 2. Snapshot with reuse

`hash::snapshot_static_inputs(tools_dir: &Path, previous: Option<&InputRecords>, written_at: Option<SystemTime>) ->
anyhow::Result<InputRecords>`:

- Walk `tools/**/*.py` as now. Stat `tools/pyproject.toml`. If its record can be reused, take the `scan_paths` from
  `previous` too; otherwise read it **once**, hash those bytes and parse the blocks from them. Expand the globs on every
  check, so a new or removed scanned file changes the input set.
- For each input, take `fs::metadata` (which follows symlinks) **before** reading. Reuse the previous record when
  `key`, `size`, `mtime_ns`, `ctime_ns` and `ino` all match, the record is readable, `mtime_ns != 0`, and `mtime_ns`
  is strictly older than `written_at`. Otherwise read the file and hash it. `written_at == None` reuses nothing.
- A file that vanishes after listing is skipped (`NotFound`), as now. The F-16 unreadable rule is unchanged, and an
  unreadable record has `mtime_ns = 0`: `chmod 644` changes neither size nor mtime, so a reused record would hide the
  fix. (`ctime_ns` would catch it on unix; `mtime_ns = 0` is the rule that holds everywhere.)

`hash::static_hash_of(files: &[FileStat]) -> String` combines the records. `compute_static_hash(tools_dir)` becomes
`static_hash_of(&snapshot_static_inputs(tools_dir, None, None)?.files)`.

On an unchanged tree a check therefore costs a walk of `tools/`, the glob expansion and one stat per input, and reads
no input at all.

**The racy-mtime rule.** A file edited in the same timestamp tick as its snapshot can keep its size and mtime and still
change its bytes. Coarse filesystem clocks (FAT, HFS+, some NFS) make this likely. Git solves this with its index: an
entry whose mtime is not older than the index file's own mtime is always reread. toolr does the same. `written_at` is
the **manifest file's mtime**, read with `fs::metadata` by the caller. It comes from the same filesystem clock as the
inputs, so client/server clock skew doesn't matter. Taking the stat before the read covers the other side: if a file
changes between the stat and the read, the record keeps the old stat, so the next check sees a different stat and
rereads.

`ctime_ns` and `ino` close the remaining gaps that keep size and mtime: a tool that restores an older mtime
(`touch -d`, `tar`, `rsync -t`) still sets ctime, and an editor that saves by rename gets a new inode. Neither can be
set by the user.

**Residual.** The build stats the inputs before it parses them and writes the manifest after. A same-size edit inside
the same mtime tick as the stat, made after the stat, that also keeps ctime in that tick, is missed when the manifest
write lands in a later tick. With the nanosecond timestamps of APFS, ext4, btrfs, XFS and NTFS this needs two writes
within the same nanosecond-tick; on a 1 s clock it needs the build to cross a second boundary. Git accepts the same
window. Marking every record younger than a couple of seconds as never-reuse would close it, but a `git clone` followed
straight away by a build would then reread the whole tree on every TAB until its next real change (see §4), so toolr
doesn't.

### 3. `compare` uses the snapshot

`compare(cached: Option<&Manifest>, cached_written_at: Option<SystemTime>, tools_dir: &Path, venv_dir: Option<&Path>)`
replaces the hash call with `static_hash_of(&snapshot_static_inputs(tools_dir, Some(&cached.input_records),
cached_written_at)?.files)`. Its callers pass the manifest's mtime: `bootstrap::ensure_manifest_fresh`
(`crates/toolr/src/bootstrap.rs`) and `resolve_manifest_at_tab`. A manifest without `input_records` (older) reuses
nothing and behaves as today.

`build_static_manifest_inner` stamps `input_records` from `snapshot_static_inputs(tools_dir, None, None)` (on the
`py_files` it already walked) and sets `static_hash` from it. The snapshot stays before the parse: hashing after it
would let a mid-build edit produce a hash that matches the new content while the manifest holds the old commands.

### 4. No rewrite just to refresh records

If content matches but stats differ (after a `touch`, or a checkout that rewrote identical bytes), `compare` returns
`Fresh`, and nothing rewrites the manifest. Those files are then reread on each check until the next real rebuild. That
covers only the touched files, and it keeps the rule that a fresh manifest is never written. Files that were racy at
build time are reread the same way until the next rebuild.

### 5. Corrupt manifest at TAB

`resolve_manifest_at_tab` matches on the load result. `NotFound` and any other error both mean "no cache", so TAB
rebuilds in memory as now. It prints nothing and writes nothing. The bash, zsh and fish scripts
(`crates/toolr-core/src/complete/scripts/`) run `toolr __complete … 2>/dev/null`, so a TAB-time warning would never
be seen. The next non-TAB run reports it: `ensure_manifest_fresh` warns once per process ("ignoring unreadable …;
rebuilding") and, in a trusted repository, rewrites the file, so later runs are silent. In an untrusted repository
nothing writes the manifest, so each run warns until the file is fixed or the repository is trusted. The `.expect`
becomes a `match (verdict, cached)` with no panic path.

### Rejected alternatives

- **A single fingerprint hash of all stats.** Any one change forces a reread of every file.
- **System clock instead of the manifest's mtime for the racy rule.** It breaks under NFS clock skew.
- **Store the records in a sidecar file in the cache directory.** That gives two files that must agree, and two writes
  per rebuild.
- **Bump `SCHEMA_VERSION`.** The field is additive with `serde(default)`. Older binaries ignore it, and the
  `toolr_version` check makes them rebuild anyway. `plugin_warnings` was added the same way in #520 without a bump. The
  model doc says to bump on breaking changes.

## Scope and compatibility

- Manifest: additive field `input_records`, no `SCHEMA_VERSION` bump. `MANIFEST_SCHEMA_VERSION` in
  `crates/toolr-py/python/toolr/_decorators.py` and `FRAGMENT_SHAPE_SCHEMA` stay put, so the lockstep test
  `crates/toolr-core/tests/manifest_schema_version_lockstep.rs` stays green. Plugin fragments never carry the field,
  and the Python side never reads the project manifest. The runner JSON is unchanged, so `RUNNER_SCHEMA_VERSION` stays.
- `static_hash` changes value. Each manifest rebuilds once after upgrading.
- `compare` gains a parameter. Its in-tree callers and `freshness/tests.rs` are updated.
- `fix/help-untrusted-manifest-write` (F-27) also edits `ensure_manifest_fresh`. Whichever branch merges second
  rebases.
- Docs: `docs/internals/manifest.md`. Add `input_records` to the field list. In "Hashing details", describe the
  record-based `static_hash` and the stat gate with its racy rule.
- Release note (`release-notes/`):

  > Tab completion and the start-up freshness check no longer reread every file under `tools/`, and every file an
  > external command source scans, each time. toolr now keeps each file's size and timestamps in the manifest and
  > rereads only the files that changed, so completion stays fast with large `tools/` trees, large `scan_paths` and
  > network filesystems.
