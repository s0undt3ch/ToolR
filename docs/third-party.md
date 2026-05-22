# Third-party packages

Toolr supports **third-party command packages** — installable Python
packages that contribute commands to the `toolr` CLI when present in
the tools venv.

Commands are discovered via a static JSON manifest file
(`toolr-manifest.json`) that lives at the root of the installed
package. Toolr's Rust binary globs for these files at startup; no
Python import is involved.

## Why ship a JSON manifest

Toolr's CLI is a native Rust binary that boots in under 50 ms. Tab
completion and `--help` need the full command tree available
**before** Python starts. With an entry-point registration (the
approach that was removed in the 2025 rewrite), toolr had to spawn
Python and import every registered module to learn what commands
existed — one import per package, on every shell tab press.

A `toolr-manifest.json` is a sub-millisecond `glob()` + JSON parse,
no Python involved. For interactive use the difference is the
boundary between "instant" and "noticeable lag".

Command discovery globs
`<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`.
Any installed package that ships the file is picked up
automatically; no project-side configuration is needed.

## Generating the manifest

Most packages won't write the JSON by hand. They declare commands
with the usual `command_group` / `@command` API and let
`toolr self build-manifest` introspect.

Run the CLI inside the plugin's repo:

```sh
toolr self build-manifest my_pkg
```

Replace `my_pkg` with the dotted package name (e.g. `my_pkg` or
`my_pkg.sub`). The file is written to the package root — next to
`my_pkg/__init__.py`.

Re-run whenever your `command_group` / `@command` registrations
change.

### Static-only contract

`toolr self build-manifest` walks your package's source with a Rust AST
parser. It captures every `command_group(...)` / `@group.command`
declaration that the parser can see *statically* — same as the project
manifest builder. Dynamic registration (`for x in X: group.command(...)`)
is intentionally not supported: a manifest emitted from such patterns
would not match what the Rust dispatch path can resolve at runtime
anyway. If you need dynamic patterns, hand-edit the resulting
`toolr-manifest.json`.

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

## Shipping the manifest

The generated file must be included in the built wheel. The exact
mechanism depends on your build backend.

**hatchling** — add an `include` entry in `pyproject.toml`:

```toml
[tool.hatch.build.targets.wheel]
include = [
  "src/my_pkg/toolr-manifest.json",
]
```

**setuptools** — add a line to `MANIFEST.in`:

```text
include src/my_pkg/toolr-manifest.json
```

After building, verify the file is present in the wheel before
publishing:

```sh
unzip -l dist/my_pkg-*.whl | grep toolr-manifest
```

## Keeping it in sync

Run `toolr self build-manifest <pkg> --check` to detect drift
between the committed manifest and what regeneration would produce:

```sh
toolr self build-manifest my_pkg --check
```

Exit code 0 if in sync; non-zero (with a diff on stderr) if
drifted.

### Pre-commit hook

Add this to `.pre-commit-config.yaml` in your plugin's repo to
prevent committing a stale manifest:

```yaml
- repo: local
  hooks:
    - id: toolr-manifest
      name: toolr manifest in sync
      language: system
      entry: toolr self build-manifest my_pkg --check
      pass_filenames: false
      files: ^src/my_pkg/.*\.py$
```

Replace `my_pkg` and the `files` pattern to match your package.

### CI check

Add a step to your workflow to catch drift in pull requests:

```yaml
- name: Check toolr manifest is up to date
  run: toolr self build-manifest my_pkg --check
```

## What happens if you skip it

If `toolr-manifest.json` is not present in the installed package,
toolr's discovery glob will not find it and your plugin's commands
will not appear in `toolr --help` or `toolr <group> --help`.

To diagnose a missing manifest after installing a plugin, check
whether the file is in the installed package directory:

```sh
python -c "import my_pkg; print(my_pkg.__path__)"
```

That prints the on-disk path. Verify that a `toolr-manifest.json`
file is present in that directory:

```sh
ls "$(python -c 'import my_pkg; print(my_pkg.__path__[0])')"
```

If the file is absent, regenerate it (`toolr self build-manifest
my_pkg`) and rebuild the wheel with it included.

## Migration from entry-point plugins

> ⚠ The `toolr.commands` entry-point mechanism is removed.
> Entry-point declarations are now no-ops and are safe to delete.

If your plugin previously registered commands via
`[project.entry-points.'toolr.commands']`:

1. **Generate the manifest.** From inside the plugin's repo, run:

   ```sh
   toolr self build-manifest my_pkg
   ```

   This writes `toolr-manifest.json` next to `my_pkg/__init__.py`.

2. **Ship the file.** Include it in the built wheel as described
   in [Shipping the manifest](#shipping-the-manifest) above.

3. **Wire drift detection.** Add the pre-commit hook and CI step
   from [Keeping it in sync](#keeping-it-in-sync).

4. **Delete the entry-point declaration.** Remove the now-inert
   section from your `pyproject.toml`:

   ```toml
   # Delete this:
   [project.entry-points."toolr.commands"]
   commands = "my_pkg.commands"
   ```

After publishing the updated wheel, users who upgrade will have
their commands discovered automatically on the next `toolr`
invocation — no project-side changes needed on their end.

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

## Working example in the repo

[`tests/support/3rd-party-pkg/`](https://github.com/s0undt3ch/ToolR/tree/main/tests/support/3rd-party-pkg)
in the toolr repo is a complete third-party package fixture exercised
by the integration tests. Treat it as a copy-pasteable starting point.
