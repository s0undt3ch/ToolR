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
loads it only when its schema version is in the range this toolr reads,
then merges it into the project's manifest at build time.

`toolr self build-manifest` writes the file, so you never author it by hand. This
one is trimmed from the fragment of the example plugin:

```json
{
  "commands": [
    {
      "arguments": [
        {
          "allowed_values": [],
          "default": "World",
          "help": "Name to greet (default: World).",
          "kind": "optional",
          "name": "name",
          "resolved_type": {"kind": "str"},
          "type_annotation": "str"
        }
      ],
      "description": "Say hello to someone.",
      "function": "hello_command",
      "group": "third-party",
      "module": "toolr_example_plugin.commands",
      "name": "hello",
      "origin": "third_party",
      "summary": "Say hello to someone."
    }
  ],
  "groups": [
    {
      "description": "Tools contributed by a third-party plugin.",
      "name": "third-party",
      "origin": "third_party",
      "parent": null,
      "title": "Third Party Tools"
    }
  ],
  "package": "toolr_example_plugin",
  "toolr_schema_version": 2
}
```

`groups` and `commands` use the same `Group` and `Command` shapes as the project's own manifest,
so a plugin command gets the same validation, `arg()` options and completion as a local one.

### Schema version

`toolr_schema_version` is the *lowest* toolr schema that can read the fragment, not the schema of
the toolr that built it. `toolr self build-manifest` computes it from the types and features the
plugin uses, so a plugin that sticks to long-standing features stays loadable by future toolr
releases that still read this shape.
Fragments in this format need toolr 0.34.0 or newer.

A toolr whose own schema is `C` and whose oldest readable fragment schema is `F` treats a fragment
that declares `M` like this:

| Fragment | Result |
| --- | --- |
| `M` below `F` | Skipped with a warning. Rebuild the plugin. |
| `M` above `C` | Skipped with a warning. Upgrade toolr. |
| `M` between `F` and `C` | Loaded. |
| Missing, not an integer, or 0 | The manifest build fails. |

A skipped plugin doesn't break the CLI: local commands and other plugins keep working, and toolr
prints `toolr: warning: skipping plugin <pkg>: ...` on stderr on every run (except for tab
completion, `--quiet`, `project`, `self`, `init`, `--version` and `-V`). The same happens to a plugin
with a command whose argument fails validation. Malformed JSON and the same command declared by two plugins
still fail the manifest build.

### Build-time checks

`toolr self build-manifest` runs the same checks as the project's own manifest build. Positional
arguments in the wrong order, and a command in a group the plugin doesn't declare, fail the build.
To add commands to a group of the host repo, declare that group in the plugin with the same full
path (see [Command resolution](#command-resolution)).

The file lives at `<package_dir>/toolr-manifest.json` — i.e. next to
`my_pkg/__init__.py`. Toolr's manifest builder finds it via the glob
`<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json`.

## Shipping the manifest

The generated file must be included in the built wheel. The exact
mechanism depends on your build backend.

**hatchling** — name the package directory in `packages`:

```toml
[tool.hatch.build.targets.wheel]
packages = ["src/my_pkg"]
```

Hatchling ships every file in a `packages` directory, including non-`.py`
files, so `toolr-manifest.json` lands next to `my_pkg/__init__.py` in the
wheel. This is the configuration
[`examples/plugin-package/`](https://github.com/s0undt3ch/ToolR/tree/main/examples/plugin-package)
uses, and CI builds it on every run.

Don't list the manifest in `include` on its own. `include` restricts the
wheel to the matching files, so
`include = ["src/my_pkg/toolr-manifest.json"]` builds a wheel that holds only
`src/my_pkg/toolr-manifest.json`: no Python modules, and the manifest at a
path toolr doesn't search.

**setuptools** — list the manifest in `package-data`:

```toml
[tool.setuptools.package-data]
my_pkg = ["toolr-manifest.json"]
```

This works whatever `include-package-data` is set to. An
`include src/my_pkg/toolr-manifest.json` line in `MANIFEST.in` also reaches the
wheel, but only while `include-package-data` is on. That is the default for
projects configured in `pyproject.toml`, and not for a legacy `setup.py`.

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

<!-- --8<-- [start:prek-hook] -->
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
<!-- --8<-- [end:prek-hook] -->

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

- **Project commands** (defined in your `tools/`) always win over a
  plugin command with the same group and name. The plugin command is
  hidden, and toolr warns about it on every run
  (`toolr: warning: ... hiding the one from <pkg>`), so a plugin release
  can't silently change what a local command does.
- **Between third-party packages:** when two or more plugins define the
  same command and your `tools/` doesn't, toolr can't know which one you
  want, so it disables that command. Everything else keeps working.
  Uninstall all but one of them to get the command back. If a group only
  held that command, it stays in `--help` but is empty. The warning names
  every plugin:

    ```text
    toolr: warning: deploy rollout is defined by more than one plugin (toolr_a, toolr_b), so it is disabled. Uninstall all but one. Choosing a winner in config is tracked in https://github.com/s0undt3ch/ToolR/issues/522
    ```

- **Group augmentation:** to add commands to a group of the host repo,
  the plugin declares that group itself (`command_group("ci", ...)`, with
  the same full path). A command in a group the plugin doesn't declare
  fails the build. The host's title and description win over the
  plugin's. Groups are matched by their full path, so a plugin's
  `docker.image` and a local `ci.image` stay separate.

Choosing the winner in configuration isn't supported yet. If you need it,
vote on [#522](https://github.com/s0undt3ch/ToolR/issues/522).

## Distribution checklist

- Include `toolr-manifest.json` in your package via
  `package-data` (setuptools), `packages` (hatchling), or the
  equivalent in your build backend. Verify it's in the built wheel
  before publishing.
- Pin a compatible `toolr` version in your package's dependencies:
  a plugin built with this format needs `toolr>=0.34.0`.
  A fragment is loaded only when its `toolr_schema_version` is in the
  range the installed toolr reads (see [Schema version](#schema-version)).
  There are no schema migrations: a fragment outside that range is
  skipped with a warning until you rebuild it or the user upgrades toolr.

## Working example in the repo

[`examples/plugin-package/`](https://github.com/s0undt3ch/ToolR/tree/main/examples/plugin-package)
in the toolr repo is a complete third-party package that CI builds on
every run. Treat it as a copy-pasteable starting point.
