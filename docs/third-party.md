# Third-party packages

Toolr supports **third-party command packages** — installable Python
packages that contribute commands to the `toolr` CLI when present in
the tools venv.

There are two registration mechanisms, in order of preference:

1. **Static manifest fragment** (recommended) — the package ships a
   `toolr-manifest.json` next to its source. Toolr's binary discovers
   it via glob during manifest build; no import required.
2. **Entry-point fallback** (legacy) — the package declares
   `[project.entry-points."toolr.tools"]` in its `pyproject.toml`.
   Toolr discovers it via `importlib.metadata` and imports the named
   module to collect registrations.

## Why ship a static manifest

Tab completion and `--help` need the command tree available **before**
Python starts. With an entry-point registration, toolr has to spawn
Python and import every entry-point's module to learn what's there —
costly on every shell tab press.

A `toolr-manifest.json` is a sub-millisecond `glob()` + JSON parse,
no Python involved. For interactive use the difference is the
boundary between "instant" and "noticeable lag".

## The `toolr-manifest.json` fragment format

A fragment is a JSON object that declares groups and commands. Toolr
validates it against a schema version and merges it into the project's
manifest at build time.

Minimal shape:

```json
{
  "toolr_schema_version": 1,
  "groups": [
    {
      "name": "my-pkg",
      "title": "My Package",
      "description": "Commands contributed by my-pkg."
    }
  ],
  "commands": [
    {
      "name": "hello",
      "group": "my-pkg",
      "module": "my_pkg.commands",
      "function": "hello",
      "summary": "Say hello.",
      "description": "",
      "arguments": [
        {
          "name": "name",
          "kind": "keyword",
          "help": "Name to greet.",
          "default": "world",
          "type_annotation": "str",
          "allowed_values": []
        }
      ]
    }
  ]
}
```

The file lives at `<package_dir>/toolr-manifest.json` — i.e. next to
`my_pkg/__init__.py`. Toolr's manifest builder finds it via the glob
`<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`.

## The Python build helper

Most packages won't write the JSON by hand — they declare commands
with the usual `command_group` / `@group.command` API and let
`toolr.build` introspect.

```python
from toolr.build import build_manifest

result = build_manifest("my_pkg")
print(f"Wrote {result.output_path}")
```

See [`toolr.build`](reference/build.md) in the API reference for the
full signature.

Or via the bundled CLI:

```sh
python -m toolr.build my_pkg
```

Re-run whenever your `command_group` / `@group.command` registrations
change.

### `--check` for CI

In a CI job, pass `--check` to verify the committed manifest matches
what regeneration would produce:

```sh
python -m toolr.build my_pkg --check
```

Exit code 0 if in sync, non-zero (with a diff on stderr) if drifted.

## The rust CLI wrapper

If you're outside the package's own venv, the toolr binary will find
a working Python and run the build for you:

```sh
toolr self build-manifest my_pkg --check
```

Same flags as `python -m toolr.build`. See
[CLI reference → `self build-manifest`](cli.md#self-build-manifest).

## Entry-point fallback (legacy)

For packages that haven't migrated to static manifests yet, toolr
falls back to entry-points:

```toml
# pyproject.toml in the third-party package
[project.entry-points."toolr.tools"]
commands = "my_pkg.commands"
```

When toolr's binary doesn't find a `toolr-manifest.json` next to
the package, it spawns Python and imports `my_pkg.commands`. The
module's import-time `command_group` / `@group.command` calls
register the commands into the global registry.

This works but is **slower at completion time** — every tab press
pays the import cost. Once a package has any users, ship a static
manifest instead.

## Working example in the repo

[`tests/support/3rd-party-pkg/`](https://github.com/s0undt3ch/ToolR/tree/main/tests/support/3rd-party-pkg)
in the toolr repo is a complete third-party package fixture. It uses
the entry-point mechanism (older style) and is exercised by the
integration tests; treat it as a copy-pasteable starting point.

## Command resolution

When multiple sources contribute commands with the same name:

- **Project commands** (defined in your `tools/`) always win — they
  override anything from a third-party package.
- **Group augmentation:** if a third-party package targets an
  existing group name, its commands are added to that group rather
  than creating a duplicate.
- **Between third-party packages:** order is undefined — packages
  that share group/command names will produce a manifest-build error,
  pointing you to fix one of them.

## Distribution checklist

- Include `toolr-manifest.json` in your package via
  `package_data` (setuptools), `include` (hatch), or the equivalent
  in your build backend. Verify it's in the built wheel before
  publishing.
- Pin a compatible `toolr` version in your package's dependencies.
- Toolr migrates older fragment schemas in-process when a newer
  binary meets an older fragment, but pin defensively if you're
  shipping pre-1.0.
