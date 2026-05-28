---
name: toolr-command-packaging
description: |
  Ship an existing set of toolr commands as a distributable Python
  plugin. Use when packaging toolr commands as a wheel/PyPI package;
  when wiring `toolr self build-manifest` into a CI gate; when
  configuring `pyproject.toml` or a build backend to include
  `toolr-manifest.json` in the wheel; when migrating an old
  entry-point-based toolr plugin to the static-manifest format; or
  when debugging "wheel installs but commands don't appear". Triggers
  on phrases like "ship toolr commands as a plugin", "publish a
  toolr plugin", "include toolr-manifest.json in the wheel", "toolr
  plugin pyproject.toml", and the legacy "toolr.commands entry
  point". Stays inert on authoring requests (covered by the
  `toolr-command-authoring` skill) and on generic Python packaging
  questions in non-toolr projects.
---

# Packaging toolr commands as a distributable plugin

You are taking a set of already-written toolr commands and shipping
them as a pip-installable Python plugin so other projects can
`pip install <your-package>` and get the commands. If the commands
don't exist yet, that is the
[`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring)
skill's job — invoke it first.

This skill is **strictly the toolr-specific delta on regular Python
packaging**. It does not re-teach build backends, wheel layout, or
PyPI publishing — your existing Python-packaging knowledge applies
unchanged. It tells you exactly three things on top of that.

## The three rules

1. **Generate `toolr-manifest.json`** for your package via
   `toolr self build-manifest --source-dir <pkg-src> --package
   <pkg-name>`. The command writes the JSON next to your package's
   `__init__.py`. The schema is documented in
   [`references/packaging.md`](references/packaging.md); you do not
   write the JSON by hand.

2. **Include the manifest in the wheel.** Build-backend-specific —
   the canonical worked example uses hatchling, which ships every
   non-`.py` file inside the named `packages` directory by default,
   so a `[tool.hatch.build.targets.wheel] packages = ["src/<pkg>"]`
   line is all you need. For other backends (setuptools, poetry),
   consult their data-file inclusion docs; the structural
   requirement is the same — `toolr-manifest.json` must end up at
   the package's installed root.

3. **Wire `--check` as a CI gate.** `toolr self build-manifest
   --source-dir <pkg-src> --package <pkg-name> --check` exits
   non-zero when the committed manifest doesn't match what the
   builder would produce from the current source. Run it on every
   PR. A prek hook is a good local complement. The
   [`toolr-ci-setup`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-ci-setup)
   skill shows the canonical workflow.

## The canonical worked example

`examples/plugin-package/` in the toolr repository is the reference
plugin. It is a real hatchling-built package, ships
`src/toolr_example_plugin/toolr-manifest.json`, and CI builds it
end-to-end on every run. Read its `pyproject.toml` if you want a
known-good wheel-include configuration to copy.

The structure is:

```text
plugin-package/
├── pyproject.toml             # hatchling backend, packages = ["src/toolr_example_plugin"]
├── README.md
└── src/
    └── toolr_example_plugin/
        ├── __init__.py
        ├── commands.py         # @command / @command_group definitions
        └── toolr-manifest.json # generated; committed; checked in CI
```

## Verifying after install

```sh
# 1. The wheel must contain the manifest at the package root.
unzip -l dist/<pkg>-*.whl | grep toolr-manifest.json
# expect: <pkg>/toolr-manifest.json

# 2. After install, the file must reach site-packages.
python -c "import <pkg>, pathlib; print(list(pathlib.Path(<pkg>.__file__).parent.glob('toolr-manifest.json')))"

# 3. Your commands must show up in toolr --help.
toolr --help | grep <group>
```

If step 1 fails, your build backend is not picking up the JSON —
check `[tool.hatch.build.targets.wheel] packages` (or the
equivalent in your backend) actually points at the directory
containing `toolr-manifest.json`. If step 1 succeeds but step 2
doesn't, you have a multi-level layout mismatch — toolr's loader
expects the JSON at the package's installed root, not inside a
nested subdirectory.

## Common mistakes

- **Forgetting to regenerate after a source change.** The `--check`
  gate catches this; without it, your wheel ships stale metadata
  and end users see "your-package installed cleanly but the
  commands don't appear".
- **Adding the JSON to `.gitignore`.** The manifest is a committed
  build artifact, not a regenerable cache. CI must be able to diff
  it against fresh regeneration. Track it in git.
- **Configuring `[project.entry-points."toolr.commands"]`** — that
  was the pre-PR-#234 plugin mechanism and is removed. Plugins use
  `toolr-manifest.json` exclusively now. See the migration note
  below.

## Migration note for legacy plugins

<!-- review after 1.0 -->

If you have an existing plugin that registers commands via the
`[project.entry-points."toolr.commands"]` table in
`pyproject.toml`, that mechanism was removed by the
`dispatch_manifest_freshness` work and no longer functions. To
migrate:

1. Delete the `[project.entry-points."toolr.commands"]` table from
   `pyproject.toml`.
2. Run `toolr self build-manifest --source-dir <pkg-src> --package
   <pkg-name>` from the plugin source tree.
3. Commit the resulting `toolr-manifest.json` next to your
   package's `__init__.py`.
4. Ensure your build backend includes the new file in the wheel
   (hatchling's default behaviour does; verify with `unzip -l`
   against a built wheel as above).

The migration is one-shot and durable — future updates regenerate
the JSON, not the `pyproject.toml`.

## References

- [`references/packaging.md`](references/packaging.md) — the
  manifest fragment schema, the `Origin` enum, the plugin discovery
  glob, and the host-side fields your fragment merges into.
  Generated from `toolr-core`'s own serde types via
  `cargo xtask build-skill-refs`; cannot drift out of sync with
  what the loader actually accepts.

## Authoring is a different problem

If you haven't written the toolr commands yet, this skill cannot
help you. Invoke the
[`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring)
skill first to get the commands working in a project's `tools/`
tree, then come back here when you're ready to ship them as a
plugin.
