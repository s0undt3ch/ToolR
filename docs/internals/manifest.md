# Manifest layers

`tools/.toolr-manifest.json` is the cached structure of every group
and command the toolr binary knows about. Tab completion, `--help`,
and clap's subcommand-tree build all read from this single file — no
Python imports involved in the hot path.

!!! note "It's a cache — don't commit it"
    The manifest is regenerated automatically when the static hash,
    the third-party hash, or the toolr binary's own version drifts,
    and the static layer alone rebuilds in ~10 ms on typical
    projects. `toolr project init` adds the file to
    `tools/.gitignore` for you. Committing it just produces noisy
    merge conflicts on every `tools/` edit without buying anything
    the auto-rebuild doesn't already cover.

Rebuilds take a lock, so concurrent `toolr` processes that find the
manifest stale rebuild it once. The lock file lives in toolr's cache
directory (`$XDG_CACHE_HOME/toolr/locks/`), not in `tools/`.

## File shape

```json
{
  "schema_version": 2,
  "static_hash": "<blake3 hex>",
  "third_party_hash": "<blake3 hex>",
  "toolr_version": "<CARGO_PKG_VERSION>",
  "groups": [...],
  "commands": [...],
  "input_records": {"files": [...], "scan_paths": [...], "snapshot_at_ns": <ns>, "snapshot_dev": <dev>}
}
```

- **`schema_version`** — single integer; toolr refuses to load a
  manifest with a higher schema than it understands. Version 2 removed
  `path_constraints`. A cache with any other schema version is rebuilt
  on the next run.
- **`static_hash`** — blake3 over the sorted per-file digests of
  every input of the static build: each `tools/**/*.py` file, the
  `[tool.toolr.argparse.*]` blocks of `tools/pyproject.toml`, and every
  file an external command source scans. Drives static-layer rebuilds.
- **`input_records`** — what `static_hash` was computed from: one
  record per input (`key`, `size`, `mtime_ns`, `ctime_ns`, `ino`,
  `dev`, and either `digest`, the hex blake3 of its contents, or
  `unreadable`, the error kind for an input that couldn't be read),
  each argparse block's `scan_paths`, and `snapshot_at_ns` and
  `snapshot_dev`, the bound the stat gate compares mtimes against and
  the device it was read on. The freshness check uses them to
  skip rereading unchanged files. Absent on older manifests, which then
  reread everything once.
- **`third_party_hash`** — blake3 over the sorted set of
  `toolr-manifest.json` files found under
  `<tools-venv>/lib/python*/site-packages/*/`
  or `Lib/site-packages/*/` (the Windows layout, looked in on every
  platform). Each entry's path
  and content go into the hash. Drives third-party-fragment
  rebuilds: installing an unrelated package no longer invalidates
  the manifest, only changes to packages that ship a toolr
  fragment do.
- **`toolr_version`** — the `toolr` binary's own version at the time
  it built this manifest. A mismatch against the running binary means
  toolr itself was upgraded (parsing/classification behaviour may have
  changed with no `tools/*.py` edit at all) and always forces at least
  a static rebuild; with a venv available to search again, it forces a full
  rebuild instead. Deserializes as `""` on older manifests, which also
  forces one rebuild.
- **`groups`** / **`commands`** — the actual command tree. Each
  entry carries an `origin` field (`"static"` or `"third_party"`)
  recording which layer produced it.

## Group schema

```json
{
  "name": "image",
  "title": "Image",
  "description": "Image subcommands",
  "parent": "docker",
  "origin": "static"
}
```

- **`name`** — the leaf name (not the full path). For nested groups
  this is just the segment (`"image"`); the full hierarchy is the
  parent path joined to the name.
- **`parent`** — `null` for top-level groups, otherwise the dotted
  `full_path` of the parent group (`"docker"` for `docker image`,
  `"docker.image"` for `docker image build`). The clap CLI builder
  walks this tree recursively when materialising subcommands.

## Command schema

```json
{
  "name": "build",
  "group": "docker.image",
  "module": "tools.docker",
  "function": "build",
  "summary": "Build a docker image.",
  "description": "",
  "arguments": [...],
  "imports": [],
  "origin": "static"
}
```

- **`group`** — the **full path** of the owning group. Top-level
  commands have a single segment (`"ci"`); nested commands have the
  dotted parent path (`"docker.image"`).

## Argument schema

```json
{
  "name": "tag",
  "kind": "positional",
  "help": "Tag to assign.",
  "default": null,
  "type_annotation": "str",
  "resolved_type": { "kind": "str" },
  "allowed_values": []
}
```

- **`kind`** — one of `"positional"`, `"optional"`, `"flag"`,
  `"repeated"` (list[T] keyword), or `"var_positional"` (`*args`).
- **`default`** — string-encoded default value (or `null`). Numeric
  defaults render as e.g. `"5"`; enum-attribute defaults like
  `Operation.ADD` are resolved to the serialised member value
  (`"add"`).
- **`type_annotation`** — textual rendering of the annotation as
  written in source (`"int"`, `"list[str]"`, `"pathlib.Path"`). Used
  for `--help` and diagnostics.
- **`resolved_type`** — structured form of the annotation, drives
  the clap value_parser layer. Shape:

  ```json
  { "kind": "int" }
  { "kind": "list", "value": { "kind": "int" } }
  { "kind": "tuple", "value": [{ "kind": "str" }, { "kind": "int" }] }
  { "kind": "literal", "value": ["a", "b"] }
  { "kind": "enum", "value": { "name": "Operation", "values": ["add", "subtract"] } }
  { "kind": "optional", "value": { "kind": "path" } }
  { "kind": "file_path" }
  ```

  See `src/parser/types.rs` for the full enum. `null` means the type
  layer couldn't resolve a supported type — third-party fragments
  built against the legacy schema fall through to string semantics.
  Path types (`path`, `absolute_path`, `new_path`, `resolved_path`,
  `file_path`, `directory_path`, `executable_path`,
  `writable_directory_path`) name their filesystem check, if any, in
  the kind itself.
- **`allowed_values`** — for `Literal[...]` / `Enum` types, the
  values clap validates against. Also used by tab completion.

## Static layer

Built from `tools/**/*.py` via the `ruff_python_parser` Rust crate.
Pure AST traversal — never imports user code, so it's safe to run
without a working venv. Captures:

- `command_group(...)` declarations, bound (`var = command_group(...)`)
  or not, plus the deprecated nested `parent.command_group("child", ...)`
  method form.
- `@<binding>.command` decorations (the canonical single-file form) and
  standalone `@command(group="…")` decorations (for commands in a
  different file from their group).
- Function signatures (positional vs keyword, defaults, annotations).
- Google-style docstrings (summary, description, `Args:` block).
- Local `Literal[...]` and `enum.Enum` definitions (resolved across
  files via a symbol table).

The static layer is rebuilt when toolr detects `static_hash` drift
against the on-disk files (a `tools/*.py` module, the
`[tool.toolr.argparse.*]` blocks of `tools/pyproject.toml`, or a file an
[external command source](../writing-commands/external-sources.md)
scans), or when the cached `toolr_version` doesn't match the running
binary — an upgrade forces a static rebuild even if none of those files
changed.

## Third-party layer

Contributed by installed packages that ship a static
`toolr-manifest.json` fragment inside their wheel, discovered by
looking in `<tools-venv>/lib/python*/site-packages/*/`
or `Lib/site-packages/*/` (the Windows layout, looked in on every
platform) and merged into
the static layer at build time (see
[Third-party packages](../third-party.md)). No Python import happens
during discovery — it's an execution-free list-and-parse, same as
the static layer.

Toolr regenerates the manifest when:

- A command is invoked and the binary detects `static_hash` or
  `third_party_hash` drift on entry.
- The cached `toolr_version` doesn't match the running binary — an
  upgrade always forces at least a static rebuild regardless of
  `static_hash`, and forces a rebuild of both layers when a venv is
  available to find third-party fragments again too.
- The user explicitly runs `toolr project manifest rebuild`.

Automatic rebuilds are written to disk only in a trusted repository (see
[Repository trust](../concepts.md#repository-trust)). In an untrusted one the
rebuilt manifest stays in memory and an existing file is left as found.

## Manual rebuild

```sh
toolr project manifest rebuild
```

Rebuilds both layers and writes the merged result. Used by the
shipped pre-commit hook (see [Pre-commit integration](pre-commit.md))
and available for explicit invocation when you want to be sure the
manifest is current — for example before publishing a release that
includes new commands.

## Hashing details

Both hashes use blake3, written as lowercase hex.

- `static_hash` input: every `*.py` file under `tools/` (skipping
  `__pycache__` and dot-prefixed directories), `tools/pyproject.toml`,
  and every file the `scan_paths` of its `[tool.toolr.argparse.*]`
  blocks match. A symlinked `*.py` under `tools/` fails the hash instead
  of being skipped, and symlinked directories there are not followed.
  Each file is keyed by its path relative to the project root, with `/`
  separators (a scanned file outside the project keeps its full path),
  sorted by key. Each file contributes
  `len(key) as u64 LE || key || 0x00 || <hex blake3 of its contents>`.
  The build reads only the `[tool.toolr.argparse.*]` blocks of
  `tools/pyproject.toml`, so its digest is of `0x02` followed by those
  blocks as parsed, serialised in name order: a dependency bump or
  `uv add` doesn't trigger a rebuild. A `tools/pyproject.toml` whose
  blocks don't parse is digested whole, so fixing it does; it also adds
  no scanned files, and neither does a block with an invalid glob. The
  check doesn't fail on either; the build reports the error, and tab
  completion keeps serving the cached manifest until it's fixed. A
  `tools/**/*.py` module that can't be read fails the check, naming the
  file. Any other input that can't be read contributes
  `len(key) as u64 LE || key || 0x01 || <error kind>`, so an unreadable
  scanned file doesn't break every command, and fixing it still triggers
  a rebuild.
- The stat gate: the freshness check stats every input and reuses the
  `digest` in `input_records` without reading the file when all of
  these hold:
    - its size, mtime, ctime, inode and device all match the record
      (ctime, inode and device on Unix only);
    - its mtime is strictly older than `input_records.snapshot_at_ns`.
      That bound is the clock of `tools/`'s filesystem, read from a
      temporary file in `tools/` just before the build's first stat, so
      NFS clock skew doesn't affect inputs on that filesystem. A file
      saved in the same timestamp tick as the build's stat is always
      reread, as git does for its index, since it could have changed
      again without changing its stat;
    - it is under the project root and on `tools/`'s device
      (`snapshot_dev`), since the bound can't be compared with another
      filesystem's mtimes. Every other input is reread on every check;
    - its record holds a `digest`: an `unreadable` record is never
      reused, since fixing the permission changes no stat field.

  More about the gate:

    - Only runs that write the manifest take the bound; a manifest
      without one reuses nothing.
    - `tools/pyproject.toml` is read at most once per check, for both
      its digest and its `scan_paths`, which are reused with its
      digest. A `scan_paths` glob that also matches it makes it a
      scanned file, recorded whole, so it is then read twice.
    - The globs are expanded on every check, so a new or removed
      scanned file is noticed.
    - On an unchanged tree the check reads no input at all.
    - A fresh manifest is never rewritten, so a file whose stat changed
      without its contents (after `touch`), or that was saved in the
      build's own tick, is reread on each check until the next rebuild.
- Limits of the stat gate:
    - Off Unix, ctime, inode and device aren't available, so a
      same-size edit whose mtime is restored to an older value
      (`cp -p`, unpacking an archive) goes unnoticed until that file
      changes again.
    - On NFS, a stat is answered from the client's attribute cache, and
      an unchanged stat never opens the file, so close-to-open
      revalidation never happens. An edit made from another NFS client
      can go unnoticed until the attribute cache expires (typically 5
      to 60 seconds).
- `third_party_hash` input: every `toolr-manifest.json` under
  `<tools-venv>/lib/python*/site-packages/*/`
  or `Lib/site-packages/*/` (the Windows layout, looked in on every
  platform), sorted by path, with each file's path and contents in the
  hash.

This gives stable, content-addressable rebuild decisions across
machines and across CI / dev environments.

`toolr_version` costs no I/O to check, so a mismatch always wins over
a matching hash — a toolr upgrade forces at least `StaticDrift`, and
forces the stronger `ThirdPartyDrift` too when a venv is available to
search again (see the `toolr_version` field description above for why a
missing venv caps it at `StaticDrift` instead).
