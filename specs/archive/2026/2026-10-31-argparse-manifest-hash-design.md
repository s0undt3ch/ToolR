# Put argparse sources and their config in the static hash

- **Status:** implemented
- **Origin:** whole-project review finding F-16 (medium, PLAUSIBLE in the review; reproduced below).
- **Plan:** [`2026-10-31-argparse-manifest-hash-plan.md`](./2026-10-31-argparse-manifest-hash-plan.md)
- **Base:** `main` after #642 (F-17, symlinked `*.py` under `tools/` rejected) and #647. Updated 2026-11-02 against that
  base; the original draft assumed F-17 would follow symlinks.

## Problem

`build_static_manifest_inner` (`crates/toolr-core/src/parser/build.rs`) runs the argparse scanner
(`argparse::run_for_project`). The scanner reads `[tool.toolr.argparse.*]` from `tools/pyproject.toml` and parses every
file its `scan_paths` globs match, usually Django management commands outside `tools/`. The resulting children are
grafted under the user's dispatcher.

The manifest's `static_hash` covers none of that. `build_static_manifest_inner` hashes the `tools/**/*.py` list it
parsed (`hash::hash_paths`), and `freshness::compare` (`crates/toolr-core/src/freshness/compare.rs`) hashes the
same set through `hash::hash_tools_dir` and compares. So editing a scanned file
or the argparse config leaves the manifest `Fresh`, and the old grafted commands stay until
`toolr project manifest rebuild`. `docs/writing-commands/external-sources.md` ("Rebuilding the manifest") promises the
opposite.

Repro, at `25e7ffdb`, using the worked example from `external-sources.md` (`tools/dispatcher.py` with the `django`
dispatcher, the `[tool.toolr.argparse.django]` block, `apps/billing/management/commands/migrate.py` with `--check`):

1. `toolr django migrate --help` shows `--check`.
2. Add `parser.add_argument("--database", ...)` to `migrate.py`. `toolr django migrate --help` still shows only
   `--check`.
3. Change `scan_paths` to `["nowhere/*.py"]`. `toolr django --help` still lists `migrate`.
4. `toolr project manifest rebuild`, then `toolr django --help`: `migrate` is gone.

## Decision

`static_hash` covers every input of the static build: the `tools/**/*.py` files, the `[tool.toolr.argparse.*]` blocks
of `tools/pyproject.toml`, and every file the argparse `scan_paths` match.

### Inputs

A new `hash::static_inputs(tools_dir: &Path) -> anyhow::Result<Vec<StaticInput>>` lists them. `StaticInput { key:
String, path: PathBuf }`. `key` is the path relative to the project root (`tools_dir.parent()`), components joined
with `/`:

- `tools/<rel>` for each path from `list_python_files(tools_dir)`.
- `tools/pyproject.toml`, if it is a file. Only its argparse blocks are hashed; see *Hash*.
- For each `[tool.toolr.argparse.*]` block, each path from `argparse::scan::expand_scan_paths(root, &block.scan_paths)`.
  An absolute `scan_paths` match outside the project root keeps its full path as the key.

Sort by key and drop duplicate keys, since a `scan_paths` glob may match a file under `tools/`.

`static_inputs` fails only where `list_python_files` does: a symlinked `*.py` under `tools/` returns the
`SymlinkedModules` error unwrapped, so callers keep using `context_unless_symlinked`. Scanned sources are not modules
under `tools/` and never go through `list_python_files`, so the no-symlinks rule doesn't apply to them: the scanner
reads a symlinked source through the link (`glob` and `is_file` follow it), and the hash reads the same bytes.

The build already walks `tools/` once and hands that list to both the parser and the hash (#642). It keeps doing so:
`static_inputs` is a thin wrapper over a `pub(crate)` `static_inputs_from(tools_dir, &py_files)` that the build calls
with the list it parsed.

`expand_scan_paths(root: &Path, scan_paths: &[String]) -> Result<Vec<PathBuf>, ScanError>` is the glob loop that
`scan_block_paths` (`crates/toolr-core/src/argparse/scan.rs`) runs today (root escaping from F-07 included), moved
into its own function. `scan_block_paths` calls it. The hash and the scanner then agree on the file set, just as
`list_python_files` keeps the hash and the parser in step.

The hash includes underscore-prefixed matches that the scanner then skips. A superset only costs an extra rebuild when
such a file changes.

### Hash

`pub fn compute_static_hash(tools_dir: &Path) -> anyhow::Result<String>` replaces `hash_tools_dir`. `hash_paths` takes
`&[StaticInput]` instead of deriving keys from `tools_dir`, and feeds each input to blake3 as
`key || 0x00 || len(contents) as u64 LE || contents`, the scheme it uses now. A file that vanishes between listing and
reading is skipped (`NotFound`), as now. Other read errors propagate.

The build reads nothing from `tools/pyproject.toml` but its `[tool.toolr.argparse.*]` blocks (`run_for_project`;
discovery only checks the file exists, and the venv settings feed `third_party_hash`). So that input is hashed as
`key || 0x02 || len || <blocks>`: the blocks as `argparse::parse_blocks` returns them, in name order, serialised with
`serde_json`. A dependency bump or `uv add` leaves the hash alone. A file whose blocks don't parse (not UTF-8, invalid
TOML anywhere in it, or a block of the wrong shape) is hashed whole with the `0x00` tag, so fixing it changes the hash.
A pyproject that a `scan_paths` glob also matches is hashed whole too, since the scanner reads all of it. (Added after
review.)

Callers that switch: `build_static_manifest_inner` (`parser/build.rs`, through `static_inputs_from`),
`freshness::compare`, and the test helpers that forge a fresh manifest (`crates/toolr/tests/cli_smoke.rs`,
`crates/toolr/tests/freshness_dispatch.rs`, `crates/toolr/tests/common/mod.rs::write_fresh_manifest`,
`crates/toolr-core/src/freshness/tests.rs::manifest_for`). `hash_tools_dir` is removed.

### Config errors on the freshness path

`compare` runs on `--help`, dispatch and TAB, and returns errors with `?`. A typo in the argparse config must not turn
into an error on every invocation from the hash. So in `static_inputs`:

- No `tools/pyproject.toml` contributes nothing, as in `run_for_project`.
- An unreadable or unparsable `pyproject.toml`, or a bad glob in one block, contributes no scanned files. An
  unparsable pyproject is hashed whole, so fixing the typo changes the hash and triggers a rebuild.
- The build then reports the config error as it does today, through `run_for_project`.
- Read errors: only a `tools/**/*.py` module that can't be read fails the hash, naming the file. A scanned file or
  `tools/pyproject.toml` that can't be read is hashed as `key || 0x01 || <error kind>`. The scanner already turns an
  unreadable scanned file into a placeholder with a warning, and the build reports an unreadable pyproject, so neither
  may fail every command; fixing the permission still changes the hash. (Added after review.)

### TAB on a failed rebuild

`--help` and dispatch fall back to the cached manifest with a warning when the rebuild fails (`warn_and_keep_cache`).
TAB rebuilt in memory with `?`, and `run_complete` turned the error into an empty manifest, so a half-edited pyproject
or a typo'd glob emptied TAB of every user command. `resolve_manifest_at_tab` now serves the cached manifest
(`from_cache: true`) when the rebuild fails, and prints nothing. Symlinked modules still fail it (F-17), and with no
cache the error still reaches `run_complete`. (Added after review.)

### Rejected alternatives

- **Hash the whole pyproject.** First chosen, to avoid a canonical serialisation. Review showed that every unrelated
  edit then made the manifest stale, and an invalid edit anywhere in the file emptied TAB. Serialising the parsed
  blocks needs only a `Serialize` derive.
- **Keep `hash_tools_dir` and add a second hash field to the manifest.** That would be a schema change, plus a second
  axis in `compare`, for inputs that `StaticDrift` already handles.
- **Re-run the scanner during `compare`.** The scanner parses each file. Hashing bytes is cheaper and enough.

## Scope and compatibility

- No manifest or runner schema change. `static_hash` keeps its field and type. Its value changes for every repo, so
  each manifest rebuilds once after upgrading. The `toolr_version` check forces that rebuild anyway.
- Cost: `compare` now reads and parses `tools/pyproject.toml` on every call, TAB included, and globs `scan_paths` only
  when there are argparse blocks. A project without blocks pays one small file read and TOML parse. The stat gate in
  F-30 (`fix/tab-manifest-freshness`, stacked on this branch) avoids rereading unchanged files. The glob stays.
- Docs: `docs/internals/manifest.md`. The `static_hash` field description and "Hashing details" list the three input
  kinds and the actual byte scheme (the current text describes a length-prefixed path, which the code never used).
  "The static layer is rebuilt when…" mentions scanned files and `tools/pyproject.toml`. The promise in
  `external-sources.md` becomes true; its "Rebuilding the manifest" section says which edits trigger it.
- Release note, queued with `toolr release-notes add` (not breaking: the one forced rebuild already happens on every
  toolr upgrade):

  > Editing the `[tool.toolr.argparse.*]` blocks in `tools/pyproject.toml`, or any file that an external command
  > source scans, now refreshes the manifest on the next run. toolr used to keep showing the old commands and
  > arguments until `toolr project manifest rebuild`. Other `tools/pyproject.toml` edits, such as a new dependency,
  > don't trigger a rebuild, and tab completion keeps offering the cached commands while that file or a `scan_paths`
  > glob is broken.
