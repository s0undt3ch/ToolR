# Put argparse sources and their config in the static hash

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-16 (medium, PLAUSIBLE in the review; reproduced below).
- **Plan:** [`2026-10-31-argparse-manifest-hash-plan.md`](./2026-10-31-argparse-manifest-hash-plan.md)
- **Stacks on:** `fix/symlinked-tools-files` (F-17, `specs/2026-10-31-symlinked-tools-files-design.md`); merge that
  first.

## Problem

`build_static_manifest_inner` (`crates/toolr-core/src/parser/build.rs`) runs the argparse scanner
(`argparse::run_for_project`). The scanner reads `[tool.toolr.argparse.*]` from `tools/pyproject.toml` and parses every
file its `scan_paths` globs match, usually Django management commands outside `tools/`. The resulting children are
grafted under the user's dispatcher.

The manifest's `static_hash` covers none of that. It comes from `hash::hash_tools_dir`, which hashes `tools/**/*.py`
only. `freshness::compare` (`crates/toolr-core/src/freshness/compare.rs`) compares that hash. So editing a scanned file
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

`static_hash` covers every input of the static build: the `tools/**/*.py` files, `tools/pyproject.toml`, and every
file the argparse `scan_paths` match.

### Inputs

A new `hash::static_inputs(tools_dir: &Path) -> Vec<StaticInput>` lists them. `StaticInput { key: String, path:
PathBuf }`. `key` is the path relative to the project root (`tools_dir.parent()`), with `/` separators:

- `tools/<rel>` for each path from `list_python_files(tools_dir)` (F-17's list, symlinked files included).
- `tools/pyproject.toml`, if it is a file.
- For each `[tool.toolr.argparse.*]` block, each path from `argparse::scan::expand_scan_paths(root, &block.scan_paths)`.

Sort by key and drop duplicate keys, since a `scan_paths` glob may match a file under `tools/`.

`expand_scan_paths(root: &Path, scan_paths: &[String]) -> Result<Vec<PathBuf>, ScanError>` is the glob loop that
`scan_block_paths` (`crates/toolr-core/src/argparse/scan.rs`) runs today, moved into its own function.
`scan_block_paths` calls it. The hash and the scanner then agree on the file set, just as `list_python_files` keeps the
hash and the parser in step.

The hash includes underscore-prefixed matches that the scanner then skips. A superset only costs an extra rebuild when
such a file changes.

### Hash

`pub fn compute_static_hash(tools_dir: &Path) -> anyhow::Result<String>` replaces `hash_tools_dir`. It feeds each input
to blake3 as `key || 0x00 || len(contents) as u64 LE || contents`, the scheme `hash_paths` uses now. A file that
vanishes between listing and reading is skipped (`NotFound`), as now. Other read errors propagate.

Callers that switch: `build_static_manifest_inner` (`parser/build.rs`), `freshness::compare`, and the two integration
tests that forge a fresh manifest (`crates/toolr/tests/cli_smoke.rs` and `crates/toolr/tests/freshness_dispatch.rs`).
`hash_tools_dir` is removed.

### Config errors on the freshness path

`compare` runs on `--help`, dispatch and TAB, and returns errors with `?`. A typo in the argparse config must not turn
into an error on every invocation from the hash. So in `static_inputs`:

- An unreadable or unparsable `pyproject.toml`, or a bad glob in one block, contributes no scanned files. The
  pyproject's bytes are still an input, so fixing the typo changes the hash and triggers a rebuild.
- The build then reports the config error as it does today, through `run_for_project`.

### Rejected alternatives

- **Hash only the `[tool.toolr.argparse]` table, not the whole pyproject.** This needs a canonical re-serialisation of
  TOML. A dependency edit that rebuilds the manifest costs a static parse, which takes milliseconds.
- **Keep `hash_tools_dir` and add a second hash field to the manifest.** That would be a schema change, plus a second
  axis in `compare`, for inputs that `StaticDrift` already handles.
- **Re-run the scanner during `compare`.** The scanner parses each file. Hashing bytes is cheaper and enough.

## Scope and compatibility

- No manifest or runner schema change. `static_hash` keeps its field and type. Its value changes for every repo, so
  each manifest rebuilds once after upgrading. The `toolr_version` check forces that rebuild anyway.
- Cost: `compare` now reads `tools/pyproject.toml` and globs `scan_paths` on every call, TAB included. The stat gate in
  F-30 (`fix/tab-manifest-freshness`, stacked on this branch) avoids rereading unchanged files. The glob stays.
- `fix/plugin-glob-escape` (F-07) also edits the glob loop in `scan_block_paths`, to escape `root`. Whichever branch
  merges second carries the escaping into `expand_scan_paths`.
- Docs: `docs/internals/manifest.md`. The `static_hash` field description and "Hashing details" list the three input
  kinds. "The static layer is rebuilt when…" mentions scanned files and `tools/pyproject.toml`. The promise in
  `external-sources.md` becomes true and needs no change.
- `UNRELEASED.md`:

  > Editing a file scanned by an external command source (`[tool.toolr.argparse.*]`), or the `tools/pyproject.toml`
  > that configures it, now refreshes the manifest on the next run. toolr used to keep showing the old commands and
  > arguments until `toolr project manifest rebuild`.
