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
argument types (generated from toolr's own parser), the
argument-shape rules, the prek hook recipe, and the example plugin
`pyproject.toml`. Installed skills no longer point agents at the toolr docs
site or repository. The generated tables include the `toolr.types` path types.

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
"did you mean" hint where one fits (`path_must_exist` points at `toolr.types.ResolvedPath`).
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
older fragment schemas. It loads a fragment only when its `toolr_schema_version` is in
the range this toolr reads, and skips the plugin with a warning otherwise.
The "Working example" link now points at `examples/plugin-package/`.
([#506](https://github.com/s0undt3ch/ToolR/issues/506))

### Readable debug and info log colours

The `log-debug` and `log-info` console styles, and the `stdout`/`stderr` level
labels, no longer use Rich's `dim` modifier. On many terminal palettes `dim`
turned the blue and cyan into near-illegible grey. The named ANSI colours still
follow the terminal's own light or dark palette.

### Path types replace `arg(must_*)`

`toolr.types` gains path types that say what a path argument must be. The toolr binary checks each
one while it parses the command line, and your command always receives a `pathlib.Path`:

- `NewPath`: must not exist; its parent directory must.
- `FilePath`, `DirectoryPath`: must exist as that kind; canonicalised.
- `ExecutablePath`, `WritableDirectoryPath`: as above, plus executable or writable.

All seven path types, including the existing `AbsolutePath` and `ResolvedPath`, are now
`typing.NewType`s rather than plain aliases, so type checkers tell them apart. **Typing-level
break:** passing a bare `Path` where one of them is expected is now a type error.

**Breaking:** `arg(must_exist=…)`, `arg(must_be_file=…)` and `arg(must_be_dir=…)` are removed. The
manifest build fails and names the replacement:

| Before | After |
|---|---|
| `Annotated[Path, arg(must_exist=True)]` | `ResolvedPath` |
| `Annotated[Path, arg(must_be_file=True)]` | `FilePath` |
| `Annotated[Path, arg(must_be_dir=True)]` | `DirectoryPath` |

A plugin that still uses them fails when you run one of its commands, with an error saying
`arg()` has no such keyword. Rebuild it against this release (`toolr self build-manifest --check`
in its CI catches it). A command from a stale local manifest cache fails the same way.

The local manifest schema is now version 2. An existing cache is rebuilt on the next run; there is
nothing to do.

On Windows, `ResolvedPath` and the other canonical path types now hand your command a plain path
(`C:\Users\...`) instead of a verbatim one (`\\?\C:\Users\...`), which `pathlib` treated as a
different drive.

### Plugin commands behave like local commands

A command shipped in a plugin now gets the same CLI behaviour as one in a repo's `tools/`. Its
arguments are validated by clap before Python runs, so bad values and `Email` fail early, and the
path types from the section above check the filesystem. Every `arg()` field (`aliases`, `metavar`,
`env`, `hide`, `display_order`, `help_section`, `conflicts_with`, `requires`, `nargs`) now takes
effect, and `tuple[T1, T2]` arguments enforce their element
count and types. Nested plugin groups (`docker` then `image`) work too. Before, they produced a
top-level group literally named `docker.image`.
([#520](https://github.com/s0undt3ch/ToolR/issues/520))

**Migration: rebuild and republish your plugins.** The `toolr-manifest.json` fragment now carries the
manifest's own `Group` and `Command` types, so v1 fragments can't be read. toolr skips a plugin
whose fragment is too old, or needs a newer toolr, and prints
`toolr: warning: skipping plugin <pkg>: ...` on stderr on every run. The rest of the CLI, including
your local commands, keeps working. Run `toolr self build-manifest` with this release and ship the
result. toolr 0.33.0 and older abort the whole manifest merge on a fragment built by this release.
That can't be fixed in binaries that already shipped, so upgrade to toolr 0.34.0 or newer wherever
the rebuilt plugin is installed.

`toolr_schema_version` in a fragment is now the lowest toolr schema that can read it, computed by
`toolr self build-manifest` from what the plugin uses. A plugin that uses only long-standing features
stays loadable by future toolr releases that still read this fragment shape. `toolr self build-manifest
--schema-version` is removed, because the value is no longer chosen by hand.

**Plugin authors: two build errors.** `toolr self build-manifest` now runs the same checks as the
local build. A plugin whose positional arguments are out of order fails the build, and so does a
command in a group the plugin doesn't declare (with a "did you mean" hint). To add commands to a
group from the host repo, such as `ci`, the plugin now declares that group itself with the same full
path. The host's title and description win at merge.

A local command that hides a plugin command with the same group and name still wins, but now warns
(`toolr: warning: ... defines ci lint, hiding the one from <pkg>`) on every run instead of hiding it
silently. Warnings don't print for tab completion, `--quiet`, `project`, `self`, `init`, `--version`
or `-V`. Choosing the winner in configuration is tracked in
[#522](https://github.com/s0undt3ch/ToolR/issues/522).

Two plugins that define the same command no longer stop toolr from working. Before, the manifest build failed, and
on a fresh clone or a CI runner no command ran, local ones included. Now that command is disabled, and a warning on
every run names every plugin that defines it (`toolr: warning: deploy rollout is defined by more than one plugin
(toolr_a, toolr_b), so it is disabled. ...`). Uninstall all but one to get it back. Both this warning and the
shadowing one now link [#522](https://github.com/s0undt3ch/ToolR/issues/522), where you can vote for choosing the
winner in configuration.
