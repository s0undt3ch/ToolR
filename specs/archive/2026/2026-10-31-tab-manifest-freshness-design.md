# Skip rereading unchanged files in the freshness check

- **Status:** implemented
- **Origin:** whole-project review finding F-30 (low; mechanism confirmed, latency impact plausible).
- **Plan:** [`2026-10-31-tab-manifest-freshness-plan.md`](./2026-10-31-tab-manifest-freshness-plan.md)
- **Stacks on:** `fix/argparse-manifest-hash` (F-16, [`2026-10-31-argparse-manifest-hash-design.md`](./2026-10-31-argparse-manifest-hash-design.md));
  merge that first.

F-16 made `static_hash` cover every input of the static build. `hash::static_inputs(tools_dir) ->
anyhow::Result<Vec<StaticInput { key, path, kind: InputKind }>>` lists `tools/**/*.py` (`InputKind::Module`),
`tools/pyproject.toml` (`InputKind::Pyproject`, hashed by its argparse blocks) and every file its
`[tool.toolr.argparse.*]` `scan_paths` match (`InputKind::Scanned`), under project-relative keys.
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
    pub files: Vec<InputRecord>,      // one per static input, sorted by key
    pub scan_paths: Vec<Vec<String>>, // each argparse block's `scan_paths`, as parsed from tools/pyproject.toml
    pub snapshot_at_ns: u64,          // the filesystem clock just before the first stat; 0 = reuse nothing
    pub snapshot_dev: u64,            // the device that clock was read on (Unix st_dev, else 0)
}

pub struct InputRecord {
    pub key: String,      // StaticInput::key
    pub size: u64,
    pub mtime_ns: u64,    // since the Unix epoch; 0 = unknown, never reused
    pub ctime_ns: u64,    // unix only, else 0
    pub ino: u64,         // unix only, else 0
    pub dev: u64,         // unix only, else 0
    #[serde(flatten)]
    pub content: Content, // serialised inline as "digest": … or "unreadable": …
}

pub enum Content {
    Digest(String),     // lowercase hex blake3
    Unreadable(String), // the read error's kind, for a non-module input; never reused
}
```

`static_hash` becomes blake3 over the records in key order, each fed as
`len(key) as u64 LE || key || 0x00 || <hex blake3>`, or `len(key) as u64 LE || key || 0x01 || <error kind>` for an
unreadable input. It can then be computed from the records, reading only the files whose stat changed.

The pyproject record's `blake3` follows F-16's final shape: it is the digest of `0x02 ||` the
`[tool.toolr.argparse.*]` blocks as parsed, serialised in name order, so an edit elsewhere in the file rereads it once
and stays fresh. A pyproject whose blocks don't parse is digested whole, and one a `scan_paths` glob also matches is
recorded whole, as a scanned file.

### 2. Snapshot with reuse

`hash::snapshot_static_inputs(tools_dir: &Path, previous: Option<&InputRecords>) -> anyhow::Result<InputRecords>`:

- Walk `tools/**/*.py` as now. Stat `tools/pyproject.toml`. If its record can be reused, take the `scan_paths` from
  `previous` too; otherwise read it **once**, hash those bytes and parse the blocks from them. (A `scan_paths` glob
  that also matches it makes it a scanned file too, recorded whole, so it is then read twice.) Expand the globs on
  every check, so a new or removed scanned file changes the input set.
- For each input, take `fs::metadata` (which follows symlinks) **before** reading. Reuse the previous record when
  `key`, `size`, `mtime_ns`, `ctime_ns`, `ino` and `dev` all match, the file is under the project root and on the
  device the bound was read on (`previous.snapshot_dev`), the record holds a `Content::Digest`, `mtime_ns != 0`, and
  `mtime_ns` is strictly older than `previous.snapshot_at_ns`. Otherwise read the file and hash it. A bound of `0`
  reuses nothing.
- A file that vanishes after listing is skipped (`NotFound`), as now. The F-16 unreadable rule is unchanged, and an
  unreadable record (`Content::Unreadable`) is never reused: `chmod 644` changes neither size nor mtime, so a reused
  record would hide the fix.

`hash::static_hash_of(files: &[InputRecord]) -> String` combines the records. `compute_static_hash(tools_dir)` becomes
`static_hash_of(&snapshot_static_inputs(tools_dir, None)?.files)`.

On an unchanged tree a check therefore costs a walk of `tools/`, the glob expansion and one stat per input, and reads
no input at all.

**The racy-mtime rule.** A file edited in the same timestamp tick as its snapshot's stat can keep its size and mtime
and still change its bytes. Coarse filesystem clocks (FAT, HFS+, some NFS) make this likely. Git solves this with its
index: an entry whose mtime is not older than the index file's own mtime is always reread, because git stats and
writes the index back to back. toolr's build stats its inputs, then parses and scans for seconds, then waits for the
lock and writes the manifest, so the manifest's mtime is too late a bound. Review of #656 reproduced it on a 1 s HFS+
image: the file is written, the snapshot stats it in the same second, a same-size edit lands in that second too, the
manifest is written a second later, and the record was reused for good.

So the bound is `snapshot_at_ns`, taken **before the snapshot's first stat** and stored with the records.
`hash::SnapshotClock::now(tools_dir)` reads it from the filesystem's own clock, as the mtime (and `st_dev`) of a
temporary file it creates and removes in `tools/`, so NFS client/server skew doesn't matter for inputs on that
filesystem. The bound means nothing for another filesystem's mtimes: review of #656 showed `tools/` on APFS (ns
timestamps) and an absolute `scan_paths` file on HFS+ (1 s timestamps), where the HFS mtime `….000000000` is older than
the APFS bound `….2088…` taken in the same second, so a same-size rewrite in that second was reused for good. A record
is therefore reused only when its file is under the project root and on the device the bound was read on
(`snapshot_dev`); anything else is reread on every check. Off Unix there is no device number (`0` everywhere), so only
the project-root rule applies there. Only callers that are about to write the manifest
take it, since it writes to `tools/`: `ensure_manifest_present_or_bootstrap` and `ensure_manifest_fresh` when
persisting, `__build-static-manifest`, and `rebuild_manifest_full`. They stamp it on the built manifest before writing.
TAB's in-memory rebuild and untrusted runs take none and write nothing. A record whose mtime is strictly older than the
bound was last written in an earlier tick than the stat, so any later write gives it a different mtime. Taking the stat
before the read covers the other side: if a file changes between the stat and the read, the record keeps the old stat,
so the next check sees a different stat and rereads. The snapshot runs before the modules are parsed, so an edit
during the build always leaves a record that no longer matches.

`ctime_ns` and `ino` close the gaps that keep size and mtime: a tool that restores an older mtime (`touch -d`, `tar`,
`cp -p`, `rsync -t`) still sets ctime, and an editor that saves by rename gets a new inode. Neither can be set by the
user.

**Limitations.**

- On non-Unix platforms `ctime_ns` and `ino` are recorded as `0`, because stable Rust has no Windows change-time or
  file-index API. There, a same-size edit whose mtime is restored to an older value (`cp -p`, unpacking an archive) is
  missed until another change to that file.
- Off Unix, a filesystem mounted inside the project root has no device number to tell it apart, so its files are
  bounded by `tools/`'s clock.
- Inputs outside the project root, or on another device than `tools/`, are reread on every check.
- NFS attribute caching: a stat is answered from the client's attribute cache, and a check whose stats match never
  opens the file, so close-to-open revalidation never happens. An edit made from another NFS client can go unnoticed
  until the attribute cache expires (typically 5 to 60 s).
- On a coarse clock, a file saved in the same tick as the build's stat stays unreusable until the next persisting
  rebuild, because a Fresh manifest is never rewritten; it is reread on every check. Rewriting the manifest when a
  check finds it Fresh but holding such records was rejected: it would take the rebuild lock and write on hot paths
  (`--help`, dispatch), and TAB and untrusted runs must not write at all.

### 3. `compare` uses the snapshot

`compare` keeps its signature and replaces the hash call with
`static_hash_of(&snapshot_static_inputs(tools_dir, Some(&cached.input_records))?.files)`. A manifest without
`input_records` (older), or written without a bound, reuses nothing and behaves as today.

`build_static_manifest_inner` stamps `input_records` from a snapshot without reuse (on the `py_files` it already
walked) and sets `static_hash` from it. The snapshot runs before the module parse and the argparse scan: hashing after
them would let a mid-build edit produce a hash that matches the new content while the manifest holds the old commands.

### 4. No rewrite just to refresh records

If content matches but stats differ (after a `touch`, or a checkout that rewrote identical bytes), `compare` returns
`Fresh`, and nothing rewrites the manifest. Those files are then reread on each check until the next real rebuild. That
covers only the touched files, and it keeps the rule that a fresh manifest is never written. Files that were racy at
build time are reread the same way until the next rebuild.

### 5. Corrupt manifest at TAB

`resolve_manifest_at_tab` keeps `load_manifest(…).ok()`: `NotFound` and any other error both mean "no cache", so TAB
rebuilds in memory as now. It prints nothing and writes nothing. The bash, zsh and fish scripts
(`crates/toolr-core/src/complete/scripts/`) run `toolr __complete … 2>/dev/null`, so a TAB-time warning would never
be seen. The next non-TAB run reports it: `ensure_manifest_fresh` warns once per process ("ignoring unreadable …;
rebuilding") and, in a trusted repository, rewrites the file, so later runs are silent. In an untrusted repository
nothing writes the manifest, so each run warns until the file is fixed or the repository is trusted. The `.expect`
becomes a `match (verdict, cached)` with no panic path.

### Rejected alternatives

- **A single fingerprint hash of all stats.** Any one change forces a reread of every file.
- **The manifest file's mtime as the racy bound** (the first draft). It is taken after the whole build, so a same-tick
  edit after the stat is reused for good on a coarse clock.
- **The system clock for the bound.** It breaks under NFS clock skew.
- **The rebuild lock file's mtime for the bound.** It lives in the cache directory, which may be another filesystem
  with another clock.
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
- `compare` keeps its signature: the bound travels in `cached.input_records`.
- `fix/help-untrusted-manifest-write` (F-27) also edits `ensure_manifest_fresh`. Whichever branch merges second
  rebases.
- Docs: `docs/internals/manifest.md`. Add `input_records` to the field list. In "Hashing details", describe the
  record-based `static_hash` and the stat gate with its racy rule.
- Release note (`release-notes/`):

  > Tab completion and the start-up freshness check no longer reread every file under `tools/`, and every file an
  > external command source scans, each time. toolr now keeps each file's size and timestamps in the manifest and
  > rereads only the files that changed, so completion stays fast with large `tools/` trees and large `scan_paths`.
  > Files outside the project, or on another filesystem than `tools/`, are still reread every time. On NFS, an edit
  > made from another machine can take up to the attribute-cache timeout (typically under a minute) to be noticed.
