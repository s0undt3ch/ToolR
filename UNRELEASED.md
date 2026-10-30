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

Both wheels now carry the `Development Status :: 4 - Beta` PyPI classifier instead of Alpha.

`toolr-py`'s dispatch-schema mismatch error now names the side to upgrade: run
`toolr project venv sync -P toolr-py` when the binary is newer, or upgrade the binary when it is
older. It used to always point at `toolr project venv upgrade`, which was removed in 0.22.0. The
error comes from the `toolr-py` in your tools venv, so a venv still on an older `toolr-py` keeps
printing the old text; the binary's version-skew warning above it gives the right command.

Pressing Ctrl-D (or stdin closing) at the uv install prompt now declines the install. It used to
count as consent and download uv. Bare Enter still installs.

In a repo without `tools/pyproject.toml`, toolr now runs commands with `python3` (or `python`) from
`PATH`, as documented. It used to fail every command with "Python interpreter not found at python3"
unless `$TOOLR_PYTHON` held an absolute path. `$TOOLR_PYTHON` now also accepts a bare name such as
`python3.12`, looked up on `PATH`. A `PATH` interpreter older than Python 3.11 is skipped, and
the error names it if nothing newer is found.

`toolr project venv sync -P`, `venv lock -P` and `venv remove` now ignore case and treat `-`, `_` and
`.` as the same when checking that a package is declared, following PEP 503. A `tools/pyproject.toml`
that declares `toolr_py` or `Toolr.Py` used to fail the "not declared" check for `toolr-py`.

From this release, `toolr-py`'s runner checks the spec's schema version before validating the rest of
the spec. When the binary and `toolr-py` disagree, the error names which side to upgrade, even if a
field was removed or retyped between schema versions. The fix lives in `toolr-py`, so a tools venv
still on an older `toolr-py` keeps printing "failed schema validation" in those cases.

`--quiet` now means "never prompt to install uv" on every subcommand that takes it. `toolr project
init`, `venv lock`, `venv add`, `venv remove` and `venv run` used to ask on a terminal even with
`--quiet`; they now fail with the uv install hint instead. The root `toolr --quiet project …` form,
including `venv shell`, never prompts either. Set `TOOLR_AUTO_INSTALL_UV=1` to install without
asking.

`toolr project init` no longer writes a `requires-python` floor older than toolr supports. With
Python 3.9 as `python3`, it used to write `>=3.9`; it now writes `>=3.11`. A newer `python3` still
sets the floor, and an explicit `--python` is used as given.

The managed uv install no longer needs `tar` or `unzip` on the host. Toolr now unpacks the download
itself, which fixes `toolr __install-uv-now` and the install prompt on stock Windows, where `unzip`
isn't available. A download that can't be unpacked now fails with a message pointing at the manual
uv install.

A tools venv whose Python was uninstalled under it (for example by `mise prune`) is now rebuilt.
`toolr project venv sync` and command dispatch used to treat it as up to date and skip the sync.
They then failed with "does not contain a python interpreter" or "Python interpreter not found"
until the venv was deleted by hand. Now both re-sync it, and `toolr project venv run --no-sync`
says the interpreter is gone and how to fix it.

**Breaking:** a `bool` parameter on a native command must now be a flag that defaults to `False`.
Every other shape fails the manifest build with "invalid parameter declarations", and all offending
parameters are listed at once. Rewrite each as follows:

- `colour: bool = True`: write `no_colour: bool = False` (CLI `--no-colour`) and test `if no_colour:`
  in the body. Or rename it once at the top with `colour = not no_colour`.
- `*, verbose: bool`, with no default: add `= False`. A positional `flag: bool` needs the default
  too, and should be passed by keyword.
- `colour: bool | None = None` or `Optional[bool]`: use `Literal["auto", "always", "never"]` or an
  `Enum`.
- `list[bool]`, `tuple[bool, ...]` and `*flags: bool`: no replacement. Use one `bool = False` flag per
  value.
- Any other default, including `= None`, a computed value, or a constant imported from another
  module (`verbose: bool = OFF`): write the literal `verbose: bool = False`. toolr can't read an
  imported value without running your code, and a constant that turned out to be `True` would
  bring back the silently ignored default. A constant defined in the same file still works.

A parameter whose annotation is an alias of `bool`, such as `Verbose = Annotated[bool, arg(env="V")]`
with `verbose: Verbose = False`, is now a flag. It used to take a `true`/`false` value
(`--verbose true`). Plugin manifests that were already built keep their old behaviour until the
plugin is rebuilt.

Building toolr from an sdist now needs Rust 1.99.1 or newer, the toolchain toolr is built and
tested with. The crates declare it as `rust-version`, so an older toolchain fails with a clear
message instead of a compile error inside a dependency.

toolr now refuses to run a repository's commands or sync its tools venv until you trust that
repository once: run `toolr project trust` there, or answer `y` at the prompt. Repositories where
toolr already built a venv are trusted automatically. In CI, set `TOOLR_TRUST_ALL=1`; the
`s0undt3ch/ToolR` GitHub Action sets it for you. `--help`, tab completion and manifest builds never
need trust. The mise `enter` hook (`toolr project venv sync --quiet`) never prompts, so in an
untrusted repository it prints the refusal, while a directory with no `tools/pyproject.toml` still
exits silently. The interpreter provenance check is removed; repository trust replaces it.
