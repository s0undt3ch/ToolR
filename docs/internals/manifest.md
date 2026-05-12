# Manifest layers

`tools/.toolr-manifest.json` is the cached structure of every group
and command the toolr binary knows about. Tab completion, `--help`,
and clap's subcommand-tree build all read from this single file — no
Python imports involved in the hot path.

## File shape

```json
{
  "schema_version": 1,
  "static_hash": "<blake3 hex>",
  "dynamic_hash": "<blake3 hex>",
  "groups": [...],
  "commands": [...]
}
```

- **`schema_version`** — single integer; toolr refuses to load a
  manifest with a higher schema than it understands.
- **`static_hash`** — blake3 over the sorted `(path, contents)` of
  every `tools/**/*.py` file. Drives static-layer rebuilds.
- **`dynamic_hash`** — blake3 over the tools venv's installed
  package set. Drives dynamic-layer rebuilds.
- **`groups`** / **`commands`** — the actual command tree. Each
  entry carries an `origin` field (`"static"` or `"dynamic"`)
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
  "path_constraints": null,
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
  ```

  See `src/parser/types.rs` for the full enum. `null` means the type
  layer couldn't resolve a supported type — third-party fragments
  built against the legacy schema fall through to string semantics.
- **`path_constraints`** — for path-typed parameters annotated with
  `arg(must_exist=True, ...)`. Object with optional `must_exist`,
  `must_be_file`, `must_be_dir` booleans. `null` when no constraints
  were declared.
- **`allowed_values`** — for `Literal[...]` / `Enum` types, the
  values clap validates against. Also used by tab completion.

## Static layer

Built from `tools/**/*.py` via the `ruff_python_parser` Rust crate.
Pure AST traversal — never imports user code, so it's safe to run
without a working venv. Captures:

- `command_group(...)` declarations.
- `@group.command` / `@group.command("name")` decorations.
- Function signatures (positional vs keyword, defaults, annotations).
- Google-style docstrings (summary, description, `Args:` block).
- Local `Literal[...]` and `enum.Enum` definitions (resolved across
  files via a symbol table).

The static layer is rebuilt when toolr detects `static_hash` drift
against the on-disk files.

## Dynamic layer

Built by spawning `python -m toolr._introspect --tools-root <tools>`
inside the resolved tools venv. The helper:

1. Inserts `<tools>/..` on `sys.path` so `import tools` works.
2. Imports every `tools.*` module — registering every
   `command_group` / `@group.command` call.
3. Walks `importlib.metadata.entry_points(group="toolr.tools")` for
   third-party packages without a static manifest fragment.
4. Dumps a JSON payload to stdout describing the merged registry.

The dynamic layer fills in things the static parser can't see: cross-
package re-exports, runtime-generated commands, and third-party
packages that haven't shipped a static manifest fragment.

Toolr regenerates the dynamic layer when:

- The venv contents change (`dynamic_hash` drifts) — typically after
  `toolr project deps sync`.
- A command is invoked and the binary detects drift on entry.

## Manual rebuild

```sh
toolr project manifest rebuild
```

Runs both layers and writes the merged result. Used by the shipped
pre-commit hook (see [Pre-commit integration](pre-commit.md)) and
available for explicit invocation when you want to be sure the
manifest is current — for example before publishing a release that
includes new commands.

## Hashing details

Both hashes use blake3, written as lowercase hex.

- `static_hash` input: every file under `tools/` (excluding
  `__pycache__`, `.toolr-manifest.json`, and dot-prefixed names),
  sorted by path, each entry hashed as
  `len(path_bytes) || path_bytes || len(contents) || contents`.
- `dynamic_hash` input: sorted listing of `<venv>/lib/python*/site-
  packages/*` entries, each entry's name + metadata file mtime
  rounded to the nearest second.

This gives stable, content-addressable rebuild decisions across
machines and across CI / dev environments.
