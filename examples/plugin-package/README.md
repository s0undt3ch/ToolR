# toolr-plugin-example

A canonical, minimal third-party toolr plugin. This package exists to:

1. **Show plugin authors what a working plugin looks like.** The
   `pyproject.toml`, the `src/` layout, and the shipped
   `toolr-manifest.json` together form the complete contract for adding
   commands to someone else's `toolr` CLI.
2. **Serve as a packaging-contract regression test.** The toolr test
   suite builds this package into a wheel, installs it into a real
   tools venv, and asserts the commands declared here show up under
   `toolr --help`. If the discovery glob or the manifest schema ever
   drifts, this test catches it.

The package is **not published to PyPI** — it's an in-tree reference,
not a runtime dependency.

## How toolr finds these commands

At dispatch time, `toolr` globs every installed Python package in the
project's tools venv for a `toolr-manifest.json` at the package root:

```text
<tools-venv>/lib/python*/site-packages/*/toolr-manifest.json
```

For this example, that path resolves to
`site-packages/toolr_example_plugin/toolr-manifest.json` once installed
— hence the `[tool.hatch.build.targets.wheel.force-include]` block in
`pyproject.toml`.

## Authoring a plugin like this

1. Write your command modules with the usual `command_group(...)` /
   `@group.command` decorators (see `src/toolr_example_plugin/commands.py`).
2. Generate the manifest:

   ```bash
   toolr self build-manifest toolr_example_plugin
   ```

   This walks the package's registered command groups and writes the
   manifest next to `__init__.py`. Commit the result.
3. Ensure your build backend ships the manifest in the wheel. For
   hatchling, that's the `force-include` block (above). For setuptools,
   add `include src/<pkg>/toolr-manifest.json` to `MANIFEST.in`.
4. Wire `toolr self build-manifest <pkg> --check` into pre-commit and
   CI so the committed manifest never drifts from the source.

See `docs/third-party.md` for the full guide.

## Available commands

### `third-party`

- `hello [--name NAME]` — greet someone (default: "World")
- `version` — print the example plugin version

### `utils`

- `echo MESSAGE [--repeat N]` — echo a message N times
- `info` — print metadata about the example plugin
