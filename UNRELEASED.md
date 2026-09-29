<!--
UNRELEASED.md — Queued release notes for the next release.

Append narrative entries here as PRs land. On release, the
`_prepare-release.yml` workflow folds the content of this file
into the `### Notes` subsection of both the GitHub release body
and CHANGELOG.md (under the new version's heading), then resets
this file to empty for the next cycle.

Empty between releases is the steady-state — there's no header,
no scaffolding. Just write whatever should appear in the notes.
-->

### `toolr.current_context()`

Added `toolr.current_context()`, a `contextvars`-backed accessor a
command-authoring helper can call to get the `Context` of the toolr command
currently executing, without it being passed as a parameter. Purely
additive: `@command`-decorated functions keep receiving `ctx` as their
required first argument, and existing helpers that take `ctx` explicitly are
unaffected. See `toolr.testing.set_current_context()` for testing helpers
that use it.

### Agent skills are now self-contained

Each skill under `skills/` now carries everything it needs: the supported
argument types and path constraints (generated from toolr's own parser), the
argument-shape rules, the prek hook recipe, and the example plugin
`pyproject.toml`. Installed skills no longer point agents at the toolr docs
site or repository. The docs' path-constraint table previously documented
`arg(path_must_exist=...)`; the real keywords are `must_exist`,
`must_be_file` and `must_be_dir`, and the docs now generate that table from the
code.

### The authoring skill covers external command sources

The `toolr-command-authoring` skill now teaches agents how to put existing
argparse scripts and Django management commands behind a `DispatchCommand`
dispatcher, instead of rewriting each one as a toolr command. It ships the
configuration table and worked examples from the docs, and `DispatchCommand`
now has a docstring. The external sources docs gain a plain-argparse example.
They now also say that a script's subparser arguments merge into its single
command, and list the payload's `schema` field.

### Install toolr with mise's packslip backend

Releases now ship a signed [packslip](https://packslip.dev/) manifest, so
`mise use packslip:github.com/s0undt3ch/ToolR` installs toolr with signature
and checksum verification, version-matched shell completions, and the three
toolr agent skills (`mise skills sync`). The aqua backend remains available
for releases published before packslip support.

### Unknown `arg()` keywords now fail the manifest build

A misspelled or unsupported `arg()` keyword, such as
`arg(path_must_exist=True)`, and any positional argument passed to `arg()` now
fail the manifest build with the module, function and argument, plus a
"did you mean" hint where one fits (`path_must_exist` suggests `must_exist`).
Argparse-style `help=`, `type=` and `default=` instead point at where toolr
takes that information from. The check follows `arg()` calls nested in
`X | None`, `Optional[...]` and `list[...]` annotations and through
module-level aliases.
The check also covers `toolr self build-manifest`, so a plugin can't ship a
manifest containing a command that can never run. Before, the parser silently
dropped the keyword: the command showed up in `--help` and then failed with a
`TypeError` the first time it ran. No command that worked before is affected.
The build-error heading for these failures, and for unsupported parameter
types, now reads "invalid parameter declarations" instead of "unsupported
parameter types". ([#500](https://github.com/s0undt3ch/ToolR/issues/500))

### Commands without a docstring now fail the manifest build

**Breaking.** A `@command` whose docstring gives no summary line now fails the
manifest build. That covers a missing docstring, an empty or blank one, and one
with only sections such as `Args:`. The error names the module and function of
every offending command. Before, such a command built with an empty `--help`
summary, although the docs already said it was rejected. The check applies to
`toolr project manifest rebuild`, the automatic rebuild of a stale manifest, and
`toolr self build-manifest`. The `@command` decorators apply the same rule at
import time and raise a `ValueError`, so a `toolr.testing.CommandsTester`
discovery test fails the same way the build does. Commands grafted from
argparse sources are not affected. To fix a failing build, give each listed
command a one-line docstring.

toolr now also refuses to run under `python -OO` or `PYTHONOPTIMIZE=2`, which
strip every docstring. Declaring a command, or a group described by its
docstring, raises a `RuntimeError` saying so. Before, a group declared with
`docstring=__doc__` failed there with a misleading "must pass either docstring
or description" error.
([#501](https://github.com/s0undt3ch/ToolR/issues/501))

### Plugin packaging docs: fixed hatchling recipe

The "Shipping the manifest" docs told hatchling users to list
`toolr-manifest.json` under `include`, which builds a wheel with no Python
modules and the manifest at the wrong path. They now use
`packages = ["src/<pkg>"]`, matching the packaging skill and
`examples/plugin-package/`. The setuptools recipe now uses `package-data`,
which works whatever `include-package-data` is set to, instead of relying on
`MANIFEST.in`. The docs also no longer claim that toolr migrates
older fragment schemas: it accepts only the current `toolr_schema_version`.
The "Working example" link now points at `examples/plugin-package/`.
([#506](https://github.com/s0undt3ch/ToolR/issues/506))
