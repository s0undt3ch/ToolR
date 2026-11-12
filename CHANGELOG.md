# Changelog

All notable changes to this project will be documented in this file.

This project uses [*git-cliff*](https://git-cliff.org/) to automatically generate changelog entries
from [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.37.0 - 2026-11-12

### Notes

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

**Breaking:** toolr now refuses to run a repository's commands or sync its tools venv until you
trust that repository once: run `toolr project trust` there, or answer `y` at the prompt. No
repository is trusted after upgrading, including ones toolr already built a venv for. In CI, set
`TOOLR_TRUST_ALL=1`; the `s0undt3ch/ToolR` GitHub Action sets it for you from this release on. An
older pinned action run with a newer `version:` doesn't, so set `TOOLR_TRUST_ALL: "1"` yourself in
that case. `--help`, bare `toolr` and tab completion need no trust but show only toolr's own
commands until you trust the repository; `toolr <command>`, `toolr project manifest rebuild` and
`toolr self build-manifest <package>` ask for trust first. The mise `enter` hook (`toolr project
venv sync --quiet`) never prompts, so in an untrusted repository it prints the refusal, while a
directory with no `tools/pyproject.toml` still exits silently. The interpreter provenance check is
removed; repository trust replaces it. Trust covers the whole git repository, including all its
worktrees, and untrusting from any checkout untrusts all of them. `toolr project trust` prints the
repository path on stdout and `toolr: trusted <repo>`, or `toolr: <repo> was already trusted`, on
stderr; `--quiet` keeps only the path. `toolr project untrust` likewise adds `toolr: untrusted
<repo>` on stderr unless run with the root `-q`.

**Breaking:** a `tools/` directory is a toolr project only if it contains `pyproject.toml`. toolr
finds the project by walking up to the nearest `tools/pyproject.toml`, and ignores any `tools/`
without one. An unrelated `tools/` package below the repo root, such as `src/pkg/tools/`, used to
hide every command (`unrecognized subcommand`); a bare `tools/` above a project (such as `C:\tools`)
can no longer take it over. A repo whose `tools/` has no `pyproject.toml` is no longer supported:
run `toolr project init` there to create one. Running one of its commands, or a `toolr project`
command, now names the `tools/` and says `tools/pyproject.toml` is missing. Without a
`tools/pyproject.toml` toolr has no tools venv, so the fallback that ran commands with
`$TOOLR_PYTHON`, `python3` or `python` from `PATH` is gone, and toolr no longer reads
`$TOOLR_PYTHON`. The `toolr.testing` pytest plugin finds the repo root the same way.

**Breaking:** a `*.py` under `tools/` (or in a plugin package passed to `toolr self build-manifest`) that is a symlink is now an error instead of being silently skipped, so toolr never picks up code from outside `tools/` by surprise. toolr lists every such link and stops; replace each with the file itself, or share the code as a Python package that `tools/pyproject.toml` depends on. `CommandsTester.discover()` raises the same error. Symlinked directories are still ignored.

**Breaking:** `ctx.run` timeouts now cover the whole call. If the command exits but something it started in the background keeps its output open, `ctx.run(..., timeout_secs=N)` raises `CommandTimeoutError` (or `CommandTimeoutNoOutputError`) when the limit runs out. It used to wait for that process with no limit and then return the command's exit code. Without a timeout, `ctx.run` still waits for the output to close.

When a timeout fires, toolr now also kills the processes the command started, not just the command. It used to kill only the direct child, so `ctx.run("sh", "-c", "sleep 999; true", timeout_secs=1)` left `sleep` running. A process that detached itself first, such as a daemon that double-forks, can't be found and keeps running.

**Breaking:** `toolr project trust` now runs `uv sync` on the tools venv after trusting the repository, when the venv needs one and you accept on a terminal or `TOOLR_AUTO_INSTALL_UV=1` is set, and it exits non-zero if that sync fails. It used to only record the trust and always exit 0, so scripts and hooks that run it with `TOOLR_AUTO_INSTALL_UV=1` now sync too. Pass `--no-sync` to keep the old trust-only behaviour. The trust is kept either way.

A fresh clone now needs one step before its commands run. On a terminal `toolr project trust` asks first (`Sync it now? [y/N]`); declining keeps the trust and points you at `toolr project venv sync`. Running a command in an untrusted repository works the same way: answer `y` to the trust prompt and, if the venv is missing, out of date or broken, toolr offers to sync it and then runs the command. The sync prompt replaces the uv install prompt for that sync, so you see at most two prompts. With `--quiet` or no terminal toolr doesn't ask and doesn't sync unless `TOOLR_AUTO_INSTALL_UV=1` is set.

**Breaking:** bash completion now needs bash 4.4 or newer. On bash 4.0 to 4.3, which worked before, sourcing the script prints one warning and registers nothing, so completion is lost until you upgrade bash.

In return, completion works for file names and choice values containing `:`, `=`, `@` or `$` (bash split `:` and `=` into separate words, so they completed nothing or repeated the typed prefix), and it no longer needs the bash-completion package. The install paths in `docs/cli.md` are corrected too. Re-run `toolr self completion install bash --force`, or re-source the script, to pick it up.

The warning prints only in an interactive shell, so a bashrc sourced by scripts or `bash -c` stays quiet.

**Breaking:** `toolr` and `CommandsTester` now refuse several shapes they used to read differently. A command name defined twice in one group, in one file or across files of one `tools/` tree or plugin package, fails with "duplicate commands" listing every definition (it used to keep one silently); rename all but one, e.g. with `name="…"`. A command decorator on a nested `def` (inside another function, a class or an `if`) and a `group=` or command name that isn't a string literal fail with "unsupported command declarations"; move the `def` to module level and write the value as a literal. A local command shadowing a plugin's, or two plugins defining one command, still only warn.

Both now also reject a command decorator inside `if TYPE_CHECKING:`, a platform guard or an uncalled factory, even when that code never runs, and a `def` carrying two command decorators; each command must be a top-level `def`, with platform checks inside its body. toolr recognises `command` imported from `toolr` or any `toolr.*` submodule, re-exported within `tools/`, or assigned from `toolr.command`; one imported from anywhere else, such as click, counts only when called with `group=`, and a module's own `def command` never does.

**Breaking:** the manifest build now fails when no `command_group` call for a group passes a description or docstring (an empty one counts as none, and an expression other than a string literal, such as a constant, counts as one), listing every module that declares it, and when one call passes `docstring=` together with `description=` or `long_description=`, each as a literal or the `__doc__` of a module with a docstring. Title-only calls such as `command_group("ci", "CI")` still work as long as one declaration describes the group, in any module and in any order. Before, `command_group` raised for these only when the call created the group as its module imported, so a group created in a module no command imports could build and run; such a tree now fails to build. `CommandsTester` reports both with the same text. A plugin that adds commands to a host's group must describe that group in its own declaration, or its `toolr self build-manifest` fails.

**Breaking:** a `command_group(...)` call that isn't the whole value of a module-level `name = command_group(...)` assignment or bare statement now fails the manifest build with "unsupported command declarations", and so does `CommandsTester`. That covers tuple, chained and walrus bindings, calls under an `if`, `try`, `with`, loop or `match`, calls inside a function, lambda, comprehension or class, calls nested in another expression, and a group whose name isn't a string literal. Before, toolr dropped such a group and every command on it without a word and exited 0. Give each group its own top-level `name = command_group("name", ...)` line, and move any platform check into the command body.

**Breaking:** `toolr --quiet` (and `-q`) no longer hides warnings. It drops success lines, progress and advice, never errors, refusals or warnings, because a warning means the result may be incomplete. A `-q` run now prints on stderr what it used to drop, such as a skipped plugin or a `toolr-py` version mismatch, so scripts that run `toolr -q <command>` may see new output on every run.

**Breaking:** every `TOOLR_*` boolean variable now reads `1`, `true` or `yes` (any case, surrounding whitespace ignored) as on, and everything else as off. `TOOLR_NO_CACHE_HINT` used to mean "set to anything", so `TOOLR_NO_CACHE_HINT=0` silenced the cache hint; it now leaves it on. `TOOLR_TRUST_ALL` and `TOOLR_AUTO_INSTALL_UV` used to need exactly `1` and now also take `true` and `yes`.

**Breaking:** the installers and the action now share one spelling, with one meaning, for how much to verify. Use `install.sh --verify=MODE` or `install.ps1 -Verify MODE`, where `MODE` is `full` (the default: SHA-256 and the SLSA attestation), `auto` (SHA-256, and the attestation only when `gh` is available), `checksum` (SHA-256 only) or `none` (nothing). The action takes `verify: full` (the default) or `verify: checksum`; it has no `none` or `auto` because it always checks the SHA-256, and any other value fails the step. The old spellings still work but are no longer documented, and are removed in 1.0: `--verify-attestation=require|auto|skip` and `-VerifyAttestation` become `full`, `auto` and `checksum`; `--no-verify` and `-NoVerify` still skip only the SHA-256; the action input `skip-attestation: true` becomes `verify: checksum` and now carries a deprecation message.

**Breaking:** `arg()` no longer takes `required=`, `choices=`, `nargs=`, `action=` or `group=`; they were deprecated and promised for removal in 1.0. A command that still passes one fails the manifest build with the replacement named. Use `T | None` or `*args: T` for `required=` and `nargs=`, a `Literal[...]` or `Enum` annotation for `choices=`, the parameter type for `action=` (`bool` is a flag, `list[T]` appends, `Count` counts), and `conflicts_with=[...]` or `help_section=` for `group=`.

**Breaking:** the method form `parent.command_group("child", ...)` is removed; it was deprecated and promised for removal in 1.0. `CommandGroup` no longer has a `command_group` method, and a module-level `x.command_group(...)` statement fails the manifest build with a pointer to the replacement. Declare the subgroup with a dotted name instead: `command_group("parent.child", ...)`, or `command_group("child", parent="parent")`. See the migration page.

**Breaking:** `toolr.__all__` shrinks to the names the documentation covers. `toolr.report_on_import_errors`, `toolr.MANIFEST_SCHEMA_VERSION` and `toolr.DispatchCommand` are no longer exported from the top-level package. Import `DispatchCommand` from `toolr.sources`. `MANIFEST_SCHEMA_VERSION` had no documented use. `report_on_import_errors` is deleted, along with `toolr.utils._imports`, not just unexported; if you used it, replace it with a plain `try`/`except`:

```python
try:
    import yaml
except ImportError as exc:
    raise SystemExit("install the extra: uv add pyyaml") from exc
```

The remaining surface is `Context`, `command`, `command_group`, `arg`, `arg_section`, `ArgSection`, `current_context` and `__version__`.

**Breaking:** `Context.run` takes named arguments instead of `**kwargs`: `cwd`, `env`, `input`, `text` and `encoding`, besides the timeout and output options it already had. Any other keyword now raises `TypeError` where it used to be passed on to the runner. `ctx.which` and `ctx.warn` are documented.

**Breaking:** toolr's own flags (`-d`, `-q`, `--timestamps`, `--no-timestamps`, `--timeout-secs`, `--no-output-timeout-secs`) are accepted only at the root, before the command. The per-command `-q` / `--quiet` of `project init`, `project trust` and `project venv run/sync/lock/add/remove` is gone: use the root `--quiet`, as in `toolr --quiet project venv lock`. The flags are listed under "ToolR Options" in `toolr --help`. If you put one after the command, toolr now says where it goes, for example ``toolr: `--debug` is a toolr option; put it before the command: `toolr --debug ci hello ...` ``. When other root flags were typed before the command, a literal `…` stands for them in the example. The passive cache hint no longer prints when clap rejects the command line.

**Breaking:** `--quiet` (`-q`) no longer turns errors into a silent exit 0; it only means less output. The new `toolr project venv sync --if-project` exits 0 silently when there is nothing to sync (not in a toolr project, no readable venv location, or uv missing and not installable without asking). Change the mise enter hook from `toolr project venv sync --quiet` to `toolr --quiet project venv sync --if-project`.

The yes/no prompts now all accept `y` and `yes`, so `yes` no longer declines the overwrite prompt of `project init`.

Both wheels now carry the `Development Status :: 4 - Beta` PyPI classifier instead of Alpha.

`toolr-py`'s dispatch-schema mismatch error now names the side to upgrade: run
`toolr project venv sync -P toolr-py` when the binary is newer, or upgrade the binary when it is
older. It used to always point at `toolr project venv upgrade`, which was removed in 0.22.0. The
error comes from the `toolr-py` in your tools venv, so a venv still on an older `toolr-py` keeps
printing the old text; the binary's version-skew warning above it gives the right command.

Pressing Ctrl-D (or stdin closing) at the uv install prompt now declines the install. It used to
count as consent and download uv. Bare Enter still installs.

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

Building toolr from an sdist now needs Rust 1.99.1 or newer, the toolchain toolr is built and
tested with. The crates declare it as `rust-version`, so an older toolchain fails with a clear
message instead of a compile error inside a dependency.

Bash tab completion no longer expands candidate values: a value such as `$(…)` declared in a
repository's `tools/` used to run when you pressed TAB. Values are now shell-quoted when inserted, and
values containing spaces complete as one word. Control characters in command names, choices, help text
and plugin warnings are no longer passed to your terminal, and the manifest build rejects group names,
command names and argparse flags containing whitespace or control characters, and choice values
containing control characters. Re-run `toolr self completion install bash` (or re-source the script) to
pick up the fix.

The same name and choice checks now also reject zero-width and bidi characters (such as U+200B or
U+202E), which could make one command or value display as another. Help text and descriptions keep
them, so right-to-left text and emoji sequences still render.

Editing `tools/pyproject.toml`, `tools/uv.toml` or `tools/.python-version` now marks the tools venv
as out of date, the same way a changed `tools/uv.lock` does. The next command, or `toolr project
venv sync`, re-syncs it. Before, a dependency added to `pyproject.toml` without re-locking was never
installed: `venv sync` printed "synced" without running uv. `--force` is now only needed after
changing `UV_*` environment variables or a user-level `uv.toml`.

Plugins installed in a tools venv whose path contains `[`, `]`, `*` or `?` (for example a repo
checked out under `~/src/[wip]/`) are found again. toolr used to treat those characters as glob
syntax, so it dropped the plugin's commands without a warning. The same fix applies to argparse
`scan_paths` under such a project root and to `toolr self build-manifest <package>`.

Command arguments are now converted to their annotated types even when the annotation's import
sits under `if TYPE_CHECKING:` (as ruff's TC001-TC003 rules arrange). One such annotation used to
stop conversion for every argument of that command, so `count: int` arrived as the string `"3"`.
When toolr can't resolve an annotation, it now names the argument in a warning instead of silently
passing the raw string.

In an untrusted repository toolr reads only what it needs to find the repository and its trust
record: it checks that `tools/pyproject.toml` exists, without opening it, and reads git's `.git` and
worktree back-link files. It neither reads nor writes `tools/.toolr-manifest.json`, parses nothing
under `tools/` and doesn't look inside `tools/.venv`, so inspecting an unfamiliar checkout leaves it
untouched. `toolr project venv path` now asks for trust too. Once you trust a repository, toolr uses
an in-tree `tools/.venv` only if it built that venv itself. That proves toolr created the venv, not
what is in it now, and `TOOLR_TRUST_ALL=1` skips the check, so it also runs an in-tree venv restored
from a CI cache. toolr never runs any other in-tree venv, including one an older toolr built or one
made with `uv venv`, since it may have come with the repository. The first time a command needs the
venv, plugin commands included, toolr explains why and asks before it removes the venv and builds a
fresh one; until then `--help` leaves out its plugin commands. Only a `y` on a terminal or
`TOOLR_REPLACE_UNTRUSTED_VENV=1` approves the removal, not `TOOLR_AUTO_SYNC`; with `-q` or no
terminal toolr leaves the venv in place and says how to approve. When toolr can't take the sync lock
in its trust store, for example because the store is read-only, it leaves the venv in place: the
sync fails and names the store.

`ctx.run(..., input=...)` no longer hangs when the command writes a lot of output before it reads
its input. toolr now feeds stdin from a separate thread, so the timeouts apply while the command
runs and reads it. A command that exits without reading all of its input no longer raises "Failed
to write to stdin"; its exit code is returned as usual.

The Setup ToolR action's `version: latest` now always resolves to the newest ToolR release. It used to
ask for the newest release of the repository running the workflow first, so a repository with releases of
its own got its own tag back and the install failed or picked the wrong toolr version.

The Setup ToolR action no longer throws away its `tools/.venv` cache when the repository's root `uv.lock`
changes. The cache key now follows the same `tools/` inputs toolr's own freshness check tracks:
`tools/pyproject.toml`, `tools/uv.lock`, `tools/uv.toml` and `tools/.python-version`. Expect one cache miss after
upgrading the action.

`toolr.testing.CommandsTester.discover()` now raises when a `tools/*.py` module fails to import, for
example because a command has no docstring. It imports every module first, then raises one
`ExceptionGroup` holding each module's error, so a discovery test fails the way `toolr` does. It
used to hide those errors and pass, sometimes with a group that had no commands. Discovery also
keeps the standard library on `sys.path` now: a `tools/*.py` that imports `sqlite3`, `tomllib` or
another stdlib module not loaded yet used to vanish from `collected_command_groups()` without an
error. A discovery test that passed only because of either problem now fails.

Toolr no longer runs a `uv` or `python3` found through an empty or relative `PATH` entry such as `.`
or a trailing `:`. Those entries point at the current directory, which is usually the repository
toolr is running in, so a file in the repo could run in place of the real tool. Toolr now skips them.
If you relied on one, put the absolute directory on `PATH` instead.

A relative default on a path-typed argument, such as `cfg: FilePath = "tools/pyproject.toml"`, now resolves from
the repo root, the same place the command runs. It used to be checked against the directory you ran `toolr`
from, so from a subdirectory both the command and its `--help` failed with "path does not exist". An
`AbsolutePath` or `NewPath` default, or `DirectoryPath = "."`, now also names a path under the repo root wherever
you run from. A path you type on the command line still resolves from your current directory.

Pressing Ctrl-C during a toolr command now interrupts it once. toolr used to forward the terminal's SIGINT to the
command as well, so a second `KeyboardInterrupt` could cut a `finally:` or `except KeyboardInterrupt:` block short.
toolr now forwards SIGTERM, and forwards SIGINT only when it can tell another process sent it. On Linux, a SIGINT sent
to the `toolr` process alone (`kill -INT`, `Popen.send_signal`) still reaches the command, while one sent to the whole
process group with `killpg` reaches it twice. On macOS, where a Ctrl-C and a `kill` look the same, toolr never forwards
SIGINT, so `kill -INT` aimed at the `toolr` process alone no longer reaches the command; signal the process group or
send SIGTERM instead.

`toolr.testing.CommandsTester` now records commands declared with the standalone `@command(group="…")` decorator, whether the group is declared in the same file or another one. They used to be missing from `collected_command_groups()`, although `toolr` ran them. A `group=` that names no declared group now makes `collected_command_groups()` raise a `ValueError` with the same message the manifest build gives.

A standalone `@command` with no `group=` now makes `collected_command_groups()` raise too, as the manifest build fails on it.

When a command module fails to import, whether it's one of your `tools/*.py` files or comes from a plugin, toolr now prints the traceback, so an import that fails in a helper module names that file and line. The "run `toolr project venv sync`" hint no longer appears when the missing module is part of your own `tools` package (a typo or a circular import), since syncing the venv can't fix those.

`toolr.testing` now exports `ConsoleVerbosity`, the type `make_context(verbosity=...)` takes.

Running a command in a repo with an in-tree venv (`venv-location = "in-tree"`) no longer leaves an untracked `tools/meta.json`. toolr now keeps that file in its cache directory, in an entry of its own (`<repo-key>-in-tree`) that `toolr project venv sync` writes too, so `toolr self cache list` shows the repo. If the repo used a cache venv before switching to in-tree, that old venv no longer looks recently used, and `toolr self cache prune` reclaims it once it goes stale. You can delete any `tools/meta.json` an earlier toolr left behind.

The Setup ToolR action now only accepts a toolr archive whose attestation was produced by ToolR's release workflow on `main` for that exact archive name. It used to accept an archive attested by any workflow in the ToolR repository, including CI builds of unreleased branches, and a signed archive of another version or platform renamed into place. Every published release still installs as before. Verifying an archive now needs gh 2.56 or later on the runner.

`toolr self build-manifest <package>` now finds the package on Windows, where it lives under `Lib\site-packages` in the tools venv. It used to look only under `lib/python*/site-packages` and always reported the package as not found. toolr now finds `site-packages` in the same way everywhere it looks.

`toolr.testing.CommandsTester` now resolves the top-level `tools` package from the fixture's `search_path` only, and no longer removes other entries from `sys.path`. Before, any entry whose path contained `site-packages` or `dist-packages` was kept even when it provided `tools`, so a repo checked out under a directory such as `windows-site-packages` could have its own `tools/` imported instead of the fixture's. Everything else on `sys.path` stays importable, including site directories that ship a stray top-level `tools` package, and such a package can no longer replace a fixture `tools/` that has no `__init__.py`.

ToolR can now release pre-release versions such as `1.0.0-rc.1`. They are published as GitHub
pre-releases, so the Setup ToolR action's `version: latest` and the install scripts keep resolving to
the newest final release. A pre-release's notes show everything since the last final release, and the
final release's notes and CHANGELOG entry cover all of it, pre-releases included. The release tooling
also no longer breaks once a pre-release tag exists, and it checks an explicit release version (a
leading `v` is accepted) before writing anything.

The `install.sh` and `install.ps1` installers, and the manual `gh attestation verify` command in the installation docs, now only accept a toolr archive whose attestation was produced by ToolR's release workflow on `main` for that exact archive name, as the Setup ToolR action does. They used to accept an archive attested by any workflow in the ToolR repository, including CI builds of unreleased branches, and a signed archive of another version or platform renamed into place. Every published release still installs as before. Verifying an archive now needs gh 2.56 or later, also with `--verify-attestation=auto` (`-VerifyAttestation auto`) when `gh` is installed. The mise installation docs now recommend pinning the packslip signer with `workflow = "release.yml@refs/heads/main"` (mise 2026.10.5 or later).

The installers, the documented manual command and the Setup ToolR action now also require the attestation subject that names the archive to carry the archive's SHA-256. With `TOOLR_REPO` (`install.sh`) or `-Repo` (`install.ps1`) set, both the downloads and the attestation identities come from that repository, and a rejection names it.

Editing the `[tool.toolr.argparse.*]` blocks in `tools/pyproject.toml`, or any file that an external command source scans, now refreshes the manifest on the next run. toolr used to keep showing the old commands and arguments until `toolr project manifest rebuild`. Other `tools/pyproject.toml` edits, such as a new dependency, don't trigger a rebuild, and tab completion keeps offering the cached commands while that file or a `scan_paths` glob is broken.

Tab completion and the start-up freshness check no longer reread every file under `tools/`, and every file an external command source scans, each time. toolr now keeps each file's size and timestamps in the manifest and rereads only the files that changed, so completion stays fast with large `tools/` trees and large `scan_paths`. Files outside the project, or on another filesystem than `tools/`, are still reread every time. On NFS, an edit made from another machine can take up to the attribute-cache timeout (typically under a minute) to be noticed.

A child killed by a signal is now reported consistently. `ctx.run(...).returncode` is `-N` for a child killed by signal `N`, as with `subprocess` (it used to be `-1` for every signal). On Unix, `ctx.exit()` turns a negative status into `128 + N`, as a shell does, so `ctx.exit(result.returncode)` after a SIGTERM'd child exits 143 rather than 255. `toolr project venv run` and `toolr project venv shell` now exit `128 + N` for a signal-killed child instead of 1. An exit code that doesn't fit in a byte (Windows crash statuses, or 256) now makes toolr exit 1; it used to be truncated, and could turn a failure into exit 0.

`toolr self cache prune` now also removes leftover manifest rebuild locks under the cache's `locks/` dir that no cached project still uses.

`ctx.run(..., capture_output=True)` keeps capturing when whatever reads its streamed copy of the output goes away, for example when toolr's output is piped into `head`. Both copies used to stop together, so the command was cut off and `result.stdout` came back truncated without a warning. A failure to write the captured output itself now raises `CommandError` instead of returning partial output.

A streamed copy that fails for any other reason, such as a full disk behind `toolr cmd > file`, raises `CommandError` too.

The uv auto-install now checks the downloaded archive against the SHA-256 checksum published with the same uv release, and refuses to install it if the checksum is missing or doesn't match.

Path-typed arguments (`pathlib.Path` and the `toolr.types` path types) now tab-complete files, or directories for `DirectoryPath`, `WritableDirectoryPath` and `NewPath`, in bash, zsh and fish. So do `toolr project trust`, `untrust` and `self build-manifest --source-dir` / `--output`. Re-run `toolr self completion install <shell> --force` to pick it up.

`@toolr.command(...)`, `@t.command(...)` after `import toolr as t`, and an alias from `from toolr import command as cmd` now register commands; `toolr` used to skip them while `CommandsTester` accepted them. A mistyped group equally close to several declared groups now always suggests the alphabetically first one.

`command_group()` now raises for contradictory arguments (such as passing both `docstring` and `description`) even when the group already exists, instead of silently returning the existing group. It no longer raises at runtime for a missing description: a title-only call, `command_group("ci", "CI")`, returns the group if it exists and creates it otherwise, and the manifest build checks that some declaration describes the group instead.

When several calls declare the same group, a later call fills a title, description or long description the group still has empty, so `toolr <group> --help` and `CommandsTester` show the description whichever module gives it. A conflicting re-declaration, with a different non-empty title, description or long description, keeps the first group: Python logs it at DEBUG level, and toolr warns when it rebuilds the manifest or `toolr self build-manifest` builds a plugin, naming the module each kept value came from and the module it ignores.

The manifest build now reads a group's `title=` passed by keyword and `parent=` passed by position, as `command_group` does. Before, it ignored a keyword title, so `toolr --help` showed no title for `command_group("ci", title="CI", description="…")`. `toolr --help` shows a group's title and description only when they are string literals or the module docstring through `__doc__`; any other expression, such as a constant, counts as given but shows empty.

Bash tab completion on a path option keeps a typed leading `$NAME/` or `${NAME}/` as typed, as native bash does: `$HOME/xd<TAB>` now completes to `$HOME/xdir/` instead of inserting a literal `\$HOME/xdir` and dropping the trailing `/`.

Bash path completion offers nothing for any other word containing `$` or a backtick, such as `${VAR:-$(cmd)}/`, `$_/` or a typed `\$`. Bash 5.3 expands such a word while completing it and could run a command substitution on Tab.

`toolr self completion install bash` now prints `source <installed path>` to add to your bash startup file when it can't find the bash-completion package, which is the only thing that loads the install directory. Without the package, the script used to be installed but never loaded. If bash-completion is installed but not enabled in your startup file, toolr can't tell: enable it, or add the `source` line yourself. Interactive shells read `~/.bashrc`; login shells, such as the ones macOS Terminal opens, read `~/.bash_profile`. `toolr --quiet` drops the hint.

`--opt=value` now tab-completes the value, choices or file paths, in bash, zsh and fish, for both your commands and built-ins such as `toolr self build-manifest --output=`. Re-run `toolr self completion install <shell> --force` to pick it up.

Command groups declared through an aliased `command_group` (such as `from toolr import command_group as cg`) or with an annotated assignment (`group: CommandGroup = command_group(...)`) now appear in the manifest with their commands. Before, toolr dropped them without an error. An annotated alias of `command` (`cmd: Any = command`) is now followed too.

A bare annotation (`cg: object`) no longer hides an imported `command` or `command_group`, and a self-copy (`cg = cg`) now resolves to the import.

An empty `XDG_DATA_HOME`, `XDG_STATE_HOME` or `XDG_CACHE_HOME` now falls back to the platform default, as the XDG spec says, instead of resolving to a `toolr` directory under the current directory. An empty `HOME` now makes `toolr self completion install` fail with "$HOME is not set or empty".

Type checkers now accept `ctx.run(..., stream_output=None)` and treat `with ctx.chdir(...)` as a context manager. The type stub for `Context` had fallen behind the code, and it now declares `ctx.run`'s `interactive` parameter.

Bash tab completion now completes paths typed with a backslash escape or a quote, such as `files/it\'s` or `"files/my f`, and inside an open quote inserts the name quoted for that quote instead of with stray backslashes (tested on bash 5.1 to 5.3). An open double quote before a leading `$NAME/` now completes too. A `!` completed inside double quotes no longer triggers history expansion, and dotfiles are listed whatever `GLOBIGNORE` is set to.

Tab completion no longer offers flags, flag values or option names after a bare `--`: everything after it is a positional, as the CLI reads it. Choices starting with `-` are offered only where the CLI accepts them: after `--opt=`, or as a positional after `--`.

`toolr --quiet self completion install` now prints nothing on success: the `wrote`, `already up to date` and zsh `fpath` lines are suppressed along with the bash source hint. Refusals and errors still print.

An empty `NO_COLOR` no longer disables colour in help output, and a relative `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` or `XDG_CONFIG_HOME` is now ignored, as the XDG spec says, instead of resolving against the current directory.

Anything toolr wrote under a relative location (venvs, the managed uv, trust records) stays where it was: toolr rebuilds or re-downloads it under the default directory, asks for trust again, and the old directories can be deleted.

`toolr --quiet` (and `-q`) now silences the success lines of `self build-manifest`, `self trusted --prune`, `project manifest rebuild`, `project init` and `project venv sync|lock|add|remove`, and passes `--quiet` on to the uv runs of `project init` and the `venv` commands. Errors, refusals and warnings still print, and the `self trusted` listing is unchanged.
`project venv shell` and `project venv run` also quiet their uv auto-sync. Unlike `project venv sync --quiet`, the root `-q` still exits non-zero with a message when there is no toolr project or uv needs install consent.

`toolr --quiet` (and `-q`) now silences the success output of `self cache prune`: the per-entry lines, the totals, the rebuild-lock counts and the `aborted` line. `--dry-run` still prints what would go, its per-entry lines and its totals. Warnings, refusals and the confirmation prompt are unchanged, except that prune's warnings now go to stderr instead of stdout.

Tab completion now offers groups, commands and further root flags after a root flag (`toolr --quiet <TAB>`, `toolr --quiet --<TAB>`), and no longer offers a root flag that was already given or conflicts with one.

A line clap would reject (a repeated or conflicting root flag, a bad `--timeout-secs` value, `--version`) now completes to nothing.

zsh and fish completion now match choice values typed with a backslash or inside quotes, such as `--mode dry\ r`, `'dry r` or `"dry r`. In zsh this also covers `$'…'`, quotes opened mid-word (`--mode='dry r`) and quoted earlier words such as `'--mode'`.

Bash completion handles paths whose names hold a newline or other control character. A word in `$'…'` quoting, open or closed and also after `~/` or `$NAME/`, now completes instead of offering nothing, and each control character in a reply is quoted as its own `$'…'` run, so a Tab that inserts the common prefix no longer leaves a quote open. Under an open quote, a name equal to the typed text no longer adds an empty candidate, and a reply ending in the quote character still closes it. A `~` word with a quoted part before the first `/`, such as `~"root"/`, is now taken literally, as bash does, rather than listed as a home directory.

This also fixes a case where Tab on a file name starting with `!`, after an open `"` with `set -H`, could run code. Pressing `M-*` (`insert-completions`) inside an open quote no longer inserts names unquoted, where a name could also run code. Names holding a quote could also run code or reach the command as the wrong argument on Tab; that is fixed too.

Bash completion now offers nothing for a `$'…'` word holding `\'` or `\\'`, which readline misreads.

Fix zsh completion installed to `~/.zfunc/_toolr` doing nothing on the first Tab in a new shell; the script now completes when autoloaded as well as when sourced. Run `toolr self completion install zsh --force` to replace an existing install.

zsh and fish completion now leave matching the typed word to the shell, so zsh's `matcher-list` style (case-insensitive, partial-word or substring matching) and fish's case-insensitive and fuzzy matching work on toolr groups, commands, flags and choices: `toolr ci lint --mode FA<Tab>` can complete `fast`. Rerun `toolr self completion install zsh` (or `fish`) to pick up the new script. bash completion is unchanged.

toolr now completes in PowerShell (PowerShell 7 or newer; Windows PowerShell 5.1 prints a warning and registers nothing): run `toolr self completion install powershell` and add the printed line to your profile. Installing completion no longer needs `HOME` to be set.

The docs now serve llms.txt and llms-full.txt from the Read the Docs root, so agents without the toolr skills get a curated Markdown index of the docs. The GitHub Pages copy of the docs is no longer published; use toolr.readthedocs.io.

toolr --help now points at the docs and at llms.txt.

zsh completion now lists each command, flag and value next to its description. Before, a short list showed only the descriptions.

`toolr project init` now prints `toolr: trusted <repo>` when it records trust in the repository, as `toolr project trust` does. `--quiet` keeps it silent.

Outside a project, an unknown command now ends with the line `toolr: no toolr project found above <directory>`, which says to run `toolr project init`, so a wrong working directory no longer looks like a typo.

On Windows, `ctx.repo_root` and the command's working directory are now plain paths (`C:\repo`) rather than the verbatim `\\?\C:\repo` form whenever the plain form is valid (long and reserved paths keep the prefix), so `git -C` and relative-path code behave.

toolr now starts the command runner with `PYTHONIOENCODING=utf-8` on every OS, so stdin, stdout and stderr use UTF-8 and a command that prints non-ASCII text to a pipe no longer raises `UnicodeEncodeError` on Windows with Python 3.11 to 3.14. It affects stdio only, not the default encoding of `open()`, and child Python processes inherit it. A `PYTHONIOENCODING` you set yourself is kept; an empty value counts as unset.

Control characters in repository paths now show escaped in `toolr self cache list`, project-discovery errors, hashing and freshness errors, and the interpreter-not-found and runner-spawn messages.

A command group named `init` in your `tools/` directory now works on a fresh clone. Previously `toolr init …` failed with "unrecognized subcommand" until another command had built the manifest.

The hidden `toolr project deps` migration shim, which only printed a hint about the 0.22 rename to `project venv`, is gone. `toolr project deps …` now fails as an unknown subcommand.

`toolr --help` now lists `-V, --version`.

Ordinary commands no longer pause to measure the size of every cached virtualenv, which added a
quarter of a second to each run on a large cache. `--version` and `--help` are instant again. The
passive prune hint now appears only when more than ten cache entries belong to repositories that no
longer exist, and no longer quotes a size. `toolr self cache list` still shows sizes.

The Python runner also starts faster: `rich.prompt`, `packaging` and `importlib.metadata` now load
only when a command uses them.

`ctx.prompt` run without a terminal, such as in CI or with standard input closed, now exits with
status 1 and says that no input is available, instead of printing a Python traceback.

Option help now shows the value placeholder (`--name <name>`), and multi-value arguments such as `toolr project venv add` show `<PACKAGE>...`. Several documentation pages that had drifted from the behaviour are corrected, including the plugin dependency advice (`toolr-py`, not `toolr`). The release workflow now refuses to run from any branch but `main`.

`TOOLR_AUTO_SYNC=1` is the new name for answering the sync and uv-install prompts with yes, since the variable also covers the `Sync it now?` prompt after `toolr project trust`. `TOOLR_AUTO_INSTALL_UV` still works as an alias, removed in 1.0; switch to `TOOLR_AUTO_SYNC`. Once `TOOLR_AUTO_SYNC` is set, it decides alone (empty or unparsable means off); the alias counts only while it is unset.

`bool` option values read from text now accept `1`, `true`, `yes`, `0`, `false` and `no` (any case, trimmed): a positional, a list or tuple element, `--opt=VALUE` and an `arg(env=)` variable, where only `true` and `false` were accepted. `--help` no longer lists `[possible values: true, false]` for those options.

The venv cache key no longer depends on the toolr major version, so the 1.0 upgrade keeps every existing venv instead of orphaning them all. Keys are unchanged for the 0.x releases, so nothing is orphaned by this change either.

The hidden `toolr __build-static-manifest` command is gone. It wrote the manifest without the venv's plugin commands and without checking repository trust. Use `toolr project manifest rebuild` instead.

A `tools/` module that fails to build now reports the same error whether or not the tools venv exists. A parse error loses its `static build error:` prefix and repeated file name, the fallback warning's `cause:` line names the file directly, and errors such as a bad argparse `scan_paths` glob no longer repeat their message.

A broken plugin fragment in a tools venv that has no `pyvenv.cfg` now stops the first run on a freshly cloned trusted repository with the error. Before, toolr skipped such a venv when it first built the manifest, then warned and carried on without the plugin.

A command with its own `--version` option no longer skips the manifest build, so `toolr rel bump --version 1.2` works on a fresh clone instead of failing with `unrecognized subcommand`; only `--version` before the command is toolr's. Help for a built-in after a root flag (`toolr -q self --help`, `toolr --timeout-secs 5 project venv -h`) now shows that command's help instead of the root help.

A user command group named like one of toolr's hidden built-in commands (`__complete`, `__install-uv-now`) is now ignored with a warning, as `self` and `project` already were, instead of making toolr panic. Tab completion no longer offers those ignored groups or their commands.

There is no 0.36.0. Its release failed partway through, before anything reached PyPI, so it was withdrawn from GitHub too. 0.37.0 is what 0.36.0 would have been, plus fixes to the release pipeline.

### <!-- 0 -->🚀 Features

- *(parser)* [**breaking**] Boolean parameters must be flags defaulting to False ([`51d6afa`](https://github.com/s0undt3ch/ToolR/commit/51d6afad68ddba344c2f1e090acb78b667001335))
- *(security)* Add repository trust store ([`8be5163`](https://github.com/s0undt3ch/ToolR/commit/8be516351ab84ef2336af3848eef90ad04965d58))
- *(security)* Require repository trust before running repo code ([`747a3b6`](https://github.com/s0undt3ch/ToolR/commit/747a3b678fbf03525d86fb32ed62f7b564b391a1))
- *(security)* [**breaking**] Start with no trusted repositories ([`e33d125`](https://github.com/s0undt3ch/ToolR/commit/e33d125591f3414509faff6c63a437d2ec52409e))
- *(testing)* Export ConsoleVerbosity from toolr.testing ([`cb5731d`](https://github.com/s0undt3ch/ToolR/commit/cb5731d1d5a4c60be9348cb4896ceccdc97eaf3a))
- *(trust)* [**breaking**] Sync the tools venv right after trusting a repository ([`f9393c1`](https://github.com/s0undt3ch/ToolR/commit/f9393c12ec2f0ebe80b9487e53e7bd2fa0eff239))
- *(cache)* Sweep rebuild locks no cached repo needs ([`41d671b`](https://github.com/s0undt3ch/ToolR/commit/41d671b294bf704a6e4634720f381d73273367d4))
- *(cache)* Prune leftover rebuild locks ([`445a418`](https://github.com/s0undt3ch/ToolR/commit/445a418cf496d5c1ee30f70c915d8a962106bd5d))
- *(types)* One path completion hint per supported type ([`4282aed`](https://github.com/s0undt3ch/ToolR/commit/4282aed8f0148b3f63e43baebc56715598de58a3))
- *(complete)* Report path slots from the completion engine ([`0ea109a`](https://github.com/s0undt3ch/ToolR/commit/0ea109a052152d1b934ada69dd54205f3205e298))
- *(cli)* Signal path slots from toolr __complete ([`edcd04c`](https://github.com/s0undt3ch/ToolR/commit/edcd04c8b3c4b03e260d0674e5bcc614de546e22))
- *(complete)* Native file completion for path arguments in bash, zsh and fish ([`455f219`](https://github.com/s0undt3ch/ToolR/commit/455f2192d49a1ae1338db213350a433e061a6628))
- *(complete)* No space after any directory candidate in bash 4+ ([`84e536f`](https://github.com/s0undt3ch/ToolR/commit/84e536fa2a9ddb4e6bbe29eb914e73bc69da82a2))
- *(parser)* Recognise @toolr.command and aliased command imports ([`3afbba0`](https://github.com/s0undt3ch/ToolR/commit/3afbba0ba8a920d550a355ea1b0d4248d9d76c1c))
- *(cli)* Hint to source the bash script when bash-completion is missing ([`9a1f609`](https://github.com/s0undt3ch/ToolR/commit/9a1f6098b1c2be398740014b4ed4933880351ebf))
- *(complete)* Complete the value of --opt=value ([`ccdc605`](https://github.com/s0undt3ch/ToolR/commit/ccdc6059831ca79a3010a2f57c18c9d41738f877))
- *(complete)* Complete --opt=value in bash, zsh and fish ([`ac3678c`](https://github.com/s0undt3ch/ToolR/commit/ac3678ccff8c60dbf746b4e137a0939f3457e39c))
- *(trust)* Resolve a project root to its git repository key ([`d69bac8`](https://github.com/s0undt3ch/ToolR/commit/d69bac81d598ebf251b54606b125b6269f316a75))
- *(trust)* Key records and proofs on the repository ([`abb1d30`](https://github.com/s0undt3ch/ToolR/commit/abb1d305ebccbea0c945d678f6df3318e13c0cc7))
- *(complete)* Let the shell match candidates when asked ([`7ca1de7`](https://github.com/s0undt3ch/ToolR/commit/7ca1de7bd7af2cf49aa8e74826c22ad69a19f15e))
- *(complete)* Zsh matcher-list applies to toolr candidates ([`a8d1948`](https://github.com/s0undt3ch/ToolR/commit/a8d1948b9940e948b61047d95332de314bcc2309))
- *(complete)* Fish fuzzy matching applies to toolr candidates ([`772864e`](https://github.com/s0undt3ch/ToolR/commit/772864e6d0ade66240a774fb7acb176a9419f2ce))
- *(completion)* Add the powershell shell value and install path ([`f320e09`](https://github.com/s0undt3ch/ToolR/commit/f320e09b8c76e6aaee59fe74e982485d78c3ecb0))
- *(completion)* Add the powershell completion script ([`e552743`](https://github.com/s0undt3ch/ToolR/commit/e55274358fe1114595fd5dfeee3e7c6d9138fadc))
- *(completion)* Require PowerShell 7 for completion ([`96e791e`](https://github.com/s0undt3ch/ToolR/commit/96e791edb1d66a457a93cc309a9cfe068335b16d))
- *(docs)* Generate llms.txt and llms-full.txt ([`19e1cf1`](https://github.com/s0undt3ch/ToolR/commit/19e1cf1751e63a3503d0bc61b9fa942eb69b9a65))
- *(docs)* Fail the build when llms.txt drifts from the nav ([`3178765`](https://github.com/s0undt3ch/ToolR/commit/3178765f949e909cfc3bdc017428834b42bbde92))
- *(cli)* Point agents at llms.txt from toolr --help ([`8833d7f`](https://github.com/s0undt3ch/ToolR/commit/8833d7ff511bc127f28cd2dca2197ba36b3d8ae9))
- *(docs)* Link each page's Markdown copy from its head ([`5748bb8`](https://github.com/s0undt3ch/ToolR/commit/5748bb80d9091f7c103c455ec5676f2e31b2df98))
- *(cli)* Hint that no project was found after an unknown command ([`edb57d9`](https://github.com/s0undt3ch/ToolR/commit/edb57d9717e7ea612ad29d16ac10d5e856364be3))
- *(cli)* [**breaking**] Root-only toolr flags with a placement hint, venv sync --if-project (audit WP6) ([`d1c405a`](https://github.com/s0undt3ch/ToolR/commit/d1c405a2973a9e51ce69a60ab529f120951e0d42))
- *(env)* [**breaking**] One boolean rule for TOOLR_* variables, TOOLR_AUTO_SYNC replaces TOOLR_AUTO_INSTALL_UV ([`40061f9`](https://github.com/s0undt3ch/ToolR/commit/40061f9b98ec2777c6056b71c9524210473f1ba2))
- *(install)* [**breaking**] One --verify spelling in the installers and the action ([`196d214`](https://github.com/s0undt3ch/ToolR/commit/196d214c5bde3f569c5262ca9aa17489d879e940))
- *(venv)* Drop the major version from the cache key and record a layout version ([`f5dfd06`](https://github.com/s0undt3ch/ToolR/commit/f5dfd06c6902d5086fb4710ed39d96cb44b579ad))
- *(py)* [**breaking**] Remove the deprecated arg() keywords ([`5452e05`](https://github.com/s0undt3ch/ToolR/commit/5452e05062469e08bf599ce4f56bce71324ff6d3))
- *(py)* [**breaking**] Remove the CommandGroup.command_group method form ([`35d1c4c`](https://github.com/s0undt3ch/ToolR/commit/35d1c4cd9736b6bbf53cb34d58f7d22aa1294d60))
- *(py)* [**breaking**] Trim toolr.__all__ and name the Context.run arguments ([`d84e96a`](https://github.com/s0undt3ch/ToolR/commit/d84e96a67e7086219a983cb75fab1e50e414bdca))

### <!-- 1 -->🐛 Bug Fixes

- *(runner)* Point schema-mismatch error at venv sync -P ([`170361d`](https://github.com/s0undt3ch/ToolR/commit/170361d39471a5957cf572966b79acf08e8019d7))
- *(runner)* Name the stale side in the schema-mismatch error ([`ca3cdb2`](https://github.com/s0undt3ch/ToolR/commit/ca3cdb273dc5192b4e86f038452c8a64e497619c))
- *(uv)* Treat EOF at the install prompt as refusal ([`14d3017`](https://github.com/s0undt3ch/ToolR/commit/14d30177cf462d05c1a52d620b96faf8e494a2a5))
- *(uv)* End the prompt line before the refusal error on EOF ([`f91da24`](https://github.com/s0undt3ch/ToolR/commit/f91da247a695f3f6e56cce4e98bcc0b8f28b0fbd))
- *(dispatch)* Resolve the PATH Python fallback to an absolute path ([`10fc53f`](https://github.com/s0undt3ch/ToolR/commit/10fc53fb44fd0662d0bf327a8466a65708ea29b7))
- *(dispatch)* Skip PATH interpreters older than Python 3.11 ([`1949afa`](https://github.com/s0undt3ch/ToolR/commit/1949afa1ae67a6a0cfe2eaa380b8047b5be59283))
- *(project)* Compare dependency names the way packaging does ([`fb450b3`](https://github.com/s0undt3ch/ToolR/commit/fb450b3f3bba7c7e3b42ed707b4413272dd2d70f))
- *(project)* Tighten PEP 503 matching wording and tests ([`9202660`](https://github.com/s0undt3ch/ToolR/commit/92026608d987a321c650e5a09f0102cc853b80ef))
- *(runner)* Check the spec schema version before the full decode ([`f8756c2`](https://github.com/s0undt3ch/ToolR/commit/f8756c26e6744fbb9ef38a0b60084972b341b8b0))
- *(runner)* Pin schema_version as a frozen contract ([`a624fad`](https://github.com/s0undt3ch/ToolR/commit/a624fad020fe034b5ad61b08b9331091abdc9e80))
- *(execute)* Keep MIN_PYTHON_MINOR in lockstep with requires-python ([`1a86ea3`](https://github.com/s0undt3ch/ToolR/commit/1a86ea38a7bbaa5f8bb7e67d42e461333fa61c67))
- *(cli)* Make --quiet never prompt to install uv ([`c79c810`](https://github.com/s0undt3ch/ToolR/commit/c79c810aa6c4f17e9ecbf0e74b8f3ce567b23b8e))
- *(cli)* Make root --quiet never prompt to install uv under project ([`38510a5`](https://github.com/s0undt3ch/ToolR/commit/38510a59829dabf26b85078f7f2e5f3c5d04377d))
- *(project)* Never scaffold a requires-python floor below what toolr supports ([`afbbf1e`](https://github.com/s0undt3ch/ToolR/commit/afbbf1e00c6ec134064d189c8d06776b82b9c9ac))
- *(uv)* Unpack the managed uv download in-process ([`b401080`](https://github.com/s0undt3ch/ToolR/commit/b401080c4decd10912f05cb663644db4a80e0d1b))
- *(deps)* Take zip 9.0.5 for GHSA-m665-v7pm-7grj ([`93de394`](https://github.com/s0undt3ch/ToolR/commit/93de3944e7a4a1310613960310d960e52b373c86))
- *(uv)* Keep the cause chain on archive extraction errors ([`99db125`](https://github.com/s0undt3ch/ToolR/commit/99db1250aba31062d1a4c3c84a8498086a354088))
- *(venv)* Rebuild a venv whose interpreter was uninstalled ([`329eac9`](https://github.com/s0undt3ch/ToolR/commit/329eac955b3185a7f481cf1c8bae9ef9ab7ac409))
- *(venv)* Detect a removed base Python through pyvenv.cfg ([`7578293`](https://github.com/s0undt3ch/ToolR/commit/757829394554f1dcce6c47ffe8f139e8cd4f90f4))
- *(venv)* Only treat a confirmed-missing pyvenv.cfg home as broken ([`bf7a01e`](https://github.com/s0undt3ch/ToolR/commit/bf7a01eb5ee2ea952239e637163b6c029083799d))
- *(parser)* Only suggest the no_<name> rewrite for a True default ([`7579d67`](https://github.com/s0undt3ch/ToolR/commit/7579d67cb3b3cdd5e44aa2b7201dbb5cd7b07fb5))
- *(security)* Enforce trust store location on every operation ([`dc692c3`](https://github.com/s0undt3ch/ToolR/commit/dc692c3b6184a460daf8852b88812b01183454cb))
- *(security)* Make trust migration one-shot ([`b04f6ef`](https://github.com/s0undt3ch/ToolR/commit/b04f6ef11957d8cea94aea5c2c2b6e83bb47eb8a))
- *(security)* Check trust after resolving the venv path ([`3d16565`](https://github.com/s0undt3ch/ToolR/commit/3d16565f6a0a9b70f5e8033ab78989eac777bfbe))
- *(security)* Never treat a trust refusal as a benign quiet exit ([`437122a`](https://github.com/s0undt3ch/ToolR/commit/437122ad9038f71a93b384e80b254da89a6f33eb))
- *(security)* Allow trust when the repository root is the home directory ([`509e425`](https://github.com/s0undt3ch/ToolR/commit/509e42566a69f1db5d628bb9213757ec5443afd5))
- *(security)* Keep trust migration one-shot across store resets ([`39ba577`](https://github.com/s0undt3ch/ToolR/commit/39ba5776f48ad3b5c45ca253bed9037c789cfe49))
- *(security)* Migrate trust only from finished syncs ([`c9087ec`](https://github.com/s0undt3ch/ToolR/commit/c9087ec808652ba378b2e3a4c61c39ca7f4ec824))
- *(security)* Never fail project init on a trust store problem ([`35035a9`](https://github.com/s0undt3ch/ToolR/commit/35035a9aa0e9cf6387d485653dfe77becbafada9))
- *(complete)* Quote bash candidates instead of expanding them ([`0acb992`](https://github.com/s0undt3ch/ToolR/commit/0acb99280107b9d16f10687fabc3ae22cbbed215))
- *(complete)* Strip control characters from completion and warnings ([`22307f5`](https://github.com/s0undt3ch/ToolR/commit/22307f577cf23f9017ef66434464b2bb93efe4c4))
- *(parser)* Reject control characters and whitespace in names ([`ee7969b`](https://github.com/s0undt3ch/ToolR/commit/ee7969b0b7b234434e4d0b061a6afd3ce56826cb))
- *(cli)* Keep control characters in manifests away from clap ([`20b0268`](https://github.com/s0undt3ch/ToolR/commit/20b0268984bcdd128ce69474f46694ef2b6cf3b1))
- *(parser)* Reject control characters in group refs and argument names ([`7129c36`](https://github.com/s0undt3ch/ToolR/commit/7129c364cb1dbc84f07c7db38c7acde94e3b913f))
- *(parser)* Reject bidi and zero-width characters in names ([`73dbd3c`](https://github.com/s0undt3ch/ToolR/commit/73dbd3cd5892ab910ffb5d8286b0facf8d5a0c5a))
- *(venv)* Treat pyproject.toml and .python-version edits as a stale venv ([`a791d70`](https://github.com/s0undt3ch/ToolR/commit/a791d7097e6fe623af88db3508e8003f238a0ff5))
- *(venv)* Treat tools/uv.toml edits as a stale venv too ([`8dbc5d2`](https://github.com/s0undt3ch/ToolR/commit/8dbc5d2c85c2b5343cbd166a521e9a43cff79b15))
- *(plugins)* Escape venv and project paths before globbing ([`301d8b0`](https://github.com/s0undt3ch/ToolR/commit/301d8b095e6f176b940e4a4aab432f2b53609188))
- *(plugins)* Keep the Windows path prefix verbatim when escaping ([`17400fd`](https://github.com/s0undt3ch/ToolR/commit/17400fd89798077fcf4683a5b828db2ea564f0c5))
- *(plugins)* Match the escaped venv path case-sensitively ([`61f8ada`](https://github.com/s0undt3ch/ToolR/commit/61f8adaef9abd95472b122e4815754e67f42317d))
- *(runner)* Resolve command type hints one parameter at a time ([`c3036b1`](https://github.com/s0undt3ch/ToolR/commit/c3036b15b40d65c392bbadd6cfed1887561bae23))
- *(dispatch)* Detect DispatchCommand when another annotation can't resolve ([`9f4cb10`](https://github.com/s0undt3ch/ToolR/commit/9f4cb10810634b65409f2d3c560d06f8d39c13a1))
- *(runner)* Read lazy 3.14+ annotations without evaluating them eagerly ([`8acbd36`](https://github.com/s0undt3ch/ToolR/commit/8acbd36ed2fc7db07a72620275a3afe3744af601))
- *(runner)* Rescue aliased TYPE_CHECKING guards and report failed rescue imports ([`4f46b71`](https://github.com/s0undt3ch/ToolR/commit/4f46b71a4e3866e8b0954b881ac85b7e141d279b))
- *(runner)* Rescue every sibling submodule imported under TYPE_CHECKING ([`a0d8bb1`](https://github.com/s0undt3ch/ToolR/commit/a0d8bb1db59b212861b8042381df578073f9a3b4))
- *(security)* Don't write the manifest into an untrusted repository ([`e09520d`](https://github.com/s0undt3ch/ToolR/commit/e09520dbc2115d7313b591709ddda169bd4667c3))
- *(command)* Write ctx.run stdin from its own thread ([`989d481`](https://github.com/s0undt3ch/ToolR/commit/989d481e457ec4894466bac3a8cc2cb668c54961))
- *(release)* Fail release prep when a prek hook fails ([`c012c4d`](https://github.com/s0undt3ch/ToolR/commit/c012c4ddf872ecfe1042cb9459a9e1d34cd108cc))
- *(release)* Regenerate skill references right after the version bump ([`80e170e`](https://github.com/s0undt3ch/ToolR/commit/80e170ef5e2694252e66b8f7478249c519748811))
- *(release)* Show what prek changed outside the release before failing ([`4268e68`](https://github.com/s0undt3ch/ToolR/commit/4268e68c492c96600fa771d60f2e26b86d808391))
- *(release)* Fail release prep on skill reference drift already on main ([`6852da9`](https://github.com/s0undt3ch/ToolR/commit/6852da9b04671551524f4efa58c6e4bba79f35e2))
- *(release)* Handle quoted paths and show the diff when prek fails ([`0d958d3`](https://github.com/s0undt3ch/ToolR/commit/0d958d3ef0e0a595611a7c60071483db6d7a6a98))
- *(action)* Resolve version latest against s0undt3ch/ToolR only ([`c5d4ecf`](https://github.com/s0undt3ch/ToolR/commit/c5d4ecfebd9ce96d2297882ff86fe729790f1c22))
- *(release)* Push the release commit and tag atomically ([`c5be22a`](https://github.com/s0undt3ch/ToolR/commit/c5be22aa114216314924e210c51f6435946f1fba))
- *(release)* Refuse a release patch applied off the dispatched commit ([`0703360`](https://github.com/s0undt3ch/ToolR/commit/07033609c6c365ced52e62231c6df1663059d9f1))
- *(release)* Tell a re-run its release is already published ([`955e211`](https://github.com/s0undt3ch/ToolR/commit/955e2118faae0581e3328f5f9b1201abb9995bbd))
- *(release)* Diagnose a failed release push from origin's state ([`3f56801`](https://github.com/s0undt3ch/ToolR/commit/3f568016519734e0e375fc0d1322cc74f7148bad))
- *(action)* Key the tools venv cache on tools/ files only ([`87d6f8e`](https://github.com/s0undt3ch/ToolR/commit/87d6f8e95f1420def6dc2fb3215f8670bcff34d9))
- *(action)* Key the tools venv cache on every tools/ freshness input ([`565a3fe`](https://github.com/s0undt3ch/ToolR/commit/565a3fe5bfbdbec6f651e8fb0187e11332a8c915))
- *(discovery)* [**breaking**] Require tools/pyproject.toml to find the project root ([`e560456`](https://github.com/s0undt3ch/ToolR/commit/e560456e5bd162dd3cac84830bbafa9429c35548))
- *(testing)* Make CommandsTester.discover() raise on tools import errors ([`5d4ff7b`](https://github.com/s0undt3ch/ToolR/commit/5d4ff7b7a8bba3dda2e6735b10135b80aa95fe2d))
- *(testing)* Keep the stdlib on sys.path during CommandsTester discovery ([`19fcd7e`](https://github.com/s0undt3ch/ToolR/commit/19fcd7e5c2b067fc2272f59212bd6a1523e7bd73))
- *(testing)* Report failing tools subpackages instead of aborting discovery ([`c5e492a`](https://github.com/s0undt3ch/ToolR/commit/c5e492aab53e5a40f395ce04c00affd87aa1ba25))
- *(testing)* Walk tools subdirectories without __init__.py ([`fbf2474`](https://github.com/s0undt3ch/ToolR/commit/fbf2474be3e4647f583dd70aa7f89fa11eafc2a2))
- *(testing)* Match the tools name exactly when isolating sys.path ([`7e979fe`](https://github.com/s0undt3ch/ToolR/commit/7e979fe786284ee4a6e5ab4378d3485e2bfb5ecf))
- *(cli)* Resolve relative path defaults from the repo root ([`348804d`](https://github.com/s0undt3ch/ToolR/commit/348804d369a25ed675f54fee162ae43f2180a70a))
- *(cli)* Resolve an env-var path value from the current directory ([`09210fe`](https://github.com/s0undt3ch/ToolR/commit/09210fee6931d5b0da7d33ec8d008f7797a03b64))
- *(cli)* Report a bad path value before the trust prompt ([`977320e`](https://github.com/s0undt3ch/ToolR/commit/977320e37394f7a1c18131e9102a9136bb36effd))
- *(cli)* Render a bad deferred path value as clap does ([`3c79c7a`](https://github.com/s0undt3ch/ToolR/commit/3c79c7a5680efa636bbdc2123219b325c76376d3))
- *(runner)* Stop forwarding SIGINT the terminal already delivered ([`d309181`](https://github.com/s0undt3ch/ToolR/commit/d30918164bc5c7a7e8dc6caee44d8d2f394935ba))
- *(runner)* Forward a process-sent SIGINT on Linux ([`a12389f`](https://github.com/s0undt3ch/ToolR/commit/a12389f4dfddd462920ea7aa1ad524fec8c2cbd4))
- *(uv)* Skip relative PATH entries when looking up uv and python3 ([`e6babab`](https://github.com/s0undt3ch/ToolR/commit/e6babab37c9b1d57ff0b1e0fbd16e03ed384b964))
- *(uv)* Require an exec bit when searching PATH, as execvp does ([`64e700e`](https://github.com/s0undt3ch/ToolR/commit/64e700ea16f523d3064e0b0910a71acd3043ed28))
- *(uv)* Only run an ldd from an absolute PATH directory ([`8c6e30a`](https://github.com/s0undt3ch/ToolR/commit/8c6e30a6df2dc5ca3932fae60d034b7eb56ec309))
- *(uv)* Judge PATH executables with access(X_OK), as execvp does ([`0beca2d`](https://github.com/s0undt3ch/ToolR/commit/0beca2df4ef83fc438e8acf89e406a2092a4408e))
- *(testing)* Record standalone @command(group=...) commands in the registry ([`5464414`](https://github.com/s0undt3ch/ToolR/commit/546441423ae6da10c34fcaa0cc703bc526590910))
- *(testing)* Fail CommandsTester reads on unknown group references ([`1b975b1`](https://github.com/s0undt3ch/ToolR/commit/1b975b1cd3fa6ea37e0d3b47a1f8775cb4437ed2))
- *(testing)* Word unknown-group errors exactly as the manifest build does ([`8b167d9`](https://github.com/s0undt3ch/ToolR/commit/8b167d980ae3e6b56948b87ed29d159926456f4c))
- *(decorators)* Keep pending standalone commands only inside CommandsTester ([`2d953da`](https://github.com/s0undt3ch/ToolR/commit/2d953da25589150026c583232a24b5df21d50601))
- *(testing)* Report standalone @command without group= under CommandsTester ([`9189886`](https://github.com/s0undt3ch/ToolR/commit/9189886c753c8f21428b6002821a5bec92a26467))
- *(testing)* Order unknown-group errors by file path, as the manifest build does ([`4fe8c23`](https://github.com/s0undt3ch/ToolR/commit/4fe8c232f62ffc0de5c72f2dc2f04c4850e5dadb))
- *(testing)* Escape invisible characters in unknown-group errors ([`76a25b9`](https://github.com/s0undt3ch/ToolR/commit/76a25b99851337d09e5f74f4817755652c0dbe73))
- *(parser)* [**breaking**] Reject symlinked modules under tools/ ([`91c0b1c`](https://github.com/s0undt3ch/ToolR/commit/91c0b1cd3f5844cd8af878749fe29338247daf0e))
- *(dispatch)* Fail on symlinked modules instead of using the stale cache ([`dc38396`](https://github.com/s0undt3ch/ToolR/commit/dc3839637d65984a812322b2911d0e67e5b2ca92))
- *(parser)* Ignore .py-named directory links and name relative roots ([`b577e56`](https://github.com/s0undt3ch/ToolR/commit/b577e5622396e090d7868d1c432f729490fa8d02))
- *(parser)* Point at Python packages in the symlinked-modules hint ([`10f6f86`](https://github.com/s0undt3ch/ToolR/commit/10f6f863dd5ba142f7f11431b9610e27789da5f0))
- *(testing)* Report symlinked modules discovery can't resolve ([`e68b673`](https://github.com/s0undt3ch/ToolR/commit/e68b673d50cd599f242070a201a4fe9c60cddbde))
- *(dispatch)* Print symlinked-module errors the same on every path ([`138a370`](https://github.com/s0undt3ch/ToolR/commit/138a3706a79a86cdd9ebd21a92b139e086c75e76))
- *(cli)* Skip root option values when finding the subcommand ([`8375cdf`](https://github.com/s0undt3ch/ToolR/commit/8375cdfd89513215aba5495c18cd127a08773bd8))
- *(runner)* Show the traceback when a command module fails to import ([`9fa2ca4`](https://github.com/s0undt3ch/ToolR/commit/9fa2ca48d24ec5685340d2e06e0f28ce7c549ecc))
- *(runner)* Keep importlib frames out of command import tracebacks ([`59a1de6`](https://github.com/s0undt3ch/ToolR/commit/59a1de69dc5688cd38c4ff15c8b3f366ef467100))
- *(runner)* Fall back to import_module when an import hook skips sys.modules ([`1df3e07`](https://github.com/s0undt3ch/ToolR/commit/1df3e071db266672f0948d9cce2ad877f933c54b))
- *(dispatch)* Keep an in-tree venv's meta.json out of tools/ ([`fc8eca6`](https://github.com/s0undt3ch/ToolR/commit/fc8eca67addd98692c9187628bbc6b56dabf9bb2))
- *(dispatch)* Give an in-tree venv's meta.json its own cache slot ([`bfd8cd1`](https://github.com/s0undt3ch/ToolR/commit/bfd8cd11becb91d588202f15ef29f588c45df38e))
- *(dispatch)* Keep the meta.json touch warning out of quiet and in-tree runs ([`6e1e7b3`](https://github.com/s0undt3ch/ToolR/commit/6e1e7b38ce7b39f11723cb6e7ea768291b8a664f))
- *(action)* Only accept archives attested by the release workflow ([`f2af388`](https://github.com/s0undt3ch/ToolR/commit/f2af3881610f7bad2758cda433022490a9ca7cfd))
- *(action)* Pin the attestation check to the release workflow on main ([`2164088`](https://github.com/s0undt3ch/ToolR/commit/2164088e40b08540514a798ebeeb0587ec1f12c2))
- *(action)* Bind the attestation to the archive and require gh 2.56 ([`8803f51`](https://github.com/s0undt3ch/ToolR/commit/8803f51f1e7bdbd7565df151b4011fa086cb5d57))
- *(testing)* Only exempt the interpreter's site directories in CommandsTester ([`70d612e`](https://github.com/s0undt3ch/ToolR/commit/70d612e0cb2f3bfd64976cbe7cdaed3e317769e0))
- *(testing)* Resolve the fixture's tools without pruning sys.path ([`d52d97c`](https://github.com/s0undt3ch/ToolR/commit/d52d97c98af1665a60fbf04871af5f4202a88c58))
- *(testing)* Walk only the fixture's tools and always drop the finder ([`c072b08`](https://github.com/s0undt3ch/ToolR/commit/c072b0877862bd0b31be9f239373acb1ac60a9ed))
- *(release)* Support pre-release versions in version bump ([`6101a06`](https://github.com/s0undt3ch/ToolR/commit/6101a0677da254810767bcf6fcf85f9b07da82b3))
- *(release)* Roll pre-releases up into the final release ([`cc37004`](https://github.com/s0undt3ch/ToolR/commit/cc37004feed6e6b760921dc0b638bdd8bd89894a))
- *(release)* Skip release commits in git-cliff whatever their case ([`c9935eb`](https://github.com/s0undt3ch/ToolR/commit/c9935eb826645099a617ce6325517c18bbb57a0e))
- *(command)* [**breaking**] Kill the process tree on ctx.run timeouts ([`9b12637`](https://github.com/s0undt3ch/ToolR/commit/9b1263723d31051d7508d748789c215401d17564))
- *(command)* Give ctx.run reader threads their own output fds ([`cf8edc7`](https://github.com/s0undt3ch/ToolR/commit/cf8edc77ceec668729d0790026f3f67270b3f6d8))
- *(command)* Vet each edge of the ctx.run tree walk ([`746b699`](https://github.com/s0undt3ch/ToolR/commit/746b69933d85a8a048a0259fb0d793d5d9f0a1c5))
- *(command)* Tell slow sinks from pipe holders in ctx.run timeouts ([`296bc0b`](https://github.com/s0undt3ch/ToolR/commit/296bc0b04d33e9f1dd86ff8d24eebbd180ce4e95))
- *(trust)* Prove trust with a token and keep the sync prompt the only one ([`757a9e3`](https://github.com/s0undt3ch/ToolR/commit/757a9e354781609887f55a853882b8cbc5ec092b))
- *(build-manifest)* Find packages in Windows venvs ([`64e0a80`](https://github.com/s0undt3ch/ToolR/commit/64e0a80ae89c62033f86bbf6ef8727920ec54076))
- *(venv)* Report unreadable venv directories instead of skipping them ([`11a9653`](https://github.com/s0undt3ch/ToolR/commit/11a96531c681fe61ed43456fff7af1cae2f3d5c3))
- *(venv)* Prefer the venv's own Python when locating toolr-py ([`72a4167`](https://github.com/s0undt3ch/ToolR/commit/72a4167c6104d09dc4f9b56ef587848b5ff0b7f3))
- *(venv)* Don't fail on venv directories main never needed to read ([`88e61da`](https://github.com/s0undt3ch/ToolR/commit/88e61da3ed9559d706f8ae44153b9f689f4bf890))
- *(venv)* Report unreadable venv directories in toolr-py validation ([`097c9ae`](https://github.com/s0undt3ch/ToolR/commit/097c9ae2d223f708f95d1840d0b204d49d0c5297))
- *(manifest-build)* Say finding, not globbing, third-party manifests ([`7e99a58`](https://github.com/s0undt3ch/ToolR/commit/7e99a58d6e1f75a4575daca402957459b87e9d29))
- *(install)* Bind install.sh attestation check to the release workflow ([`4cba1a1`](https://github.com/s0undt3ch/ToolR/commit/4cba1a1a3876bfd0a0d536ee99ae3c1c9e7e3d6c))
- *(install)* Bind install.ps1 attestation check to the release workflow ([`adf901f`](https://github.com/s0undt3ch/ToolR/commit/adf901fb99ae213cc680ffb0055b826c9581f6cd))
- *(install)* Require one attested subject to carry the archive's name and digest ([`bd7bf45`](https://github.com/s0undt3ch/ToolR/commit/bd7bf4517646a1b4778bd1e80234d6b835db769a))
- *(install)* Fail closed when install.sh can't compute a SHA-256 ([`bc7e2b0`](https://github.com/s0undt3ch/ToolR/commit/bc7e2b0f0943e40d7cb639959b44bc6d8fcee372))
- *(manifest)* Hash argparse sources and tools/pyproject.toml into static_hash ([`441b505`](https://github.com/s0undt3ch/ToolR/commit/441b505ace745c2fcbc4ff1c9526938f1be82503))
- *(manifest)* Don't fail the static hash on an unreadable scanned file ([`a1731e1`](https://github.com/s0undt3ch/ToolR/commit/a1731e13610085ba41d7425df9e74e01c4c9816b))
- *(complete)* Serve the cached manifest when a TAB rebuild fails ([`03a4824`](https://github.com/s0undt3ch/ToolR/commit/03a482424ae4cb9efb6d77b9e34784f7bc287305))
- *(manifest)* Hash only the argparse blocks of tools/pyproject.toml ([`eac0ad0`](https://github.com/s0undt3ch/ToolR/commit/eac0ad0953b54ef1659035891065fee3cce63bd0))
- *(freshness)* Bound record reuse by the snapshot, not the manifest write ([`64dbbf1`](https://github.com/s0undt3ch/ToolR/commit/64dbbf1e72bc58dec9d21b773d94655686d12edd))
- *(freshness)* Only reuse records from tools/'s filesystem ([`657b182`](https://github.com/s0undt3ch/ToolR/commit/657b1823849c7da9e569534d65fed679e3434db5))
- *(test)* Decode git-cliff output as UTF-8 on Windows ([`31ec83c`](https://github.com/s0undt3ch/ToolR/commit/31ec83cefe95924f061a7b6085afb12ff486860a))
- *(venv)* Exit 128 + signal for a signal-killed child, 1 for out-of-range codes ([`9cc614f`](https://github.com/s0undt3ch/ToolR/commit/9cc614ff46ac593099de7824912c43e6992fac78))
- *(run)* Return -N from ctx.run for a child killed by signal N ([`df93879`](https://github.com/s0undt3ch/ToolR/commit/df938790e5d992f1e7c8dcad516b5f702b24d5bf))
- *(context)* Exit 128 + N for a negative ctx.exit status ([`ca24a52`](https://github.com/s0undt3ch/ToolR/commit/ca24a526aabcb07a9210c66c962500e75b495e29))
- *(context)* Keep a negative Windows exit status out of the 128 + N rule ([`807d7b8`](https://github.com/s0undt3ch/ToolR/commit/807d7b8086c9052f5ad2d79bca51614f7e9e5e39))
- *(context)* Map only int statuses in ctx.exit, and correct the old exit code in the note ([`7251c13`](https://github.com/s0undt3ch/ToolR/commit/7251c134c1222cc29e9484c398734cafcce9060d))
- *(cache)* Keep sweeping past a vanished lock, and lock read-only ([`08bb614`](https://github.com/s0undt3ch/ToolR/commit/08bb614de7ce414f71720c32ecae7fd20e394d5e))
- *(cache)* Report swept locks instead of "nothing to prune" ([`f33d1cc`](https://github.com/s0undt3ch/ToolR/commit/f33d1cc39fdc8e0b40aaa97674dee89ad5f0f6b1))
- *(command)* Keep ctx.run capture when the stream target fails ([`3f2447a`](https://github.com/s0undt3ch/ToolR/commit/3f2447ac1989022b175c275ee2373d7404613597))
- *(command)* Say an fd couldn't be duplicated, not that it's invalid ([`e5d6e59`](https://github.com/s0undt3ch/ToolR/commit/e5d6e59272363f918a547f055cebc30fc6692f68))
- *(uv)* Verify the auto-installed uv against its release checksum ([`e4433bb`](https://github.com/s0undt3ch/ToolR/commit/e4433bb68abd785883ee35660850fb127fcd2797))
- *(uv)* Cap uv download sizes and install atomically ([`139cca6`](https://github.com/s0undt3ch/ToolR/commit/139cca607b56f318b2b67e37a51a682310475d8f))
- *(uv)* Point download failures at a manual install, and name the path ([`6899501`](https://github.com/s0undt3ch/ToolR/commit/68995018a6a50d1cf1b7d8ff5ebb03f5a33b3133))
- *(venv)* Report a uv sync that can't start without the install-uv hint ([`1060a66`](https://github.com/s0undt3ch/ToolR/commit/1060a661b56972d0f7a723673935948c9a251562))
- *(complete)* Harden bash path completion ([`0806253`](https://github.com/s0undt3ch/ToolR/commit/0806253e7be4621ba94149250147eba8b71e57ca))
- *(complete)* Keep a no-match directory probe clear of set -e ([`ea43f86`](https://github.com/s0undt3ch/ToolR/commit/ea43f86cd5e5fd4e69c4885c0359380fcd54133c))
- *(complete)* Mark bash directories in linear time ([`71e490a`](https://github.com/s0undt3ch/ToolR/commit/71e490a0f150159f8ca2342b1bdf7790230569f9))
- *(complete)* Survive nounset with no path candidates ([`4011395`](https://github.com/s0undt3ch/ToolR/commit/4011395935082d49f999692df684665c73658157))
- *(parser)* Break group-suggestion ties by the smallest name ([`3a55846`](https://github.com/s0undt3ch/ToolR/commit/3a558461c8cf81fd7a8df79740c4917c415e1ed1))
- *(parser)* [**breaking**] Reject duplicate commands in a group ([`a98eda6`](https://github.com/s0undt3ch/ToolR/commit/a98eda6ee224d59b512a72816f55528a36760fcd))
- *(parser)* [**breaking**] Reject nested and non-literal command declarations ([`5e6d7f2`](https://github.com/s0undt3ch/ToolR/commit/5e6d7f2a92b4283c6cae612f77fb0a35f7f91678))
- *(ci)* Print tests-layout paths with forward slashes ([`ef8df02`](https://github.com/s0undt3ch/ToolR/commit/ef8df02bcc94e164be9f1718dc20ef331c21617c))
- *(parser)* Ignore other libraries' `command` and reject stacked decorators ([`daebcc0`](https://github.com/s0undt3ch/ToolR/commit/daebcc0f3eb82a689286fb691bf142f25ede8ddd))
- *(testing)* Read every tools module's source for unsupported commands ([`3e8f833`](https://github.com/s0undt3ch/ToolR/commit/3e8f833582e28f6c6540f518f9a80ee37ea2b11c))
- *(parser)* Follow a re-exported `command` back to toolr ([`4948b17`](https://github.com/s0undt3ch/ToolR/commit/4948b173d858d278713bad23d4a16321a3de9596))
- *(cli)* Suggest pytest tests/, not tools/, in venv run hints ([`37fa8bf`](https://github.com/s0undt3ch/ToolR/commit/37fa8bfc6c1e0bff474c1b5c2ae0601fc4d67208))
- *(parser)* Follow toolr's `command` through submodules, assignments and unread modules ([`3713ce1`](https://github.com/s0undt3ch/ToolR/commit/3713ce163908d34b7236d82aaff1db90159f4e52))
- *(testing)* Read tools sources as bytes for the shape check ([`ba80c4d`](https://github.com/s0undt3ch/ToolR/commit/ba80c4d350a3693ee343f540bbd9b9a0fb1ea7c5))
- *(decorators)* Validate command_group arguments before returning an existing group ([`e053ae1`](https://github.com/s0undt3ch/ToolR/commit/e053ae1c72e3587bff9b91e82e28eb4eda7f79cf))
- *(decorators)* Allow a title-only re-fetch of an existing command group ([`897f2e8`](https://github.com/s0undt3ch/ToolR/commit/897f2e8cf0115d81451b93036231dc9523261182))
- *(complete)* Bash completes words with colons, without bash-completion ([`e7cea51`](https://github.com/s0undt3ch/ToolR/commit/e7cea51aed47768168d736e473884de075ac7a0b))
- *(complete)* Keep a typed `@` or `$` in bash completion replies ([`11f0d6e`](https://github.com/s0undt3ch/ToolR/commit/11f0d6e30d004280df876dd5fd222b72112fc1fc))
- *(complete)* Warn about an old bash only in an interactive shell ([`35fc070`](https://github.com/s0undt3ch/ToolR/commit/35fc070d5a1bd784bee6e57d513564b56f93e408))
- *(decorators)* Make command_group lenient about a missing description at runtime ([`4b15ee6`](https://github.com/s0undt3ch/ToolR/commit/4b15ee6e474f23bc4e07679b575f4c94ca58051b))
- *(parser)* Warn when a group is re-declared with different fields ([`1868790`](https://github.com/s0undt3ch/ToolR/commit/186879095c8a714bbf5a557dbc3bf1ed7b690a72))
- *(parser)* Fail the build for undescribed or contradictory groups ([`504a87e`](https://github.com/s0undt3ch/ToolR/commit/504a87ef3443bbfcb9036cf92e01ab1f55589e99))
- *(testing)* Report undescribed and contradictory groups like the build ([`7a73518`](https://github.com/s0undt3ch/ToolR/commit/7a73518409b57d882c373f2fce81ad66ca0a2bce))
- *(decorators)* Let a later command_group fill an empty field ([`5121c1b`](https://github.com/s0undt3ch/ToolR/commit/5121c1b5a1c27a493ab2dbbd7c5d11af9372a880))
- *(parser)* Let a later group declaration fill an empty field ([`38381ef`](https://github.com/s0undt3ch/ToolR/commit/38381ef1e0b1f6d0e2fc340ca134030e986eaa88))
- *(testing)* Read tools sources as bytes for the group checks ([`b56df17`](https://github.com/s0undt3ch/ToolR/commit/b56df1795ef47caaced5d089cf00f0f37ab9c57c))
- *(parser)* Count an empty group description as none ([`92104d5`](https://github.com/s0undt3ch/ToolR/commit/92104d577b80f5a76f041819c2206a7531910246))
- *(parser)* Read group fields the way command_group binds them ([`ad68f58`](https://github.com/s0undt3ch/ToolR/commit/ad68f587600d02967319e54d449db90c5d98ad25))
- *(testing)* Read group constants and unreadable descriptions like the build ([`b4c3644`](https://github.com/s0undt3ch/ToolR/commit/b4c364428211303d5c0a4a26ca31e95bb39a0d8f))
- *(parser)* Read only literals and the module docstring in group fields ([`36f4cd3`](https://github.com/s0undt3ch/ToolR/commit/36f4cd3d0fc3f35d3c59079fa211e2350a4fbf4f))
- *(testing)* Read group fields like the build without constants ([`8445441`](https://github.com/s0undt3ch/ToolR/commit/8445441ce69fe3457e03efff2328b52a7d67efb0))
- *(cli)* Print every build warning of the run's first build ([`844c578`](https://github.com/s0undt3ch/ToolR/commit/844c578efaf9e0246d8a3dba4d46386cddce28ed))
- *(cli)* Print group warnings from toolr self build-manifest ([`f3e8eef`](https://github.com/s0undt3ch/ToolR/commit/f3e8eef6bae6a620a5894b1672959d70132799d9))
- *(complete)* Keep a typed leading `$NAME/` bare in bash path completion ([`dac6873`](https://github.com/s0undt3ch/ToolR/commit/dac6873686bf210d071dc7b87827062cdb16f8f8))
- *(complete)* Read a typed `$NAME/` from the user's variable, not a script local ([`eeef57d`](https://github.com/s0undt3ch/ToolR/commit/eeef57de8c2c9e87dcd8aa3a668214a879f18167))
- *(completion)* Don't treat $_ as a variable prefix in bash path completion ([`a676f20`](https://github.com/s0undt3ch/ToolR/commit/a676f20f890b06e05357210aa97e1bd268ed2da5))
- *(complete)* Never hand bash compgen a word holding `$` or a backtick ([`c781b0c`](https://github.com/s0undt3ch/ToolR/commit/c781b0c6f928ab3a5c26afbd3802b0a5eaacff33))
- *(cli)* Tighten the bash install source hint ([`de0ec9b`](https://github.com/s0undt3ch/ToolR/commit/de0ec9b22b45b79d46a969e499c79522ac59c4a1))
- *(cli)* Stop treating bash-completion 1.x as 2.x in the source hint ([`f7720be`](https://github.com/s0undt3ch/ToolR/commit/f7720be805262139720c488220df0203ced9a4f6))
- *(complete)* Complete --opt= paths on fish older than 3.7 ([`54c8c8d`](https://github.com/s0undt3ch/ToolR/commit/54c8c8d8fb9380f1aa4f906fcfbb9689ca7070ca))
- *(parser)* Read command_group aliases and annotated group bindings ([`24f7e28`](https://github.com/s0undt3ch/ToolR/commit/24f7e287234293578a441f55a7351d57e7f444e1))
- *(testing)* Read command_group aliases and annotated bindings in CommandsTester ([`aece5de`](https://github.com/s0undt3ch/ToolR/commit/aece5dec956940d40785a8d65ed57b4ddc7c437e))
- *(parser)* Follow annotated aliases of command and command_group ([`88049ab`](https://github.com/s0undt3ch/ToolR/commit/88049ab877299e8903e622fd9704d23d16836a3e))
- *(testing)* Resolve command_group aliases statically in CommandsTester ([`ad35e7c`](https://github.com/s0undt3ch/ToolR/commit/ad35e7c589ff07ec53fed6714fad5ff4e57f4c81))
- *(parser)* Bind nothing for a bare annotation, resolve self-copies and memoise alias resolution ([`187ec3f`](https://github.com/s0undt3ch/ToolR/commit/187ec3fedaa8d2643481088739a4e989b493f582))
- *(execute)* Reset inherited signal dispositions in the signal tests ([`1ebe1aa`](https://github.com/s0undt3ch/ToolR/commit/1ebe1aa9b90871c4c61f6ec6a538849459e9e014))
- *(tests)* Reset an inherited SIGINT ignore in the e2e signal tests ([`63da29b`](https://github.com/s0undt3ch/ToolR/commit/63da29b7c730b872c35b768bff054bf04415dfa0))
- *(toolr-py)* Declare status_to_exit_code in the _context stub ([`55a4fcf`](https://github.com/s0undt3ch/ToolR/commit/55a4fcfbb41874814d3b6e2f2a0cf8e52ceffef3))
- *(toolr-py)* Sync the _context stub with run and chdir ([`2f0d548`](https://github.com/s0undt3ch/ToolR/commit/2f0d548686596f8b4f08e2898704e5374d5ce283))
- *(tests)* Fail the stub check in CI without the extension, correct the note ([`527ad4c`](https://github.com/s0undt3ch/ToolR/commit/527ad4c8b5ffe35e8c34e4dff4dd4972ca6c7d20))
- *(env)* Treat empty XDG_* variables as unset ([`3c54bd5`](https://github.com/s0undt3ch/ToolR/commit/3c54bd5ee6dc4202075ab4859ae2057b7b96bef0))
- *(cli)* Say $HOME is unset or empty when completion install can't use it ([`6e3a35a`](https://github.com/s0undt3ch/ToolR/commit/6e3a35ad656b5438c9fcca6759eee1e3351d47ae))
- *(complete)* Complete quoted and escaped paths in bash ([`c52aa14`](https://github.com/s0undt3ch/ToolR/commit/c52aa14ba5e69542cfa75fefadee982f19606a21))
- *(complete)* Keep dotfiles, escape ! in double quotes, and two edge cases ([`6d4d5ec`](https://github.com/s0undt3ch/ToolR/commit/6d4d5ecf6ab354fbda65dfdb83e420c367b63075))
- *(complete)* Balance quotes for a !-ending directory, filter ~user newline splits ([`f5a9dbb`](https://github.com/s0undt3ch/ToolR/commit/f5a9dbba403e16232da5751b995f3125ef49345c))
- *(complete)* Treat everything after -- as positional and drop dash-leading choices where the CLI rejects them ([`cfba52c`](https://github.com/s0undt3ch/ToolR/commit/cfba52c54f12aff6bd52b781b1e949bc5e9de5a5))
- *(complete)* Treat a root -- as a terminator and other round-1 fixes ([`fdd8e65`](https://github.com/s0undt3ch/ToolR/commit/fdd8e65e4339f29cd09c72491d6a7ecc70c08e82))
- *(complete)* An empty --opt= before -- also completes nothing ([`d37c129`](https://github.com/s0undt3ch/ToolR/commit/d37c129dff16cdb6139fc5bb55be44f36133bac4))
- *(parser)* Reject command_group calls the static parser can't bind ([`0943d28`](https://github.com/s0undt3ch/ToolR/commit/0943d281759542877e15e137e3e650aa1f1d3f57))
- *(parser)* Only flag toolr's own command_group calls, skip bound statements whole ([`c6141f2`](https://github.com/s0undt3ch/ToolR/commit/c6141f256c3c404452c451250b5685f16128ad53))
- *(parser)* Source-order problems, flag every off-statement command_group, keyword names ([`3fb9514`](https://github.com/s0undt3ch/ToolR/commit/3fb95144f661cc1b1b41920a5d200206e2ec0487))
- *(completion)* Honour --quiet in self completion install ([`4db11c6`](https://github.com/s0undt3ch/ToolR/commit/4db11c63dab5603a5423115fdee5402562172c87))
- *(env)* Honour an empty NO_COLOR and ignore relative XDG dirs ([`30c7648`](https://github.com/s0undt3ch/ToolR/commit/30c764815d6be7bc7bc681542b0d5c482591b9c4))
- *(cli)* Honour the root --quiet in the other self and project built-ins ([`0819664`](https://github.com/s0undt3ch/ToolR/commit/0819664e78cb70fe0cbaad393d5afb80ede01146))
- *(cli)* Quiet uv auto-sync in venv shell and run under root -q ([`e124a8a`](https://github.com/s0undt3ch/ToolR/commit/e124a8a063a520bc9a568511b4680dac93a45458))
- *(trust)* Keep key kinds apart and narrow the repair hint ([`76f8a10`](https://github.com/s0undt3ch/ToolR/commit/76f8a104414c69a19d6d4f647d70edd76e5b45c7))
- *(trust)* Resolve the key once per decision ([`381fbac`](https://github.com/s0undt3ch/ToolR/commit/381fbac4a9c7e767bbcdf7e82b6596f103f302fe))
- *(trust)* Bound git file reads and name the worktree top in the repair hint ([`e8078ec`](https://github.com/s0undt3ch/ToolR/commit/e8078ec01148b5160cfc4a947421b94738850b61))
- *(cli)* Confirm project trust and untrust on stderr ([`11cace4`](https://github.com/s0undt3ch/ToolR/commit/11cace4f4c22a3bd93251a498995a5ece723c547))
- *(complete)* Complete ANSI-C quoted words in bash ([`f5cbc68`](https://github.com/s0undt3ch/ToolR/commit/f5cbc688c4aea202737654dc3de169c50d29e334))
- *(complete)* Escape a C1 control in bash replies byte by byte ([`be88bb4`](https://github.com/s0undt3ch/ToolR/commit/be88bb484bd924c0dfa4c8ac7117d9a213b223b0))
- *(complete)* Never let a quote inside $'…' desync readline ([`47311d1`](https://github.com/s0undt3ch/ToolR/commit/47311d12769b2a1649218b31a98187f3dc289c93))
- *(complete)* Double a leading quote readline would swallow; decline $'…' forms readline misreads ([`776b123`](https://github.com/s0undt3ch/ToolR/commit/776b123914d6cec27683a7b967b8161d78cde2fe))
- *(complete)* Offer nothing to insert-completions inside an open quote ([`b5f0cf8`](https://github.com/s0undt3ch/ToolR/commit/b5f0cf8b139db8d8774e2a3edbbc66f2126e3c16))
- *(complete)* Match escaped and quoted partials in zsh and fish ([`ae3599a`](https://github.com/s0undt3ch/ToolR/commit/ae3599ac87a4fd04eee4458a64ce8f5747080e76))
- *(complete)* Zsh mid-word quotes and fish one-word unescape ([`22c502b`](https://github.com/s0undt3ch/ToolR/commit/22c502b95fc2a842aa578a4e0ce25bd3edad397d))
- *(complete)* Fish keeps an empty unescaped word ([`9eda550`](https://github.com/s0undt3ch/ToolR/commit/9eda550378b6756abfad831b47c2bda8d129012e))
- *(complete)* Offer groups and root flags after a root flag ([`4e1c3bf`](https://github.com/s0undt3ch/ToolR/commit/4e1c3bfb7b1aa0f9f4a18d92d317f39e136b0524))
- *(complete)* Accept attached negative values, see --version, check value types ([`8cb24c3`](https://github.com/s0undt3ch/ToolR/commit/8cb24c3b48de80927530a2c55ff6317fafbd2822))
- *(cache)* Honour the root --quiet in self cache prune ([`ce14766`](https://github.com/s0undt3ch/ToolR/commit/ce14766bed3db348a6ebe8219f30af1869002f2e))
- *(cli)* [**breaking**] Keep warnings under the root --quiet ([`1470bbe`](https://github.com/s0undt3ch/ToolR/commit/1470bbe1b1942f6720eb79686eb9cc948ab750ab))
- *(cli)* [**breaking**] Python-side warnings survive -q, prune warns on stderr ([`4d95c27`](https://github.com/s0undt3ch/ToolR/commit/4d95c271b92560c636248250644b6ee2b03827d0))
- *(completion)* Complete on the first autoloaded zsh Tab ([`946261e`](https://github.com/s0undt3ch/ToolR/commit/946261e9654d999c652a73ada3d4a62c9762bbd5))
- *(complete)* Zsh shows descriptions when 20 or fewer candidates match ([`f694004`](https://github.com/s0undt3ch/ToolR/commit/f694004e46d079c628642df322e834d5c365f79b))
- *(complete)* Neutral zsh completion tag, and a live row that needs the dequote ([`b6332dd`](https://github.com/s0undt3ch/ToolR/commit/b6332dd0848f2f17e4acb7d8ee530924d3f78518))
- *(completion)* Give native commands the literal path in powershell ([`f945aac`](https://github.com/s0undt3ch/ToolR/commit/f945aac9097c7b4d8f28e9938a9901b8294ae60b))
- *(completion)* Expand a completed ~ path and pass powershell the parsed partial ([`2932ae7`](https://github.com/s0undt3ch/ToolR/commit/2932ae783e4c401fc82b4de9e49559f2a225ed52))
- *(completion)* Match an unclosed [ literally in powershell path partials ([`bad34cf`](https://github.com/s0undt3ch/ToolR/commit/bad34cfcbfc83d36db8a8fc619afe928f34f52aa))
- *(completion)* Match a literal backtick in powershell path partials ([`082bd2a`](https://github.com/s0undt3ch/ToolR/commit/082bd2a654e8d458dc6f8b0bb646731144ad79c7))
- *(docs)* Harden the llms.txt gate's link, copy and pattern checks ([`60d2d98`](https://github.com/s0undt3ch/ToolR/commit/60d2d984158e9fab466fd69812bc6e68bcd6312b))
- *(docs)* Skip the llms.txt gate on dirty builds and name a missing plugin ([`2d1607f`](https://github.com/s0undt3ch/ToolR/commit/2d1607f62bfcb8bf177e22d72902f59c7982eafb))
- *(docs)* Check each listed page's last line reaches llms-full.txt ([`5e28953`](https://github.com/s0undt3ch/ToolR/commit/5e28953ed50f1e5886a984c7a5e84771bdddf7e6))
- *(docs)* Check each page's whole copy reaches llms-full.txt ([`fd363cf`](https://github.com/s0undt3ch/ToolR/commit/fd363cf418fd6c1c7d829eca6534a8f35df4bdd5))
- *(docs)* Flag llms.txt entries that aren't listed nav pages ([`05208e0`](https://github.com/s0undt3ch/ToolR/commit/05208e0fcbd7ed40ad68e5d0681287ee768843e0))
- *(completion)* Show names next to descriptions in zsh listings ([`b4300cf`](https://github.com/s0undt3ch/ToolR/commit/b4300cf26a0eeb67608cf3388ff5a114d6768dee))
- *(completion)* Build the zsh listing only for the matched candidates ([`cb20d91`](https://github.com/s0undt3ch/ToolR/commit/cb20d91c8935d72d58710f3d1287ec94425b1c81))
- *(bootstrap)* Let a user command group be named init ([`48f9df1`](https://github.com/s0undt3ch/ToolR/commit/48f9df15a09a9baf0216ccc2d9a62878196dd9d6))
- *(cli)* List -V and --version in the root help ([`fc7ded4`](https://github.com/s0undt3ch/ToolR/commit/fc7ded431f9ca720550ffb1a27de93d0eec9b6e4))
- *(py)* Exit with a clear message when ctx.prompt has no input ([`dd75da9`](https://github.com/s0undt3ch/ToolR/commit/dd75da91e02707ea10b4bc58663f55f7fa9f6a63))
- *(py)* Print the ctx.prompt no-input message without mangling markup characters ([`196d58e`](https://github.com/s0undt3ch/ToolR/commit/196d58e9d0f4391c96715e16f1ebe20e76df65d5))
- *(help)* Show the value placeholder on options and an ellipsis on multi-value arguments ([`30a9d8d`](https://github.com/s0undt3ch/ToolR/commit/30a9d8dd4081dccb2fce23aae448b20b128a09f1))
- *(help)* Keep fixed-arity positionals plain and hyphenate option placeholders ([`5d301b4`](https://github.com/s0undt3ch/ToolR/commit/5d301b4809787dc53ce7359c59f211a1ad358358))
- *(project)* Say so when init records trust ([`96e7fe9`](https://github.com/s0undt3ch/ToolR/commit/96e7fe9a83b53df23fd7a4a05ca1ed8a56e3a965))
- *(security)* Escape control characters in repo-derived paths in messages ([`cf07270`](https://github.com/s0undt3ch/ToolR/commit/cf072703f1905d2e0ea8f1d028191bd6f5b70cb2))
- *(discovery)* Use a plain path for the project root on Windows ([`d94a91c`](https://github.com/s0undt3ch/ToolR/commit/d94a91c845854efa98170bc9d4a4496bc2e97756))
- *(execute)* Start the runner in UTF-8 mode ([`155b809`](https://github.com/s0undt3ch/ToolR/commit/155b8093b614a09db72b3916d5609bd2fb394ec0))
- *(cli)* Show the no-project hint only for top-level unknown commands ([`34af3a7`](https://github.com/s0undt3ch/ToolR/commit/34af3a7749e99e8118a96e41214fdd100fdd615e))
- *(security)* Escape control characters in hashing and freshness errors ([`6c2bdd1`](https://github.com/s0undt3ch/ToolR/commit/6c2bdd1233068289fd016a2bdcbf0808dfa05f4a))
- *(execute)* Set PYTHONIOENCODING instead of UTF-8 mode for the runner ([`90d6935`](https://github.com/s0undt3ch/ToolR/commit/90d693547c0835d249dc618dc97a943043920939))
- *(manifest)* Flatten every build error the same way, venv or not ([`b18a93b`](https://github.com/s0undt3ch/ToolR/commit/b18a93bf0c4c00da8f69c41d0112514931b28ccb))
- *(manifest)* Decide a fresh clone from the locked check, not a pre-lock look ([`4c771f3`](https://github.com/s0undt3ch/ToolR/commit/4c771f392b6902fb6edf2660e3164797701be375))
- *(precommit)* Read and write workflows as UTF-8 with LF endings ([`f41056b`](https://github.com/s0undt3ch/ToolR/commit/f41056b2930e9719de1af51c0098e6237b4b616b))
- Validate the action's verify input and drop the unreachable old-layout check ([`9910a75`](https://github.com/s0undt3ch/ToolR/commit/9910a75a927ea382ab76bc57c626b6db389db4c8))
- One meaning for verify, checksum tests for install.sh, TOOLR_AUTO_SYNC decides when set ([`7f9a49b`](https://github.com/s0undt3ch/ToolR/commit/7f9a49bc8c1cc80371a400c16459d9f96328f814))
- *(cli)* Read toolr's argv once, through one RootArgv ([`5dae00f`](https://github.com/s0undt3ch/ToolR/commit/5dae00fb7fa9c235b467353832fdc105ef138d11))
- *(cli)* Reserve every built-in command name for user groups ([`d548685`](https://github.com/s0undt3ch/ToolR/commit/d5486851a667b6c4423830012a1558d3abf0b92c))
- *(cli)* Keep --version off the tree walk and stop root parsing at -- ([`773a343`](https://github.com/s0undt3ch/ToolR/commit/773a343377981329dfdebcfe9348c82e353f7c80))
- *(complete)* Do not offer user groups reserved by built-ins ([`a3e4873`](https://github.com/s0undt3ch/ToolR/commit/a3e4873953e892b6635254539f1eb3a306e91a6c))
- *(trust)* Show only built-ins in an untrusted repository ([`a9357c9`](https://github.com/s0undt3ch/ToolR/commit/a9357c9002f75979ac29998d2d709e4950b91d8c))
- *(trust)* Ask for trust before reading an untrusted repository ([`e173339`](https://github.com/s0undt3ch/ToolR/commit/e1733395915651758f2e4751dc9e53347f53cd5b))
- *(venv)* Use an in-tree venv only if toolr built it ([`3705898`](https://github.com/s0undt3ch/ToolR/commit/370589888ecf21c1df7e3d63a51a6f225da2e885))
- *(venv)* Lock in-tree venv syncs and never read a foreign venv ([`66fa4df`](https://github.com/s0undt3ch/ToolR/commit/66fa4df972421048877e8bdeae919588d37df9f9))
- *(trust)* Ask for trust before reading tools/pyproject.toml ([`3184066`](https://github.com/s0undt3ch/ToolR/commit/3184066b320a3736882deba8779ef199ad694a84))
- *(cli)* Read `-h` in a root-flag cluster as help ([`21a1a89`](https://github.com/s0undt3ch/ToolR/commit/21a1a897e75e0042b056a535e5e28d1a523d2029))
- *(venv)* Bound the sync marker read and fail on an unlockable store ([`ea05b8a`](https://github.com/s0undt3ch/ToolR/commit/ea05b8ad96fc2d65a1ac0315188196f41d13c4b1))
- *(venv)* Refuse a marker link at open and flush the lock wait line ([`ceb1ceb`](https://github.com/s0undt3ch/ToolR/commit/ceb1ceb8ee450b06d665e3e4cb0dd08727b921c5))
- *(venv)* Fail on an unlockable store only for a foreign venv ([`c646db1`](https://github.com/s0undt3ch/ToolR/commit/c646db1b401ad242dfa9d894a12f83a10e066f88))
- *(trust)* Tidy venv sync lock files and keep the lock wait quiet ([`5a25699`](https://github.com/s0undt3ch/ToolR/commit/5a25699bdc5bcd749ec1450a89eae7888cc9b2c7))
- *(dispatch)* Rebuild the venv when clap doesn't know a plugin command ([`da69cd4`](https://github.com/s0undt3ch/ToolR/commit/da69cd4565a96e90fb20c6c3b038a302d4cf65c2))
- *(venv)* Ask before replacing an in-tree venv toolr didn't build ([`ecde394`](https://github.com/s0undt3ch/ToolR/commit/ecde394b0f7588ec43bafdd903def0b019e70dd6))
- *(venv)* Keep a venv toolr began building, and ask before the lock ([`9cb583c`](https://github.com/s0undt3ch/ToolR/commit/9cb583c960c66052546677146016c3de50dd5988))
- *(cli)* Print clap's error first when the venv wasn't rebuilt ([`6fd606b`](https://github.com/s0undt3ch/ToolR/commit/6fd606b693e1996de1ee151e4b27d6d74b05217d))
- *(venv)* Give Windows venvs a real identity so removal asks once ([`15816aa`](https://github.com/s0undt3ch/ToolR/commit/15816aafbce98bd93675c4ed8031361f2368e72e))
- *(release)* Remove the Test PyPI attestations before they reach dist consumers ([`a7eae61`](https://github.com/s0undt3ch/ToolR/commit/a7eae611c607fd188d500d0d1d104bee6892ea46))
- *(tests)* Write git fixture files as UTF-8 ([`da11955`](https://github.com/s0undt3ch/ToolR/commit/da11955e2a5aefa3b3df2b187fdf12e8c16b6d53))

### <!-- 10 -->💼 Other

- *(rust)* Declare rust-version as the pinned mise toolchain ([`c93ae5a`](https://github.com/s0undt3ch/ToolR/commit/c93ae5a328c433fec10858a33dab62bcd2f10615))
- *(xtask)* Add patch-coverage to check changed-line coverage ([`b0fe96f`](https://github.com/s0undt3ch/ToolR/commit/b0fe96fbe83bfdab2261e9dd6d6af4b99ece7cd0))
- *(toolr-py)* Link only toolr-core's command and docstring modules ([`bebb999`](https://github.com/s0undt3ch/ToolR/commit/bebb999b8fb99d2ba4ca00218913bd7581dd866c))
- *(deps)* Override mdformat-tables' cap to keep markdown-it-py 4 ([`94e5ed8`](https://github.com/s0undt3ch/ToolR/commit/94e5ed8fdc2f134a866e69d81cb2bda604c2fd12))
- *(deps)* Install mkdocs for the gate tests and mypy ([`bc75eae`](https://github.com/s0undt3ch/ToolR/commit/bc75eaedebf2447d4dc2013661bc67b2df0c1189))
- Enable fat LTO and a single codegen unit for release builds ([`2ece203`](https://github.com/s0undt3ch/ToolR/commit/2ece203b6b007cb7aa8e4af6d58088c280c290f5))

### <!-- 2 -->🚜 Refactor

- *(project)* Read python3's minor with the shared --version probe ([`1c485b3`](https://github.com/s0undt3ch/ToolR/commit/1c485b3ea18210aa6658fb8c526d857d11382979))
- *(venv)* Name and document the broken-venv resync path ([`9835e19`](https://github.com/s0undt3ch/ToolR/commit/9835e190f6307a9f9fc991e1f61794f76e27d65a))
- *(parser)* Push type resolution errors from one place ([`517f8aa`](https://github.com/s0undt3ch/ToolR/commit/517f8aaedf21f048da2efe1863399cb332e3a01b))
- *(rust)* Collapse nested if-lets into let-chains ([`7c70131`](https://github.com/s0undt3ch/ToolR/commit/7c70131d76e08d070d7b8a291008de31fc4059c5))
- *(tests)* Tidy the env guard and move workspace gates to xtask ([`b57a346`](https://github.com/s0undt3ch/ToolR/commit/b57a34625bd13c55500066176edb439b6956b0b1))
- *(venv)* Drop interpreter provenance gate ([`1c573b5`](https://github.com/s0undt3ch/ToolR/commit/1c573b5e573826e2749c2d42cdd9f2d82c99d450))
- *(hash)* Drop hash_file, unused since the provenance gate went ([`2be595f`](https://github.com/s0undt3ch/ToolR/commit/2be595fd9247ad02e09c2e60a8b733f206df5573))
- *(security)* Tidy trust helpers and comments ([`c16aaaa`](https://github.com/s0undt3ch/ToolR/commit/c16aaaa31af44f7361ed02c873ad9e6cb75a12f5))
- *(venv)* Share the freshness inputs message and fixture edit helpers ([`a9ffd24`](https://github.com/s0undt3ch/ToolR/commit/a9ffd24721aafd608382636b90a7aa70e1f1641c))
- *(plugins)* Flatten venv layouts and qualify glob_path::escape_path ([`7df3b9e`](https://github.com/s0undt3ch/ToolR/commit/7df3b9ec035ed1594af91b85a26df47e4b05a0b2))
- *(runner)* Simplify per-parameter hint resolution ([`f604bf3`](https://github.com/s0undt3ch/ToolR/commit/f604bf3db28b28933e5a8bb3ae4e0f8181679de0))
- *(bootstrap)* Decide manifest persistence once per run ([`4b92c7e`](https://github.com/s0undt3ch/ToolR/commit/4b92c7e7855a10d50d99875ea55d0989c69fcc62))
- *(command)* Tidy the stdin deadlock tests and comments ([`cfe1f2f`](https://github.com/s0undt3ch/ToolR/commit/cfe1f2f5682e8b295f10d18559df3af34406fc52))
- *(release)* Split the commit step's path comparison from its reporting ([`1f70d99`](https://github.com/s0undt3ch/ToolR/commit/1f70d9949439f3a5b347779b97ac0ee1c4366bac))
- *(release)* Give push-failure messages one shape ([`4e26ea5`](https://github.com/s0undt3ch/ToolR/commit/4e26ea5ccf6bcc5b2ec58cc6cdf1fcaa825ce234))
- *(testing)* Tie the discovery walk to the manifest walk ([`c7b23fb`](https://github.com/s0undt3ch/ToolR/commit/c7b23fb9b4348bde6ab1da390d34e2cd86817e6e))
- *(cli)* Carry the deferred path rule instead of re-deriving it ([`57ebe2c`](https://github.com/s0undt3ch/ToolR/commit/57ebe2c13be8319b9ad4a4ea64702c12dfdd0a57))
- *(runner)* Unregister signal handlers through a drop guard ([`767807b`](https://github.com/s0undt3ch/ToolR/commit/767807beff07205c5c3041724179973beda07ae0))
- *(runner)* Name the signal poll interval and tidy its tests ([`a3da27c`](https://github.com/s0undt3ch/ToolR/commit/a3da27c886270386b2da366d141b2cae110eb172))
- *(uv)* Narrow path_search's API and share its test helpers ([`a05fbb6`](https://github.com/s0undt3ch/ToolR/commit/a05fbb6392d97898502f025fb5e32f7f760822e3))
- *(tests)* One floor-settings list checked with subtests, fewer redundant asserts ([`7d3748d`](https://github.com/s0undt3ch/ToolR/commit/7d3748dbd1668c53e900e07eddf9a7eec8ff5bad))
- *(testing)* Name pending command fields and the tools. prefix rule ([`f118042`](https://github.com/s0undt3ch/ToolR/commit/f11804247104a838daf584a7460a7e09cea4e35e))
- *(testing)* Move the unknown-group error port into its own module ([`dbdd30a`](https://github.com/s0undt3ch/ToolR/commit/dbdd30aa6dbbbb5ee69545aedda1aee35febc9e0))
- *(parser)* Walk tools/ once and give symlinked modules one error shape ([`33c1f26`](https://github.com/s0undt3ch/ToolR/commit/33c1f267f9938d57c351329c3e961968791cc4a9))
- *(testing)* Share the pruned-directory check between discovery walks ([`723106c`](https://github.com/s0undt3ch/ToolR/commit/723106ceb12f5f203438f10a7bf9fb967134095e))
- *(testing)* Reuse the shared control-character escaping ([`22fc4c2`](https://github.com/s0undt3ch/ToolR/commit/22fc4c210236582428ce19803f3499e28fff385a))
- *(runner)* Rename the repo tools check and tighten its comments ([`be121d7`](https://github.com/s0undt3ch/ToolR/commit/be121d7f14accffd631fb86a9bc255d18e4f46f5))
- *(venv)* Share one cache slot rule between resolve and meta_cache_dir ([`7d2716c`](https://github.com/s0undt3ch/ToolR/commit/7d2716cf8a5a7d8ef96c7e9c3eefadc934769dbb))
- *(action)* Name the pinned attestation identities once ([`b0c6184`](https://github.com/s0undt3ch/ToolR/commit/b0c618493016b891343da632ae098731d736f935))
- *(testing)* Gather the private namespace-path dependency in one helper ([`d2b2356`](https://github.com/s0undt3ch/ToolR/commit/d2b2356a2ca29f080d38b99b55fe49d8343e1451))
- *(release)* Render the Cargo version once in version bump ([`f98b778`](https://github.com/s0undt3ch/ToolR/commit/f98b77803d6d4fa64cd0e33af5b032b62da2de53))
- *(command)* Spell out the ctx.run timeout phases ([`a105241`](https://github.com/s0undt3ch/ToolR/commit/a105241fa782d0f746017405bef245ed53fa1c37))
- *(command)* Give infallible-in-practice branches no line of their own ([`db0fd60`](https://github.com/s0undt3ch/ToolR/commit/db0fd60cbc9d3eb74dd237c3e5bca49041229939))
- *(parser)* Call @group.command the binding form, not legacy ([`174cef5`](https://github.com/s0undt3ch/ToolR/commit/174cef5cdbe8e8e4cb8528b33789ee036db78670))
- *(parser)* Name the string-path decorator form StringPath ([`e0ee6bd`](https://github.com/s0undt3ch/ToolR/commit/e0ee6bd63d750b0a00ccf2b359e5671f36cdbf40))
- *(trust)* Mint trust proofs only from the default store ([`c0da331`](https://github.com/s0undt3ch/ToolR/commit/c0da33153b0a2ccb8ed2d95085a6a2c8f274ba74))
- *(trust)* Name the sync choice and the prompt-granted proof ([`391ebe7`](https://github.com/s0undt3ch/ToolR/commit/391ebe7c5cb18d0e5dd61d2937e15eb3be203cda))
- *(venv)* One site-packages locator for both venv layouts ([`24ae699`](https://github.com/s0undt3ch/ToolR/commit/24ae6994523c977ec51b584d2fa762ebdbf7167c))
- *(venv)* Name the site-packages read rule and share the pyvenv.cfg reader ([`67ca9e3`](https://github.com/s0undt3ch/ToolR/commit/67ca9e3dba13bbf4d8271170d0378a4b9237f694))
- *(third-party)* Rename glob.rs to find.rs and glob_manifests to find_manifests ([`cf15506`](https://github.com/s0undt3ch/ToolR/commit/cf15506aa40418083376177599825289f9c232da))
- *(ci)* Pin toolr through one setup-toolr wrapper action ([`e9dad2e`](https://github.com/s0undt3ch/ToolR/commit/e9dad2ea568d4a63c9a0bd83bb5ea11963c56f2e))
- *(install)* Reject an unusable digest in one place in install.sh ([`727938e`](https://github.com/s0undt3ch/ToolR/commit/727938e714973d93406fb8ac9a074e1ec3ec5c6b))
- *(install)* Compare gh versions with [version] and drive install.ps1's filter env from one table ([`0ceb586`](https://github.com/s0undt3ch/ToolR/commit/0ceb586c24d16398bd2ba7835bc01c468ebbfafb))
- *(manifest)* Name static_hash input kinds and tags ([`4e24eea`](https://github.com/s0undt3ch/ToolR/commit/4e24eea64b025d08dc27aab60d73440056341073))
- *(freshness)* Type record contents and stamp the clock in one place ([`b2960c4`](https://github.com/s0undt3ch/ToolR/commit/b2960c4626c6dbe5c9541990c886f871f024cd39))
- *(exit-status)* Simplify the mappers and make platform-specific tests explicit ([`0134b86`](https://github.com/s0undt3ch/ToolR/commit/0134b8645e7557ce6e9deba1058f52b4805bd679))
- *(cli)* Build each built-in subtree in its own function ([`ffa6ea0`](https://github.com/s0undt3ch/ToolR/commit/ffa6ea0f641fdc3aaa0a267d67c6e75df5c2c857))
- *(cli)* Attach the hidden commands with Command::subcommands ([`48bbc82`](https://github.com/s0undt3ch/ToolR/commit/48bbc82a0ec439c578c87e30593f5bf265e9acd0))
- *(cache)* Take kept repos as PathBufs and drop a duplicate test ([`d76760d`](https://github.com/s0undt3ch/ToolR/commit/d76760dce31e4fe7e62509374ee7556b82b26d0d))
- *(cache)* Keep prune's output calls on one line, restore the --all prompt ([`835b1f5`](https://github.com/s0undt3ch/ToolR/commit/835b1f54267d2ef367edda619e6684aab8aa0ab4))
- *(command)* Decide up front whether a reader drains to EOF ([`9c82af4`](https://github.com/s0undt3ch/ToolR/commit/9c82af4627b6a37b7268a17163e908aefcb6b6c4))
- *(command)* Pass ctx.run output destinations as owned files ([`4bed4f5`](https://github.com/s0undt3ch/ToolR/commit/4bed4f5cbc6de02e1423a717eedcacf8184b5a40))
- *(command)* Name ctx.run's output files by role ([`41a5b85`](https://github.com/s0undt3ch/ToolR/commit/41a5b8580cabc7262477bfccc06a758d3f24ffb9))
- *(uv)* Share the capped fetch and name install failures ([`cc2241c`](https://github.com/s0undt3ch/ToolR/commit/cc2241c87438d7accdee19626281fa5349570ebc))
- *(complete)* Share bash quoting and drop the bash 4 lookup branch ([`c69f5ab`](https://github.com/s0undt3ch/ToolR/commit/c69f5abdba07d4c14f53754aba250937abf89684))
- *(ci)* Derive ALL_CPYTHONS from requires-python ([`81a8367`](https://github.com/s0undt3ch/ToolR/commit/81a8367a908b5c4e3bcecca3887f62dfcbb4fd23))
- *(ci)* Check Python version lists from a toolr pre-commit command ([`91f0163`](https://github.com/s0undt3ch/ToolR/commit/91f016313ecb7b1577440bd6f2aaf3fc699fa5c0))
- *(tests)* Reuse tests/_git helpers in the release-notes tests ([`09b844f`](https://github.com/s0undt3ch/ToolR/commit/09b844fe32bf677a138b9d01ccf6102a39d1b29e))
- *(ci)* Rename check-test-layout to check-tests-layout and say what to fix ([`0e5eed5`](https://github.com/s0undt3ch/ToolR/commit/0e5eed598ed7e1d5eb8e50041cbfaa6e5933a937))
- *(parser)* Document the Binding contract and share the toolr member check ([`07b5f85`](https://github.com/s0undt3ch/ToolR/commit/07b5f85374e2cf1fb3c21de2bc340b451ce1430c))
- *(complete)* Share the strip-then-quote step in the bash script ([`d0226e1`](https://github.com/s0undt3ch/ToolR/commit/d0226e19cd65fc16a8bae636f3139306a209c9fa))
- *(parser)* Keep each group field once and name the doc states ([`4fbe57c`](https://github.com/s0undt3ch/ToolR/commit/4fbe57c1744d8a88de4833a1b1eba4fe00dc76c0))
- *(testing)* Mirror the parser's group-field model in the tester ([`ee063d6`](https://github.com/s0undt3ch/ToolR/commit/ee063d66229ff60f9c9b40ca8c58da7649e69dcf))
- *(build-fragment)* Return a named BuiltFragment ([`e604b07`](https://github.com/s0undt3ch/ToolR/commit/e604b077928118ad2f9a0b320ca9f73c72d2585d))
- *(manifest)* Return build warnings beside the manifest ([`1e0b5d8`](https://github.com/s0undt3ch/ToolR/commit/1e0b5d8513fff9fd56ca75722f7fc5c8181a068c))
- *(complete)* Tidy bash path-completion script and tests ([`f2d72f6`](https://github.com/s0undt3ch/ToolR/commit/f2d72f6fdc4d4903d09dfe0aa0fd07a9ce11cc2a))
- *(cli)* Read every bash probe path through non_empty_env_path ([`a5d0c9b`](https://github.com/s0undt3ch/ToolR/commit/a5d0c9b76da1ac8222cbc0c175788fddfca4c5ba))
- *(complete)* Keep bash_completion_candidates crate-private ([`f82293b`](https://github.com/s0undt3ch/ToolR/commit/f82293b5042c4f9db3b01f19ccea0de5dba16a46))
- *(complete)* Tidy the --opt= completion scripts, tests and specs ([`9e7f7f7`](https://github.com/s0undt3ch/ToolR/commit/9e7f7f74ed498ec5498f39bb2d16233b2c31a181))
- *(parser)* Share one ToolrNames and memo per build, resolve through methods ([`d5fd6bd`](https://github.com/s0undt3ch/ToolR/commit/d5fd6bde74ea033f1a356a651fb506f70abff2c5))
- *(testing)* Drop the unused member parameter and align _binding argument order ([`aabd2f3`](https://github.com/s0undt3ch/ToolR/commit/aabd2f3facf3664cdeab76b57ac04cbfadf33cfa))
- *(tests)* Rename the _context stub check, document it ([`6f95a57`](https://github.com/s0undt3ch/ToolR/commit/6f95a579f75c6b05de5747a54515cae04184b510))
- *(complete)* Split the bash parser, document each quoting workaround ([`6933f56`](https://github.com/s0undt3ch/ToolR/commit/6933f565349ac4e744caebcf66d6598e5fa701fa))
- *(complete)* One dead-end gate for --, tidy tests and spec ([`c409efd`](https://github.com/s0undt3ch/ToolR/commit/c409efd484e367b9668b6f8519e40473f90b6ede))
- *(parser)* One bound-call helper per side, align mirror names ([`1f7742d`](https://github.com/s0undt3ch/ToolR/commit/1f7742d21d83a169e64623aaaf77b60c3ee2942b))
- *(completion)* Gate install output in one place per arm ([`933558b`](https://github.com/s0undt3ch/ToolR/commit/933558b25512585cfd12bc45a220016894ebc55c))
- *(cli)* State the root --quiet rule once, compute venv_run's quiet once ([`0ccec76`](https://github.com/s0undt3ch/ToolR/commit/0ccec76e382c236be7190575d5678174be604238))
- *(complete)* List bash's readline invariants, share hostile-name fixtures ([`ed448f3`](https://github.com/s0undt3ch/ToolR/commit/ed448f3b168d97140f01d3179557290c9110601e))
- *(complete)* Explain each zsh and fish quoting workaround ([`31c96f2`](https://github.com/s0undt3ch/ToolR/commit/31c96f2155be7d9fb52ea8e8a776ad7a44c14917))
- *(complete)* Derive root flag specs once, split the scan ([`510f5c4`](https://github.com/s0undt3ch/ToolR/commit/510f5c40a6cb8b2df469c5882d3c7b7fa8c2fdae))
- *(cache)* Build prune report lines as strings ([`076882e`](https://github.com/s0undt3ch/ToolR/commit/076882e6593535f14c84808a0a4de5ae112abe80))
- *(cli)* One statement of the -q rule, simpler prune Report ([`dca7854`](https://github.com/s0undt3ch/ToolR/commit/dca7854225d3c825de698a3cd07303f0b4686d25))
- *(test)* Collapse the zsh pty helpers and merge the uppercase-partial tests ([`b5575f3`](https://github.com/s0undt3ch/ToolR/commit/b5575f3976eb67f71c359bd5c3e3d7f853d68816))
- *(completion)* Pick each shell's install inputs before one path call ([`e27943b`](https://github.com/s0undt3ch/ToolR/commit/e27943b38780822d3a1258c7ae61a6913ac4660d))
- *(completion)* Tidy the powershell script's quoting and comments ([`98bb86a`](https://github.com/s0undt3ch/ToolR/commit/98bb86a708152d538733bdf397eb907623676af2))
- *(docs)* Simplify the llms.txt gate hook ([`1169d91`](https://github.com/s0undt3ch/ToolR/commit/1169d915a57fb5705ae02f845a44a9e181cde15d))
- *(completion)* Name the zsh candidate arrays and tidy the listing tests ([`2e0072a`](https://github.com/s0undt3ch/ToolR/commit/2e0072a61d265dbe27d9fe49626a40353d706ae2))
- *(cli)* Default the unset ConsentMode field in __install-uv-now ([`4323323`](https://github.com/s0undt3ch/ToolR/commit/43233238c80bc7c00bdfa9c022736262720f53ea))
- *(dispatch)* Remove an unreachable group-help branch and an unused parameter ([`7023bfc`](https://github.com/s0undt3ch/ToolR/commit/7023bfcc0827ce5696fade26f2b0e839b7b15409))
- *(parser)* Remove the never-read registered group set ([`4a0137e`](https://github.com/s0undt3ch/ToolR/commit/4a0137e63a84a34fd1f53a764bcd58114d23f5d9))
- *(cli)* Remove the project deps migration shim ([`d612b3c`](https://github.com/s0undt3ch/ToolR/commit/d612b3ced7d83b8caa59aed2cf3e17b5c9dbc75f))
- *(core)* Narrow ten functions nobody outside their file calls ([`5c9e562`](https://github.com/s0undt3ch/ToolR/commit/5c9e562d2fa6d5f75a7c677792e6fd4a55c60f4d))
- Tidy the bootstrap tests and note why the test-only seams exist ([`946f811`](https://github.com/s0undt3ch/ToolR/commit/946f8119a726c61e2b2592d94591f49e9fd24144))
- *(startup-latency)* Simplify the hint API and argv scan, document the lazy imports ([`d3181e9`](https://github.com/s0undt3ch/ToolR/commit/d3181e9828448ec78fcddf26b13a11ee6e3e48de))
- *(cli)* Reuse the discovery error for the no-project hint ([`64b88a0`](https://github.com/s0undt3ch/ToolR/commit/64b88a0f11a128529acd8856d2c8618b84a5a6dc))
- *(security)* Import shown in hash and freshness ([`92d2073`](https://github.com/s0undt3ch/ToolR/commit/92d2073fa95e27f801b2b4d39bdb9f8856ea3433))
- *(cli)* Remove the hidden __build-static-manifest command ([`c5f00f7`](https://github.com/s0undt3ch/ToolR/commit/c5f00f72cd94733864d35c934915aa02f96080c4))
- *(manifest)* One rebuild pipeline for bootstrap, dispatch, TAB and explicit rebuilds ([`083321a`](https://github.com/s0undt3ch/ToolR/commit/083321ad8f98718d451f1f2e9a0f377b5a1dbd76))
- *(manifest)* Name the persist mode and drop the refresh side channels ([`0ea7e3d`](https://github.com/s0undt3ch/ToolR/commit/0ea7e3d8f54094494b54bb93fd6d0177301eb159))
- *(py)* Delete the unused report_on_import_errors helper ([`389d997`](https://github.com/s0undt3ch/ToolR/commit/389d99769dd9dfcf59e8d56e46aae097dd675465))
- Tidy the verify and method-form code, tabulate the TOOLR_* variables ([`c3cfe79`](https://github.com/s0undt3ch/ToolR/commit/c3cfe79a402f32c68e9c62b27310e5c9f91846ec))
- *(cache)* Drop the unread venv layout_version ([`5e02454`](https://github.com/s0undt3ch/ToolR/commit/5e024546a410ef9bc830ddc391a525951f4ba53e))
- *(manifest)* Add Manifest::empty() and use it for every default-filled literal ([`1a24caa`](https://github.com/s0undt3ch/ToolR/commit/1a24caa1a6b4c37fb64bc57487ccccada72d09d8))
- *(cli)* Make OutputOptions.verbosity an enum ([`641e89d`](https://github.com/s0undt3ch/ToolR/commit/641e89d1f4ae8a6be90f4096d0405434ddd2658a))
- *(cli)* Split dispatch.rs into run, self, complete and router modules ([`a82a4b6`](https://github.com/s0undt3ch/ToolR/commit/a82a4b6f8c8dd6c867bbe2be830f0a012c2e44ae))
- *(cli)* Share root-flag spellings and one reserved-group rule ([`638fb38`](https://github.com/s0undt3ch/ToolR/commit/638fb38f7a99586a318680f3a42a5d60297ccad6))
- *(trust)* One source for the venv reason, token size and quiet trust gate ([`c1c60b8`](https://github.com/s0undt3ch/ToolR/commit/c1c60b83d4b6923fbcc0e9bc0056b3638a681cb5))
- *(trust)* Move venv entries and locks into trust::venv_entries ([`d4c2e76`](https://github.com/s0undt3ch/ToolR/commit/d4c2e76cb0a2ab85ab96660a3df6a580135389c1))
- *(trust)* Fewer venv entry writers ([`fe678b1`](https://github.com/s0undt3ch/ToolR/commit/fe678b109ca58d1f7e525d014eecf1a1b97240bd))
- *(venv)* Name the venv token check states ([`5f46095`](https://github.com/s0undt3ch/ToolR/commit/5f460952b980f9168be3d8af316b29ae5b069b47))
- *(venv)* Name the sync steps ([`c1187cd`](https://github.com/s0undt3ch/ToolR/commit/c1187cd54aa87e037f7ac0383156ed253f535169))
- *(venv)* One accessor for a venv toolr may read ([`9d922ba`](https://github.com/s0undt3ch/ToolR/commit/9d922bad1ebae552cd4b4bd7d4f1bbe1ec2b50a7))
- *(dispatch)* Pass the preflight result as one value ([`53c5a8a`](https://github.com/s0undt3ch/ToolR/commit/53c5a8aaee5c6cf60c828ee93e3626f9fcf8f7a0))

### <!-- 3 -->📚 Documentation

- Document uv and Python resolution, back speed claims with measurements ([`4601eae`](https://github.com/s0undt3ch/ToolR/commit/4601eae2d41f62c8fe38781a5203401eae9e034c))
- Cover the non-uv Python paths and correct the managed uv location ([`9230385`](https://github.com/s0undt3ch/ToolR/commit/92303852925266ca5a3ef880b2f6d18d0b38ea0f))
- Correct uv consent, no-venv Python and resync claims ([`85ecbea`](https://github.com/s0undt3ch/ToolR/commit/85ecbea0c2ae5c4305f0b220dddd4de73c08f574))
- Tighten uv section and cross-reference the two skew remedies ([`d6c9cf5`](https://github.com/s0undt3ch/ToolR/commit/d6c9cf5aa558665491204b07ce8d160f7590162d))
- Replace the known-bugs page with a link to open bug issues ([`491f110`](https://github.com/s0undt3ch/ToolR/commit/491f110d5362070a65269ec6399099a941b2d797))
- Drop the stale known-bugs pointer from the snippet list ([`3d9777b`](https://github.com/s0undt3ch/ToolR/commit/3d9777b945519408e18c1f114f408dea0f936ca9))
- List known bugs at the top level, not in the authoring guide ([`a0b6f55`](https://github.com/s0undt3ch/ToolR/commit/a0b6f5572e32ed7b2c993412584ae5b817561c58))
- *(execute)* Point both sides of the Python floor lockstep at each other ([`9820cdf`](https://github.com/s0undt3ch/ToolR/commit/9820cdf4584e591ae1e6461090ae72f305313b53))
- *(project)* Name the real --python flag and describe the clamped default ([`6c0b4e4`](https://github.com/s0undt3ch/ToolR/commit/6c0b4e4baaae0c00bd7bdf66e8bfefee0f6fa696))
- *(arguments)* Document boolean flag rules ([`13efd5a`](https://github.com/s0undt3ch/ToolR/commit/13efd5a1e248d92d0c09d982dbfb968e0c8d3e80))
- *(specs)* Archive bool flags design and plan ([`b2cf911`](https://github.com/s0undt3ch/ToolR/commit/b2cf911a484a501a8bdb8f814c763856c99becc4))
- *(specs)* Mark bool flags design as implemented ([`bd58163`](https://github.com/s0undt3ch/ToolR/commit/bd58163c8c620e334b3ce17d2864718b609e863b))
- *(arguments)* Document env var values for boolean flags ([`b21969f`](https://github.com/s0undt3ch/ToolR/commit/b21969f93fe37e158789fa70b31ad0424f7990d5))
- *(specs)* Add repository trust design and plan ([`57319bc`](https://github.com/s0undt3ch/ToolR/commit/57319bcacb80bca454d6175e98edecb34c4e8842))
- *(security)* Document repository trust ([`88a93e2`](https://github.com/s0undt3ch/ToolR/commit/88a93e2cb0dfb9a63e7882795c73f75a2f155bd0))
- *(security)* Describe the trust store location and one-shot migration ([`c357fc7`](https://github.com/s0undt3ch/ToolR/commit/c357fc7cb3d715e79842ad0f4d1ad9f6b9e374c3))
- *(specs)* Archive repo trust design and plan ([`1e4157f`](https://github.com/s0undt3ch/ToolR/commit/1e4157fe5bb4eae902429855fd6c51e74c8d6cdc))
- *(security)* Document the store-location refusal and path-keyed trust ([`b765f09`](https://github.com/s0undt3ch/ToolR/commit/b765f0997055abdd9a5e59d6bc49c8f7745127c5))
- *(security)* Document the trust commands and the action version caveat ([`3e81f4d`](https://github.com/s0undt3ch/ToolR/commit/3e81f4de3a02c7a9becd7030c7845f6da59d0ed4))
- *(specs)* Add safe shell output design and plan ([`c30a941`](https://github.com/s0undt3ch/ToolR/commit/c30a9414fe947b9b99c9bae089c737b40303b37d))
- *(claude)* Document coverage commands and gates ([`ca83f28`](https://github.com/s0undt3ch/ToolR/commit/ca83f28475ec1ddf5d2b0e8f0aa3681ef1ae0187))
- *(specs)* Archive safe shell output design and plan ([`0439292`](https://github.com/s0undt3ch/ToolR/commit/0439292987ecf659ecbb572f0771a5d123684573))
- *(specs)* Design and plan for venv freshness covering pyproject edits ([`711ae3c`](https://github.com/s0undt3ch/ToolR/commit/711ae3c477b5ca1858411efca4a5f991048488ce))
- *(venv)* Describe which tools/ files make the venv stale ([`113c0da`](https://github.com/s0undt3ch/ToolR/commit/113c0da0338d1f6ccf8df834a5252e34b965885a))
- *(specs)* Archive venv freshness inputs design and plan ([`084cf42`](https://github.com/s0undt3ch/ToolR/commit/084cf42789a4bfc5fb2fbc4ddef51f61cfbbe489))
- *(venv)* Say a python-version change needs a sync on cache venvs ([`170fcf6`](https://github.com/s0undt3ch/ToolR/commit/170fcf66b4da2dde65b9b9d316cb5a8294f3ff41))
- *(specs)* Design and plan for escaping paths before globbing ([`5fd2d72`](https://github.com/s0undt3ch/ToolR/commit/5fd2d721eb772c61d892eb54eabbc5bc253cd902))
- *(specs)* Archive plugin glob escape design and plan ([`267a4ed`](https://github.com/s0undt3ch/ToolR/commit/267a4ed769d338285e8a289784f1b24b3c3969f3))
- *(specs)* Point the archived glob escape plan at the shipped approach ([`1b1d73f`](https://github.com/s0undt3ch/ToolR/commit/1b1d73ff7968a63198c70d86e61888bec1c5705e))
- *(specs)* Design and plan for per-parameter type hint resolution ([`525b740`](https://github.com/s0undt3ch/ToolR/commit/525b740928b62d5748b976bc170eed35aa31fd5f))
- *(arguments)* Document TYPE_CHECKING imports in command signatures ([`86361e9`](https://github.com/s0undt3ch/ToolR/commit/86361e9d827e1e6d33f32dd0911f89a71f6bbe1d))
- *(specs)* Archive runner type hints design and plan ([`853ea14`](https://github.com/s0undt3ch/ToolR/commit/853ea14a82006dcc8e9cfe79813b243a3dd3879f))
- *(specs)* Spec and plan F-27 help untrusted manifest write ([`cf46deb`](https://github.com/s0undt3ch/ToolR/commit/cf46debf64ea5b3d199a4e87aca78d5424fae999))
- *(security)* Note that untrusted repos get no manifest file ([`fe4e330`](https://github.com/s0undt3ch/ToolR/commit/fe4e330fdfacaedb7f1ff246d0a97f70057bc089))
- *(specs)* Archive help untrusted manifest write design and plan ([`72a0af6`](https://github.com/s0undt3ch/ToolR/commit/72a0af6534397364354889af573b04d0d2de961b))
- *(security)* Narrow the untrusted-manifest promise to automatic rebuilds ([`3b01fd1`](https://github.com/s0undt3ch/ToolR/commit/3b01fd1f214e875e52d8728b6e55257746ebd123))
- *(specs)* Spec and plan F-08 ctx.run input deadlock ([`cd2718a`](https://github.com/s0undt3ch/ToolR/commit/cd2718aa426bdac5c6ab9b64e3110e97f1ed499f))
- *(specs)* Archive ctx.run input deadlock design and plan ([`a1fa76a`](https://github.com/s0undt3ch/ToolR/commit/a1fa76a40b508f3b34affba6bf025e1ec7a8e2e3))
- *(specs)* Spec and plan F-24 release prek failures ([`d1994c6`](https://github.com/s0undt3ch/ToolR/commit/d1994c6c854a80aa7ac2feb44b1876a30e67a8d7))
- *(specs)* Archive release prek failures design and plan ([`77573ad`](https://github.com/s0undt3ch/ToolR/commit/77573adcff21de5ecbf98ff6e21791757664e5b1))
- *(specs)* Note the pre-bump skill reference check in the release prek design ([`e71a393`](https://github.com/s0undt3ch/ToolR/commit/e71a3935064482dcc77052c7a5f756aa9dfc539a))
- *(specs)* Spec and plan F-22 action latest repo ([`2c70ad8`](https://github.com/s0undt3ch/ToolR/commit/2c70ad823dee316996727fae4165acdfe49fc195))
- *(specs)* Archive action latest repo design and plan ([`1ab49bd`](https://github.com/s0undt3ch/ToolR/commit/1ab49bdee01388565fd9a19f24e5bbf62e0dbef6))
- *(specs)* Spec and plan F-10 release tree binding ([`63a16ef`](https://github.com/s0undt3ch/ToolR/commit/63a16ef0ac982847a9a36a79a95d294ef66c6cbb))
- *(specs)* Archive release tree binding design and plan ([`d11fa56`](https://github.com/s0undt3ch/ToolR/commit/d11fa56927db84022a35f3ad49e8db79ba56f837))
- *(specs)* Spec and plan F-39 action cache key ([`29c9d12`](https://github.com/s0undt3ch/ToolR/commit/29c9d12b5a0e0ea910a697f1b8be37ed85b2f38c))
- *(specs)* Archive action cache key design and plan ([`b9af617`](https://github.com/s0undt3ch/ToolR/commit/b9af6176cd9ea01a33c48548a5f73ff5aaf739cc))
- *(specs)* Archive the ancestor tools discovery design and plan ([`c6b0d74`](https://github.com/s0undt3ch/ToolR/commit/c6b0d7445bcc53d5e7a9cca3a74cf8720f6aaa28))
- *(specs)* Spec and plan F-09 tester discover errors ([`d2251c3`](https://github.com/s0undt3ch/ToolR/commit/d2251c39b77d7b7c4ac0fc79150233ce04c5d0f0))
- *(testing)* Document CommandsTester discovery errors and sys.path ([`ba93817`](https://github.com/s0undt3ch/ToolR/commit/ba938172e93cdaec04bf0a45ce6992296edfd90f))
- *(specs)* Archive tester discover errors design and plan ([`ff2104f`](https://github.com/s0undt3ch/ToolR/commit/ff2104f5a8245465c89f1366aa0b52953f1d4980))
- *(testing)* Say a stray tools package replaces a namespace fixture ([`25a5496`](https://github.com/s0undt3ch/ToolR/commit/25a5496b46f1690668416fdb20bd4ae1dc59ffb2))
- *(testing)* Correct which sys.path entries CommandsTester hides ([`df5e844`](https://github.com/s0undt3ch/ToolR/commit/df5e844b0c913634c54569cc93f895f3da07c599))
- *(specs)* Add release notes fragments design ([`a88230c`](https://github.com/s0undt3ch/ToolR/commit/a88230cdbd3f3f6c65d2cacfc172112ac7812b1b))
- *(specs)* Add release notes fragments plan ([`3a1f2ad`](https://github.com/s0undt3ch/ToolR/commit/3a1f2adcdb5ab91bb73e29fc1bdb26124d4ff695))
- *(specs)* Apply adversarial review findings to release notes design and plan ([`90d113b`](https://github.com/s0undt3ch/ToolR/commit/90d113bf0f6af6eb4c867b7e2df472966ee52e3d))
- Point contributors at toolr release-notes ([`d59d8cc`](https://github.com/s0undt3ch/ToolR/commit/d59d8cc1b47422e9e5f63b42aafda1ecdc3cb59c))
- Spell out the release-notes gate for contributors ([`3e64705`](https://github.com/s0undt3ch/ToolR/commit/3e64705c463ae891011e7a80c26b26488538758f))
- *(specs)* Drop the skill name from the rebase step ([`9a85a4f`](https://github.com/s0undt3ch/ToolR/commit/9a85a4f6fc2b482bb36950935e96765ad75e075b))
- *(specs)* Archive release notes fragments design and plan ([`2c6ef0a`](https://github.com/s0undt3ch/ToolR/commit/2c6ef0a3358916f0a818077b5b824357d7628124))
- Make release-notes/README.md the canonical gate rules ([`844ce8e`](https://github.com/s0undt3ch/ToolR/commit/844ce8e9caad0c822a9c93fc7967ac13de877f40))
- Link the release-notes README by URL so mkdocs --strict resolves it ([`3cfbcd1`](https://github.com/s0undt3ch/ToolR/commit/3cfbcd1e1f2bc2444ec7fbc56084340826aaa467))
- *(specs)* Spec and plan F-15 toolr-py dependencies ([`ca0425e`](https://github.com/s0undt3ch/ToolR/commit/ca0425ed92e6292ec22574fe75d2b7b9b5b19db8))
- *(specs)* Archive toolr-py deps design and plan ([`64a5235`](https://github.com/s0undt3ch/ToolR/commit/64a52352fc484214b763aa67cb4833fcd6c8d8ef))
- *(contributing)* Explain toolr-core's full feature and the lean build ([`f7d430b`](https://github.com/s0undt3ch/ToolR/commit/f7d430bfb811cd9e227531d1d90c13258c38d7ac))
- *(specs)* Spec and plan F-11 path default repo root ([`33fdfc7`](https://github.com/s0undt3ch/ToolR/commit/33fdfc7ad51386a71993743b104978f616a2d155))
- *(cli)* Relative path defaults resolve from the repo root ([`31def93`](https://github.com/s0undt3ch/ToolR/commit/31def937e55227e717637521d093c308ce3fc6db))
- *(specs)* Archive path default repo root design and plan ([`2893276`](https://github.com/s0undt3ch/ToolR/commit/28932764724b48dbcac963e8c4febac21d561c07))
- *(specs)* Spec and plan F-29 single SIGINT delivery ([`32ac9eb`](https://github.com/s0undt3ch/ToolR/commit/32ac9eb5393996cbacf0d75545ea0af283f72771))
- *(specs)* Archive sigint-forwarding spec and plan ([`17ec386`](https://github.com/s0undt3ch/ToolR/commit/17ec38637dcafcb620b9ca318c66fc23817208eb))
- *(specs)* Record the Linux SIGINT origin check ([`7dbdf07`](https://github.com/s0undt3ch/ToolR/commit/7dbdf0735dd1b007d9735956f2f32ead834029ff))
- *(specs)* Mark sigint-forwarding sections superseded by the revision ([`b524762`](https://github.com/s0undt3ch/ToolR/commit/b5247628e79d91779673589ef92c48d7571885c5))
- *(specs)* Spec and plan F-35 uv PATH lookup ([`b0dc1d0`](https://github.com/s0undt3ch/ToolR/commit/b0dc1d010951f7e340b94f0aa61657042ef17b93))
- *(uv)* Document that relative PATH entries are skipped ([`bd2a699`](https://github.com/s0undt3ch/ToolR/commit/bd2a6992492df0f92ea357bcf14b78ca8f9431c6))
- *(specs)* Archive uv path lookup design and plan ([`478bc30`](https://github.com/s0undt3ch/ToolR/commit/478bc307874814211528829fb4aac7d4c9ee0f98))
- *(specs)* Record the extra PATH lookups in the uv path lookup design ([`f6e074b`](https://github.com/s0undt3ch/ToolR/commit/f6e074bd25a097c1c62dfa1f0b07bbf033460c9e))
- *(specs)* Note access(X_OK) and unset PATH in the uv path lookup design ([`4874471`](https://github.com/s0undt3ch/ToolR/commit/4874471c11bca16e9f82bf461feb90e2a65486ec))
- *(specs)* Spec and plan F-38 python version lists ([`821327f`](https://github.com/s0undt3ch/ToolR/commit/821327fd95e9551251bb63589bb2be3732343028))
- *(ci)* Shorten the version-list comments and cross-link the patch-floor rule ([`3283d4d`](https://github.com/s0undt3ch/ToolR/commit/3283d4d964936fe2f74cd2ba0de531cbe428e831))
- *(specs)* Archive python version lists design and plan ([`8d65322`](https://github.com/s0undt3ch/ToolR/commit/8d65322eb2fc4c42b70c218d5543e4e9a14ff850))
- *(specs)* Spec and plan F-19 tester standalone group ([`03805c9`](https://github.com/s0undt3ch/ToolR/commit/03805c98c3990e25822641e199e68ae600a85188))
- *(testing)* Document standalone commands under CommandsTester ([`e3fb4f8`](https://github.com/s0undt3ch/ToolR/commit/e3fb4f8e7076071f6a327fb1e9caef1cc790f172))
- *(testing)* Describe the pending store in the CommandsTester docstrings ([`37b5ed4`](https://github.com/s0undt3ch/ToolR/commit/37b5ed4ef163add7de59a2f8ceec34aafcdb2e2f))
- *(testing)* State the @command shape toolr's static parser recognises ([`c55a7ae`](https://github.com/s0undt3ch/ToolR/commit/c55a7ae70f3432446a6d4c178e60d7e13c8117d7))
- *(testing)* Keep the pending-store patch out of the CommandsTester docstring ([`d62d7e7`](https://github.com/s0undt3ch/ToolR/commit/d62d7e7f4c863bd9ee8abd61d48ba2ffbebe1c66))
- *(tests)* Drop the audit ID from the unknown-group parity test ([`b316ded`](https://github.com/s0undt3ch/ToolR/commit/b316ded9f0e1a9eb5eb3eecb7017ad295c68d6ee))
- *(core)* Point the unknown-group error code at its Python mirror ([`65dd4b8`](https://github.com/s0undt3ch/ToolR/commit/65dd4b829befa297bcf5d6694f8e53a4bbeea82d))
- *(specs)* Archive tester standalone group design and plan ([`db7d264`](https://github.com/s0undt3ch/ToolR/commit/db7d26459e1a225384f744a94e566bb39a4967b8))
- *(specs)* Spec and plan F-17 symlinked tools files ([`ee57f94`](https://github.com/s0undt3ch/ToolR/commit/ee57f94f701efa131c4eee8cbe22e99ed4359345))
- Note that symlinked modules under tools/ are rejected ([`0fdfaa0`](https://github.com/s0undt3ch/ToolR/commit/0fdfaa08756ada73a34b670b4ca80d370a259bec))
- Note how Windows checkouts without symlinks fail ([`fbf61fa`](https://github.com/s0undt3ch/ToolR/commit/fbf61faff4e925e1a0ea6121b12553ea083a761a))
- Explain symlinked-module rejection as a no-surprises rule ([`958ae4e`](https://github.com/s0undt3ch/ToolR/commit/958ae4ef305a05843a884cec0dd29e86a52afbb7))
- *(specs)* Archive symlinked tools files design and plan ([`17eccf4`](https://github.com/s0undt3ch/ToolR/commit/17eccf431d1678ad6467664dee6ee64e97fd9c31))
- *(specs)* Spec and plan F-28 runner import traceback ([`1eea858`](https://github.com/s0undt3ch/ToolR/commit/1eea85854b96b2d9c592d869a30f30d506eab022))
- *(specs)* Archive runner import traceback design and plan ([`e45f45c`](https://github.com/s0undt3ch/ToolR/commit/e45f45cffc11b99867b4b8020350798ee410b31b))
- *(specs)* Spec and plan F-18 in-tree venv cache dir ([`bb5ef83`](https://github.com/s0undt3ch/ToolR/commit/bb5ef83f6bfe31e947190c45382232c4a3291a5b))
- *(specs)* Archive in-tree venv cache dir design and plan ([`9d002ed`](https://github.com/s0undt3ch/ToolR/commit/9d002ede34bc2524994507a63570c8470de336a5))
- *(specs)* Spec and plan F-42 tester stdout docstring ([`9c1ff3d`](https://github.com/s0undt3ch/ToolR/commit/9c1ff3dbe0b16ecc1a4853763c18b390ac2643b5))
- *(testing)* Say which ContextForTesting stream each method writes to ([`3a11e75`](https://github.com/s0undt3ch/ToolR/commit/3a11e75084748917ca4d64c543ff3d0024353afe))
- *(testing)* State the verbosity and prompt caveats of the captured streams ([`fb6e684`](https://github.com/s0undt3ch/ToolR/commit/fb6e6844e1966521b5c6bec79e7e23ee97dede4d))
- *(testing)* Make the ContextForTesting docstrings the one copy of the stream split ([`3220972`](https://github.com/s0undt3ch/ToolR/commit/3220972fb70d26a4d4cc9bc03690f03c260e8ed6))
- *(specs)* Archive tester stdout docstring design and plan ([`b8b7cdb`](https://github.com/s0undt3ch/ToolR/commit/b8b7cdbfe00f39b63171fb15e092118f5131f2dc))
- *(specs)* Spec and plan F-23 action attestation signer ([`bbcb456`](https://github.com/s0undt3ch/ToolR/commit/bbcb456a7549578760b49ee8d73fbb017b73225a))
- *(specs)* Archive action attestation signer design and plan ([`69e77cf`](https://github.com/s0undt3ch/ToolR/commit/69e77cf6b23106bc2144917eb48ad77921f4495c))
- *(specs)* Spec and plan FU-10 tester sys.path filter ([`c06c984`](https://github.com/s0undt3ch/ToolR/commit/c06c984e9a730d4e18e367feb66b383997016cbd))
- *(specs)* Match the FU-10 design and plan to the tools finder ([`c59c9c9`](https://github.com/s0undt3ch/ToolR/commit/c59c9c9b61a5acbbd55305d1104ea055036d52fb))
- *(specs)* Record the FU-10 extend_path, find_spec and exit-order follow-ups ([`726ae45`](https://github.com/s0undt3ch/ToolR/commit/726ae45960b3d8c4ca393f5d2e7b10b7f99a3d9d))
- *(testing)* Place the discover walk scope with discover and note assertion rewriting ([`3f0e2ae`](https://github.com/s0undt3ch/ToolR/commit/3f0e2ae3b8d3d738fe8d68f36e4405aa65ca4be7))
- *(specs)* Record the FU-10 maintainability pass ([`9d7ac3f`](https://github.com/s0undt3ch/ToolR/commit/9d7ac3f9ca41fb107c5d375db7b782e7c7f3d9e9))
- *(specs)* Archive tester sys.path filter design and plan ([`45682ac`](https://github.com/s0undt3ch/ToolR/commit/45682ac924681a0f01b545cc3e6df3ddb21382d4))
- *(specs)* Spec and plan F-26 version py tags ([`cbb3883`](https://github.com/s0undt3ch/ToolR/commit/cbb3883d6bb42b6e860770244bbbe997af99995a))
- *(specs)* Update F-26 version py tags spec to the pre-release decision ([`3dacc2d`](https://github.com/s0undt3ch/ToolR/commit/3dacc2d666b1f585b4fb62a06bccc26c1e322ba9))
- *(specs)* Record the F-26 pre-release rollup decision and review follow-ups ([`d48834d`](https://github.com/s0undt3ch/ToolR/commit/d48834da28cb694ca48560a20ba3d1d3e09c7378))
- *(specs)* Record the F-26 round-2 release fixes ([`0acfb2f`](https://github.com/s0undt3ch/ToolR/commit/0acfb2f62794e48a1a0dcfd496e1586001be0130))
- *(specs)* Record the F-26 round-3 maintainability changes ([`ec1dbd9`](https://github.com/s0undt3ch/ToolR/commit/ec1dbd933dbd0c40e35311be7043965ccd93bc3b))
- *(specs)* Archive version py tags design and plan ([`b370703`](https://github.com/s0undt3ch/ToolR/commit/b3707035aa50c5af64731285cdebeaf12c2a4d12))
- *(specs)* Spec and plan F-21 ctx.run timeout kills tree ([`43b1f61`](https://github.com/s0undt3ch/ToolR/commit/43b1f61f18319473a6282c827aaff38f54aa040c))
- *(command)* Document ctx.run timeouts killing the process tree ([`9d65a13`](https://github.com/s0undt3ch/ToolR/commit/9d65a1310c5ae5d1e9f429b8cbc30ad78b282776))
- *(command)* Say which processes a ctx.run timeout can't kill ([`b057e2b`](https://github.com/s0undt3ch/ToolR/commit/b057e2bc227b9a616999f527543cf64de4cdb1ae))
- *(command)* Note how far a ctx.run timeout can overrun ([`c3f58d5`](https://github.com/s0undt3ch/ToolR/commit/c3f58d59b43d7eb5a732a2f9bc8cb6e0dd09b41f))
- *(specs)* Archive ctx-run timeout kills tree design and plan ([`fac27a3`](https://github.com/s0undt3ch/ToolR/commit/fac27a36d0b979a37a53813635e579622b3a9044))
- *(specs)* Spec and plan F-43 misc maintainability ([`e3cd0fe`](https://github.com/s0undt3ch/ToolR/commit/e3cd0fe1f222242fc142c7e5cfcc8dd5cc4d5239))
- *(specs)* Archive misc maintainability design and plan ([`174b00e`](https://github.com/s0undt3ch/ToolR/commit/174b00e8882a883273f16da3da8ee8e86d3c3d65))
- *(specs)* Spec and plan F-32 README venv first run ([`a0f0844`](https://github.com/s0undt3ch/ToolR/commit/a0f08444b121711a98dfdcbb3128e9cae304599c))
- *(specs)* Update F-32 spec to trust-then-sync ([`051c733`](https://github.com/s0undt3ch/ToolR/commit/051c733e6b61cfa4732de1e6200659d2a08ad8e4))
- *(trust)* Document the sync prompt's env var, quiet and exit code ([`7347e47`](https://github.com/s0undt3ch/ToolR/commit/7347e47344d61d558392ba7bd9570298b8563c72))
- *(trust)* Keep the sync consent rules in one place ([`9187ece`](https://github.com/s0undt3ch/ToolR/commit/9187eced30c4764dd7ef7438b188e2247a206811))
- *(specs)* Archive trust-then-sync design and plan ([`91e3f46`](https://github.com/s0undt3ch/ToolR/commit/91e3f46b782dd178124ab87d12b8bf7427b3595a))
- *(specs)* Spec and plan F-14 windows site-packages locator ([`d7d381b`](https://github.com/s0undt3ch/ToolR/commit/d7d381b302183d336506911c6045dce1bfa8c86f))
- *(plugins)* Document the Windows site-packages layout ([`3e64036`](https://github.com/s0undt3ch/ToolR/commit/3e640369229a102a21b00a299982d3b6350290bd))
- *(plugins)* Describe fragment discovery as a lookup, not a glob ([`2d06820`](https://github.com/s0undt3ch/ToolR/commit/2d06820dceadf614750c6a2de1a7ab7f60b153de))
- *(plugins)* Name Lib/site-packages as a layout looked in everywhere ([`2c8f451`](https://github.com/s0undt3ch/ToolR/commit/2c8f451c04847cae9fb2a363a4b5bca159da330e))
- *(specs)* Archive windows site-packages design and plan ([`dc4df47`](https://github.com/s0undt3ch/ToolR/commit/dc4df4791f3920fc89e049d75e9af49cd16c35a8))
- *(specs)* Spec and plan F-25 dogfood pinned version ([`24c298f`](https://github.com/s0undt3ch/ToolR/commit/24c298f6a0ae0a9bb9a64a4fb85e95669376b5ee))
- *(specs)* Archive dogfood pinned version design and plan ([`76c71d3`](https://github.com/s0undt3ch/ToolR/commit/76c71d3a45316975a7026a5232955e97625e7f5d))
- *(specs)* Spec and plan F-44 installer attestation signer ([`76c414d`](https://github.com/s0undt3ch/ToolR/commit/76c414de76bdb8fd77e7ecb04516f3817a5f086a))
- *(installation)* Document the release-workflow attestation check and mise pin ([`e61d791`](https://github.com/s0undt3ch/ToolR/commit/e61d79142a56614d5f41513c957b1e2ba8d2dec1))
- *(installation)* State the gh 2.56 floor and show the mise pin in the recommended config ([`b202a65`](https://github.com/s0undt3ch/ToolR/commit/b202a653b5c0d94563be29d616053d69496266ab))
- *(installation)* Give the Linux hashing command for manual verification ([`db5cf9f`](https://github.com/s0undt3ch/ToolR/commit/db5cf9fa77a8640b7dc8ef0e7016d4fafd8eca0e))
- *(installation)* Trim the note after the manual verification command ([`475534d`](https://github.com/s0undt3ch/ToolR/commit/475534d440fefb1788e718ffb882cce0864142ce))
- *(specs)* Archive installer attestation signer design and plan ([`1201967`](https://github.com/s0undt3ch/ToolR/commit/120196768b6b0cbb17e3a3b2a925c840ced65d40))
- *(specs)* Spec and plan F-16 argparse manifest hash ([`c6ff1af`](https://github.com/s0undt3ch/ToolR/commit/c6ff1afff5457de7ad7ba02043846c1185e7ceb7))
- *(specs)* Update F-16 spec to current main ([`61d643e`](https://github.com/s0undt3ch/ToolR/commit/61d643ee894779981e11f884e4e65c48085f9699))
- *(internals)* List every static_hash input ([`0716ba7`](https://github.com/s0undt3ch/ToolR/commit/0716ba7bd542067fa34907c54770e721e9784b8b))
- *(internals)* Describe unreadable static_hash inputs ([`1fc04ed`](https://github.com/s0undt3ch/ToolR/commit/1fc04ed6f7625ce809092750291ced01ee880c84))
- *(internals)* Hash only the argparse blocks of tools/pyproject.toml ([`024e97c`](https://github.com/s0undt3ch/ToolR/commit/024e97ce5742225d921c0b2ee2952b67360fa51b))
- *(specs)* Archive argparse manifest hash design and plan ([`4fb4cc6`](https://github.com/s0undt3ch/ToolR/commit/4fb4cc6b64dac761990d7212743054c3fc99b2a7))
- *(specs)* Spec and plan F-30 tab manifest freshness ([`89d45c7`](https://github.com/s0undt3ch/ToolR/commit/89d45c78b288b69c04b56f9dbf33a76bc5cea053))
- *(specs)* Update F-30 spec to the F-16 hash inputs ([`2afbe75`](https://github.com/s0undt3ch/ToolR/commit/2afbe75cb03ab58e2b281616bd389dbf5b7c595a))
- *(internals)* Document the input_records stat gate ([`5c767e0`](https://github.com/s0undt3ch/ToolR/commit/5c767e002f040ebcb26f4c8c9f3ee3ac994f7c05))
- *(internals)* Describe the snapshot bound and the non-Unix gap ([`68f96ff`](https://github.com/s0undt3ch/ToolR/commit/68f96ff29bd08585dfd4cc3a67c1ac2179fb534c))
- *(internals)* Limit the stat gate to tools/'s filesystem and note NFS caching ([`10ed7f3`](https://github.com/s0undt3ch/ToolR/commit/10ed7f3e65e6614ae29b0c90e3ebd95f8aec4a59))
- *(internals)* Split the stat gate into its rules and qualify the single pyproject read ([`24dfd9d`](https://github.com/s0undt3ch/ToolR/commit/24dfd9dab2573742c59ece5508087486987a69b9))
- *(specs)* Archive tab manifest freshness design and plan ([`d92bbe4`](https://github.com/s0undt3ch/ToolR/commit/d92bbe4e95d19fac65f3fc647c0a9049c33261b1))
- *(claude)* Say how to close a coverage gap ([`704cdc1`](https://github.com/s0undt3ch/ToolR/commit/704cdc16392088cbdeb8414aafba563afe42b0c7))
- *(specs)* Spec and plan F-20 exit codes for signal-killed children ([`9ae9cce`](https://github.com/s0undt3ch/ToolR/commit/9ae9ccecc30afb1d833ae4934cfbd697b163be4d))
- *(specs)* Update F-20 spec to the -N returncode decision ([`0c247ce`](https://github.com/s0undt3ch/ToolR/commit/0c247ce712f131039b1454922fb600c80cde2a65))
- Document exit codes for signal-killed children ([`f3f87d3`](https://github.com/s0undt3ch/ToolR/commit/f3f87d3ebaed409e09eabdf04e6bd487fc120e77))
- *(specs)* Archive venv-run-signal-exit spec and plan ([`64c35bb`](https://github.com/s0undt3ch/ToolR/commit/64c35bbdc41e120b0942a6414813c3036215c7b9))
- *(specs)* Spec and plan F-31 cli build command split ([`93eb291`](https://github.com/s0undt3ch/ToolR/commit/93eb291412de61ab36b197d02f8a9d10e798acad))
- *(specs)* Refresh cli build command spec for current main ([`fa80009`](https://github.com/s0undt3ch/ToolR/commit/fa800091593e1ffc28106ecc1c39a6c0e401788e))
- *(specs)* Archive cli build command design and plan ([`92b7f04`](https://github.com/s0undt3ch/ToolR/commit/92b7f0447aecda8d087dadf0652293baf9b7e06d))
- *(specs)* Note the reserved-group skip had no test on main ([`3077e50`](https://github.com/s0undt3ch/ToolR/commit/3077e5006aaa341d10eaf57eac1efa565156f972))
- *(specs)* Correct the dump manifest, test module and group wording ([`ec04a70`](https://github.com/s0undt3ch/ToolR/commit/ec04a70ccc89337d4cb5afaca3c363ebac0b9e32))
- *(specs)* Design and plan for the rebuild lock leak ([`4f10434`](https://github.com/s0undt3ch/ToolR/commit/4f104346e7d5f1846d68d6d2f94f5acf14f0cde7))
- *(specs)* Archive the rebuild lock leak design and plan ([`617b2bd`](https://github.com/s0undt3ch/ToolR/commit/617b2bdc8b92eb927e268ff2cd17fd1e529d78b9))
- *(specs)* Spec and plan F-40 ctx.run reader drain ([`e5a0625`](https://github.com/s0undt3ch/ToolR/commit/e5a06253c09002febcebf8a8d5899a82fc5cffed))
- *(specs)* Revise F-40 against the single reader loop from #645 ([`c5d0faf`](https://github.com/s0undt3ch/ToolR/commit/c5d0faf73e49a22619826a055240b1468eea22b8))
- *(specs)* Archive the F-40 ctx.run reader drain design and plan ([`2285ad3`](https://github.com/s0undt3ch/ToolR/commit/2285ad35cd1e7158bd34c9e8a5836440f9ff0373))
- *(specs)* Spec and plan F-41 ctx.run owned fds ([`a6ea688`](https://github.com/s0undt3ch/ToolR/commit/a6ea688d96c5b31fa8b4c574733442fac8781b73))
- *(specs)* Trim F-41 to what #645 left: owned files in CommandConfig ([`41f7662`](https://github.com/s0undt3ch/ToolR/commit/41f7662cfe1a314eb69cfa043797358a3fb2f8d3))
- *(specs)* Archive the F-41 ctx.run owned files design and plan ([`9d9acd1`](https://github.com/s0undt3ch/ToolR/commit/9d9acd19d1cd18c696f0c25be0c9e2792453e37c))
- *(specs)* Spec and plan F-06 uv install artefact verification ([`f0e1f94`](https://github.com/s0undt3ch/ToolR/commit/f0e1f9424c820d00e01ca8cf622e8f45dce1a34a))
- *(specs)* Record the confirmed uv install verification decisions ([`c6b4b6f`](https://github.com/s0undt3ch/ToolR/commit/c6b4b6faf930225eb2181ff907d208be31ccd1a0))
- *(specs)* Archive the F-06 uv install verification design and plan ([`2931607`](https://github.com/s0undt3ch/ToolR/commit/293160759517b88e6ca60b525be7bb8e8289fe5f))
- *(specs)* Spec and plan F-36 path argument completion ([`bcbaa36`](https://github.com/s0undt3ch/ToolR/commit/bcbaa3678fe622971456a74910b9bf79bc2ff240))
- *(specs)* Record the path completion decisions ([`e66ce35`](https://github.com/s0undt3ch/ToolR/commit/e66ce35ad727fb88667a3ffd674b2dbb2f6d9800))
- *(specs)* Drop the space only after a lone directory candidate ([`bd8d7cd`](https://github.com/s0undt3ch/ToolR/commit/bd8d7cd4995ec0f73e011e2c5e21a329649a364d))
- *(complete)* Document path argument completion ([`15f4aa4`](https://github.com/s0undt3ch/ToolR/commit/15f4aa4c073f50a9e03be670e47d93d29eb6f131))
- *(specs)* Keep bash's nospace for every directory-only completion ([`19dfa50`](https://github.com/s0undt3ch/ToolR/commit/19dfa50c66cad72a8b045cb822c34f2a853e11a3))
- *(specs)* Archive the F-36 path completion design and plan ([`7f4180e`](https://github.com/s0undt3ch/ToolR/commit/7f4180e6be51fc8bbee132d22def95c1c0b07fe8))
- *(cli)* Say what older completion scripts do with path slots ([`2e35214`](https://github.com/s0undt3ch/ToolR/commit/2e3521484a26f8e6e41b600bfd3f697707d46c18))
- *(specs)* Spec and plan FU-1/2/3 parser and tester parity ([`49dc223`](https://github.com/s0undt3ch/ToolR/commit/49dc223d11080aa7486e402fa88dd7c2df041510))
- *(specs)* Record the decided parser and tester parity open points ([`add0267`](https://github.com/s0undt3ch/ToolR/commit/add0267569bbc5de9962ef9e89899261aceec384))
- *(writing-commands)* Document the accepted command forms and the new errors ([`d2c2be1`](https://github.com/s0undt3ch/ToolR/commit/d2c2be171bdc86f732e77397ab1cecf582a92667))
- *(concepts)* Keep command tests out of tools/ ([`edd6cbf`](https://github.com/s0undt3ch/ToolR/commit/edd6cbfebe7f17e9c8e794d09e93daadbb88114c))
- *(specs)* Archive the FU-1/2/3 parser and tester parity design and plan ([`26c60fa`](https://github.com/s0undt3ch/ToolR/commit/26c60faa516c16703639839fa642813a84547d73))
- *(specs)* Spec and plan FU-4/FU-5 derived Python versions and a fast hook ([`a28492e`](https://github.com/s0undt3ch/ToolR/commit/a28492e8e6165028a677bf2325418d99b90f0273))
- *(specs)* Archive the FU-4/FU-5 Python versions design and plan ([`7d2715f`](https://github.com/s0undt3ch/ToolR/commit/7d2715fbed5b11994901f76320b3247e719ef535))
- *(writing-commands)* Commands must be top-level defs, with platform checks inside ([`5037123`](https://github.com/s0undt3ch/ToolR/commit/50371235eae3d7887220a5505bb1d23794859fbd))
- *(tests)* Describe the parity corpus layout ([`f796bc7`](https://github.com/s0undt3ch/ToolR/commit/f796bc79731b1cbe5018868474d794968912318b))
- *(specs)* Spec and plan FU-12 registry test isolation ([`5556600`](https://github.com/s0undt3ch/ToolR/commit/55566005462d0d9c40c9040b871e61a361a76435))
- *(specs)* Archive the FU-12 registry test isolation design and plan ([`0770bf1`](https://github.com/s0undt3ch/ToolR/commit/0770bf192289a17f231518f8462a96b9b0edae1a))
- *(writing-commands)* Give the command_group examples a description ([`0831ec8`](https://github.com/s0undt3ch/ToolR/commit/0831ec8fa3e12d4f111532ee0745391e376aa404))
- *(specs)* Spec and plan FU-16/FU-17 bash completion word handling ([`1a642a1`](https://github.com/s0undt3ch/ToolR/commit/1a642a17f366dc464657c98c11636313fc24f6ed))
- *(specs)* Record the bash completion words decisions ([`3354501`](https://github.com/s0undt3ch/ToolR/commit/33545017a83638e92c1cb2b49ed450e07e864c4d))
- *(cli)* Fix completion install paths and state the bash 4.4 floor ([`8ed0eaf`](https://github.com/s0undt3ch/ToolR/commit/8ed0eafefc9d3cb7ef104ff2bd45cc53c3dcf2df))
- *(specs)* Archive the FU-16 bash completion words design and plan ([`ba47f66`](https://github.com/s0undt3ch/ToolR/commit/ba47f66f52f0f55819633336adc120b5addc45f9))
- *(specs)* Spec and plan FU-21 order-independent tests and pytest-randomly ([`6b02597`](https://github.com/s0undt3ch/ToolR/commit/6b02597d0cd7ef6a745fa0ce65bfacd1877f6e51))
- *(contributing)* Reproduce a test failure from its seed ([`6bbfc8f`](https://github.com/s0undt3ch/ToolR/commit/6bbfc8fe1d8aaef59ed66ce51b813659b9d7f857))
- *(specs)* Record that order-dependent distribution tests are fixed, not exempted ([`9c6d53d`](https://github.com/s0undt3ch/ToolR/commit/9c6d53d0470a59f5bb1611cc154a06dd0090b7c9))
- *(specs)* Archive the FU-21 order-independent tests design and plan ([`d2cc928`](https://github.com/s0undt3ch/ToolR/commit/d2cc928c15a5586d4dcdddd71a028f19884efeb1))
- *(contributing)* Drop the redundant -p randomly from the replay example ([`70e767e`](https://github.com/s0undt3ch/ToolR/commit/70e767e6fd39e0dcee12b2c579a25cfedc19f0b2))
- Note random test order and the autouse fixture convention ([`169c66a`](https://github.com/s0undt3ch/ToolR/commit/169c66a5ce6a658e868186ee78acb486a19cfb71))
- *(writing-commands)* Describe every group a title-only example creates ([`a938c4b`](https://github.com/s0undt3ch/ToolR/commit/a938c4b0c515e276966b57e676de4b7c47bc4e41))
- *(specs)* Spec and plan FU-22 group re-declaration parity ([`46c835c`](https://github.com/s0undt3ch/ToolR/commit/46c835c7e38abd193d9a577a5ddfd69bde266e12))
- *(writing-commands)* Say every group needs one described declaration ([`5c169df`](https://github.com/s0undt3ch/ToolR/commit/5c169df3b65fbe9d62c3e9951adaaf53b23f1b98))
- *(writing-commands)* Say a later declaration fills an empty group field ([`8534e7a`](https://github.com/s0undt3ch/ToolR/commit/8534e7a4421b337b1dca1a310181724b92067a5d))
- *(specs)* Archive the FU-22 group re-declaration parity design and plan ([`de23cfc`](https://github.com/s0undt3ch/ToolR/commit/de23cfcb3f8424b1dc21c638ab77a9a3fc4c1ac2))
- *(writing-commands)* Say which group descriptions the build can read ([`89a3ad8`](https://github.com/s0undt3ch/ToolR/commit/89a3ad8a273ce11e56fbc6dfa38882993c9acc95))
- *(writing-commands)* Say which group values the build shows ([`5597005`](https://github.com/s0undt3ch/ToolR/commit/5597005cb6766bfc37c817cbd387c0cff9380189))
- *(specs)* Design and plan the bash install source hint ([`a22deb5`](https://github.com/s0undt3ch/ToolR/commit/a22deb5b4541a31de1f95aa56909716e23c7a37e))
- *(specs)* Archive the bash install source hint design and plan ([`8d3af8e`](https://github.com/s0undt3ch/ToolR/commit/8d3af8e2a009e30934b4dcf80bf602dde4bb2f71))
- *(cli)* List the bash completion install notes one per line ([`04a03eb`](https://github.com/s0undt3ch/ToolR/commit/04a03ebdaeeeb44ca3869e069b9735560bc8d251))
- *(specs)* Spec and plan FU-15 --opt=value completion ([`e0ea238`](https://github.com/s0undt3ch/ToolR/commit/e0ea238b51b0f81351fae9611e05dcfbe89f3642))
- *(specs)* Align FU-15 spec and plan with main's bash script ([`42dc89d`](https://github.com/s0undt3ch/ToolR/commit/42dc89d478e49c3064a512217232099ea4ae5454))
- *(complete)* Document --opt=value completion ([`88b1b5c`](https://github.com/s0undt3ch/ToolR/commit/88b1b5c8c3b1e07706185b2d595b3c5792042abf))
- *(specs)* Archive FU-15 --opt=value completion spec and plan ([`07c8120`](https://github.com/s0undt3ch/ToolR/commit/07c812081bdb2117aefb1829ce1f710c4beebfe7))
- *(complete)* Note that ~ isn't expanded after --opt= ([`1c9acfc`](https://github.com/s0undt3ch/ToolR/commit/1c9acfc49168090d861359ac94e94fa0ea920ea1))
- *(specs)* Spec and plan for command_group aliases (FU-26) ([`83035c9`](https://github.com/s0undt3ch/ToolR/commit/83035c92221af9690aefee15ffc9fa8dd33ed27d))
- *(specs)* Archive the command_group aliases spec and plan ([`0010a11`](https://github.com/s0undt3ch/ToolR/commit/0010a111220f43db540eb5ed2910ff36c5bdc99a))
- *(specs)* Describe the shipped static mirror in the command_group aliases design ([`8087eb1`](https://github.com/s0undt3ch/ToolR/commit/8087eb1f20b0378d7c928908febeaba4b899e3a6))
- *(specs)* Spec and plan for the inherited SIG_IGN signal-test flake ([`071b4c8`](https://github.com/s0undt3ch/ToolR/commit/071b4c864ccc0eb4aec908a858572e98f50483c4))
- *(specs)* Archive the inherited SIG_IGN signal-test spec ([`53499b3`](https://github.com/s0undt3ch/ToolR/commit/53499b3cd8a519dc4f2b435dfe5e5a1e798e4645))
- *(tests)* Say why the signal tests reset SIGINT, sync the plan ([`e16bcfd`](https://github.com/s0undt3ch/ToolR/commit/e16bcfdcd439c3ec69362a63daacf908850a9c20))
- *(specs)* Spec and plan for the stale _context stub ([`fd63d7f`](https://github.com/s0undt3ch/ToolR/commit/fd63d7f1675182a9ae59872ee03eeced2e246a4b))
- *(specs)* Archive the stale _context stub spec ([`0b42fba`](https://github.com/s0undt3ch/ToolR/commit/0b42fbadae09e3c0a480e737aeba7630b383ed75))
- *(specs)* Spec and plan for empty XDG_DATA_HOME handling ([`697c391`](https://github.com/s0undt3ch/ToolR/commit/697c391e7fbd78f0e857e1e3e96ccbf651c22c19))
- *(specs)* Archive the empty XDG_DATA_HOME spec ([`a801e4c`](https://github.com/s0undt3ch/ToolR/commit/a801e4c34d9d353afc67e97e0fbf65b2b47472d9))
- An empty XDG_* variable counts as unset ([`e8cbf21`](https://github.com/s0undt3ch/ToolR/commit/e8cbf21c8085a0af39af3ed33a463ae3117a1ede))
- *(env)* Explain the empty-means-unset reads ([`584e6dc`](https://github.com/s0undt3ch/ToolR/commit/584e6dc7508674a90249c589e0320c45cddc73ea))
- *(specs)* Spec and plan for bash 5.3 compgen dequoting ([`dc49dc1`](https://github.com/s0undt3ch/ToolR/commit/dc49dc16ae296cc8e125bdc41dc1cd3bcafca200))
- *(specs)* Record the bash compgen dequoting decisions ([`7f50aba`](https://github.com/s0undt3ch/ToolR/commit/7f50aba948163ffd11198882243bc40cad6f2e0a))
- *(specs)* Archive the bash compgen dequoting spec ([`c278a30`](https://github.com/s0undt3ch/ToolR/commit/c278a300b2b4f6532df99b3229b845e9b3c881a4))
- *(complete)* Sync the bash spec and release note with what shipped ([`2fd15b1`](https://github.com/s0undt3ch/ToolR/commit/2fd15b10e4af81a68a69e47b5fecfb2d3e48e766))
- *(specs)* Spec and plan for completion after -- and dash-leading choices ([`6b46a4a`](https://github.com/s0undt3ch/ToolR/commit/6b46a4a411444768d51c0a82fdee9bac658d9522))
- *(specs)* Archive the completion after -- spec ([`7b0b7d4`](https://github.com/s0undt3ch/ToolR/commit/7b0b7d4c7ecdbaa0bf0231d4f86340c52155b9d9))
- *(specs)* Spec and plan for unsupported command_group binding forms ([`7876829`](https://github.com/s0undt3ch/ToolR/commit/787682933991105ca696f6a3963e60912e338e89))
- *(specs)* Archive the command_group binding forms spec and plan ([`0ebc6e0`](https://github.com/s0undt3ch/ToolR/commit/0ebc6e03a595da04e448b1558df06a270798239d))
- Command_group creates groups, so no call is a harmless lookup ([`72ed711`](https://github.com/s0undt3ch/ToolR/commit/72ed711f7427928c0a81fb31de83cdc76fb944e8))
- One statement of the command_group placement rule ([`3ad4e5b`](https://github.com/s0undt3ch/ToolR/commit/3ad4e5b6d01221f6dacb396d9fffd36d0d86b6a2))
- *(specs)* Spec and plan for --quiet completion install ([`8f3bb5e`](https://github.com/s0undt3ch/ToolR/commit/8f3bb5e24dad1b58079b92e7f855ef43670964a0))
- *(specs)* Archive quiet-completion-install spec and plan ([`e37b453`](https://github.com/s0undt3ch/ToolR/commit/e37b453667b847b71da81c9951847a1d26f9353a))
- *(specs)* Spec and plan for empty and relative env values ([`c00de79`](https://github.com/s0undt3ch/ToolR/commit/c00de79fd69708c6f26ba610c66d46794a6f7025))
- *(specs)* Archive empty and relative env values specs ([`e568c73`](https://github.com/s0undt3ch/ToolR/commit/e568c736f9605ebcbebcebe1432643713b386f23))
- XDG dirs count only when set to an absolute path ([`5931071`](https://github.com/s0undt3ch/ToolR/commit/5931071dd272fa84ae16d0e7d57bcf7a8ffdff59))
- *(env)* Say which path reader to use, fix two stale doc comments ([`63f0717`](https://github.com/s0undt3ch/ToolR/commit/63f071769abdbe7da60a7d488ab2069beeef579b))
- *(specs)* Spec and plan for root --quiet in the other built-ins ([`6f0b7aa`](https://github.com/s0undt3ch/ToolR/commit/6f0b7aa3720d0cd5f97e041d847ce9bb75a06c59))
- *(specs)* Archive the root --quiet specs ([`593f1c9`](https://github.com/s0undt3ch/ToolR/commit/593f1c9d41af74d1b522804c558ef8e606f0b089))
- *(specs)* Drop a stray private check from two archived plans ([`7ff3dd4`](https://github.com/s0undt3ch/ToolR/commit/7ff3dd4dc6596f2ec90d0495b9dd9ea7d8ade912))
- *(specs)* Design for trust keyed on the git common dir ([`e361151`](https://github.com/s0undt3ch/ToolR/commit/e3611513605e56e43beb0a411f1557b1da05dd2d))
- *(specs)* Close crafted-gitdir holes in the trust design ([`b0c4b98`](https://github.com/s0undt3ch/ToolR/commit/b0c4b98e8d52b5683e3b79c21e60dd05ebe21a54))
- *(specs)* Stop the trust walk at $HOME ([`d11a5c5`](https://github.com/s0undt3ch/ToolR/commit/d11a5c5c899cec96214ee307907c2831e5651941))
- *(specs)* Fold the independent review into the trust design ([`46cedfd`](https://github.com/s0undt3ch/ToolR/commit/46cedfde8d9fc0241994e90f39f3a5e55577b62c))
- *(specs)* State that trust imposes no worktree layout ([`f06ecf3`](https://github.com/s0undt3ch/ToolR/commit/f06ecf376ed5d46c6c1911d87e9fcfa794811e55))
- *(specs)* Apply the second review of the trust design ([`5929828`](https://github.com/s0undt3ch/ToolR/commit/5929828045ced0c6220fa5c3c5788feaee537c36))
- *(specs)* Plan for trust keyed on the git common dir ([`6e0ca43`](https://github.com/s0undt3ch/ToolR/commit/6e0ca43f95fc96c66a910da3677fcc113946aaac))
- *(specs)* Drop a repo-wide rule from the trust plan ([`b6eb7be`](https://github.com/s0undt3ch/ToolR/commit/b6eb7beb753a25215bbcde5190c53d1e141bbb5c))
- *(specs)* Separate key kinds and narrow the repair hint ([`d3522b5`](https://github.com/s0undt3ch/ToolR/commit/d3522b5a131bc9dc3638a48435b0bc742d2236f5))
- *(trust)* Describe repository-wide trust and its worktrees ([`4e5fd29`](https://github.com/s0undt3ch/ToolR/commit/4e5fd29c749f89b840e2210860b7265688faccbb))
- *(specs)* Name the worktree top in a cautious repair hint ([`8dca40a`](https://github.com/s0undt3ch/ToolR/commit/8dca40a04d141af6fd2d485049aba23e529883aa))
- *(trust)* Restore the same-path caveat for git repositories ([`49c9bda`](https://github.com/s0undt3ch/ToolR/commit/49c9bda7d62662b70522f7ff97359608257b8bcd))
- *(specs)* Archive the trust common-dir spec and plan ([`b74d4de`](https://github.com/s0undt3ch/ToolR/commit/b74d4deca9c7d56fe1a7a5f531fbb2dc28d90c08))
- *(agents)* Always run checks through prek ([`6483e23`](https://github.com/s0undt3ch/ToolR/commit/6483e2308d548812476ae4d42cf2e4ca8e96ab48))
- *(specs)* Design and plan for the execute-timeout test flakes ([`6e053ac`](https://github.com/s0undt3ch/ToolR/commit/6e053accafff1ad6fef6dddf4bf575d87eb58a59))
- *(specs)* Archive the execute-timeout flakes specs ([`de0df27`](https://github.com/s0undt3ch/ToolR/commit/de0df27ef62b5c4e635c7e2984e4132fd15a37b5))
- *(specs)* Record the pid file, escaped pattern and collision proof ([`7092fda`](https://github.com/s0undt3ch/ToolR/commit/7092fda9795c46fca2e8ad481b4f15198050bccf))
- *(specs)* Spec and plan for bash ANSI-C quoted words ([`6376441`](https://github.com/s0undt3ch/ToolR/commit/6376441fd7b1ef204c7243084b7374576ca1d630))
- *(specs)* Record implementation findings and archive bash ANSI-C words spec ([`937119b`](https://github.com/s0undt3ch/ToolR/commit/937119bac43c998f7c68a053b749fdc8d5a5c440))
- *(specs,release-notes)* Mark superseded spec parts, note quote-bearing names ([`2c64dbd`](https://github.com/s0undt3ch/ToolR/commit/2c64dbd215a13d44c509dff8328915e24c72833f))
- *(specs)* Spec and plan for zsh and fish quoted partials ([`6a77e77`](https://github.com/s0undt3ch/ToolR/commit/6a77e77efd53df29f371208b2caa9efbe20eae10))
- *(specs)* Settle the zsh and fish quoted partials open questions ([`cccb2c3`](https://github.com/s0undt3ch/ToolR/commit/cccb2c3b7d564742c1067c15397f705941fe1b3f))
- *(specs)* Archive the zsh and fish quoted partials spec ([`f19b576`](https://github.com/s0undt3ch/ToolR/commit/f19b576c1df173a29e7cfaf96d583d9a30d66b32))
- *(complete)* Describe the shipped zsh and fish quoting ([`5755b8a`](https://github.com/s0undt3ch/ToolR/commit/5755b8ad093ab927751988ab8b29921ed78bcaf0))
- *(specs)* Spec and plan for root flags before the first group ([`0a44b20`](https://github.com/s0undt3ch/ToolR/commit/0a44b20eb5952c8f2214363813d7130819fd4f09))
- *(specs)* Resolve root-flags open questions against main ([`b6bd638`](https://github.com/s0undt3ch/ToolR/commit/b6bd638a9866677deb80f336841e9079cfc3644c))
- *(specs)* Archive root-flags-after-root-flag spec and plan ([`3522bb1`](https://github.com/s0undt3ch/ToolR/commit/3522bb1fbbec5ba38fd34f3bb64ae4e3effa2164))
- *(complete)* Update the root-flags spec and release note after review ([`90d1e4c`](https://github.com/s0undt3ch/ToolR/commit/90d1e4cb9c5a5aac692ebe5c23fdef7677c7bee7))
- *(specs)* Spec and plan for quiet-cache-prune ([`39336a6`](https://github.com/s0undt3ch/ToolR/commit/39336a67eea519d86952593d1514350b24375c9d))
- *(specs)* Record the -q warnings decision (keep warnings everywhere) ([`0b5343a`](https://github.com/s0undt3ch/ToolR/commit/0b5343ac64996b7c93ba70c1707a02a4ae42fd6c))
- *(specs)* Quiet-cache-prune is written against main now that #695 merged ([`11b9c54`](https://github.com/s0undt3ch/ToolR/commit/11b9c5489585c8b144448340e53a6b25cde3ac88))
- *(specs)* Archive the quiet-cache-prune spec and plan ([`60c810b`](https://github.com/s0undt3ch/ToolR/commit/60c810b2d230fcbd72ed0cd6ecb786d0f1fd9af0))
- *(specs)* Spec and plan for zsh autoload first tab ([`ff2cffa`](https://github.com/s0undt3ch/ToolR/commit/ff2cffaa09e217b1acf6f25a2614453b7889b871))
- *(specs)* Archive zsh autoload first tab spec ([`f002fad`](https://github.com/s0undt3ch/ToolR/commit/f002fadffef3dc39d52ea154b886e346597a488a))
- *(specs)* Spec and plan for zsh matcher-list completion matching ([`37fefb1`](https://github.com/s0undt3ch/ToolR/commit/37fefb108a896d0402c6a4b799b694af674bc2eb))
- *(specs)* Archive the shell-matching design ([`50a5f88`](https://github.com/s0undt3ch/ToolR/commit/50a5f88cd4d79983198874c0aeffa0b8dcf7e8f2))
- *(cli)* Bash filters the completion reply by prefix itself ([`ed5ca88`](https://github.com/s0undt3ch/ToolR/commit/ed5ca88fc259d068f085a5e5757748be5fc678ee))
- *(specs)* Correct the zsh matcher-list spec after implementation ([`f685c7f`](https://github.com/s0undt3ch/ToolR/commit/f685c7f1dc321bee4af8059343bcd351aaca0530))
- *(complete)* Trim shell script comments and say complete prefix-matches ([`43844bf`](https://github.com/s0undt3ch/ToolR/commit/43844bfa87434699f97d25649366212778ab98c8))
- *(specs)* Spec and plan for powershell completion ([`be6c6d6`](https://github.com/s0undt3ch/ToolR/commit/be6c6d68d0d48e5f4af84c9ac9e40d6e21b44d75))
- *(specs)* Record the user's powershell completion decisions ([`a3b307a`](https://github.com/s0undt3ch/ToolR/commit/a3b307a154bdb3eca1a053416a042055c19c2528))
- *(completion)* Document powershell completion ([`7e23230`](https://github.com/s0undt3ch/ToolR/commit/7e23230bcd122878813951226bd5ff5be49f95ea))
- *(specs)* Archive the powershell completion spec ([`cbe5e97`](https://github.com/s0undt3ch/ToolR/commit/cbe5e97c3bd0a22eaa97fd1da7b021d423581c56))
- *(specs)* Describe the shipped powershell path completion and known limits ([`ea28ff9`](https://github.com/s0undt3ch/ToolR/commit/ea28ff9f142d9039d966b998c5e99c624e48d8a7))
- *(specs)* Add llms.txt design and plan ([`9d96ee8`](https://github.com/s0undt3ch/ToolR/commit/9d96ee8edaee2ea37b9bcc2919eb095e4ae377ca))
- *(skills)* Point agents without skills at llms.txt ([`6f48f03`](https://github.com/s0undt3ch/ToolR/commit/6f48f03470962941cf43998fa738c5d097f23df3))
- *(specs)* Add agent discovery of llms.txt to the design and plan ([`5eeb246`](https://github.com/s0undt3ch/ToolR/commit/5eeb246b70502073b672d118d05396dd7d9e05a4))
- Link llms.txt from the README and PyPI ([`b7aa724`](https://github.com/s0undt3ch/ToolR/commit/b7aa7245f7a3ce4531f5aa44fd876cbd79b49736))
- *(specs)* Archive llms.txt design and plan ([`a38521c`](https://github.com/s0undt3ch/ToolR/commit/a38521c0d3b58bdd7d75136b7871d428f36c2046))
- *(specs)* Spec and plan zsh listings that show names ([`d1a758c`](https://github.com/s0undt3ch/ToolR/commit/d1a758cd36f1223522c78aa52f7d67f87ba68b1f))
- *(specs)* Archive zsh listing names spec and plan ([`e56d74d`](https://github.com/s0undt3ch/ToolR/commit/e56d74d03fc974ed79b2d201ad5758fe4fc0f65e))
- *(specs)* Link PR #721 in the archived design ([`c27722e`](https://github.com/s0undt3ch/ToolR/commit/c27722e969dd829cde897d8a72325e7451dad089))
- *(specs)* Say the zsh listing width comes from the matched candidates ([`869d15c`](https://github.com/s0undt3ch/ToolR/commit/869d15c083189386135784ef1d1d06ed990c9462))
- *(specs)* Audit quick deletions design and plan ([`93344d5`](https://github.com/s0undt3ch/ToolR/commit/93344d5f924502e4c1097c0628d3c62d73e583f6))
- *(specs)* Archive audit quick deletions spec ([`c15f2b7`](https://github.com/s0undt3ch/ToolR/commit/c15f2b7f29631d6f98ba812e502ed05cbcd056bb))
- *(specs)* Record the reverted LTO change and the unlocked set_var ([`04c8251`](https://github.com/s0undt3ch/ToolR/commit/04c8251a2e69b73793a4ffd820a3639b0dc5b32d))
- *(uv)* Unlink functions narrowed to pub(crate) from public docs ([`ebdf94e`](https://github.com/s0undt3ch/ToolR/commit/ebdf94e9f3adab3615cabff24ac7a5585a3545e5))
- *(specs)* Add startup-latency design and plan ([`710bb69`](https://github.com/s0undt3ch/ToolR/commit/710bb69c55933873225245bc86ddee5a60c2da6c))
- *(specs)* Archive startup-latency spec ([`bacb139`](https://github.com/s0undt3ch/ToolR/commit/bacb13992e5dab9b49ea1a5bf221147772717860))
- *(internals)* Describe the passive orphan hint and pin that it skips the size walk ([`ebe5cf8`](https://github.com/s0undt3ch/ToolR/commit/ebe5cf82e9d5aea1e976a0899a35b684e9c7f61b))
- *(specs)* Design and plan for the docs drift and release guard work ([`9209708`](https://github.com/s0undt3ch/ToolR/commit/9209708c165c41726c337a97e4af0ae8645f2ee8))
- Correct pages that drifted from the code ([`35307cc`](https://github.com/s0undt3ch/ToolR/commit/35307cce7a1292fd37c75b4b4c206ee779772ded))
- *(specs)* Archive audit-docs-drift spec ([`8c5d956`](https://github.com/s0undt3ch/ToolR/commit/8c5d9562a9e83d6b276183374ad7bda77f31bece))
- *(contributing)* Name the real release artifacts in the recovery recipe ([`4345a71`](https://github.com/s0undt3ch/ToolR/commit/4345a712789b82ad3392e84c0b0e8313dca6e2c1))
- Restructure the release recovery checklist and tidy the review follow-ups ([`dbbc60c`](https://github.com/s0undt3ch/ToolR/commit/dbbc60cb2659d47481841a23d3d05feff1e7167f))
- *(specs)* First-run hardening design and plan ([`2ee9eba`](https://github.com/s0undt3ch/ToolR/commit/2ee9eba2e6321f1ddd7f9bf958780b2f9fbcb19e))
- *(specs)* Archive first-run-hardening spec ([`18c10a5`](https://github.com/s0undt3ch/ToolR/commit/18c10a557db17d75a6fc1500fc3ccb41131fad6f))
- *(specs)* Design and plan for audit test honesty ([`b2bb114`](https://github.com/s0undt3ch/ToolR/commit/b2bb1146fa89fc41eff443d0ad053a3da99f94bc))
- *(specs)* Archive audit-test-honesty spec ([`d8275e3`](https://github.com/s0undt3ch/ToolR/commit/d8275e361fffccc3631914fa54b8a7ae04b308ef))
- *(specs)* Design and plan for CI de-duplication ([`61099e2`](https://github.com/s0undt3ch/ToolR/commit/61099e25af3fe7e63942e62f2a8f1be862e5b961))
- *(specs)* Archive audit-ci-dedup spec ([`a699f0a`](https://github.com/s0undt3ch/ToolR/commit/a699f0ad4cb2908e19bdbbc5a170f2e2b3d10136))
- *(specs)* Design and plan one manifest rebuild pipeline ([`f4ef1f7`](https://github.com/s0undt3ch/ToolR/commit/f4ef1f73e9239ef27893ed59d93830e6281b546c))
- *(specs)* Record line counts and the flattened build errors ([`b4e7eb7`](https://github.com/s0undt3ch/ToolR/commit/b4e7eb7403b53ecfe36e28d1666978fdf3b3ee7c))
- *(specs)* Archive audit-manifest-pipeline spec ([`1f40deb`](https://github.com/s0undt3ch/ToolR/commit/1f40deb23e73e0b84142b4b2c04d179a7609c00a))
- *(specs)* Design and plan for the 1.0 surface cleanup ([`7ea93ee`](https://github.com/s0undt3ch/ToolR/commit/7ea93eee91a098cf7600bc18d8fb86e89b34b2c7))
- *(specs)* Archive audit-1-0-surface spec ([`c433c18`](https://github.com/s0undt3ch/ToolR/commit/c433c18b8e1912b973301f8de456c0270e5c0230))
- *(installation)* Document --verify instead of the old installer flags ([`3121537`](https://github.com/s0undt3ch/ToolR/commit/312153772bdca2feffaf9e190f26cf082b87577c))
- Record accepted audit findings (trust, Windows, completion) ([`5470cd9`](https://github.com/s0undt3ch/ToolR/commit/5470cd94fa1644450cee3df484461ffe8bf46c20))
- Do not overstate short-alias completion ([`e744d8e`](https://github.com/s0undt3ch/ToolR/commit/e744d8e8901a78cb3e2a68bda5b7e3a62c94ab58))
- *(specs)* Design and plan for argv and dispatch consolidation (audit WP8) ([`ebf5357`](https://github.com/s0undt3ch/ToolR/commit/ebf535769931fa7e1dd68231aaa6762fdc905ed2))
- *(specs)* Archive the argv and dispatch consolidation spec ([`4b9744d`](https://github.com/s0undt3ch/ToolR/commit/4b9744dd86149ddbba69d97a84cbe265cf0a672b))
- Point stale dispatch.rs references at the split modules ([`c843019`](https://github.com/s0undt3ch/ToolR/commit/c8430198b985013fb9f58256783b3cd425c2b4b2))
- *(specs)* Design for ignoring the manifest cache before trust ([`839eec5`](https://github.com/s0undt3ch/ToolR/commit/839eec5f7602cebf878d596efc132fefae2eebff))
- *(specs)* Untrusted repos show only built-ins; in-tree venvs need a token ([`f65bfb0`](https://github.com/s0undt3ch/ToolR/commit/f65bfb097e3d9b17b9b22f2fe253c68ee2d9180d))
- *(specs)* Plan for trusting nothing in an untrusted repository ([`0d7bdb7`](https://github.com/s0undt3ch/ToolR/commit/0d7bdb786744c57a4268fc8d77fe8e8507178cd3))
- *(trust)* Untrusted repositories show only built-ins ([`3378612`](https://github.com/s0undt3ch/ToolR/commit/3378612a8d464624e2ed599fd69611487029bd84))
- *(trust)* Name what toolr reads before trust, and trust for hooks ([`4980960`](https://github.com/s0undt3ch/ToolR/commit/4980960c80ef2497d0f6708351d9bde9b1081b91))
- *(specs)* Archive the untrusted-repository specs ([`c8fcb5a`](https://github.com/s0undt3ch/ToolR/commit/c8fcb5aead5980610145ef4074e9b80339580de7))
- *(trust)* Say when toolr replaces an in-tree venv it didn't build ([`7a1a6b4`](https://github.com/s0undt3ch/ToolR/commit/7a1a6b43eaa903392d36d770a1a5b6a95306e263))
- *(trust)* Say what the venv token proves and what TOOLR_TRUST_ALL trusts ([`50d86c3`](https://github.com/s0undt3ch/ToolR/commit/50d86c3d4af82aadba0f211b46f9d3524d91ce47))

### <!-- 4 -->⚡ Performance

- *(freshness)* Reread only static inputs whose stat changed ([`168ba1b`](https://github.com/s0undt3ch/ToolR/commit/168ba1b8c526201ca5d9abe65a1331dd16449f7e))
- *(cache)* Count orphans only in the passive hint and skip it for --version/--help ([`ab4aa94`](https://github.com/s0undt3ch/ToolR/commit/ab4aa941ef591d9c60b508dcd62b8bfdf5bddd68))
- *(py)* Import rich.prompt, packaging and importlib.metadata lazily ([`2726bed`](https://github.com/s0undt3ch/ToolR/commit/2726bed0854b5d819d1eab7e8f6a8cdace626290))
- *(trust)* Read the trust store only for a run that needs it ([`6b8f724`](https://github.com/s0undt3ch/ToolR/commit/6b8f7242c08e69f39a5498d5a3d5a4d0b66bebb6))

### <!-- 5 -->🎨 Styling

- *(rust)* Apply rustfmt 2024 style and enforce it in pre-commit ([`d400d13`](https://github.com/s0undt3ch/ToolR/commit/d400d132fbc8d3ecd060d581ba92b5587bfd3fda))
- *(security)* Apply edition 2024 formatting and let-chains to repo trust ([`2313566`](https://github.com/s0undt3ch/ToolR/commit/23135669b91cec64bc467f920fa34fb42f3220a7))

### <!-- 6 -->🧪 Testing

- *(uv)* Name the EOF test after what it asserts ([`246ad19`](https://github.com/s0undt3ch/ToolR/commit/246ad196cebc37f2a00f661aeb3198f1cd865a09))
- *(uv)* Cover the interactive install prompt ([`09e46a4`](https://github.com/s0undt3ch/ToolR/commit/09e46a4fb9b1092185a20c81ad27075b1dc21e59))
- *(execute)* Reject a patch-level requires-python floor in the lockstep gate ([`699c0d9`](https://github.com/s0undt3ch/ToolR/commit/699c0d9685f12d8e9df0cfb88f3ed312fa4a371d))
- *(project)* Pin requires-python detection against the real python3 ([`2f74242`](https://github.com/s0undt3ch/ToolR/commit/2f74242e3772067b521daa124d2200c267288548))
- *(project)* Fold the python3 match check into one assertion ([`a3c9679`](https://github.com/s0undt3ch/ToolR/commit/a3c9679d8c47740ad391e6e532d14d5ddd12873a))
- *(uv)* Match the binary name per platform and pin traversal outcomes ([`262664d`](https://github.com/s0undt3ch/ToolR/commit/262664d71a01ed5c852e8d46f96b785d11ebd091))
- *(uv)* Assert the cause chain, not library wording ([`0c272fc`](https://github.com/s0undt3ch/ToolR/commit/0c272fc0b0a34e7d14d46ccf02ffc6efeb81e8bb))
- *(uv)* Assert extract errors with one matches! check ([`54bcbd5`](https://github.com/s0undt3ch/ToolR/commit/54bcbd5a5d90890159d1614df24ebf22e045d1b8))
- *(cli)* Cover boolean flag rules through the binary ([`c529b41`](https://github.com/s0undt3ch/ToolR/commit/c529b417874a6e9d40a35ec94c20b61376341adf))
- *(cli)* Check a bool alias flag reads its env var ([`1a3c5f1`](https://github.com/s0undt3ch/ToolR/commit/1a3c5f1e4646f317b40deefda0e3278e08082a60))
- *(core)* Serialise env-mutating unit tests on one crate-wide lock ([`4a7213a`](https://github.com/s0undt3ch/ToolR/commit/4a7213acfa16b8781e12dc2776d84e1811b0b589))
- *(core)* Test the uv PATH scan without mutating PATH ([`8bbf086`](https://github.com/s0undt3ch/ToolR/commit/8bbf0866864be819adab1e796600744844f1a463))
- *(core)* Make tests that read mutated env vars take the env lock ([`960b288`](https://github.com/s0undt3ch/ToolR/commit/960b28869416befaf8a8f79025ab34b02d90191a))
- *(core)* Keep unit tests out of the real toolr cache ([`6ddb8b6`](https://github.com/s0undt3ch/ToolR/commit/6ddb8b6616769d9f9a238bfa5e010652be6c833e))
- *(core)* Take the env read lock in the uv consent prompt tests ([`68d75b5`](https://github.com/s0undt3ch/ToolR/commit/68d75b504f1adb549a221cfe8714be85b3d85fbb))
- Keep cargo-launched toolr out of the real trust store ([`94a1155`](https://github.com/s0undt3ch/ToolR/commit/94a11550969d1ec628cf36203daf782be34594ce))
- *(security)* Cover trust store failure paths and run commands on Windows ([`efaa190`](https://github.com/s0undt3ch/ToolR/commit/efaa190f736031f79954f43ec27e335b18eba4e2))
- Turn the trust check off per test, not for every cargo process ([`ad13535`](https://github.com/s0undt3ch/ToolR/commit/ad135359e067582454f8e3cb91258fdc07a3f28c))
- *(parser)* Render every invalid-name item kind through real builds ([`e29e130`](https://github.com/s0undt3ch/ToolR/commit/e29e130a189503862d143d4244919a089786cafd))
- *(cli)* Cover control characters in metavar and help-section title ([`ccb5b22`](https://github.com/s0undt3ch/ToolR/commit/ccb5b22da088e85cfec2716144e3cc18f024e8df))
- *(plugins)* Cover the glob escape error and dot-component paths ([`66ecfb7`](https://github.com/s0undt3ch/ToolR/commit/66ecfb78edb89ab58855047e18ee316aeaef4e88))
- *(runner)* Cover the remaining TYPE_CHECKING rescue paths ([`b60141d`](https://github.com/s0undt3ch/ToolR/commit/b60141dec2dd9a99aa26ab5d57921c86850a5b1e))
- *(command)* Bound the stdin deadlock regression test from outside ([`2e22731`](https://github.com/s0undt3ch/ToolR/commit/2e227314d12ddd593a66fec6e0cb12aacce924dd))
- *(command)* Make the stdin deadlock tests fail instead of hang ([`e0cd895`](https://github.com/s0undt3ch/ToolR/commit/e0cd89584b1fe7515231af7fe93dfe9701939c84))
- *(release)* Prove the strict prek pass is what fails the commit step ([`75d0f8f`](https://github.com/s0undt3ch/ToolR/commit/75d0f8ff9d4ddcc491810c66a14df9b3aeb5c452))
- *(action)* Simplify the resolve-version harness ([`5cd23f3`](https://github.com/s0undt3ch/ToolR/commit/5cd23f3679a18e3c4dd533432f16eefb1d7a9f79))
- *(testing)* Cover a tools package that raises on import ([`bc85737`](https://github.com/s0undt3ch/ToolR/commit/bc85737a08b2a57b847a0dc9182c6032a064ba9d))
- *(cli)* Relative path defaults from a subdirectory ([`2b72913`](https://github.com/s0undt3ch/ToolR/commit/2b729139ad08d05d397f288724fccd506f1ded12))
- *(cli)* Run path default fixtures under an in-tree stand-in venv ([`39508b5`](https://github.com/s0undt3ch/ToolR/commit/39508b5310ef1e34166babacd99149fc306e4f32))
- *(cli)* Pin the deferred path parser and extraction pairing ([`c1ae6fa`](https://github.com/s0undt3ch/ToolR/commit/c1ae6fa9495ad7532c463fcef0b2a514a833260b))
- *(cli)* Share one path-default fixture and run helper ([`72f3cd7`](https://github.com/s0undt3ch/ToolR/commit/72f3cd75ba42f809b321a3646cbd19c097848844))
- *(e2e)* Reproduce double SIGINT delivery to commands ([`90e33ea`](https://github.com/s0undt3ch/ToolR/commit/90e33ea91e2afe66f5fd23935cc369db9638f131))
- *(e2e)* Parametrise SIGINT senders and name signal test timings ([`fc52be1`](https://github.com/s0undt3ch/ToolR/commit/fc52be17f5dbd5f7276d854a8c9ce8d2adc4d4b2))
- *(runner)* Assert SIGINT classification without a platform-only branch ([`e33d197`](https://github.com/s0undt3ch/ToolR/commit/e33d19736bf91a26de3da829dcfbce1c1958974e))
- *(uv)* Planted repo uv and python3 never run through relative PATH entries ([`9a741eb`](https://github.com/s0undt3ch/ToolR/commit/9a741eb68c13169d9546777a8e809482b9790816))
- *(uv)* Plant an ldd behind a relative PATH entry ([`01bc93d`](https://github.com/s0undt3ch/ToolR/commit/01bc93dbc12b7985408cc507ba372d1f3a950753))
- *(ci)* Check every Python version list against requires-python ([`3934cf0`](https://github.com/s0undt3ch/ToolR/commit/3934cf04bebb948639536601feebb4c3b8f1b47f))
- *(ci)* Read version-list sources as UTF-8 ([`76288b1`](https://github.com/s0undt3ch/ToolR/commit/76288b10a512889c75f75a76c7f24b86e930dae3))
- *(ci)* Check the remaining hard-coded CPython floors ([`213834a`](https://github.com/s0undt3ch/ToolR/commit/213834a32e5b4f2172feafe87f8931fae71c13ff))
- *(ci)* Reject ambiguous requires-python bounds and check the example floors ([`d610a4a`](https://github.com/s0undt3ch/ToolR/commit/d610a4a09bf6fec4e7d8687d4f58b93ddeda3c21))
- *(ci)* Fail only the floor case whose requires-python is malformed ([`26b2162`](https://github.com/s0undt3ch/ToolR/commit/26b21620d6f2a912a6782357222d6ef6a89d6ab6))
- *(registry)* Cover @command(name) without group= recording nothing ([`2ac1942`](https://github.com/s0undt3ch/ToolR/commit/2ac194292b55fb98b11cbea12bba97d9b66bd819))
- *(registry)* Make the pending-store leak test able to fail ([`739d9b7`](https://github.com/s0undt3ch/ToolR/commit/739d9b7e6e97f0fb5f6b39823f0cbb91b8ed919d))
- *(parser)* Cover symlinked modules whose target can't be stat'd ([`ef1cb7d`](https://github.com/s0undt3ch/ToolR/commit/ef1cb7d6d861ad3e295f7db950dfbe638bb1615f))
- *(cli)* Share the symlinked-module CLI assertions ([`51a8bab`](https://github.com/s0undt3ch/ToolR/commit/51a8bab6fec207a8133f128f778e8d51502dba29))
- *(runner)* Share the import-failure fixture files and scope module eviction ([`36bb215`](https://github.com/s0undt3ch/ToolR/commit/36bb21581a92cb08acd02c1d54ef9c482ddc7f6b))
- *(dispatch)* Table-drive the meta.json touch warning policy ([`4032295`](https://github.com/s0undt3ch/ToolR/commit/4032295e2127bad9e80916b1d6418da0d0c17651))
- *(testing)* Pin the ContextForTesting stream split across verbosity levels ([`3337b62`](https://github.com/s0undt3ch/ToolR/commit/3337b620626081fb43b59cb9aa51e13be09ac283))
- *(testing)* Drive the stream-split test with RunMock ([`28bed8b`](https://github.com/s0undt3ch/ToolR/commit/28bed8bbd43bff0ed0b2c24de10ae0046c9b04f7))
- *(registry)* Contain tester leaks and trim the sys.path filter tests ([`b10bde1`](https://github.com/s0undt3ch/ToolR/commit/b10bde14f4f6fd889c0e8e7775b5840c4ca7a092))
- *(distribution)* Compare the binary and wheel versions as parsed versions ([`c4c8f13`](https://github.com/s0undt3ch/ToolR/commit/c4c8f13e0d0466b1a30065c6f971ea41ff5d5b4e))
- Share the git repository helpers between test modules ([`be05554`](https://github.com/s0undt3ch/ToolR/commit/be055547217b88f9763753fc9077dbb178bb8c4f))
- *(release)* Cover the version bump write path ([`1bf8a50`](https://github.com/s0undt3ch/ToolR/commit/1bf8a50b9bcbe94fa9ffda122c18654c3916f528))
- *(command)* Cover the post-kill grace for an escaped pipe holder ([`437e75f`](https://github.com/s0undt3ch/ToolR/commit/437e75f60cf2571a73eb63968c44419e148cb3b7))
- *(command)* Reproduce the sh grandchild shape and reap test leftovers ([`cbb71da`](https://github.com/s0undt3ch/ToolR/commit/cbb71dab394ddd0e1bbd8dc2f873bd665163a389))
- *(command)* Fold the ctx.run tree tests into shared helpers ([`8fc6491`](https://github.com/s0undt3ch/ToolR/commit/8fc64916010d8a33e4980d05269a0e08dc5d87d3))
- *(command)* Prove the timed-out grandchild was alive before it died ([`862d10e`](https://github.com/s0undt3ch/ToolR/commit/862d10ef91d5c843572f747b1adcb5925919b5e8))
- *(uv)* Drop offline install tests that duplicate unit tests ([`81aeb40`](https://github.com/s0undt3ch/ToolR/commit/81aeb40c59d6491c739484fff2cf058a6272c510))
- *(parser)* Compare the whole string-path decorator in the conflict test ([`8f773c2`](https://github.com/s0undt3ch/ToolR/commit/8f773c2766e5d7fff12be845156c113de7d2c48d))
- *(trust)* Assert dispatch outcomes and stop pty runs that hang ([`a3b8b79`](https://github.com/s0undt3ch/ToolR/commit/a3b8b79236d6163aa5faa4cb2190441ae121479f))
- *(trust)* Share the pty harness and its deadline across prompt tests ([`436b1a6`](https://github.com/s0undt3ch/ToolR/commit/436b1a650bc74ad0baeaedd9be447d93bac4a188))
- *(action)* Pass the archive path in POSIX form, as the action's download step does ([`d723e29`](https://github.com/s0undt3ch/ToolR/commit/d723e296053f05c2e38b8539a1a4737966a63429))
- *(venv)* Fold the root check into the chmod helpers ([`7eb7420`](https://github.com/s0undt3ch/ToolR/commit/7eb742016e7eb024057992bb39c687da66075815))
- *(manifest-build)* Pin third_party_hash for uv and Windows venv layouts ([`b0174cb`](https://github.com/s0undt3ch/ToolR/commit/b0174cb7393f7c10b89945afe9d3c04ac64fe861))
- *(ci)* Reject conditional guards and TOOLR_VERSION overrides, inherit env for bash ([`5573b7b`](https://github.com/s0undt3ch/ToolR/commit/5573b7b162083b0694265d3a0f3d744f2f2bb3ae))
- *(ci)* Compare workflow paths in posix form so the wrapper check passes on Windows ([`8518fdb`](https://github.com/s0undt3ch/ToolR/commit/8518fdbb3e32dcc272593241d54cb102600778a5))
- *(installation)* Pin install.ps1's lowercase archive digest ([`1d973b1`](https://github.com/s0undt3ch/ToolR/commit/1d973b1dce9a65b0bf52f4cd0fdaf5b8a9b92289))
- *(installation)* Share the gh attestation stub with the action's tests ([`848c0cd`](https://github.com/s0undt3ch/ToolR/commit/848c0cde98f44d1e2dbd97727cb4afdc0c053266))
- *(installation)* Check the gh floor everywhere and assert install.ps1's behaviour ([`d13401e`](https://github.com/s0undt3ch/ToolR/commit/d13401e95ce305d190645a6f46f2f04c16f79d0a))
- *(installation)* Write the gh stub and scripts so Git Bash on Windows can run them ([`c12ec2d`](https://github.com/s0undt3ch/ToolR/commit/c12ec2d008e3e033c617a16919143e05f18d8454))
- *(freshness)* Cover a pyproject edit outside the argparse blocks ([`653fcc6`](https://github.com/s0undt3ch/ToolR/commit/653fcc6e6e18dcba882f847a1d7a375ffad486f8))
- *(freshness)* Use a nanos offset Windows' 100 ns SystemTime can hold ([`ae6e982`](https://github.com/s0undt3ch/ToolR/commit/ae6e982303875586ebb380cbf8a8ac7a97ee4918))
- *(venv)* Cover venv shell exiting 128 + signal for a signal-killed shell ([`2eea9e1`](https://github.com/s0undt3ch/ToolR/commit/2eea9e1629ca03878f75fd0585949225d5f52b8f))
- *(cli)* Pin top-level subcommand order and the reserved-group skip ([`1b417a0`](https://github.com/s0undt3ch/ToolR/commit/1b417a00d007fd30a239aefea41e7c4b8e7f4445))
- Keep spawned toolr processes out of the real cache dir ([`813baa1`](https://github.com/s0undt3ch/ToolR/commit/813baa1c842d3515ca9013b32a2eb17cc3b66322))
- *(complete)* Route complete_shells through the isolated toolr helper ([`eca1403`](https://github.com/s0undt3ch/ToolR/commit/eca14036572c99907ff43310d83c4ee1aa830cb2))
- Mute the orphan cache hint in the shared test cache home ([`7f55bb3`](https://github.com/s0undt3ch/ToolR/commit/7f55bb329023760c4a9069a549e157d0aff2df09))
- Drop cache settings the new defaults already cover ([`752a729`](https://github.com/s0undt3ch/ToolR/commit/752a729cc9b29d49f277453ee8a8de41c8db401f))
- *(cache)* Cover the hint's self-cache exit and every human_ago bucket ([`bedac06`](https://github.com/s0undt3ch/ToolR/commit/bedac067ac88ea880b99f79adef9dfdf3427c99c))
- *(bootstrap)* Rebuild in a temp cache home, not the real one ([`c936ef2`](https://github.com/s0undt3ch/ToolR/commit/c936ef2481b54ac6165a6636db6c0f5151dec43e))
- *(cache)* Expect prune to sweep stray rebuild locks ([`92382e2`](https://github.com/s0undt3ch/ToolR/commit/92382e2e3c0876d616e60d773869051287de8692))
- *(command)* Prove a capture failure lets the child finish ([`6e070a8`](https://github.com/s0undt3ch/ToolR/commit/6e070a88cea562c3a3c91f944396d9d048dd0f8e))
- *(command)* Run the write-failure tests on Windows too ([`37fa900`](https://github.com/s0undt3ch/ToolR/commit/37fa9004f0b103c3bd5c00bf42f5c69cbf0e5f40))
- *(command)* Use an fd no one can reuse, and document the Windows abort ([`439bad9`](https://github.com/s0undt3ch/ToolR/commit/439bad992f7fca79e58361fd84d909bb02b08e52))
- *(uv)* Serve several routes from the install test server ([`544acfa`](https://github.com/s0undt3ch/ToolR/commit/544acfa4f75b3bead0ef7a15816d9b71283f2c72))
- *(uv)* Follow GitHub's asset redirect ([`3de5c96`](https://github.com/s0undt3ch/ToolR/commit/3de5c9670eb64f799463db84d3deecd46e6abf51))
- *(complete)* Run the zsh script for path slots ([`d867d35`](https://github.com/s0undt3ch/ToolR/commit/d867d35d1bb4ca61598b51772b7e86641617aae7))
- *(complete)* Read the ~user directory from the home dir instead of writing one ([`6d643fa`](https://github.com/s0undt3ch/ToolR/commit/6d643fa01f896a9e799f56dd7001774639643c9b))
- *(complete)* Reuse the fish driver ([`2bb6cdd`](https://github.com/s0undt3ch/ToolR/commit/2bb6cdde76501f9c15f347ec422d972868939d89))
- *(e2e)* Differential parser/tester corpus ([`51771c1`](https://github.com/s0undt3ch/ToolR/commit/51771c148954db4e3168931e6be102d4122d718b))
- *(ci)* Move tool tests into tests/tools/ ([`f8e808c`](https://github.com/s0undt3ch/ToolR/commit/f8e808cc5c668cc5de3a70021dbbe64eb07ede26))
- *(parser)* A dotted `command = a.b.command` isn't followed ([`eba5376`](https://github.com/s0undt3ch/ToolR/commit/eba5376dd3aa277e4f783fe24af5be4b535bef7d))
- *(registry)* Isolate the command-group registry for every test ([`3939b68`](https://github.com/s0undt3ch/ToolR/commit/3939b6889b0b725d4a18970f42767c0e59e1ffbc))
- *(complete)* Accept any pre-4.4 bash in the old-bash warning test ([`dd6a2e4`](https://github.com/s0undt3ch/ToolR/commit/dd6a2e4bfd76fc146bc8bfadb20c07430f5139fb))
- *(complete)* Check bash path completion scales linearly, not a time bound ([`3db9a49`](https://github.com/s0undt3ch/ToolR/commit/3db9a49734f564e906988f8c51059e3d099a2c07))
- *(complete)* Compare 16x the entries in the bash path scaling test ([`3039a46`](https://github.com/s0undt3ch/ToolR/commit/3039a46943ab5424f99c1c41ace3834c4fa9a071))
- *(complete)* Read the pty echo so bash 5 starts on macOS ([`d5ef850`](https://github.com/s0undt3ch/ToolR/commit/d5ef85092afc13492e0e90482daeb536c479b6fb))
- Restore cwd, logging and tools state between tests ([`8f0200c`](https://github.com/s0undt3ch/ToolR/commit/8f0200c87a77c921b0ee94cff93ef2cf46f6dc2b))
- *(logs)* Test setup_logging through caplog and real handlers, not mocks ([`efff996`](https://github.com/s0undt3ch/ToolR/commit/efff996a1f1e1438526132a17f935e7b0ec332f9))
- *(logs)* Name and document the autouse fixtures, parametrise the timestamp-off test ([`1893ef8`](https://github.com/s0undt3ch/ToolR/commit/1893ef86cc40d89f797a9de3b0b4edc20fb85c41))
- *(complete)* Unify the bash wordbreaks and document the legacy fixtures ([`5d0a10d`](https://github.com/s0undt3ch/ToolR/commit/5d0a10d3de188ee2fd39854e5356e0fd1a67d5ec))
- *(e2e)* Pin group re-declaration parity in the corpus ([`c73b4a4`](https://github.com/s0undt3ch/ToolR/commit/c73b4a45e0cf35e8f075b9284e94b39a7a1cda22))
- *(e2e)* Pin a title-only group re-fetch in the corpus ([`7361736`](https://github.com/s0undt3ch/ToolR/commit/73617363781676c627e99ad0e511ecb6b6f36be0))
- *(e2e)* Drop the lenient command_group xfails from the parity corpus ([`e3d26d6`](https://github.com/s0undt3ch/ToolR/commit/e3d26d6eb817319a121fac40e21f19ed4fdd6e2c))
- *(e2e)* Pin later declarations filling empty group fields ([`00d42dd`](https://github.com/s0undt3ch/ToolR/commit/00d42dd4ab2cdf11de7e1885340d16b1f12b54e4))
- *(e2e)* Pin keyword titles, constants and long descriptions in the corpus ([`814b170`](https://github.com/s0undt3ch/ToolR/commit/814b17050ccdd6b07365e59ddccf391896b7ceab))
- *(e2e)* Pin kept-field modules, empty docstrings and None constants ([`01e00a1`](https://github.com/s0undt3ch/ToolR/commit/01e00a1b7c33b3a150cc7c3028004c59e81bc86a))
- *(command)* Close the pipe write ends in the worker, not after start() ([`be787de`](https://github.com/s0undt3ch/ToolR/commit/be787de77e033a1c727bd1882b8db36d2afe2c03))
- *(command)* Close the pipe readers in the fd streaming tests ([`af92ac7`](https://github.com/s0undt3ch/ToolR/commit/af92ac73439b5fe03f20fb59ffada4019dee071f))
- *(command)* Share the fd streaming plumbing in a run_streaming fixture ([`6e51504`](https://github.com/s0undt3ch/ToolR/commit/6e51504b05b5057ced70c58ba32e7d720b30bf30))
- *(cli)* Tidy the bash install source hint tests ([`eb54d50`](https://github.com/s0undt3ch/ToolR/commit/eb54d50248414080047589c662ec6003ce7e2445))
- *(cli)* Join the expected bash install path the way the installer does ([`4a1f875`](https://github.com/s0undt3ch/ToolR/commit/4a1f875162e4a02bb749273d4f8dedcbfa1390df))
- *(complete)* Cover --opt=value through __complete ([`ae62b8d`](https://github.com/s0undt3ch/ToolR/commit/ae62b8dfbdfa774209054371316b1e83384e907b))
- *(complete)* A positional word with an = is not an attached value ([`cc58ff9`](https://github.com/s0undt3ch/ToolR/commit/cc58ff9902713795a89e3c1a255d1181c89c86dc))
- *(toolr-py)* Check the _context stub with stubtest ([`c0b828c`](https://github.com/s0undt3ch/ToolR/commit/c0b828cdb2d6ae73b9891a11bf4c8035ce591978))
- *(complete)* Reproduce bash completion of quoted and escaped paths ([`9a3d960`](https://github.com/s0undt3ch/ToolR/commit/9a3d960a808664a9ede45f7c26073c927a3a7c2b))
- *(completion)* Cover -q and drop a duplicate zsh install ([`253e109`](https://github.com/s0undt3ch/ToolR/commit/253e1094fe595716a10b13dd4cc4df74a25dbe36))
- *(registry)* Give the wrapped-command fixture a literal group name ([`e1c28dd`](https://github.com/s0undt3ch/ToolR/commit/e1c28dd978b68b2a90ab63a4166d72c392587d33))
- *(testing)* Build the deep-expression tree without tripping CPython's parser limit ([`6248875`](https://github.com/s0undt3ch/ToolR/commit/624887520cd7c32732d1ef8e58c710520a880a76))
- *(uv)* Use platform-absolute XDG paths in the dir tests ([`3610892`](https://github.com/s0undt3ch/ToolR/commit/36108922fae1ebeb6dff2dd704e2006c3027b619))
- *(cli)* Cover worktree trust end to end ([`43ec4d9`](https://github.com/s0undt3ch/ToolR/commit/43ec4d96dc4420ff140ab000c4946697fdc08347))
- *(cli)* Pin prune and git-env trust behaviour ([`bf70ffb`](https://github.com/s0undt3ch/ToolR/commit/bf70ffb4f18d800ea935086c764d0a2b08f9e0a6))
- *(trust)* Run git on non-verbatim paths and drop dead helper branches ([`958369c`](https://github.com/s0undt3ch/ToolR/commit/958369c9e0c1c790e231e53c5c1ad02375df7346))
- *(e2e)* Decode toolr subprocess output as UTF-8 ([`dbc2142`](https://github.com/s0undt3ch/ToolR/commit/dbc2142f69b042b540afeaa764b4773f46cf21d7))
- Skip git-backed tests with skip_if_binaries_missing ([`10ffa00`](https://github.com/s0undt3ch/ToolR/commit/10ffa007535507f70f5dc306dd9cd2542f1d4030))
- Use pytest-skip-markers for Windows and missing-binary skips ([`f7e74a4`](https://github.com/s0undt3ch/ToolR/commit/f7e74a46656e451de82380ef832114d5a0ecbe0c))
- Skip git-backed tests through a git fixture ([`9b2185d`](https://github.com/s0undt3ch/ToolR/commit/9b2185d1bc7ba21484e254e9de560fd0c56e11b6))
- *(cli)* Keep a system uv out of the relative-PATH-entry tests ([`4799e9f`](https://github.com/s0undt3ch/ToolR/commit/4799e9f0734d9bdc343728e058c7529c5304d590))
- *(command)* Stop timeout tests colliding with concurrent runs ([`76e63ed`](https://github.com/s0undt3ch/ToolR/commit/76e63eda76464c80511f3997209a366cbbd10ffd))
- *(command)* Quote the writer's pid file path inside sh -c ([`3e4068b`](https://github.com/s0undt3ch/ToolR/commit/3e4068b4c719618c0b0782897c0641c48e804f2a))
- *(command)* Point process-matching helpers at unique_sleep ([`c2430b5`](https://github.com/s0undt3ch/ToolR/commit/c2430b5267b43afc7eaef9abc7dcd1fd48c002f5))
- *(command)* Cover wait_for_pid_file's wait and deadline paths ([`4dfecb4`](https://github.com/s0undt3ch/ToolR/commit/4dfecb4386a54ccaada588ba4551492dfb8eb428))
- *(runner)* Keep the quiet-warning test's contextvar out of other tests ([`34c9814`](https://github.com/s0undt3ch/ToolR/commit/34c9814f2606d5a0033967f90b2a0f74da8a247c))
- *(runner)* Make the quiet-warning test work before Python 3.14 ([`aa8c868`](https://github.com/s0undt3ch/ToolR/commit/aa8c8684c2f71e008b2c1d9ca0847bc8062cb6b5))
- *(command)* Stream to the real stdout when sys.stdout has no fd ([`e5577f7`](https://github.com/s0undt3ch/ToolR/commit/e5577f79b5a0e9df30ba68cd76df6dc79536d962))
- *(completion)* Name the zsh load mode in failures and clarify the autoload comment ([`1b8bbc7`](https://github.com/s0undt3ch/ToolR/commit/1b8bbc7d4fdaa9a02b91b7b6da457c03ee051749))
- *(completion)* Pin powershell completion after --% and record the constant word rule ([`57fe23f`](https://github.com/s0undt3ch/ToolR/commit/57fe23f405b6b7eb7a0395b270c4c7f22d080159))
- *(completion)* An empty HOME no longer fails the install ([`e79ef48`](https://github.com/s0undt3ch/ToolR/commit/e79ef489498345b90d89074faa24ffff6f1f3fbc))
- *(completion)* Name the home-less powershell install test for what it checks ([`17fe305`](https://github.com/s0undt3ch/ToolR/commit/17fe305174fecd323c210aa2a6eb3d1d319cbfb7))
- *(completion)* Pin the empty powershell directory partial to directories ([`8addd01`](https://github.com/s0undt3ch/ToolR/commit/8addd0137d20f43ae4f7651fee4855da8c5a87c3))
- *(completion)* Pin Windows PowerShell 5.1's dash-word gap ([`bd82217`](https://github.com/s0undt3ch/ToolR/commit/bd8221788f858d2a2b7e8efbd50aee66aed8507e))
- *(completion)* Assert the powershell driver markers exist ([`359451d`](https://github.com/s0undt3ch/ToolR/commit/359451d80fdd1768ee355aeaaf5de4c73a634701))
- *(completion)* Ignore the powershell tests unless run with --ignored ([`c902789`](https://github.com/s0undt3ch/ToolR/commit/c9027892e172fcd9d40b025a323904af983b796b))
- *(completion)* Compare powershell home paths in long form ([`6dfacb5`](https://github.com/s0undt3ch/ToolR/commit/6dfacb5abdc6da2b955474a27778ad5c45018285))
- *(completion)* Pin Windows PowerShell 5.1's backtick partial gap ([`dd0b075`](https://github.com/s0undt3ch/ToolR/commit/dd0b075e8da6f9e03b4e299f88dd546a41692fa9))
- *(docs)* Move the llms.txt gate tests under tests/docs/ ([`32ff647`](https://github.com/s0undt3ch/ToolR/commit/32ff6471521ff8c82d37b7d3857c5d4f69542636))
- *(docs)* Load the llms.txt gate through a fixture ([`776f195`](https://github.com/s0undt3ch/ToolR/commit/776f1950346c1620d6c4d780846bd7435cacea6c))
- *(docs)* Build gate test configs with one fixture ([`b8f1e49`](https://github.com/s0undt3ch/ToolR/commit/b8f1e4983049a964bc7567cbad85e460224fb7b5))
- *(cli)* Pin COLUMNS in the root help footer test ([`9269fe3`](https://github.com/s0undt3ch/ToolR/commit/9269fe3b845ba9cb0592b89b33fa7743b3cdb722))
- Use the toolr plugin's repo_root fixture in the gate tests ([`72cc4cc`](https://github.com/s0undt3ch/ToolR/commit/72cc4cc9998a56e1e364d3328767c52d76183192))
- *(completion)* Pin zsh display padding for wide names and the displayed values ([`d40863c`](https://github.com/s0undt3ch/ToolR/commit/d40863c602f850c91a1fc0376b30f45e51bab105))
- *(completion)* Pin the literal lookup of zsh display descriptions ([`6fcd825`](https://github.com/s0undt3ch/ToolR/commit/6fcd8258a5c57bd7e1c5b2a2e0acce9bd2f985b6))
- *(cli)* Cover __install-uv-now with uv already on PATH ([`58caf5b`](https://github.com/s0undt3ch/ToolR/commit/58caf5b666b6ad8bcdbf2ab53982a827eb655d7e))
- Use the shared toolr helper and lock the env mutation in execute_build ([`e054585`](https://github.com/s0undt3ch/ToolR/commit/e0545857ebfab22c77e6a1da48373b381e508d54))
- *(bootstrap)* Pin names_user_command for a group named init; fix stale project deps comments ([`f9bab39`](https://github.com/s0undt3ch/ToolR/commit/f9bab3962779e298f5a0d9409734f8e60ac88598))
- *(bootstrap)* Check every BUILTINS name is a root subcommand; drop the ineffective env lock ([`a22d912`](https://github.com/s0undt3ch/ToolR/commit/a22d9129726bceb570fb67c5fdc37b5bd4723685))
- Close the lazy __version__ coverage gap and de-race the ctrl-c e2e command ([`59e8cdd`](https://github.com/s0undt3ch/ToolR/commit/59e8cdd4f9514fe564082fe91251b033290fbc73))
- Move the top-level test modules under tests/<area>/ ([`a6cfb40`](https://github.com/s0undt3ch/ToolR/commit/a6cfb403bc9bcfb66b609d15b8d820e3fe256db6))
- *(project)* Quiet init still records trust ([`8bc86d8`](https://github.com/s0undt3ch/ToolR/commit/8bc86d85d8009301ad9e63abefaa05adf5ebadf1))
- *(discovery)* Compare against plain canonical paths ([`a6066fe`](https://github.com/s0undt3ch/ToolR/commit/a6066feaec8e30720e6675fda68718c518165e76))
- *(security)* Pin escaping in hash and freshness errors ([`b2c6c3e`](https://github.com/s0undt3ch/ToolR/commit/b2c6c3e88ad3905c248244fbdb67949fffd8ba97))
- *(execute)* Test the runner env policy once per layer ([`c16f4ac`](https://github.com/s0undt3ch/ToolR/commit/c16f4ac97299fe383f92f6f834650d677ad3aa44))
- Share the ancestor tools/ precondition check ([`2502592`](https://github.com/s0undt3ch/ToolR/commit/2502592fd37afc680d0f89a94d11e43453538aa8))
- *(completion)* Keep complete_smoke's own tools/pyproject.toml precondition ([`0dbe50b`](https://github.com/s0undt3ch/ToolR/commit/0dbe50b3e75870bc2348d67ead655209cd1967f3))
- *(discovery)* Build the expected tools path with nested joins ([`4a6be64`](https://github.com/s0undt3ch/ToolR/commit/4a6be64c160ff7148c9a29db6e512bb8307ccd4a))
- *(completion)* Find mise-installed bash and warn when none ran ([`02bcafa`](https://github.com/s0undt3ch/ToolR/commit/02bcafa47610872b301715df9b1408d71d86e298))
- *(completion)* Pin the exit codes and match variable shared with the scripts ([`94c7578`](https://github.com/s0undt3ch/ToolR/commit/94c757878bf8474bb6bb15dc0d5ed0dd23942f10))
- *(tools)* Cover check-gate-needs ([`cdd6a75`](https://github.com/s0undt3ch/ToolR/commit/cdd6a75e968c72ce704978fc86b6f7dce14fdaa7))
- *(py)* Stubtest the _rust_utils stub and fix its parameter order ([`fd25389`](https://github.com/s0undt3ch/ToolR/commit/fd25389dec30f886ebd51405c8e4c0d6602bec20))
- Keep host state out of the attestation and llms.txt tests ([`9c741c2`](https://github.com/s0undt3ch/ToolR/commit/9c741c285cb2c62b75cae1f2166fc19c77c7e733))
- Make the missing-bash pass fail loudly and anchor the lockstep checks ([`86bb33c`](https://github.com/s0undt3ch/ToolR/commit/86bb33cf5b1d2df85f46f9063774e78def414207))
- *(completion)* Set PWD for the bash drivers so conda bash 5.1 finds its cwd ([`0907086`](https://github.com/s0undt3ch/ToolR/commit/0907086c1ece7e51ce442e197700052e7f892d1e))
- *(completion)* Correct the bash install hint and parse TOOLR_TEST_BASH as a path list ([`8a1f282`](https://github.com/s0undt3ch/ToolR/commit/8a1f28230833e3dfe6f645a5a260674d892f735e))
- *(completion)* Document the bash probe and the protocol lockstep, and fix the spec ([`9a0a39e`](https://github.com/s0undt3ch/ToolR/commit/9a0a39e1d4f8eba32a965a7365d54b671eac0d53))
- *(manifest)* Pin rebuild error text, first-run freshness and the explicit rebuild lock ([`cab5443`](https://github.com/s0undt3ch/ToolR/commit/cab544371f0bc549c01c9cd38d99d1c6601ff8ed))
- *(manifest)* Destructure refresh results without uncovered panic arms ([`b8938b1`](https://github.com/s0undt3ch/ToolR/commit/b8938b10f66ddf492a66a6c25737b09a0914ff6c))
- *(completion)* Cover walk_group skipping hidden children ([`8e81a67`](https://github.com/s0undt3ch/ToolR/commit/8e81a67de2ee854417a022298f1d035e00f77221))
- *(manifest)* Pin the fresh-clone plugin hash and the lock-free in-memory path ([`434a0de`](https://github.com/s0undt3ch/ToolR/commit/434a0decab1332f1bf4428a081bf09547b7359e5))
- *(cli)* Check the ToolR Options note with whitespace collapsed ([`b13298f`](https://github.com/s0undt3ch/ToolR/commit/b13298f69a3a7689cca984068928de279dac08a2))
- *(cli)* Bool env values follow the shared boolean spellings ([`969e3ff`](https://github.com/s0undt3ch/ToolR/commit/969e3ffb366bdbc85b0ab5961f375f610fd4ab6d))
- *(venv)* Cover the old-layout resync and refusal ([`f5de917`](https://github.com/s0undt3ch/ToolR/commit/f5de917bca251ec47dd073df9ed5e700f8f72ce0))
- *(cli)* Cover run_self's defensive errors and a missing --check target ([`a4baf11`](https://github.com/s0undt3ch/ToolR/commit/a4baf112e2a3983ad07dae19e068b97624391e8b))
- *(command)* Give slow-sink timeout tests headroom for loaded hosts ([`3d22d8d`](https://github.com/s0undt3ch/ToolR/commit/3d22d8dea565b4fd62c60f8e9039ae934386b8a7))
- *(venv)* Cover the sync lock re-check and venv entry removal failure ([`734aaec`](https://github.com/s0undt3ch/ToolR/commit/734aaec99cc5e393ab807da56b12685a352e5e92))
- *(venv)* Order sync inputs with set_modified, not sleeps ([`197976d`](https://github.com/s0undt3ch/ToolR/commit/197976d5f9c0106c856e68ea91ef1c5393f4ebf1))
- *(trust)* Cover dispatch asking for trust when main read help ([`8ee3e86`](https://github.com/s0undt3ch/ToolR/commit/8ee3e86150e32d4f5feb9e6bef0458c818e19daa))
- *(complete)* Run the PowerShell harness under TOOLR_TRUST_ALL ([`117ebb5`](https://github.com/s0undt3ch/ToolR/commit/117ebb50211a988fe9f9599f05328520ac7ff593))
- Cover the branch's remaining uncovered lines ([`c682d04`](https://github.com/s0undt3ch/ToolR/commit/c682d044bd5c0ed0893845b5a1bc253effd7c43a))
- *(trust)* Pin that settling a claimed venv keeps its token ([`63377d6`](https://github.com/s0undt3ch/ToolR/commit/63377d616a25a571cd1eff347f7084966416a34f))
- *(release-notes)* Test UTF-16 counting on the helper ([`ae9bb98`](https://github.com/s0undt3ch/ToolR/commit/ae9bb98bfc42ee7c2042c5d591da5850c7ee1894))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(packaging)* Mark both wheels as Beta on PyPI ([`e4372f3`](https://github.com/s0undt3ch/ToolR/commit/e4372f3bc48fb6dcef59d5553e3edfa21ea98f5c))
- *(pre-commit)* Switch from actionlint to jactionlint ([`17f90f8`](https://github.com/s0undt3ch/ToolR/commit/17f90f89a9f45d3bc89243259de1210c1c1dab9c))
- Fix the workflow findings jactionlint reports ([`9120b9c`](https://github.com/s0undt3ch/ToolR/commit/9120b9ced9a5f028477d8d455fa5d242f38554f1))
- Drop the push-on-branch paths the main-only trigger made dead ([`20c90a1`](https://github.com/s0undt3ch/ToolR/commit/20c90a14c7bd5d53fde435a3cd74e139f81c3189))
- Stop passing GH_TOKEN to check-run-build ([`1047fff`](https://github.com/s0undt3ch/ToolR/commit/1047fff8a3ab6c2a094fe9fa960dbd25c0d5d091))
- Drop the should-run-build gate ([`09c5d5a`](https://github.com/s0undt3ch/ToolR/commit/09c5d5a17f8e550e5e8c983fee6157c26cff936d))
- *(rust)* Migrate workspace to edition 2024 ([`352b8bc`](https://github.com/s0undt3ch/ToolR/commit/352b8bc60b9ae77e240db99544f76d42b7d1d6eb))
- *(pre-commit)* Drop the duplicate cargo-check hook ([`af89b09`](https://github.com/s0undt3ch/ToolR/commit/af89b094f32921190d2b7f422ef1e78b966bca93))
- *(build)* Build macOS and Windows wheels with the mise Rust toolchain ([`f7f18c3`](https://github.com/s0undt3ch/ToolR/commit/f7f18c34d664edc9a61e84fffc2ddccdd58a5dc3))
- *(renovate)* Give Rust toolchain bumps a week before they land ([`2abd49a`](https://github.com/s0undt3ch/ToolR/commit/2abd49ac19744b32e95291c2643f685e785d6edc))
- Move macOS arm64 jobs from macos-14 to macos-15 ([`e03ed13`](https://github.com/s0undt3ch/ToolR/commit/e03ed13f8a88f5e499ccc9a4adb46df1ba670e08))
- *(action)* Export TOOLR_TRUST_ALL ([`0e305af`](https://github.com/s0undt3ch/ToolR/commit/0e305afd117d44eb930485a4d100dc28b0fab889))
- *(release)* Show the committed release diff and drop the unused local tag ([`aa0478c`](https://github.com/s0undt3ch/ToolR/commit/aa0478cc997a9714dfb926d1d931fb97e7c4c855))
- *(tools)* Add release-notes render command ([`e9bfcdd`](https://github.com/s0undt3ch/ToolR/commit/e9bfcdd9249ad0dd2c6a4a44237956aa51305ff1))
- *(tools)* Add release-notes add and clear commands ([`9af3ce3`](https://github.com/s0undt3ch/ToolR/commit/9af3ce3ec3e8527bbaa987cb6007ab1f4529ca2b))
- *(tools)* Add release-notes check command ([`81810a1`](https://github.com/s0undt3ch/ToolR/commit/81810a17aa082c6d0c61bdf3ee95043867ba39b1))
- *(tools)* Track markdown fences properly in release-notes check ([`ce264e3`](https://github.com/s0undt3ch/ToolR/commit/ce264e3ed4be9edbe60da8e804fe5c271d1f1471))
- *(tools)* Fix indented-fence test input ([`6cfe7e5`](https://github.com/s0undt3ch/ToolR/commit/6cfe7e586c2e5a042074aae5f47c82e7b46efeb8))
- *(tools)* Gate PRs on release notes with check --base ([`4e4b7a3`](https://github.com/s0undt3ch/ToolR/commit/4e4b7a3e3686906e82ba5c73078c6e9877a35a83))
- *(tools)* Print release notes verbatim ([`33d0f78`](https://github.com/s0undt3ch/ToolR/commit/33d0f7842f63a89f82a8d4075d8098072f5a88df))
- *(release)* Build release notes from release-notes/ ([`b813316`](https://github.com/s0undt3ch/ToolR/commit/b813316f914acbd65278cf7792f9c605c5ad93fb))
- *(tools)* Fix release-notes edge cases from final review ([`98f99e0`](https://github.com/s0undt3ch/ToolR/commit/98f99e0ed0d2e5253f489db2640256bf0519194e))
- *(tools)* Flag setext headings in release notes ([`437953b`](https://github.com/s0undt3ch/ToolR/commit/437953b36e7dd69c073e105c8d2de8d97014f9a4))
- *(tools)* Flag unclosed fences and ignore editor files in release notes ([`ce2da6b`](https://github.com/s0undt3ch/ToolR/commit/ce2da6be4091ea7a7dcf1d0352b3b162c955b600))
- *(lint)* Exempt release notes from MD013 ([`24b6238`](https://github.com/s0undt3ch/ToolR/commit/24b62387df19e26a95f22622c6f25ab255ab37e4))
- *(tools)* Only treat release-notes/*.md files as notes ([`2fa7834`](https://github.com/s0undt3ch/ToolR/commit/2fa78341f557a1d1f8dffa25f45a3534d0eec5a0))
- *(tools)* Tidy the release notes tool and its tests ([`5f6a9f3`](https://github.com/s0undt3ch/ToolR/commit/5f6a9f34456906ec261b55e73b85459de49d93aa))
- Explain the release-notes ties in ci.yml and cliff.toml ([`37d6e1b`](https://github.com/s0undt3ch/ToolR/commit/37d6e1bbc025ff26f25b698dbced6131e1371703))
- *(tools)* Print the new note path with forward slashes on Windows ([`3990883`](https://github.com/s0undt3ch/ToolR/commit/3990883ce3c8b0f84bfdc63c924a3c50bec09794))
- *(prek)* Keep toolr-py's toolr-core build lean ([`c2004ae`](https://github.com/s0undt3ch/ToolR/commit/c2004ae6f60b767de94dc377e888645df4614af8))
- *(prek)* Fail the toolr-py-deps hook when cargo tree fails ([`56c2376`](https://github.com/s0undt3ch/ToolR/commit/56c2376824ae5322360595145324021c7dc645fe))
- *(prek)* Check toolr-py itself without toolr-core's full feature ([`10ac0e4`](https://github.com/s0undt3ch/ToolR/commit/10ac0e4b1e00f935df418658295cc545882d5170))
- *(prek)* Allowlist toolr-core's lean dependencies ([`f526d8e`](https://github.com/s0undt3ch/ToolR/commit/f526d8e9c8444e021f01b96854863b16ad0f04f4))
- *(release)* Point the release workflows at the action's attestation pins ([`88de463`](https://github.com/s0undt3ch/ToolR/commit/88de4639a01bec1dc20cd092cc3bfa241cef536c))
- *(release)* Pin git-cliff in mise and the release workflow ([`631b60c`](https://github.com/s0undt3ch/ToolR/commit/631b60c1cdf62305c7a258a76178fbdf41b1a375))
- *(release)* Run the mise-installed git-cliff in the release workflow ([`e485bef`](https://github.com/s0undt3ch/ToolR/commit/e485bef25c25c2c97973c0102811773e5bfea6ab))
- Dogfood the pinned toolr version instead of latest ([`a9f566c`](https://github.com/s0undt3ch/ToolR/commit/a9f566cec8acda2eeca11d5dbe980e838511ff97))
- *(renovate)* Bump the CI toolr pin with the toolr release group ([`c52e3ad`](https://github.com/s0undt3ch/ToolR/commit/c52e3ad7cea38d7e4f26461c5be644dfe5f54035))
- Fail on an empty toolr pin before every Setup ToolR call ([`7f3be7c`](https://github.com/s0undt3ch/ToolR/commit/7f3be7ce4a76a5c5f6d95c3c7f5a25e16fa2ad27))
- *(renovate)* Look up the CI toolr pin by tag, like mise.toml ([`aeefd22`](https://github.com/s0undt3ch/ToolR/commit/aeefd22345acee1cdd996b3be3c17510b6a0c7ca))
- *(renovate)* Correct why the CI toolr pin uses github-tags ([`e13a4b3`](https://github.com/s0undt3ch/ToolR/commit/e13a4b399197e0482cc48906e2fe0df5f2e50b60))
- *(renovate)* Name the setup-toolr wrapper in the CI toolr pin manager ([`3d99559`](https://github.com/s0undt3ch/ToolR/commit/3d99559fb15641ba057c44a8958b45d05ff7ee1b))
- *(install-smoke)* Run install.ps1 under Windows PowerShell 5.1 too ([`aae8848`](https://github.com/s0undt3ch/ToolR/commit/aae88487892b3384f18735509a189310ca5e5d7c))
- *(test)* Install fish so the fish completion tests run ([`1705158`](https://github.com/s0undt3ch/ToolR/commit/170515864e3356b80802623778ab226f13d93501))
- *(coverage)* Measure subprocesses with coverage's own patch = subprocess ([`7dc87f0`](https://github.com/s0undt3ch/ToolR/commit/7dc87f098e5491ec437ebbc93b3ed08ce9fc8ac0))
- *(pre-commit)* Run the Python version checks as a prek hook ([`5d41e52`](https://github.com/s0undt3ch/ToolR/commit/5d41e524e6b278d2fd0791bc6f12ad7c01270c47))
- *(ruff)* Mark the repo's tools package as first-party ([`f6b6a6b`](https://github.com/s0undt3ch/ToolR/commit/f6b6a6b548de7eedf045b78c9c3e6e426f06f0d5))
- *(ci)* Drop the tests/support/coverage exemption ([`f836a90`](https://github.com/s0undt3ch/ToolR/commit/f836a90b58ed6a3ff5cb838d05c72dd281213e68))
- *(ruff)* Sort the tools package in its own import block ([`19013ee`](https://github.com/s0undt3ch/ToolR/commit/19013ee8fbb00f290b24a73d778a9de0c2332f51))
- *(test)* Install bash 4.4+ on macOS for the completion tests ([`72c5aa7`](https://github.com/s0undt3ch/ToolR/commit/72c5aa7b5d20280194ec3affbf9f7440575170a3))
- *(lint)* Enforce ruff's unspecified-encoding rule ([`8dea0f7`](https://github.com/s0undt3ch/ToolR/commit/8dea0f7d27b726e220d2599459658eb162315571))
- Skip build and test jobs for PRs that only touch CI-inert paths ([`50c173c`](https://github.com/s0undt3ch/ToolR/commit/50c173cf7ae3d17966b48d5a6bd842fcb46627b7))
- Check distributions on every run and upload to Test PyPI only at release ([`b6771d8`](https://github.com/s0undt3ch/ToolR/commit/b6771d8693fff8e1264940f8d7d2d9e50a5f7368))
- *(completion)* Exercise powershell completion on 5.1 and pwsh ([`e40d4ad`](https://github.com/s0undt3ch/ToolR/commit/e40d4ad3587d750c4d24130d63b0e55a51d68779))
- *(test)* Install powershell only for the completion test run ([`49ce489`](https://github.com/s0undt3ch/ToolR/commit/49ce48980036eae9e6f1c0c80cab47db50b2d2ce))
- *(test)* Use the runner's pwsh and run the powershell tests once per os ([`6b89ede`](https://github.com/s0undt3ch/ToolR/commit/6b89ede2b392b8040deb8baaf436736b1db6e491))
- *(test)* Double-quote the powershell-python description ([`3777dfb`](https://github.com/s0undt3ch/ToolR/commit/3777dfba0552e1cb841b46f8fe46d038835fe109))
- *(docs)* Stop deploying docs to GitHub Pages ([`32dc2c0`](https://github.com/s0undt3ch/ToolR/commit/32dc2c0526316643ab5948b04f6cc0ae2648214b))
- Tidy the plan wording ([`43db887`](https://github.com/s0undt3ch/ToolR/commit/43db887e76f3d9c6d1af3f26aa5b6d10658715aa))
- Explain the root help footer and dev mkdocs dependency ([`03ca69e`](https://github.com/s0undt3ch/ToolR/commit/03ca69e14b887b4fa8cfa846bc9d01eae9d84c31))
- Drop the redundant cargo-check hook and the needless prepare-ci wait ([`8bba73b`](https://github.com/s0undt3ch/ToolR/commit/8bba73b11257600a39000b275f9564a044df0382))
- Stop persisting credentials in the docs job and drop the gate job's environment ([`53771ff`](https://github.com/s0undt3ch/ToolR/commit/53771ff6083113d8c189ff12dab3e7290ce3ea7c))
- *(release)* Dispatch only from main and document half-published recovery ([`97d193f`](https://github.com/s0undt3ch/ToolR/commit/97d193f48caad642d11dea69dbd9fec7cc2aafbc))
- Run Rust coverage and drift gates once per OS ([`d10c841`](https://github.com/s0undt3ch/ToolR/commit/d10c841911016cddfd6ce178b709607c67a34936))
- Key the Rust caches without the Python version; fix stale comments ([`b0d1ea3`](https://github.com/s0undt3ch/ToolR/commit/b0d1ea325390cdebcb6d7be08f752f15ea2e51b9))
- Run the PowerShell pass in the rust-python job ([`f0c2376`](https://github.com/s0undt3ch/ToolR/commit/f0c2376c5f53bf3ea7d34c04f4487ff274f64219))
- Compute the Rust job switch once and tidy comments ([`40a80e8`](https://github.com/s0undt3ch/ToolR/commit/40a80e83f9e0d0ddced25bb8c4e9623b333efa91))
- *(mise)* Bump mise to v2026.11.6 and the lockfile to version 4 ([`2335b43`](https://github.com/s0undt3ch/ToolR/commit/2335b4392505511a5c82d9d50b3edba59646d6c1))
- *(mise)* Require mise 2026.11.1, the first that reads mise.lock v4 ([`ee70cdb`](https://github.com/s0undt3ch/ToolR/commit/ee70cdb7abd3df590015a87e85d81954ad4ba99a))
- Expand a bare X.Y test Python to its mise.toml pin ([`cf7423e`](https://github.com/s0undt3ch/ToolR/commit/cf7423e6faaf2936ea049e4be6e99f44720fbf24))
- *(dev)* Add hyperfine for CLI latency budgets ([`fe4195d`](https://github.com/s0undt3ch/ToolR/commit/fe4195deaa19a23d9a34b4cbdbd6a0715a6daab7))
- *(release)* Keep commit lists out of the GitHub release body ([`9f075cf`](https://github.com/s0undt3ch/ToolR/commit/9f075cf4d8e085da1aba8cd00e5166161ff35edc))
- *(release)* Address review of the release body change ([`054d50d`](https://github.com/s0undt3ch/ToolR/commit/054d50dd9483e32f0640d2d9728a96c2b25920a2))
- *(test-distribution)* Use the built toolr-py wheel instead of a source build ([`7c56f3c`](https://github.com/s0undt3ch/ToolR/commit/7c56f3c39966e60ff9f5a846e3b5c59eaac420bb))

### <!-- 9 -->◀️ Revert

- *(logs)* Keep the root log level under -q; fix the notes ([`0fffe2c`](https://github.com/s0undt3ch/ToolR/commit/0fffe2c18caada0b5e1d383e7bf904fa52c53705))
- *(build)* Keep the default release profile ([`484098c`](https://github.com/s0undt3ch/ToolR/commit/484098ca6c8308d6714a6c5a3eb6f70a82bc5c1c))
## 0.35.1 - 2026-10-27

### Notes

Bumped the bundled `rustls-webpki` to 0.103.16 to fix
[RUSTSEC-2026-0382](https://osv.dev/RUSTSEC-2026-0382), where certificate verification could
accept distrusted intermediates. This affects HTTPS downloads made by the `toolr` binary and
the `toolr-py` extension.

Tab completion no longer stops after a multi-word or aliased option that takes a value, such
as `--log-level debug` or `--ref abc`. The completion engine didn't recognise these flags, so it
read their value as the next positional argument and offered nothing, or the wrong candidates,
from there on. Count flags (`--verbose`) are now also recognised as taking no value.
([#585](https://github.com/s0undt3ch/ToolR/issues/585))

### <!-- 1 -->🐛 Bug Fixes

- *(deps)* Bump rustls-webpki and certifi for security advisories ([`87b6f45`](https://github.com/s0undt3ch/ToolR/commit/87b6f4561e469ae6d5f5c6425ec5b355935039a7))
- *(deps)* Bump webpki-root-certs to 1.0.11 for RUSTSEC-2026-0383 ([`612e124`](https://github.com/s0undt3ch/ToolR/commit/612e1242ace411f0beca1bd19b0a71996e3027c0))
- *(complete)* Recognise hyphenated and aliased flags when walking tokens ([`ebc90d4`](https://github.com/s0undt3ch/ToolR/commit/ebc90d44bb824bb51449e4179080a24d6e0237b2))

### <!-- 3 -->📚 Documentation

- *(deps)* Exempt security fixes from the uv dependency cooldown ([`d60295f`](https://github.com/s0undt3ch/ToolR/commit/d60295f4460f85a693be81da702233ffd18ad617))
- *(renovate)* Correct osvVulnerabilityAlerts scope to direct deps only ([`354c822`](https://github.com/s0undt3ch/ToolR/commit/354c822029ce1b91648904076bc2ec9ddf412b92))
## 0.35.0 - 2026-10-19

### Notes

`toolr-py` now supports CPython 3.15. The abi3 wheel already loaded there, but
`requires-python` excluded it; the bound is now `<3.16`. CI tests 3.15 alongside 3.11–3.14.

### <!-- 0 -->🚀 Features

- *(python)* Support CPython 3.15 ([`087ca00`](https://github.com/s0undt3ch/ToolR/commit/087ca004f3d101958ac9550d8f3e2dec55e00301))

### <!-- 1 -->🐛 Bug Fixes

- *(deps)* Update rust crate serde-saphyr to v2 ([`a0e61ab`](https://github.com/s0undt3ch/ToolR/commit/a0e61ab2e79d72fb3a99a8ee10e73fe990d63426))

### <!-- 3 -->📚 Documentation

- *(unreleased)* Note CPython 3.15 support ([`51254e0`](https://github.com/s0undt3ch/ToolR/commit/51254e0bf76d451451f45046bf7d63e6e89dd080))

### <!-- 5 -->🎨 Styling

- Satisfy ruff 0.17 ICN001 datetime import alias ([`377b4fe`](https://github.com/s0undt3ch/ToolR/commit/377b4fe0887103413612d9fc51bfe0263cec4236))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(codecov)* Compute statuses only after every coverage upload ([`949947c`](https://github.com/s0undt3ch/ToolR/commit/949947c9fb75a9de232006bd3fede75377dcbc39))
- *(codecov)* Skip the notification when no test job ran ([`3695b25`](https://github.com/s0undt3ch/ToolR/commit/3695b250b4c8869ee98b65c8fbdacf41994684e7))
- *(codecov)* Gate the notification on should-run-build ([`13858d4`](https://github.com/s0undt3ch/ToolR/commit/13858d41c4dd90833e96fbc596151229833f7ec0))
- *(packslip)* Pin the CLI attestation to packslip's release-plz branch ([`7f7e807`](https://github.com/s0undt3ch/ToolR/commit/7f7e807f797b7ae5dca086cd37c6d47db04668cb))
- *(mise)* Drop stale musl lockfile caveat ([`24b2415`](https://github.com/s0undt3ch/ToolR/commit/24b2415a7c416f21b24e126293c52e801fb15208))
- *(ruff)* Ignore ICN001 instead of renaming datetime imports ([`08d57ae`](https://github.com/s0undt3ch/ToolR/commit/08d57ae96cfbad72d679a4652f763b2e4a9ce563))
## 0.34.1 - 2026-10-09

### Notes

Commands with a named positional parameter ahead of `*args` (for example
`def cmd(ctx, foo: Foo, *paths: pathlib.Path)`) now run. They used to crash with
`TypeError: ... got multiple values for argument`. Commands with positional-only
(`/`) parameters now work too: the manifest parser used to drop them, plus the
parameter right after them.

A Google-style `Args:` entry that wraps onto more than one line now shows in
full in `--help`. The parser used to keep only the first line. A continuation
line containing a colon (`see: the README`, a URL) also used to register a bogus
parameter, or overwrite a real one with the same name.

Running several `toolr` processes at once against the same project no longer prints
"failed to touch cache meta.json" warnings or leaves a truncated `meta.json` in the cache.

With `venv-location = "in-tree"`, toolr no longer reads all of `tools/.venv` on every
run to check whether the manifest is stale. It also no longer fails with
`hashing <repo>/tools: No such file or directory (os error 2)` when `uv sync` recreates
that venv while toolr commands are running. The freshness hash now skips `__pycache__`
and dot-directories under `tools/`, the same way the manifest parser already skipped
dot-directories.

Running several `toolr` commands in parallel (for example as pre-commit hooks) no longer fails at
random with `error: unrecognized subcommand`. Each process rewrote `tools/.toolr-manifest.json` in
place, so another process could read it half written, and toolr then quietly treated the manifest as
empty. The manifest is now replaced atomically, and a manifest toolr can't read is reported as an
error naming the file instead of hiding your commands.

When several `toolr` processes start at once against a stale or missing manifest
(for example parallel pre-commit hooks after editing `tools/*.py`), only one now
rebuilds it; the others wait for it and reuse the result. The lock lives in toolr's
cache directory, so nothing new appears in `tools/`. If the lock can't be taken, for
example on a read-only filesystem, the rebuild runs without it as before.

### <!-- 1 -->🐛 Bug Fixes

- *(runner)* Pass params ahead of `*args` positionally ([`2fe4039`](https://github.com/s0undt3ch/ToolR/commit/2fe40399a273cbe0bd1bd1254e7199b5ca932252))
- *(parser)* Read positional-only params from the signature ([`77e21b2`](https://github.com/s0undt3ch/ToolR/commit/77e21b2bfdc8c14b985ad8ca5551116a0911fcb4))
- *(docstrings)* Keep wrapped `Args:` entries' continuation lines ([`c50dec1`](https://github.com/s0undt3ch/ToolR/commit/c50dec1735f517fce516318bbce12af412f42b7d))
- *(cache)* Stage meta.json through a unique temp file ([`f30e73a`](https://github.com/s0undt3ch/ToolR/commit/f30e73a0be6be922b19215d960c27274aaa4e3df))
- *(cache)* Rename meta.json with std so Windows allows concurrent replaces ([`78e3980`](https://github.com/s0undt3ch/ToolR/commit/78e39802b2f0bc101a5f98205bab0ea84a4bbc61))
- *(freshness)* Skip the in-tree venv when hashing tools/ ([`91bb3d9`](https://github.com/s0undt3ch/ToolR/commit/91bb3d936a25f85934c8c8352bba4eed08df1259))
- *(manifest)* Write the manifest atomically ([`4213da7`](https://github.com/s0undt3ch/ToolR/commit/4213da7761b80c61b2472f112c8d4e49395eae26))
- *(cli)* Stop treating an unreadable manifest as an empty command set ([`dd23160`](https://github.com/s0undt3ch/ToolR/commit/dd23160c213dfb71d22ae59c09c9b1e1bddb3c81))
- *(manifest)* Rename with std so Windows allows concurrent replaces ([`f5a62f0`](https://github.com/s0undt3ch/ToolR/commit/f5a62f00ff0680fa85d4781e9b25f2b3a69d7b0f))
- *(manifest)* Retry reads Windows denies during a concurrent replace ([`ed5fe57`](https://github.com/s0undt3ch/ToolR/commit/ed5fe57e999d182730e97ea25aa2e9f84c1d19ce))

### <!-- 3 -->📚 Documentation

- *(install)* Name 0.34.0 as the first release with a packslip ([`33aa9a8`](https://github.com/s0undt3ch/ToolR/commit/33aa9a8b737b742667095aacf055b124b53a8b69))

### <!-- 4 -->⚡ Performance

- *(manifest)* Rebuild a stale manifest once under a file lock ([`96cae05`](https://github.com/s0undt3ch/ToolR/commit/96cae059e0f3e45ec8d66ba8551de1e5940aa836))

### <!-- 6 -->🧪 Testing

- *(ci)* Smoke-test packslip and aqua installs through pytest ([`6d3429e`](https://github.com/s0undt3ch/ToolR/commit/6d3429e618ef25589dd50eb64f09e26cd2750a0d))
- *(ci)* Harden the mise smoke tests against false greens ([`9104827`](https://github.com/s0undt3ch/ToolR/commit/910482702814838228ead8640d4c90de4ec21e93))
- *(ci)* Make the mise smoke shell checks fail loudly in CI ([`43b3857`](https://github.com/s0undt3ch/ToolR/commit/43b38577f9f120eb8e57d1daae93d51893e2603f))
- *(ci)* Explain the mise smoke test couplings ([`4d24353`](https://github.com/s0undt3ch/ToolR/commit/4d243534baae69e8b57c1cca28cf84d0fcaaa6de))
- *(ci)* Run the arm64 macOS mise smoke leg on macos-15 ([`2028658`](https://github.com/s0undt3ch/ToolR/commit/20286586009fa6bccba4ae3d46192ab10749540e))
- *(e2e)* Drive #539's shapes through the checkout's toolr binary ([`117c1dd`](https://github.com/s0undt3ch/ToolR/commit/117c1ddfcad4f39c28f3670dbc7a2e3e94ba92c4))
- *(sources)* Drop the dead TOOLR_TEST_PYTHON env from the e2e tests ([`b459f3a`](https://github.com/s0undt3ch/ToolR/commit/b459f3a69722ae4662176306e64bacd9d1f3e4a6))
- Compare toolr versions semantically and print paths portably ([`a1e69a1`](https://github.com/s0undt3ch/ToolR/commit/a1e69a1e06f44731411ff205d8a65efea90112c2))
- *(runner)* Cover the default and missing-param paths ahead of `*args` ([`006ad72`](https://github.com/s0undt3ch/ToolR/commit/006ad7297c1d939867af350b776051119148c954))
- *(hash)* Cover the non-NotFound read error path ([`b0f981a`](https://github.com/s0undt3ch/ToolR/commit/b0f981acfd31b65c31a803f69d561f0aaec335e6))
- *(manifest)* Cover the missing-manifest rebuild path ([`7955b59`](https://github.com/s0undt3ch/ToolR/commit/7955b591d261b6f38c9ef89b8cbdbbaf886c33f1))
- *(manifest)* Keep never-called closures off their own lines ([`9ee57a0`](https://github.com/s0undt3ch/ToolR/commit/9ee57a05c06debde39d0413b173555370f13abe0))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Bump mise to v2026.10.3 ([`55867f7`](https://github.com/s0undt3ch/ToolR/commit/55867f79a6b65e9026022ba1de021fd17ed62c91))
- *(renovate)* Track MISE_VERSION in .github/env ([`879bf48`](https://github.com/s0undt3ch/ToolR/commit/879bf48d9a5da69b0372a7d271df49e8305c8154))
## 0.34.0 - 2026-10-01

### Notes

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
(toolr_a, toolr_b), so it is disabled. ...`). Uninstall all but one to get it back, or define it in your `tools/`,
and the local one wins. Both this warning and the shadowing one now link
[#522](https://github.com/s0undt3ch/ToolR/issues/522), where you can vote for choosing the winner in configuration.

### Commands re-sync a stale tools venv before running

Running a toolr command now checks whether `tools/uv.lock` has changed since the tools venv was
last synced. If it has, toolr syncs the venv before running the command. Until now, a dependency
bump (including `toolr-py` itself) never reached the venv until you ran
`toolr project venv sync` by hand, so commands kept running the old code without any warning.
When the sync can't happen, for example because uv is missing and installing it wasn't
approved, toolr refuses to run the command and points you at `toolr project venv sync`.

### Warning when the tools venv's `toolr-py` is from another minor release

Before running a command, toolr now checks the `toolr-py` version installed in the tools venv. If
its minor release differs from the toolr binary's, toolr prints one warning naming both versions.
The two can still talk to each other, so the command runs, but the mismatch used to stay hidden
and made bugs already fixed in your toolr release look like new ones. When the venv is older, the
warning suggests `toolr project venv sync -P toolr-py`, after loosening the `toolr-py` pin in
`tools/pyproject.toml` if it caps the version. When the venv is newer, it suggests upgrading the
toolr binary. Patch-level differences don't warn, and `--quiet` suppresses the warning.

### <!-- 0 -->🚀 Features

- *(toolr-py)* Add current_context() accessor and NoCurrentContextError ([`5c49421`](https://github.com/s0undt3ch/ToolR/commit/5c49421f4db3f615841f2ca2229de5babb64bfe7))
- *(toolr-py)* Add toolr.testing.set_current_context ([`1c5dcc3`](https://github.com/s0undt3ch/ToolR/commit/1c5dcc353c28ead05856127b251a28c3295662c3))
- *(toolr-py)* Set current_context() before dispatching a command ([`7b97fde`](https://github.com/s0undt3ch/ToolR/commit/7b97fde25b9f14b1221a4f417815ba19e8e30c36))
- *(toolr-py)* Export toolr.current_context from the top-level package ([`3a14b77`](https://github.com/s0undt3ch/ToolR/commit/3a14b772bb012416622f791b33da95ffaa691471))
- *(parser)* Add supported-type and path-constraint catalogues ([`855999e`](https://github.com/s0undt3ch/ToolR/commit/855999e2a44d7935afda4a34f9c8acbea420d96d))
- *(xtask)* Generate supported-type and path-constraint tables from code ([`93ae116`](https://github.com/s0undt3ch/ToolR/commit/93ae116b1b1187ccfa992d76b1a75f293903aa5a))
- *(xtask)* Add named docs-section extractor for skill references ([`0a504ce`](https://github.com/s0undt3ch/ToolR/commit/0a504ce40b312cf1915a906eba7b0a51c321aef0))
- *(skills)* Ship argument shapes, prek hook and plugin example inside the skills ([`66c4191`](https://github.com/s0undt3ch/ToolR/commit/66c41912c35cf4c01b22658aceb13d4420c8c332))
- *(xtask)* Fail build-skill-refs when a skill reaches outside its directory ([`114d4d0`](https://github.com/s0undt3ch/ToolR/commit/114d4d09d860db43369192458de202151e6b9279))
- *(parser)* [**breaking**] Fail the manifest build on commands without a docstring (#501) ([`7f224e5`](https://github.com/s0undt3ch/ToolR/commit/7f224e560117f36621d4b0eb8504c2abb45b6879))
- *(decorators)* [**breaking**] Reject undocumented commands at import and refuse -OO (#501) ([`83dd6e7`](https://github.com/s0undt3ch/ToolR/commit/83dd6e7a9ecda61c87dd354b2ed119ab9e7babdd))
- *(types)* Add path-state types with clap-side checks (#502) ([`345ad3d`](https://github.com/s0undt3ch/ToolR/commit/345ad3d1688870fc2e2931a7f5932c9b422a665f))
- *(types)* [**breaking**] Remove arg(must_*) in favour of path types (#502) ([`1762d42`](https://github.com/s0undt3ch/ToolR/commit/1762d4266a24b9803d540d81d6986d08c7ccddd1))
- *(manifest)* Bump schema to 2 and rebuild caches on a schema mismatch (#502) ([`b988538`](https://github.com/s0undt3ch/ToolR/commit/b988538a626e509da37048a0e60eadd29ac72a69))
- *(types)* Expose path types as NewTypes in toolr.types (#502) ([`4b270fe`](https://github.com/s0undt3ch/ToolR/commit/4b270fecbfdd8c0aa24cc02a3f2336b302de2df4))
- *(manifest)* Record the schema each type and field arrived in (#520) ([`9417609`](https://github.com/s0undt3ch/ToolR/commit/9417609062b4475f9f0d72f87a0108286fa02823))
- *(manifest)* [**breaking**] Ship plugin fragments as full manifest commands (#520) ([`a943a78`](https://github.com/s0undt3ch/ToolR/commit/a943a78889df6eac52f7dd0130702ec6ce08dd47))
- *(manifest)* Skip unloadable plugins and record plugin warnings (#520) ([`963d3fe`](https://github.com/s0undt3ch/ToolR/commit/963d3fe2ac15a728eb00ef708da390294e507411))
- *(cli)* Warn about skipped and shadowed plugins on every run (#520) ([`db45679`](https://github.com/s0undt3ch/ToolR/commit/db4567955dd1c688bfa14a6834f3353429bf5c03))
- *(plugins)* Disable a command two plugins define instead of failing (#522) ([`988c776`](https://github.com/s0undt3ch/ToolR/commit/988c776e5d82ec7af78997960975e858fda253df))
- *(dispatch)* Warn when the venv's toolr-py minor version differs from the binary (#529) ([`906ca41`](https://github.com/s0undt3ch/ToolR/commit/906ca41334f3510f9906c8fa4e988af00e5c5bef))

### <!-- 1 -->🐛 Bug Fixes

- *(parser)* Keep Markdown in catalogue cells verbatim ([`718eb88`](https://github.com/s0undt3ch/ToolR/commit/718eb8889cf472c6902b2c9f8c9143428d91d3db))
- *(parser)* Add TypeDoc.note and an ordinal guard against missed catalogue rows ([`835ee0e`](https://github.com/s0undt3ch/ToolR/commit/835ee0e53fd58f04a11554dcdac54af66120d8e9))
- *(xtask)* Give the supported-types table an output flavour for pipe-bearing annotations ([`6ea55da`](https://github.com/s0undt3ch/ToolR/commit/6ea55daed87473586df9e4de62ece5bc14d4cdb5))
- *(xtask)* Reject or flatten docs constructs the section extractor passed through ([`1af4e00`](https://github.com/s0undt3ch/ToolR/commit/1af4e003be406c298d92fb9d71938982a4343887))
- *(xtask)* Map paragraph offsets to each line's own number in docs extractor errors ([`6093f03`](https://github.com/s0undt3ch/ToolR/commit/6093f0341051bd54f5deeaa0142c9dc0dde7c97a))
- *(xtask)* Support named-section includes and drop dangling out-of-section pointers ([`d04ca37`](https://github.com/s0undt3ch/ToolR/commit/d04ca37daf3f1d6efc8041754dd789a365e0d580))
- *(xtask)* Lint every file a skill ships and close gate bypasses ([`460e4c0`](https://github.com/s0undt3ch/ToolR/commit/460e4c085b9faabf509e6ea8b721a95c670669b0))
- *(xtask)* Normalise CRLF sources before generating skill refs ([`ae8fe45`](https://github.com/s0undt3ch/ToolR/commit/ae8fe45ad9e5ab1b96a07c801f6d699fc0e7ceef))
- *(xtask)* Lint unquoted HTML href/src values in skills ([`5d00f7b`](https://github.com/s0undt3ch/ToolR/commit/5d00f7b8ed920212750e99abf096ec922525cd70))
- *(xtask)* Print self-containment violations with forward slashes on every OS ([`52b21fc`](https://github.com/s0undt3ch/ToolR/commit/52b21fc93efcbe6acd16e42bdbfdced183967cbb))
- *(packslip)* Harden packslip-check.sh path resolution and error handling ([`1f904b9`](https://github.com/s0undt3ch/ToolR/commit/1f904b90e7818f67fb7996eae52ce14236d35fb4))
- *(release)* Publish the packslip after a partial release failure and pin the verify identity ([`c0dffcd`](https://github.com/s0undt3ch/ToolR/commit/c0dffcde1a4b7b67e81910f99c41dd628b07d2c6))
- *(packslip)* Require each skill resource's repo path to be skills/<name> ([`72cf981`](https://github.com/s0undt3ch/ToolR/commit/72cf98191c7d8bc95dd424071f0c79b08ba15747))
- *(ci)* Satisfy mypy in tools/ci.py's generate-build-matrix ([`485e11f`](https://github.com/s0undt3ch/ToolR/commit/485e11f41ab89a3b1eadae830970eaf880b7cb83))
- *(parser)* Reject unknown arg() keywords at manifest build (#500) ([`91ec5e7`](https://github.com/s0undt3ch/ToolR/commit/91ec5e7a7d2f784bc52475a676c5fb0bb81f7b7a))
- *(parser)* Check nested and aliased arg() calls, only toolr's (#500) ([`3e9914b`](https://github.com/s0undt3ch/ToolR/commit/3e9914b791fcd9e24223e18b795407443bc9811a))
- *(parser)* Scope arg() checks to what each module can see (#500) ([`cd09eff`](https://github.com/s0undt3ch/ToolR/commit/cd09eff64efb8edcffb55e9589400ad2dbf2c2d2))
- *(parser)* Follow star-imported aliases and binding order in arg() check (#500) ([`eb8bc52`](https://github.com/s0undt3ch/ToolR/commit/eb8bc5244ee843e90886fe5056dbe3c6cbcaf52d))
- *(parser)* Don't guess aliases behind unparsed star imports (#500) ([`70f0dd7`](https://github.com/s0undt3ch/ToolR/commit/70f0dd762cdcce8e3d7bbaaee902af4dbf80c57d))
- *(xtask)* Make the docs section extractor faithful to mkdocs (#505) ([`92db602`](https://github.com/s0undt3ch/ToolR/commit/92db6021862dd1c66c910a171f4c9cae509c18bc))
- *(console)* Drop dim modifier from log-debug/log-info styles ([`7999a00`](https://github.com/s0undt3ch/ToolR/commit/7999a008f4c67b613c9b0bc5dc642fcc954466b0))
- *(xtask)* Lint the files a skill ships, not its directory tree (#504) ([`1d7e76b`](https://github.com/s0undt3ch/ToolR/commit/1d7e76bfd4584b8e308f790a795188fb272393da))
- *(xtask)* Flag backticked `../` paths missing inside the skill (#504) ([`ea0381a`](https://github.com/s0undt3ch/ToolR/commit/ea0381a231cd45bb272b9a888f73a5560d8abc7b))
- *(xtask)* Lint prefixed HTML href/src attributes (#504) ([`d1eb41d`](https://github.com/s0undt3ch/ToolR/commit/d1eb41d58983e32f6e48ddd15ce77e59adbb521b))
- *(xtask)* Report malformed Markdown at the offending line (#504) ([`2081ee2`](https://github.com/s0undt3ch/ToolR/commit/2081ee2f7272f62c75897ed8d61f09f80c744e49))
- *(xtask)* Match repo URLs on a path-segment boundary (#504) ([`c41ee50`](https://github.com/s0undt3ch/ToolR/commit/c41ee50f37e97cd17095d3da54b7422a561afb3e))
- *(execute)* Read every path type back as a path (#502) ([`bc9b1a8`](https://github.com/s0undt3ch/ToolR/commit/bc9b1a8669eb8f7c42a8379773a90387b6c23001))
- *(types)* Tighten path-check errors and document path-type limits (#502) ([`9bd98e4`](https://github.com/s0undt3ch/ToolR/commit/9bd98e4628f8a94a5df2d89da3564c0263bb9f7d))
- *(runner)* Report an unknown arg() keyword at import as a clear error (#502) ([`8990988`](https://github.com/s0undt3ch/ToolR/commit/899098862cd32b3bdadcc600f8a867afe9f016eb))
- *(runner)* Catch a removed arg() keyword when annotations evaluate lazily (#502) ([`fc3338c`](https://github.com/s0undt3ch/ToolR/commit/fc3338c44f97025f2d741d9c08b183b81ad3a8a8))
- *(types)* Hand Windows canonical paths back without the \\?\ prefix (#502) ([`2a3001b`](https://github.com/s0undt3ch/ToolR/commit/2a3001b6bafdd7c974310662052a5e34951f3910))
- *(manifest)* Dedup groups by full path, not leaf name (#520) ([`04efd1a`](https://github.com/s0undt3ch/ToolR/commit/04efd1a13c236231d57c573c48093f250ebc7b93))
- *(ci)* Harden packslip-check against odd statements and dot-dirs (#508) ([`d17884b`](https://github.com/s0undt3ch/ToolR/commit/d17884b5dc4c886357ac88a473b06b88196bfd29))
- *(ci)* Label the packslip CLI pin as a GitHub Actions dependency (#508) ([`f699276`](https://github.com/s0undt3ch/ToolR/commit/f699276b9b8588e80d1b58eecede75811508b2ea))
- *(dispatch)* Re-sync a stale tools venv before running a command (#527) ([`0f58c7f`](https://github.com/s0undt3ch/ToolR/commit/0f58c7f1aeb27b75e44875f772c3dd205f975a42))

### <!-- 2 -->🚜 Refactor

- *(parser)* Drop ordinal guard from type catalogue ([`b3fa5b6`](https://github.com/s0undt3ch/ToolR/commit/b3fa5b658ca31d2c3299bfc254702472ede115da))
- *(xtask)* Derive self-containment URL list from project metadata ([`a5d51a6`](https://github.com/s0undt3ch/ToolR/commit/a5d51a6cfee4f75e83a5f57c8fb893962e7b364f))
- *(ci)* Move the packslip manifest check into toolr ci packslip-check ([`254fa42`](https://github.com/s0undt3ch/ToolR/commit/254fa420d4b8d080be769bc2fc510f9dc227031a))
- *(parser)* Build the type catalogue from an exhaustive kind mirror (#503) ([`e534da6`](https://github.com/s0undt3ch/ToolR/commit/e534da619e98667c41b67e74e3b8035bce640904))
- *(parser)* Share one builder between tools/ and plugins (#520) ([`a3568cf`](https://github.com/s0undt3ch/ToolR/commit/a3568cf2ef648bed1720387806f06ad571612498))

### <!-- 3 -->📚 Documentation

- *(specs)* Add context-local helpers design ([`a3e3191`](https://github.com/s0undt3ch/ToolR/commit/a3e3191da324543fe35c97ab1dcbd1defb5dfabc))
- *(specs)* Address adversarial review of context-local helpers spec ([`cfc0937`](https://github.com/s0undt3ch/ToolR/commit/cfc0937d0b7a272dc0fb7d2b1e93a61cf76e1226))
- *(specs)* Add context-local helpers implementation plan ([`3002977`](https://github.com/s0undt3ch/ToolR/commit/3002977b8975fd5a71071abc6b4bd66cda1e6060))
- *(toolr-command-authoring)* Document current_context() as a helper option ([`af5cebf`](https://github.com/s0undt3ch/ToolR/commit/af5cebfd50d38f9b7ad88a5ae2d13446dceb823e))
- *(changelog)* Queue release note for current_context() ([`ec26239`](https://github.com/s0undt3ch/ToolR/commit/ec2623990791fc4d5c04b63ae086174469693ce8))
- *(toolr-py)* Stop citing internal spec path in current_context() docstring ([`ee77edf`](https://github.com/s0undt3ch/ToolR/commit/ee77edfe2f5f37950a88299d30acf40952522621))
- *(toolr-command-authoring)* Note module-scope caveat for current_context() ([`5939558`](https://github.com/s0undt3ch/ToolR/commit/5939558b23426274066886e2c6e77683bd049885))
- *(reference)* Add current_context to the mkdocs site ([`8e39cdc`](https://github.com/s0undt3ch/ToolR/commit/8e39cdcb3b13292cbd5da116e6f779a64ce6ea82))
- *(specs)* Add self-contained agent skills design ([`b3aefce`](https://github.com/s0undt3ch/ToolR/commit/b3aefcea12e7bd6c27ce1e24236f39bede3eaf12))
- *(specs)* Add self-contained agent skills implementation plan ([`ebde75c`](https://github.com/s0undt3ch/ToolR/commit/ebde75c95d7c095aa42dcf11c1975fd9bec41384))
- *(skills)* Inline facts the skills relied on docs for; reference sibling skills by name ([`8b6baf0`](https://github.com/s0undt3ch/ToolR/commit/8b6baf0fe8ead7f3dcd91b869e58966cb5473ad0))
- Note self-contained skills in release notes and skills guide ([`e6c3725`](https://github.com/s0undt3ch/ToolR/commit/e6c3725cb67173b41be2425261c998d2e73c4e3d))
- *(writing-commands)* Correct stale path_must_* keyword docs ([`0068ec6`](https://github.com/s0undt3ch/ToolR/commit/0068ec69e101a331a55cae2c4f7039e03df08200))
- Sharpen self-contained-skills wording and list generated skill refs ([`b24cd6a`](https://github.com/s0undt3ch/ToolR/commit/b24cd6abef082eaa7acf2128d15a32eb4c1f8966))
- Separate never-accepted path_must_* spellings from deprecated kwargs ([`b7d89cb`](https://github.com/s0undt3ch/ToolR/commit/b7d89cb10bbff5a32d103690bcea083f12a38ee7))
- *(writing-commands)* Use the canonical @group.command form in single-file examples ([`c634aee`](https://github.com/s0undt3ch/ToolR/commit/c634aee0350d2884171b6675f4537d1de3aea686))
- *(internals)* The bound @group.command form is canonical, not deprecated ([`56ce228`](https://github.com/s0undt3ch/ToolR/commit/56ce228a10edfaad8da634410c6e067f4a8363f8))
- *(specs)* Add packslip release manifest design ([`e92a5c0`](https://github.com/s0undt3ch/ToolR/commit/e92a5c0530a74601c686c282b886370a0df4b046))
- *(specs)* Add packslip release manifest implementation plan ([`2733672`](https://github.com/s0undt3ch/ToolR/commit/2733672b97aac36689ae709fdb61bc99fca6f2f3))
- Document installing toolr and its skills through mise's packslip backend ([`76b330e`](https://github.com/s0undt3ch/ToolR/commit/76b330e43177e9661b345515876f86c6288c87bc))
- *(mise)* Don't imply a version cut-over in the aqua fallback example ([`b976016`](https://github.com/s0undt3ch/ToolR/commit/b976016e60776f5074937e3c2e5c33e6c330cf18))
- *(packslip)* Fix release-age/identity wording and don't imply a pinned version exists yet ([`2799dc5`](https://github.com/s0undt3ch/ToolR/commit/2799dc5145a316b721b035e892289a81eff03cda))
- *(specs)* Document packslip partial-failure recovery and the CLI install path ([`ef87462`](https://github.com/s0undt3ch/ToolR/commit/ef87462759f23b0939d90bddc2d65e456dcd7509))
- Note that unknown arg() keywords now fail the manifest build ([`9045e7d`](https://github.com/s0undt3ch/ToolR/commit/9045e7d82030c96f5e535c44d376d4e2a0ec5fd6))
- Note nested arg() checks and argparse-keyword hints (#500) ([`26f0d0a`](https://github.com/s0undt3ch/ToolR/commit/26f0d0a10257c367b81e6483fcfa18b478cd5131))
- *(skills)* Cover external command sources in the authoring skill (#507) ([`bcb8c78`](https://github.com/s0undt3ch/ToolR/commit/bcb8c78e3ea11cb9a5612a9b6f4432705548ca71))
- *(third-party)* Fix the hatchling recipe and fragment-schema claim (#506) ([`8a4a524`](https://github.com/s0undt3ch/ToolR/commit/8a4a524a8469d15ef42220e0c6372b57522603c7))
- *(third-party)* Ship the manifest via setuptools package-data (#506) ([`6aee50c`](https://github.com/s0undt3ch/ToolR/commit/6aee50c27ca2c6f6fd69b6b66e34e7f1c173a4f3))
- *(contributing)* Add a checklist for adding a supported type (#503) ([`8d93388`](https://github.com/s0undt3ch/ToolR/commit/8d93388a1ea5ba1e56264a6acd123025c419abad))
- *(specs)* Design path-state types for toolr.types (#502) ([`f472743`](https://github.com/s0undt3ch/ToolR/commit/f472743ec7bc580009d49936e996424f2203b13c))
- *(specs)* Cover xtask, docs and local-cache fallout in path-state types design ([`39d8d14`](https://github.com/s0undt3ch/ToolR/commit/39d8d14e4808cf718557aafd0caee9290b5d0591))
- *(specs)* Adopt pydantic-style path type names, record rejected composite form ([`e70f7cd`](https://github.com/s0undt3ch/ToolR/commit/e70f7cdef2a52965a103d215a0bf52b4e36f1c83))
- *(specs)* Enforce manifest schema bump, keep fragment schema, place checks in rust ([`94b0dc1`](https://github.com/s0undt3ch/ToolR/commit/94b0dc175f0bbd0449983ec5d146bb5523d61c62))
- *(specs)* State the plugin type-checking gap precisely ([`0b776bb`](https://github.com/s0undt3ch/ToolR/commit/0b776bb9efcb455558d52552795c1310652d0f30))
- *(specs)* Add path-state types implementation plan (#502) ([`83db8bb`](https://github.com/s0undt3ch/ToolR/commit/83db8bb10ce3e98d25e51112edb13451253c913e))
- *(specs)* Correct the derived-path typing claim and pin the suggestion path ([`c33b23e`](https://github.com/s0undt3ch/ToolR/commit/c33b23e1eb7546f375607cb895f38172f8c95d55))
- *(types)* Document path types and the arg(must_*) removal (#502) ([`183fd79`](https://github.com/s0undt3ch/ToolR/commit/183fd790690318388d00d495df549991f3bd1c91))
- *(specs)* Archive the path-state types design and plan (#502) ([`a692500`](https://github.com/s0undt3ch/ToolR/commit/a692500f5966788d05d0e95ff3bc55bff521e677))
- *(unreleased)* Note plain canonical paths on Windows (#502) ([`868afe3`](https://github.com/s0undt3ch/ToolR/commit/868afe36d1dbb424cc5a5611a877a3d5dc0a7175))
- *(specs)* Design plugin command parity and fragment v2 (#520) ([`50b2e44`](https://github.com/s0undt3ch/ToolR/commit/50b2e44b4d9524bc3cc590e147235e6c0e0af8b6))
- *(specs)* Fold adversarial review into the plugin parity design (#520) ([`4030418`](https://github.com/s0undt3ch/ToolR/commit/403041864dfed4ebd6f9383d7bfd4b1ad650f99d))
- *(specs)* State the full-path group identity invariant (#520) ([`33b24ba`](https://github.com/s0undt3ch/ToolR/commit/33b24baac6129efab8a75c440f1392323f413b5b))
- *(specs)* Warn when a local command hides a plugin command (#520) ([`8301a9f`](https://github.com/s0undt3ch/ToolR/commit/8301a9fed6f2c3ec2b9ebb321baeb80d5c1da197))
- *(specs)* Fold second adversarial review into the plugin parity design (#520) ([`afd3db2`](https://github.com/s0undt3ch/ToolR/commit/afd3db233333d3fb7e8b2dc91c805696782dfbb1))
- *(specs)* Plan the plugin parity implementation (#520) ([`e321178`](https://github.com/s0undt3ch/ToolR/commit/e321178d52e6bd310b15e1186b4ba6e5364d542f))
- *(plugins)* Document fragment v2, the load rule and plugin warnings (#520) ([`8c74433`](https://github.com/s0undt3ch/ToolR/commit/8c74433df3b7d9a58e9d6a8c23f9fb04d8f53123))
- *(plugins)* State the toolr 0.34.0 requirement for rebuilt plugins (#520) ([`3335902`](https://github.com/s0undt3ch/ToolR/commit/333590294d39a2870f6bc905f63a011cc71e3674))
- *(specs)* Archive the plugin parity design and plan (#520) ([`380b3f5`](https://github.com/s0undt3ch/ToolR/commit/380b3f5239493657e3ba85ca4034beca2780584b))
- *(specs)* Design plugin clash resolution (#522) ([`cbd25c9`](https://github.com/s0undt3ch/ToolR/commit/cbd25c94d4dee9a16a80c32391887fcd12136b2b))
- *(specs)* Address adversarial review of clash resolution design (#522) ([`9500ae0`](https://github.com/s0undt3ch/ToolR/commit/9500ae066715f3e8e994d7f4b5fe4e6d82231db7))
- *(specs)* Narrow #522 to disabling plugin command conflicts ([`1a62517`](https://github.com/s0undt3ch/ToolR/commit/1a6251726a0af1d026447eceda6db28bb2084fd3))
- *(specs)* Address adversarial review of plugin conflicts design (#522) ([`cb6f8af`](https://github.com/s0undt3ch/ToolR/commit/cb6f8af6875f36f1008487165d837dcb7c749f8b))
- *(specs)* Plan the plugin command conflicts change (#522) ([`b7d0869`](https://github.com/s0undt3ch/ToolR/commit/b7d0869a26794eb531352ba9a1eb9f5bf9ebeda9))
- *(specs)* Address adversarial review of the plugin conflicts plan (#522) ([`1b463fd`](https://github.com/s0undt3ch/ToolR/commit/1b463fd18d62eeb1732b1c1cca45eed16b7e064c))
- *(plugins)* Document disabled plugin command conflicts (#522) ([`5f36ad8`](https://github.com/s0undt3ch/ToolR/commit/5f36ad8ab2e3d76be98296cd35a4e87a9b8b07b1))
- *(specs)* Archive the plugin command conflicts design and plan (#522) ([`4ea28ad`](https://github.com/s0undt3ch/ToolR/commit/4ea28ad911ae923f0bbb5488a79995163faa0d3f))

### <!-- 6 -->🧪 Testing

- *(xtask)* Cover docs-section extraction edge cases and generator error paths ([`c07fda2`](https://github.com/s0undt3ch/ToolR/commit/c07fda2a5dd4b9e858545d8273641b2e6a98550b))
- *(skills)* Regenerate the authoring examples manifest snapshot ([`743b875`](https://github.com/s0undt3ch/ToolR/commit/743b875691b244367215b59d3d90ec351b41d9c1))
- *(cli)* Cover unknown arg() keywords failing the manifest build ([`0d4ce23`](https://github.com/s0undt3ch/ToolR/commit/0d4ce23cc80b556248629157ce146cfe33afb68f))
- *(parser)* Cover the arg() check's remaining branches (#500) ([`935b9fc`](https://github.com/s0undt3ch/ToolR/commit/935b9fc3955ab4403d056ffa3b1017701dc6a918))
- *(cli)* Pin path-type rejections end to end (#502) ([`d9af38a`](https://github.com/s0undt3ch/ToolR/commit/d9af38a8341b7ce129cc6f2353074ba532b9e510))
- *(types)* Make the stale-keyword and permission tests hold on every Python and user ([`10f9f5e`](https://github.com/s0undt3ch/ToolR/commit/10f9f5ebc6fc130c3d6bf86ae06978273b323bf7))
- *(manifest)* Make the ArgumentKind and Nargs golden tables catch a forgotten variant (#520) ([`c102af2`](https://github.com/s0undt3ch/ToolR/commit/c102af2f7cd0241b0b34fbe1466a28dd1ee84fe1))
- *(cli)* Tighten plugin warning tests (#520) ([`f64fb9b`](https://github.com/s0undt3ch/ToolR/commit/f64fb9be60a9ebeb1460073ada12b2b08ac87377))
- *(manifest)* Cover host title winning and no-venv carry-forward (#520) ([`0f4a8e3`](https://github.com/s0undt3ch/ToolR/commit/0f4a8e34b658a63e640a7b02b1ce976d6e1e75c5))
- *(plugins)* Cover a plugin conflict on a fresh project and its recovery (#522) ([`68f153b`](https://github.com/s0undt3ch/ToolR/commit/68f153b0e53f487bfbe5f57ae1c72bc147fffab5))
- *(plugins)* Pin conflict serialisation and completion, document the local-command remedy (#522) ([`fb3ee86`](https://github.com/s0undt3ch/ToolR/commit/fb3ee8616e6876cf4ae4b596c80d90f1fb1ce578))
- *(dispatch)* Keep system PATH out of the stale-venv tests (#527) ([`f278843`](https://github.com/s0undt3ch/ToolR/commit/f2788437efb68dae6307c4bec592f89108acd574))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(specs)* Archive context-local-helpers design and plan ([`178063b`](https://github.com/s0undt3ch/ToolR/commit/178063b07e32eb6901f03cbdc861f5c1c7e5e04a))
- Widen build-skill-refs hook inputs and fail on untracked generated files ([`d5c5c4d`](https://github.com/s0undt3ch/ToolR/commit/d5c5c4dcf3a7221c9dd99e0f6ef5002453161de8))
- *(specs)* Archive self-contained skills design and plan ([`64eab09`](https://github.com/s0undt3ch/ToolR/commit/64eab09c9e46d09f59ab510244bbaa2d66185348))
- *(packslip)* Add shared release manifest and offline check script ([`46c986e`](https://github.com/s0undt3ch/ToolR/commit/46c986ef0409707dbf65b44af8790f1ef33f5150))
- *(packslip)* Check the release manifest against CI-built archives on every PR ([`dfcd4fb`](https://github.com/s0undt3ch/ToolR/commit/dfcd4fb5f33512f192a4467020e4d7818d665040))
- *(release)* Sign and publish a packslip for each release ([`3a52202`](https://github.com/s0undt3ch/ToolR/commit/3a5220245de6c59ee60724fcba5f23019b7263fd))
- *(renovate)* Bump the packslip CLI pin together with the action ([`69d1fee`](https://github.com/s0undt3ch/ToolR/commit/69d1feeeed20e97d4dd30100ae2b619bd34934c4))
- *(specs)* Archive packslip design and plan ([`142e83a`](https://github.com/s0undt3ch/ToolR/commit/142e83a64abd64c51caea3e49bcc2f3fca7e6570))
- *(packslip)* Run the manifest check through toolr and verify the packslip CLI attestation ([`f54f68c`](https://github.com/s0undt3ch/ToolR/commit/f54f68ccff111d04862a915ec638d855d6c0ff20))
- *(packslip)* Bind the CLI attestation to its release tag ([`c20f4da`](https://github.com/s0undt3ch/ToolR/commit/c20f4da158a3a8a7c57c52d88a3443da236819f6))
## 0.33.0 - 2026-09-12

### Notes

### Breaking

- Linux release archives are now musl-only. The `x86_64-unknown-linux-gnu` and
  `aarch64-unknown-linux-gnu` binaries are no longer published — the musl
  builds are statically linked and run fine on glibc hosts, so shipping both
  only created ambiguity for downstream packaging (aqua-registry's Renovate
  updater disagreed with mise over which libc the plain `linux-x64` key
  should resolve to). If you need a gnu build, `cargo build --target
  x86_64-unknown-linux-gnu` still works from source; `installation/install.sh`
  now always resolves Linux hosts to the musl asset.

### Features

- `ctx.run(...)` gains an `interactive: bool = False` parameter. It inherits
  the real stdin/stdout/stderr instead of piping them, so a command that
  needs a real terminal — `$EDITOR`, `sops`, a login prompt — sees a TTY on
  all three streams. Previously the child's stdout/stderr were always piped
  regardless of `stream_output`/`capture_output`, so any command that gates
  on `isatty()` (full-screen editors especially) opened and immediately
  exited. `interactive=True` is incompatible with `capture_output`,
  `stream_output`, `no_output_timeout_secs`, and `input` — all four require
  piping. (#485)

- Tab completion now shows a one-line description next to each group/command
  candidate, sourced from the group's title or the command's summary. fish
  renders these natively (each `__complete` line is now `value<TAB>description`,
  which fish already understands); zsh switches to a one-per-line listing via
  `compadd -d` when there are 20 or fewer candidates, falling back to the
  previous compact grid above that. bash has no per-candidate description
  support, so its script now strips the description before handing
  candidates to `compgen` — its completion behavior is otherwise unchanged.

  An already-installed zsh or bash completion script predates this
  `value<TAB>description` wire format and won't parse it correctly; re-run
  `toolr self completion install <shell> --force` after upgrading. fish's
  installed script needs no change — fish already splits candidates on tab
  natively.

### Fixes

- `DispatchCommand.argv` no longer emits a bare flag for a `nargs="+"`/`"*"`
  keyword argument that has no values. A `repeated` argument with an empty
  list in `command_args` used to reconstruct as a valueless flag — argparse
  rejects that outright for `"+"`, breaking any dispatched command with two
  or more `nargs="+"` keyword arguments (including mutually exclusive
  `nargs="+"` pairs, which were unusable entirely). The flag is now omitted
  for an empty list on both arities, since `command_args` can't distinguish
  "typed with zero values" from "never typed" for either one. (#483)

### <!-- 0 -->🚀 Features

- *(skill-refs)* Recognise attribute-docstring fields ([`c9236c5`](https://github.com/s0undt3ch/ToolR/commit/c9236c5b9d1b3cdf7b646033f3a30082139d56ad))
- *(command)* Add interactive=True to ctx.run for real TTY passthrough ([`32dca8a`](https://github.com/s0undt3ch/ToolR/commit/32dca8a76be2421617b3f5658eda545fd692dc5c))
- *(complete)* Show group/command descriptions in tab completion ([`f447390`](https://github.com/s0undt3ch/ToolR/commit/f447390120031b286172205dbc4993eff5373656))

### <!-- 1 -->🐛 Bug Fixes

- *(release)* Ship musl-only Linux binary archives ([`68cb10b`](https://github.com/s0undt3ch/ToolR/commit/68cb10b74fafdec0f0842f96a32fe7f32febff63))
- *(action)* Resolve musl-only Linux triple in setup-toolr ([`f483214`](https://github.com/s0undt3ch/ToolR/commit/f483214f3d0212ad537e3aa7ec03b816243ad533))
- *(ci)* Download the musl binary archive in _test.yml/_bench.yml ([`eb78cba`](https://github.com/s0undt3ch/ToolR/commit/eb78cba5f11f8076e75b1db1b6a3297523276bb1))
- *(skill-refs)* Render class fields, link SKILL.md to their anchors ([`f74c456`](https://github.com/s0undt3ch/ToolR/commit/f74c4563b0bcb744886a5b7558600f330b55de7a))
- *(ci)* Enable git core.longpaths on Windows CI runners ([`a60626c`](https://github.com/s0undt3ch/ToolR/commit/a60626ca829d0665898dd2c01c4ffc4b1cf887ec))
- *(sources)* Omit empty nargs="+"/"*" flag in DispatchCommand.argv ([`9289b44`](https://github.com/s0undt3ch/ToolR/commit/9289b44b256667a0e0780ab88d3f659459fc201f))

### <!-- 3 -->📚 Documentation

- *(unreleased)* Fix stale nargs="*" claim in #483 release note ([`b8b2fad`](https://github.com/s0undt3ch/ToolR/commit/b8b2fad86123e5490f09f99932cc9166333579e4))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(mise)* Bump mise and pinned python patch versions ([`b763f14`](https://github.com/s0undt3ch/ToolR/commit/b763f14e8537a5540dc85632b6f7312107ecec47))
## 0.32.0 - 2026-08-23

### Notes

**BREAKING (GitHub Actions consumers):** removed the rolling-tag sync mechanism
(`sync-rolling-tags.yml`, `toolr ci sync-rolling-tags`) that force-pushed floating `latest`/`vX`/`vX.Y`
git tags on release. Those tags still exist but will no longer move — anyone with `uses:
s0undt3ch/ToolR@latest`, `@v1`, or `@v1.2`-style pins is now frozen on whatever commit last synced,
with no error to signal it. Pin to an explicit released tag or commit SHA instead (the recommended
form has always been `uses: s0undt3ch/ToolR@<sha> # vX.Y.Z`, per the `toolr-ci-setup` skill).

### <!-- 3 -->📚 Documentation

- Flag rolling-tag removal as breaking for GH Actions consumers ([`ef21e6c`](https://github.com/s0undt3ch/ToolR/commit/ef21e6cd8ebf6ef5bcda157737e3a176b96795b6))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Remove rolling-tag sync mechanism ([`8406975`](https://github.com/s0undt3ch/ToolR/commit/8406975a8056136c9215a13797386cb6d331025c))
- *(docs)* Skip re-sync in mkdocs build step ([`570141a`](https://github.com/s0undt3ch/ToolR/commit/570141a192002dd8ee5834ce41e47a9c07fa46f2))
## 0.31.1 - 2026-08-23

### <!-- 1 -->🐛 Bug Fixes

- *(renovate)* Split toolr-py versioning from mise/gh-actions regex ([`77f0ae5`](https://github.com/s0undt3ch/ToolR/commit/77f0ae5076ab6aebb54e0a98fd055c2bae819b75))
- *(deps)* Declare attrs as a toolr-py runtime dependency ([`c1e990f`](https://github.com/s0undt3ch/ToolR/commit/c1e990fe2a0288bb117c97043bc545c7a86a07d9))

### <!-- 3 -->📚 Documentation

- *(mise)* Warn against grouping regex versioning with pep621 ranges ([`8004411`](https://github.com/s0undt3ch/ToolR/commit/80044118628572918268dc462efeece0cc09a825))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(matrix)* Narrow PR Windows/macOS tests to latest Python only ([`b255c51`](https://github.com/s0undt3ch/ToolR/commit/b255c514c73cd07fda7049f9c629e530ef3440a9))
## 0.31.0 - 2026-08-21

### Notes

Fixed #454, a regression from the #449 enum-collision fix: importing an `Enum`-typed class from a
different module than the one declaring it could hard-fail `toolr project manifest rebuild` with
"type is not supported" if another, unrelated module also declared a same-named enum class. The
static parser now follows the actual import — relative imports, `__init__.py` re-exports,
`as`-aliasing chains, and `try`/`except ImportError` dual-path imports all resolve to the class
that's genuinely in scope, rather than guessing across same-named classes elsewhere. Two constructs
that can never be resolved unambiguously — `from foo import *` and `import foo.bar` +
`foo.bar.X`-style attribute-chain usage — now get a specific, actionable error instead of the
generic "type is not supported" message.

Also fixed: `Environment | None`-style `Optional`-wrapped `Enum`/`Literal` parameters previously
built successfully but silently populated `allowed_values` as empty — no clap choice validation,
nothing in `--help`.

`if TYPE_CHECKING:`-guarded enum imports (module-level only) now work end-to-end, not just at
build time — the runner lazily imports the real class at coercion time instead of relying on the
target module's own (never-executed) guarded import, so the previous silent "whole command's
argument coercion disabled" failure mode when one annotation was unresolvable no longer applies to
these cases either.

### <!-- 0 -->🚀 Features

- *(parser)* Resolve command-signature enums through explicit imports ([`a1ba8fc`](https://github.com/s0undt3ch/ToolR/commit/a1ba8fc1d15d622f884947364370fba2b857f8a9))
- *(parser)* Specific errors for star imports and attribute-chain usage ([`8b92e31`](https://github.com/s0undt3ch/ToolR/commit/8b92e31ffdf2898ff976a91700c38570513a9e9c))
- *(runner)* Make TYPE_CHECKING-guarded enum imports work at runtime ([`c5ba349`](https://github.com/s0undt3ch/ToolR/commit/c5ba349736b67646dd687b22fd66e1fa0d463327))

### <!-- 1 -->🐛 Bug Fixes

- *(renovate)* Pin toolr release rule to full semver tags ([`fd04848`](https://github.com/s0undt3ch/ToolR/commit/fd0484877ecce963b844cba21f016faa248f1e6d))
- *(ci)* Keep uv/prek/ruff/toolr on musl Linux artifacts in mise.lock ([`9103c50`](https://github.com/s0undt3ch/ToolR/commit/9103c5045de365dc5b16ba8608530e546fbd7f09))
- *(parser)* Treat identically-shaped cross-module enum defs as unambiguous ([`e08f52c`](https://github.com/s0undt3ch/ToolR/commit/e08f52c75423d661e954276e078b02d6a2df3f3b))
- *(parser)* Populate allowed_values for Optional-wrapped Enum/Literal args ([`7d97d51`](https://github.com/s0undt3ch/ToolR/commit/7d97d51f1c0ea987a00bc2da03f39e0fd60204fc))
- *(parser)* Mark SupportedType::Enum.module serde-default, no version bump needed ([`abd5556`](https://github.com/s0undt3ch/ToolR/commit/abd55566dcda4f9b7b12a0b5b04ee464f54ccfc9))

### <!-- 3 -->📚 Documentation

- *(skills)* Add monitor-pr, a CI-babysitting loop for toolr PRs ([`11bc821`](https://github.com/s0undt3ch/ToolR/commit/11bc8216be77f1470409eca38104101cba20e43b))
- *(claude)* Document version-bump judgment and regression-test discipline ([`a0d74c3`](https://github.com/s0undt3ch/ToolR/commit/a0d74c389fb1b9ee980b910503483e1081583f99))
- *(comments)* Drop bare issue-number references from code comments ([`9e082c7`](https://github.com/s0undt3ch/ToolR/commit/9e082c7db53a3a075e023993845a47abe4f13b53))

### <!-- 6 -->🧪 Testing

- *(toolr)* End-to-end coverage for cross-module enum resolution ([`2f0579d`](https://github.com/s0undt3ch/ToolR/commit/2f0579d1cd71747ecba1aa3b9ef76c99641a92c4))
- Close patch-coverage gaps flagged by codecov on this PR ([`0d91ea5`](https://github.com/s0undt3ch/ToolR/commit/0d91ea50ee33cbd55d94b6f7567cb4f448b30f4a))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Bump pinned mise to v2026.8.10 ([`1a3d7cf`](https://github.com/s0undt3ch/ToolR/commit/1a3d7cfbfd9f3aa4ed7aa902723759fd5e51d04e))
## 0.30.0 - 2026-08-20

### Notes

- Fixed the manifest builder resolving enum CLI choices by bare class
  name instead of by defining module, which meant two unrelated
  `tools/*.py` modules declaring a same-named `Enum` subclass (e.g.
  `Database`) clobbered each other's `--help` choices depending on
  scan order. `allowed_values` and enum-attribute defaults now resolve
  against the class actually in scope for each module ([#449](https://github.com/s0undt3ch/ToolR/issues/449)).
- `toolr.testing.RunMock` now forwards `assert_called`, `assert_called_once`,
  and `assert_has_calls`, closing gaps in its otherwise-explicit
  `unittest.mock` API subset. A new test asserts the forwarded set stays in
  sync with `Mock`'s own public API going forward.
- `toolr.testing.make_context` now accepts a `width=` override for its
  captured consoles, defaulting to a new `DEFAULT_TEST_CONSOLE_WIDTH`
  (1000 columns) so `ctx.stdout`/`ctx.stderr` assertions never have to
  account for rich wrapping a long line.

### <!-- 0 -->🚀 Features

- *(testing)* Forward assert_called and assert_has_calls on RunMock ([`754f075`](https://github.com/s0undt3ch/ToolR/commit/754f0750ef34d22946cec0e49f380b3ef3e3cd15))
- *(testing)* Let make_context override captured-console width ([`93ae0e7`](https://github.com/s0undt3ch/ToolR/commit/93ae0e70fae6b612bcb3db6c16ba46b43de11e04))
- *(xtask)* Extract toolr.testing's API and a worked example into skill refs ([`ceb5f35`](https://github.com/s0undt3ch/ToolR/commit/ceb5f35946f96ab86e9d981d613b73d9d14fbcb9))

### <!-- 1 -->🐛 Bug Fixes

- *(parser)* Key enum CLI choices by defining module, not bare class name ([`cd84b8a`](https://github.com/s0undt3ch/ToolR/commit/cd84b8a11a8467cfb148408067bac8c5425cdeab))

### <!-- 3 -->📚 Documentation

- *(testing)* Add a ctx.prompt/prompt_input= worked example ([`c0ba2dd`](https://github.com/s0undt3ch/ToolR/commit/c0ba2dd3725750770c458e0afb396e6dc9c46ef1))

### <!-- 6 -->🧪 Testing

- *(parser)* Cover the enum cross-module and Optional[T]/None|T resolution paths ([`5056623`](https://github.com/s0undt3ch/ToolR/commit/50566235850626a76e9427d51777d2c2e3fe536f))
- *(xtask)* Cover testing_examples' renamed/removed-function error path ([`9eaf855`](https://github.com/s0undt3ch/ToolR/commit/9eaf855cdb81c41375257fd22e961e6880c330f1))
## 0.29.0 - 2026-08-19

### Notes

- Fixed the built-in argparse scanner (`[tool.toolr.argparse.*]`) silently
  dropping Django management commands that take no CLI arguments at all —
  e.g. a `BaseCommand` subclass with no `add_arguments` method. The
  scanner used "found zero `add_argument()` calls" as its only signal
  for "this isn't a real command," which also matched genuine zero-arg
  commands. A new opt-in `django = true` block setting recognises a
  module-level `class Command(...)` or `Command = <Name>` alias to a
  same-module class — Django's own loader contract — so zero-arg
  commands are kept while stray helper modules are still excluded.
  Plain argparse blocks (the default) are unaffected. Separately,
  underscore-prefixed filenames (`_helpers.py`, `__init__.py`) are now
  skipped outright for every block, Django or not — that convention is
  plain Python, not Django-specific.

A dispatcher's own flag (e.g. `--dry-run` on a `job` dispatcher wrapping
Django management commands) typed *after* the dispatched subcommand name
was silently forwarded to the wrapped command instead of being rejected —
the dispatcher's own parameter kept its default, so a safety flag like
`--dry-run` had no effect at all. `toolr <dispatcher> <leaf> ... --flag`
now errors, pointing at the correct position, instead of silently
executing for real ([#445](https://github.com/s0undt3ch/ToolR/issues/445)).

### <!-- 1 -->🐛 Bug Fixes

- *(docs)* Drop navigation.tabs so all top-level pages stay reachable ([`befa20d`](https://github.com/s0undt3ch/ToolR/commit/befa20d0b9fe2702ad70fd35110e21774a8ed61a))
- *(deps)* Correct aqua:s0undt3ch/ToolR pin to 0.28.0 ([`af9c93a`](https://github.com/s0undt3ch/ToolR/commit/af9c93a005a6203cde56fb00f3a05a3b0611e8b5))
- *(argparse)* Keep zero-arg Django management commands in the scanner ([`a54dcc7`](https://github.com/s0undt3ch/ToolR/commit/a54dcc79d1e19e0beee3d7fb250ffb14cdb4c039))
- *(argparse)* Gate Command-symbol detection behind an opt-in django flag ([`3698642`](https://github.com/s0undt3ch/ToolR/commit/3698642806537ce7597abd6038c16a38976d62fa))
- *(argparse)* Make the underscore-filename skip unconditional ([`2a2db0b`](https://github.com/s0undt3ch/ToolR/commit/2a2db0bf210789d41925f167859c4c81560cd698))
- *(dispatch)* Reject dispatcher flags leaked into a leaf's trailing args ([`142ba11`](https://github.com/s0undt3ch/ToolR/commit/142ba117beff849fc5760b257e9ad97a13bd17ae))

### <!-- 2 -->🚜 Refactor

- *(argparse)* Rename is_command_class/defines_command_class to django-prefixed ([`5e634da`](https://github.com/s0undt3ch/ToolR/commit/5e634da0c0a91924ea69c755b9e53d75faf9b222))

### <!-- 5 -->🎨 Styling

- *(argparse)* Tighten inline comments to one-or-two-line why statements ([`274e7ec`](https://github.com/s0undt3ch/ToolR/commit/274e7ec3a3a8923dce4dc03d87f2d8b868bcbd87))
- *(argparse)* Rewrap this PR's comments to the repo's 100/120 width policy ([`847039a`](https://github.com/s0undt3ch/ToolR/commit/847039a673ec5d4803ced07bbffa3c5303ec8501))
## 0.28.0 - 2026-08-17

### Notes

- Fixed the `tools/` dogfooding venv lagging its own `toolr-py` pin behind
  the `toolr` binary CI builds from `main` HEAD for up to a week after
  every schema-bumping release, breaking self-CI (`toolr ci
  check-run-build`) for that whole window. The root `exclude-newer = "7
  days"` dependency cooldown, inherited by `tools/`, is meant for
  third-party releases; `toolr-py` is exempted from it specifically since
  it ships from the same release as the binary that runs against it.
- Fixed Ctrl-C during a running command printing a raw Python traceback
  and exiting with an arbitrary code instead of the conventional 130.
  The toolr binary already forwards SIGINT to the Python runner
  subprocess; the runner now catches the resulting `KeyboardInterrupt`
  and exits 130 cleanly instead of leaking it as an unhandled exception.
- Fixed `list[T] | None` and `tuple[...] | None` keyword parameters
  being classified as single-value flags instead of repeatable ones,
  so `--items a` failed the runner's `array | null` type check and
  `--items a --items b` was rejected by clap as a repeated single-value
  flag. The syntactic classifier couldn't see through the `| None`
  union; the type resolver already could, so the fix promotes the
  argument's kind from the resolved type once it's known.
- `toolr.testing.make_context` gained `run`/`chdir`/`prompt_input` parameters, and
  `toolr.testing.RunMock`/`make_command_result` ship as canonical test doubles for `ctx.run`.
  `Context.run`/`chdir`/`prompt` previously had no supported way to intercept them without
  monkeypatching internals; each now reads from an injectable, defaulted field
  (`_run_impl`/`_chdir_impl`/`_prompt_stream`) that `make_context` can override.
  **Compatibility note:** if you were previously patching the module-level function directly
  (`mock.patch("toolr.utils.command.run", ...)` or `monkeypatch.setattr(os, "chdir", ...)`)
  around a `Context` you construct yourself, that patch now silently stops working — the field's
  default is bound to the real function at class-definition time, before your patch runs. Pass
  `_run_impl=`/`_chdir_impl=` explicitly at construction time instead (or use `make_context`'s
  new `run=`/`chdir=` parameters).
- **Breaking:** `make_context` now returns a `ContextForTesting` — a real `Context` subclass —
  instead of the old `ContextForTesting(ctx=, output=)` wrapper pair. The wrapper's `.ctx` and
  `.output` attributes, and the `CapturedOutput` class it used, are gone. Update call sites:
  `result.ctx.run(...)` becomes `ctx.run(...)`, and `result.output.stdout` becomes `ctx.stdout`
  (same for `.stderr`).

### <!-- 0 -->🚀 Features

- *(context)* Make Context.run's subprocess runner injectable ([`a34efe6`](https://github.com/s0undt3ch/ToolR/commit/a34efe680ec9783ba7642bae2ade2271b425081e))
- *(context)* Make Context.chdir's os.chdir call injectable ([`d83b816`](https://github.com/s0undt3ch/ToolR/commit/d83b816486b07bdcb918631338b79cf8ee1d1e33))
- *(context)* Make Context.prompt's input stream injectable ([`ef04889`](https://github.com/s0undt3ch/ToolR/commit/ef04889cac2f71f104f26a0977d04e92dd5aeece))
- *(testing)* Add RunMock and make_command_result ([`9ec5d20`](https://github.com/s0undt3ch/ToolR/commit/9ec5d209c61a807b109b11b639f99cbe1e18d40d))
- *(testing)* Make_context returns ContextForTesting directly, add run/chdir/prompt_input ([`3d91617`](https://github.com/s0undt3ch/ToolR/commit/3d916174e460fabbba9f06c52751860b25f0cfd4))
- *(testing)* Forward assert_not_called/called, document the RunMock.register() removal ([`07717fb`](https://github.com/s0undt3ch/ToolR/commit/07717fb4fbfdee10af8a363818fc39bc72ed2da4))

### <!-- 1 -->🐛 Bug Fixes

- *(ci)* Exempt toolr-py from the tools/ dependency cooldown ([`e8c807d`](https://github.com/s0undt3ch/ToolR/commit/e8c807de5a52c455dfd1d0c8fc5299306108fe54))
- *(runner)* Return exit code 130 on KeyboardInterrupt instead of a raw traceback ([`79525b9`](https://github.com/s0undt3ch/ToolR/commit/79525b9b364df2b47c30bcd7feeef85efa0028a4))
- *(mise)* Bump toolr binary pin to 0.27.0 to match tools/uv.lock's toolr-py ([`f527a8e`](https://github.com/s0undt3ch/ToolR/commit/f527a8ebdd982089f6c3d4f2e8ac23a819802b40))
- *(parser)* Classify list[T] | None and tuple[...] | None as repeatable ([`77a1276`](https://github.com/s0undt3ch/ToolR/commit/77a1276f9f5159e11cfa0bd0f0592568fce4dae1))
- *(context)* Revert scope creep and fix remaining patching test ([`eaaabf9`](https://github.com/s0undt3ch/ToolR/commit/eaaabf9770bc51b20c1f123be6ed4dbc403b08f8))
- *(testing)* Raise clear error on unconfigured RunMock, cover side_effect guard ([`d09156c`](https://github.com/s0undt3ch/ToolR/commit/d09156cfc8768e33cb50b026a6b9f3e42ebbbd54))
- *(testing)* RunMock.args reflects actual invocation, document password-prompt limitation ([`4b330ad`](https://github.com/s0undt3ch/ToolR/commit/4b330ad79e0d8df542457aefc65461c6f73c1315))
- *(testing)* Document capture_output applies to RunMock's escape hatch too ([`dbf98d2`](https://github.com/s0undt3ch/ToolR/commit/dbf98d214a2224276f6486b62b511b78e0f509ae))
- *(precommit)* Point mypy hook at the real toolr-py source path ([`a30be71`](https://github.com/s0undt3ch/ToolR/commit/a30be71365e9a216023fae85297f21cb50109fc9))

### <!-- 2 -->🚜 Refactor

- *(testing)* Drop RunMock.register(), fix mypy errors CI caught ([`6a12f4b`](https://github.com/s0undt3ch/ToolR/commit/6a12f4b38c066c2ee010f893cb8b422922936a1d))

### <!-- 3 -->📚 Documentation

- *(changelog)* Queue release note for KeyboardInterrupt exit-code fix ([`771c3cc`](https://github.com/s0undt3ch/ToolR/commit/771c3cc5eb2c00bfb072f5dee0fdead0e4240f73))
- *(specs)* Design mockable Context.run for toolr.testing ([`e641d51`](https://github.com/s0undt3ch/ToolR/commit/e641d51239f4ad16acdfbe8c05e551db61cf2432))
- *(specs)* De-identify caller project in run-mock design ([`f8187bf`](https://github.com/s0undt3ch/ToolR/commit/f8187bfa991e46afc31911c6293c8b1deda7f3b3))
- *(specs)* Fold chdir and prompt mocking into run-mock design ([`d69fb54`](https://github.com/s0undt3ch/ToolR/commit/d69fb54a7bf2a5703bf209d9cb296e9acc1f09e8))
- *(specs)* Fix frozen-struct mutation, MagicMock subclass risk, and stale examples ([`ab549b6`](https://github.com/s0undt3ch/ToolR/commit/ab549b6cac0766cd68587d23e8adb2abad7f6cc7))
- *(specs)* Correct build-skill-refs claim in run-mock design ([`5057114`](https://github.com/s0undt3ch/ToolR/commit/5057114b07cc8968c511663130bf824d831d4d3a))
- *(specs)* Write implementation plan for mockable run/chdir/prompt ([`6da53bd`](https://github.com/s0undt3ch/ToolR/commit/6da53bdfc68421f333050ec5001cff7973699373))
- *(specs)* Replace ContextForTesting wrapper with a Context subclass ([`0c0f41e`](https://github.com/s0undt3ch/ToolR/commit/0c0f41e88a38f63458aa9a89031d1e933746efa4))
- *(specs)* Fix ContextForTesting rename artifacts and update Task 6 doc examples ([`086e0c6`](https://github.com/s0undt3ch/ToolR/commit/086e0c68dd585aa22a53e3406ebbe08c4a597539))
- *(specs)* Fix stale Task 6 file list (only tests/test_make_context.py needed) ([`8f43adb`](https://github.com/s0undt3ch/ToolR/commit/8f43adb6638a00836370a41061b7a20df281efad))
- *(testing)* Fix broken ctx.prompt example, document run/chdir/prompt_input mocking ([`59c66d0`](https://github.com/s0undt3ch/ToolR/commit/59c66d0384f159855a70afdeb55c701e10fe3b7d))
- *(changelog)* Queue release notes for mockable run/chdir/prompt ([`1069ba1`](https://github.com/s0undt3ch/ToolR/commit/1069ba1aee8426cd0feb4de1a7970822e00696d4))
- *(testing)* Fix stability section, breaking-change note, and snippet indentation ([`2c4506f`](https://github.com/s0undt3ch/ToolR/commit/2c4506fb688bffeae63b45fc50544fe71d8caad7))
- *(testing)* Clarify RunMock tie-break, reset_mock, and text/stream_output scope ([`7d641df`](https://github.com/s0undt3ch/ToolR/commit/7d641dfa34bfb9d4aabc3caaa5976f17137e2443))
- *(testing)* Document capture_output applies to the escape hatch too ([`68953c2`](https://github.com/s0undt3ch/ToolR/commit/68953c21d996d1b7f875116b8d4a26962b2ede38))
- *(specs)* Archive mockable run/chdir/prompt design and plan ([`09b371e`](https://github.com/s0undt3ch/ToolR/commit/09b371ea3078125b8db0712e50138a2282d9a8e2))
- *(testing)* Document RunMock.return_value's single-use stream, sync module docstring ([`5f7aa8a`](https://github.com/s0undt3ch/ToolR/commit/5f7aa8a8897c1fe74cc7a95ca8bd93ed0b8b2e1f))
- *(testing)* Add abort-path prompt example, warn about mock.patch(command.run) no-op ([`86d2eed`](https://github.com/s0undt3ch/ToolR/commit/86d2eed023bb147b75ab6e4b8e8f33cbf5a1e576))

### <!-- 5 -->🎨 Styling

- *(context)* Trim caller-referencing comments on the injectable seam fields ([`42b684d`](https://github.com/s0undt3ch/ToolR/commit/42b684d36f273f605303e55930f65e8c5b08c3cc))

### <!-- 6 -->🧪 Testing

- *(context)* Inject run impl instead of patching command.run ([`4d0028d`](https://github.com/s0undt3ch/ToolR/commit/4d0028d2cce63d906c320f8b01fc92c54e976027))
- *(context)* Reuse the ctx fixture via msgspec.structs.replace, drop noqa from doc snippets ([`2959507`](https://github.com/s0undt3ch/ToolR/commit/29595076d376f6dcbf1675b02ea392d4d7dfd481))
- *(context)* Add ContextForTesting.replace(), dogfood it in the ctx fixtures ([`8419c00`](https://github.com/s0undt3ch/ToolR/commit/8419c00e650a0ed7c3c8bf1fc8ed9c79988e73c9))
- *(testing)* Cover the four RunMock forwarders and the prompt_input TextIO branch ([`d319ba4`](https://github.com/s0undt3ch/ToolR/commit/d319ba4b9ce63dbcc35401bbb0d2860e638124a5))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(mise)* Regenerate mise.lock for toolr 0.27.0 ([`a29cfff`](https://github.com/s0undt3ch/ToolR/commit/a29cfffb1a3ee2789c7b13b7e7b8d4e485da9990))
## 0.27.0 - 2026-08-05

### Notes

- Fixed the argparse scanner dropping every flag spelling except the
  longest `--flag` on a call declaring several (e.g.
  `add_argument("-s", "--run-synchronously", ...)`). Short flags and
  extra long spellings now all register as clap aliases.
- Fixed `tests/distribution/test_example_plugin_contract.py` leaking real
  venv provenance stubs into the developer/CI's live cache dir instead of
  a sandboxed one, which inflated `toolr self cache`'s orphan count over
  time.
- Fixed the cache orphan hint (`toolr: cache has N orphan entries (~X)`)
  reporting the whole cache's size instead of just the orphaned entries',
  which made a couple of unrelated live venvs look like the cause of a
  large orphan count.
- Corrected `docs/internals/cache.md`, which falsely claimed multiple git
  worktrees of the same repo share a single venv cache entry — they never
  have. Each worktree gets (and needs) its own.
- Fixed `DispatchCommand.argv` emitting the repeat-the-flag form
  (`--customer-ids a --customer-ids b`) for `nargs="+"`/`"*"` args.
  Standard argparse only keeps the last occurrence for those — every
  value but the last was silently dropped. Now emits one occurrence
  with all values (`--customer-ids a b`), reserving the repeat-the-flag
  form for genuine `action="append"` args.
- The argparse scanner now gives `nargs=N` (fixed-arity), positional
  `nargs="?"`, and `argparse.REMAINDER` first-class support end to end —
  clap arg construction, tab completion, and dispatch argv reconstruction
  all handle these correctly instead of silently degrading to a single
  value with a warning.
- **Breaking (dispatch wire protocol):** bumped the toolr ↔ toolr-py
  dispatch schema version from 2 to 3 to carry the above. If you see
  `toolr-py in this tools venv speaks schema N, but the toolr binary
  emitted schema M`, run `toolr project venv upgrade toolr-py` (or pin
  the `toolr` binary to a compatible version).
- Fixed the manifest freshness check silently keeping a stale
  `.toolr-manifest.json` after a `toolr` upgrade that changed
  CLI-parsing/classification behaviour but touched no `tools/*.py`
  file. The manifest now records the `toolr` version that built it, and
  any mismatch against the running binary forces a rebuild rather than
  trusting the cached hashes.

### <!-- 0 -->🚀 Features

- *(argparse)* Full nargs support — nargs=N, REMAINDER, positional nargs="?" (#423) ([`444b9a2`](https://github.com/s0undt3ch/ToolR/commit/444b9a21db9504023ed4ba7d5ace663fbb032cd5))

### <!-- 1 -->🐛 Bug Fixes

- *(argparse)* Keep every flag spelling as an alias, not just the longest ([`4a0b3b7`](https://github.com/s0undt3ch/ToolR/commit/4a0b3b7f9bb065f3c21dd8fdf9f50cf8e3f21356))
- *(cache)* Stop test leak inflating orphan count, fix misleading orphan size, correct worktree-sharing doc lie ([`43aa750`](https://github.com/s0undt3ch/ToolR/commit/43aa750d97695f78943f9817cc3e6f44c9a613e3))
- *(dispatch)* Emit single-flag-many-values for nargs="+"/"*" args ([`699f5a0`](https://github.com/s0undt3ch/ToolR/commit/699f5a02fdfaa51c96e682d7a1a020c6d18be174))
- *(dispatch)* Omit absent keyword FixedArity args instead of sending [] ([`c51641b`](https://github.com/s0undt3ch/ToolR/commit/c51641b98feaa1da36fd4c95c5595b2771b5806a))
- *(argparse)* Recognise bare REMAINDER, not just argparse.REMAINDER ([`fb42c56`](https://github.com/s0undt3ch/ToolR/commit/fb42c56f717d529bb2b5e11ad90a37d2255e9436))
- *(manifest)* Reject FixedArity arguments without a fixed nargs count ([`d06a09e`](https://github.com/s0undt3ch/ToolR/commit/d06a09e1db2ac3cf16e0504e61dd4a256b92137d))
- *(manifest)* Key freshness rebuilds off the toolr binary version too ([`8c185e6`](https://github.com/s0undt3ch/ToolR/commit/8c185e6e8da886e319dee39d1b15b082bcc147af))

### <!-- 3 -->📚 Documentation

- *(argparse)* Link the unsupported-nargs warning to #423 ([`5369758`](https://github.com/s0undt3ch/ToolR/commit/536975870fca7bcf135418e57ddcf3b0aca437fd))
- *(specs)* Design for full argparse nargs support (#423) ([`28688be`](https://github.com/s0undt3ch/ToolR/commit/28688be4214bb36f08e3a9637fe323af139bbc96))
- *(specs)* Implementation plan for full argparse nargs support (#423) ([`3394329`](https://github.com/s0undt3ch/ToolR/commit/33943290197e2b3732a2a71b52c96894fc01d323))
- *(specs)* Correct the design doc's bug-fix section to match the shipped fix ([`7c66b05`](https://github.com/s0undt3ch/ToolR/commit/7c66b051c153d8658e05dfd2ecbfc77d13e1bb68))
- *(specs)* Archive the argparse-nargs-support design and plan ([`c9b658f`](https://github.com/s0undt3ch/ToolR/commit/c9b658f3609aac0379a16283a38e2558cbab6c64))

### <!-- 6 -->🧪 Testing

- *(dispatch)* Cover positional FixedArity extraction through build_spec ([`1057268`](https://github.com/s0undt3ch/ToolR/commit/10572683619ab01622aad18b2b18eff6c21f4f60))
- *(argparse)* Cover nargs shapes argparse never produces ([`9075ee0`](https://github.com/s0undt3ch/ToolR/commit/9075ee0a3125f17290419f3e4a29e194bed9792e))
## 0.26.1 - 2026-08-04

### Notes

The argparse-introspection source now classifies `nargs="+"`/`"*"` correctly
instead of capping them at a single value. Optional flags (e.g.
`parser.add_argument("--names", nargs="+")`) are treated as repeated, the
same as `action="append"`, and a single occurrence may now take several
space-separated values (`toolr django run greet --names alice bob`) as well
as repeated invocations (`--names alice --names bob`); this widening only
applies to nargs-derived flags; `action="append"` flags keep taking exactly
one value per occurrence. Positional arguments with `nargs="+"`/`"*"` (e.g.
Django's `manage.py test app1 app2`) are now scanned as variadic positionals
instead of a single required value, with `nargs="+"` still requiring at
least one. `nargs` shapes the scanner can't fully represent — an int count,
`argparse.REMAINDER`, or `nargs="?"` on a positional — now emit a build
warning instead of silently degrading.

### <!-- 0 -->🚀 Features

- *(ci)* Dogfood gate wiring sync as `toolr ci check-gate-needs` ([`94233a7`](https://github.com/s0undt3ch/ToolR/commit/94233a7449501ee8b88696a32cf7cf23e9c166e4))
- *(testing)* Ship toolr.testing pytest plugin + make_context, fix doc drift ([`b865547`](https://github.com/s0undt3ch/ToolR/commit/b86554724fbedcbdae6c3182571fd40fed6f048e))

### <!-- 1 -->🐛 Bug Fixes

- *(ci)* Make pipeline exit-status gate jobs actually fail ([`a1599e8`](https://github.com/s0undt3ch/ToolR/commit/a1599e83ffc043c6ed708a8089a13ded3ed62bab))
- *(ci)* Add drift gate for pipeline exit-status needs lists ([`9e8b72d`](https://github.com/s0undt3ch/ToolR/commit/9e8b72d7fbdc721848d41a56b42f7fb323bd4f32))
- *(ci)* No-op regen-doc-snippets when the venv lacks the toolr runner ([`866e5f3`](https://github.com/s0undt3ch/ToolR/commit/866e5f3aa87780de90618bb67c66259c62493c10))
- *(deps)* Hold get-size2 at 0.10.1 until ruff moves to compact_str 0.10 ([`91550b4`](https://github.com/s0undt3ch/ToolR/commit/91550b4849c171081f97aa64ba536e71a735fb72))
- *(cli)* Show help when a namespace is invoked with no subcommand ([`aa9e903`](https://github.com/s0undt3ch/ToolR/commit/aa9e90387e69435b4148f6a72faae7cc5c64e11b))
- *(cli)* Pin bin_name so help output is platform-stable ([`8999ee3`](https://github.com/s0undt3ch/ToolR/commit/8999ee324aea7f3c0e86697122dc63930d27a4cb))
- *(precommit)* Pick newest of target/{release,debug}/toolr, not release-first ([`e245f59`](https://github.com/s0undt3ch/ToolR/commit/e245f59cab2b7cb6c418271dddf13ca663d9365b))
- *(lint)* Ignore flake8-copyright rule, not required under Apache-2.0 ([`cf1144f`](https://github.com/s0undt3ch/ToolR/commit/cf1144f32f47ed49780d95207168cb41daca4a0d))
- *(testing)* Repair CI failures from testing package split ([`29c5ed5`](https://github.com/s0undt3ch/ToolR/commit/29c5ed58c19aee834e7ce1b45458d4621ec76309))
- *(cli)* Don't apply unresolvable <expr> default sentinel as a real clap default ([`0c865a9`](https://github.com/s0undt3ch/ToolR/commit/0c865a9bb2a26bc840e0e2ce819e3386910519eb))
- *(argparse)* Classify nargs="+"/"*" as repeated/var-positional ([`3fbb449`](https://github.com/s0undt3ch/ToolR/commit/3fbb449379cccfbbb00a2893cc1429832a4fb191))

### <!-- 2 -->🚜 Refactor

- *(ci)* Move pre-commit hook commands into `toolr pre-commit` ([`61a04e8`](https://github.com/s0undt3ch/ToolR/commit/61a04e849b96b14b1d8c70effea72295cd25a089))

### <!-- 3 -->📚 Documentation

- *(skills)* Flag that the default's shape doesn't drive coercion ([`571e9e6`](https://github.com/s0undt3ch/ToolR/commit/571e9e6004885b62227e6a8f0c4c2894ee915bb5))

### <!-- 6 -->🧪 Testing

- *(testing)* Cover make_context and the pytest-plugin repo-root discovery ([`66a8507`](https://github.com/s0undt3ch/ToolR/commit/66a8507b9c0490c5429bec607a01c3d62d8aff7b))
- Close patch-coverage gaps flagged on b8655472's new code ([`7b3e7be`](https://github.com/s0undt3ch/ToolR/commit/7b3e7becc095f93a084ee0e24c2ae01c48770daf))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(specs)* Archive agent-feedback-skill-fixes design/plan ([`8e7881b`](https://github.com/s0undt3ch/ToolR/commit/8e7881b8be2d0ffb152d3dd598431ca847df66de))
## 0.26.0 - 2026-07-15

### Notes

The pre-commit configuration now sources its shared hooks (actionlint,
shellcheck, gitleaks, pin-github-actions, ruff, typos, rumdl, mypy, uv-lock)
from [`s0undt3ch/pre-commit-hooks`](https://github.com/s0undt3ch/pre-commit-hooks),
replacing the per-repo `rev:` pins and the local `.pre-commit-hooks/*.sh`
wrappers. Tool versions now have a single source of truth: binaries pinned in
`mise.toml` (Renovate-managed) and the venv `mypy` in the new `pre-commit` uv
dependency group. `typos` now auto-fixes on commit, and the redundant
`codespell` hook was dropped in favour of `typos` alone.

`typos` also now spell-checks commit messages (a `commit-msg`-stage hook), so
mistakes are caught before git-cliff folds commit subjects into the CHANGELOG —
which is itself no longer excluded from the check. Existing clones should re-run
`prek install --install-hooks` once to pick up the new `commit-msg` hook.

`toolr project venv run -- <cmd>` runs a command inside the managed tools venv —
the first-class one-liner for running a command-package's tests
(`toolr project venv run -- pytest tools/`). By default it syncs the venv first
(like `venv shell`); `--no-sync` runs against the existing venv and errors if it
is missing or stale, for deterministic CI.

`--quiet` now suppresses the passive "your cache is big, consider pruning" hint.
The hint is non-error output, so `--quiet` (which promises to suppress exactly
that) now keeps it silent — including the per-`cd` `toolr project venv sync
--quiet` the mise enter-hook runs, which previously leaked the hint to stderr in
non-toolr directories once the cache accumulated enough orphans.

### <!-- 0 -->🚀 Features

- *(cli)* Add `toolr project venv run` to run commands in the managed venv ([`3910ccc`](https://github.com/s0undt3ch/ToolR/commit/3910ccc0f8448fcab369f6e34a7f521cb272c522))
- *(cli)* Advertise `venv run` from `project init` and `venv sync` ([`0ae5685`](https://github.com/s0undt3ch/ToolR/commit/0ae568592a71f2cb1fe831e277a3546715e3b965))

### <!-- 1 -->🐛 Bug Fixes

- *(deps)* Bump quinn-proto to 0.11.15 for RUSTSEC-2026-0185 ([`9df2efe`](https://github.com/s0undt3ch/ToolR/commit/9df2efee5e2e7628221ba75b2752c7b4e8f4b83b))
- *(cli)* Suppress passive cache hint under --quiet ([`de97fb9`](https://github.com/s0undt3ch/ToolR/commit/de97fb9739b4fa3a0e30135e282dc0b779eb5b1d))

### <!-- 3 -->📚 Documentation

- *(writing-commands)* Prefer ctx.repo_root and document tools namespace package ([`8f2f4ec`](https://github.com/s0undt3ch/ToolR/commit/8f2f4ec74898ec924a63bb909c79f716d1d9ea3e))
- *(writing-commands)* Fix tools import example and note relative imports work ([`6d03f17`](https://github.com/s0undt3ch/ToolR/commit/6d03f1746de79bd0f31a6b2eb3f059758ab31a01))
- *(ci-setup)* Document TOOLR_VENV_LOCATION and the action's in-tree default ([`52d55cb`](https://github.com/s0undt3ch/ToolR/commit/52d55cb19208f28413da6c3b94faea8737408a8f))
- *(internals)* Venv-run design doc ([`fac9e27`](https://github.com/s0undt3ch/ToolR/commit/fac9e27f841554ca3cb38dee864325c905e901cd))
- *(cli)* Document `toolr project venv run` ([`99f5ba0`](https://github.com/s0undt3ch/ToolR/commit/99f5ba06190f9c59335d254b949148c02a29e581))
- *(skills)* Teach `toolr project venv run` for running/testing commands ([`5be24c2`](https://github.com/s0undt3ch/ToolR/commit/5be24c2ca4aacec46faccc468faebb4f87481a64))
- *(internals)* Archive venv-run design + plan ([`e9ab46d`](https://github.com/s0undt3ch/ToolR/commit/e9ab46d87d693716871c696623f0a12c2fb39340))
- *(internals)* Archive shipped specs (ci-setup, static-only-manifest, runner-path-hygiene, remove-editable-install) ([`6c20439`](https://github.com/s0undt3ch/ToolR/commit/6c2043934070e826b749cdab0eb98d89c239e73f))

### <!-- 6 -->🧪 Testing

- *(pre-commit)* Drop obsolete pin-github-actions hook test ([`d8dd8db`](https://github.com/s0undt3ch/ToolR/commit/d8dd8dbeb9b1921cfd56c4a88a3dc14432090d43))
- *(cli)* Run venv-run not-found nudge on all platforms ([`a59e616`](https://github.com/s0undt3ch/ToolR/commit/a59e61613954e6b6c70f2ff362b853432a1e3e52))
- *(cli)* Cover venv-run validate-failure and non-executable spawn-error paths ([`b8634b8`](https://github.com/s0undt3ch/ToolR/commit/b8634b8fe9dd3060699f93d5e314f9a79e052c24))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(renovate)* Enable osvVulnerabilityAlerts ([`c229783`](https://github.com/s0undt3ch/ToolR/commit/c229783cb3a653b004ba1fbe38b6a14c035d3c02))
- *(renovate)* Monthly lockFileMaintenance ([`8c0b803`](https://github.com/s0undt3ch/ToolR/commit/8c0b80351534c54466164303aa0f34604f336ff5))
- *(pre-commit)* Adopt s0undt3ch/pre-commit-hooks ([`5c23763`](https://github.com/s0undt3ch/ToolR/commit/5c23763ec088b8d6160c5fa2170a005f0c92ef39))
- *(pre-commit)* Spell-check commit messages and the CHANGELOG ([`8a0a16b`](https://github.com/s0undt3ch/ToolR/commit/8a0a16b50dc196b890b3a5a400ca3cde17655d91))
## 0.25.2 - 2026-06-30

### Notes

Fixed: `@group.command(name="…")` silently dropped the keyword and the
command took its hyphenated function name instead — the static parser
only read the positional override. The `name=` keyword is now honoured on
both `@group.command` and `@command`, which share an identical
command-name contract: the override may be passed positionally
(`@command("collect", group="…")`) or by keyword
(`@command(name="collect", group="…")`), but not both — passing both
raises `TypeError` at runtime and fails `build-manifest` with a clear
*conflicting command name* error.

The vestigial, never-implemented `aliases=` keyword has been removed from
the `@command` decorator.

### <!-- 1 -->🐛 Bug Fixes

- *(parser)* Honor name= override on command decorators ([`91f45b8`](https://github.com/s0undt3ch/ToolR/commit/91f45b85882ce2927f69d9b08aa9c3cbb6499ac7))
## 0.25.1 - 2026-06-15

### Notes

### Changed

- The binary↔runner dispatch schema is bumped to `2`. The 0.25.0 binary changed
  the runtime contract (it starts the runner with `-P` and relies on the runner
  to put the repo root on `sys.path`) without bumping the schema, so pairing it
  with an older `toolr-py` failed with a cryptic `No module named 'tools'`
  instead of the schema guard's clear "venv out of sync" message. The schema now
  covers that contract, so a future binary paired with a stale `toolr-py` fails
  loudly and points you at `toolr project venv upgrade toolr-py`. Re-sync your
  tools venv after upgrading toolr.

### <!-- 1 -->🐛 Bug Fixes

- *(tools)* Bump dogfood toolr-py to 0.25.0 (lockstep with the binary) ([`ad98dbf`](https://github.com/s0undt3ch/ToolR/commit/ad98dbfd5ecb9ed0ed0c9456d087ce66409c482a))
- *(execute)* Bump dispatch schema to 2 for the -P/sys.path contract ([`cb260e0`](https://github.com/s0undt3ch/ToolR/commit/cb260e0a5d4fb20f20431f1d9ac627909970d887))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(renovate)* Track our own toolr-py in the dogfood venv, ASAP ([`9b0ac07`](https://github.com/s0undt3ch/ToolR/commit/9b0ac07c6d7c39b035fb2a23571e50b9b6b0f6f3))
## 0.25.0 - 2026-06-15

### Notes

Automated dependency updates have moved from Dependabot to
[Mend Renovate](https://docs.renovatebot.com/). The new `renovate.json5`
preserves the previous ecosystem labels (`dependencies:rust`,
`dependencies:python`, `dependencies:github-actions`) and the weekly
Monday cadence, then cuts PR noise by grouping updates: the three
`ruff_*` crates (one `astral-sh/ruff` tag) ship together, every GitHub
Actions digest bump rolls into one PR per week, and the mise CLI tools
(`actionlint`, `shellcheck`, `prek`) share another. Language toolchain
pins (Python, uv, Rust, `cargo-edit`) and individual cargo / pyproject
crates still get their own PRs so each bump remains reviewable. GitHub
Actions stay pinned to commit SHAs with the SemVer tag in a trailing
comment.

`mise.toml` now opts into the [mise lockfile](https://mise.en.dev/dev-tools/mise-lock.html)
(`lockfile = true` in `[settings]`), and the generated `mise.lock` is
committed alongside the toml. Every CI runner — and any contributor
running `mise install` locally — installs tools from the lockfile's
pre-resolved per-platform URLs and SHA256 checksums; `jdx/mise-action`
detects `mise.lock` and adds `--locked` to `mise install` automatically,
so missing or drifted entries fail the build instead of silently
re-resolving via GitHub / aqua APIs. The Rust toolchain pin also moves
from `stable` to an explicit `1.96.0` so rustc no longer drifts between
runs. Refresh the lockfile with `mise upgrade && mise lock` (bare
`mise lock` already targets every platform CI runs on) and commit
`mise.lock` with the change.

The `pin-github-actions` pre-commit hook now verifies as well as pins, in a
single pass. By default it locks any unpinned `uses:` ref to a SHA *and*
re-resolves every already-pinned `uses: …@<sha> # <tag>` line against the SHA
its `# <tag>` comment currently points to, failing on any mismatch or
unverifiable pin — catching hand-edited or upstream-repointed pins that the
old pin-only behaviour (which skipped already-pinned lines) would never flag.
Resolutions are memoised per run. A missing `gh` CLI is now a hard error rather
than a silent skip; pass `--exit-zero` for a non-blocking, gh-optional advisory
run. The hook runs in CI too (`prek run --all-files` already covers `.github/**`),
so no separate verification workflow is needed.

### Security

- toolr no longer executes repository Python to build its command manifest.
  `toolr --help`, completion, and first-run are now fully static (AST parse +
  execution-free third-party glob). Repository code runs only on explicit
  command dispatch, through a provenance-verified interpreter. A committed
  `tools/.venv` is refused unless toolr provisioned it (`toolr project venv sync`).
- The toolr runner no longer puts the invocation directory on `sys.path` (the
  interpreter is started with `-P`), preventing a stray `.py` file in your
  current directory from shadowing stdlib/site-packages modules.
- The install scripts (`install.sh`, `install.ps1`) now **verify the release's
  SLSA build provenance by default** (`--verify-attestation=require`). Previously
  the default silently skipped verification when the `gh` CLI was absent, leaving
  only a same-release `.sha256` check that can't detect a tampered release asset.
  If `gh` is missing the install now fails with guidance rather than installing
  unverified; pass `--verify-attestation=skip` (`-VerifyAttestation skip` on
  Windows) to explicitly opt out. Matches the already-fail-closed GitHub Action.
- The `s0undt3ch/ToolR` setup Action now re-runs checksum **and** SLSA
  attestation verification on every job, **including cache hits** — previously a
  warm cache placed the binary on `PATH` with no verification. The Action now
  caches the release *archive* (not the extracted binary), so a poisoned cache
  entry is rejected by attestation before it can execute. `skip-attestation:
  true` remains the only bypass.
- The runner hardens how it handles the dispatch spec file (defense-in-depth).
  Before reading `$TOOLR_SPEC_FILE` it now refuses a spec that is a symlink, not
  a regular file, owned by another user, or group/world-writable — so a
  forgeable spec can't become a forgeable import target. It also unlinks the
  spec immediately after reading it, rather than waiting for the binary to drop
  its handle, so the 0600 spec JSON (which can carry CLI argument values) does
  not linger in `TMPDIR` if the process is hard-killed. No change to normal
  dispatch.

### Changed

- Commands now run with the working directory set to the repo root (like
  `make`/`cargo`). Relative path arguments resolve from the repo root, not your
  current directory; toolr prints a one-line note if you pass a relative path
  from a subdirectory.
- `toolr-py` now ships a single stable-ABI (`abi3`) wheel per platform, tagged
  `cp311-abi3`, instead of one wheel per CPython. The one wheel installs on every
  CPython >=3.11 (matching `requires-python`), so `pip install toolr-py` resolves
  the same wheel whether you run 3.11, 3.12, 3.13, or 3.14. No action needed —
  this only shrinks the published wheel set; the supported interpreter range is
  unchanged and still fully tested.

### Fixed

- The `Running '…'` line that `ctx.run` logs now prints the command line
  literally instead of interpreting it as rich markup. A command argument that
  looks like a rich tag (e.g. `jq '[.foo]'`, `[link=…]`) is no longer consumed
  or rendered, so the echo always reflects exactly what ran — matching how the
  command would appear in your shell.

### Removed

- The dynamic introspection layer (`toolr._introspect`) is gone. Commands
  registered dynamically (not via top-level `command_group(...)` + module-level
  `@group.command`) are no longer discovered. Third-party plugins via shipped
  `toolr-manifest.json` are unaffected.
- The retired argparse internals in `toolr.utils._signature` are gone — the
  private (`_`-prefixed) `Signature`/`Arg`/`KwArg`/`VarArg` structs,
  `get_signature`, `_parse_parameter`, the argparse `Action` subclasses, and
  their helpers. The Rust static parser now does signature extraction and the
  runner coerces arguments via `msgspec`, so none of this was reachable at
  runtime. The public surface (`arg`, `arg_section`, `ArgSection`,
  `ArgumentAnnotation`) is unchanged; any out-of-tree code importing the removed
  private names must migrate off them.
- The `[tool.toolr] editable-install` directive is removed. toolr no longer runs `uv pip install -e` itself;
  declare editable dependencies the uv-native way via `[tool.uv.sources]` (e.g.
  `foo = { path = "./packages/foo", editable = true }`), which `uv sync` installs and records in `uv.lock`.
  A `tools/pyproject.toml` that still lists `editable-install` keeps loading — the key is ignored.

### Documentation

- New "External command sources" guide under *Writing commands*. It documents
  the `[tool.toolr.argparse.*]` source-scanning feature — statically scanning
  argparse-style command files (e.g. Django management commands) and grafting
  them under a dispatcher command — which previously had no coverage in the docs.

### <!-- 0 -->🚀 Features

- *(cache)* Record interpreter provenance in meta.json (schema v2) ([`f966573`](https://github.com/s0undt3ch/ToolR/commit/f966573ade502e8f500444619a8c9f74c9be07c8))
- *(venv)* Interpreter provenance verification ([`94c7a4b`](https://github.com/s0undt3ch/ToolR/commit/94c7a4b122e40ca4883c9d98c1f89e214ed70ff5))
- *(venv)* Record interpreter provenance and rebuild manifest on sync ([`5a02790`](https://github.com/s0undt3ch/ToolR/commit/5a02790f2e767db0931b8bddaace270d853fee62))
- *(dispatch)* Refuse to execute non-provisioned in-repo interpreters (SEC-01) ([`dfa4959`](https://github.com/s0undt3ch/ToolR/commit/dfa495920c13dce962161d75cb7e9006e521614e))
- *(runner)* Append repo_root to sys.path (resolves tools.* from any cwd) ([`097de5c`](https://github.com/s0undt3ch/ToolR/commit/097de5c29b1089f5885e111a9a46c4c2aaf18329))
- *(runner)* Double-gated relative-path warning helper ([`6749619`](https://github.com/s0undt3ch/ToolR/commit/6749619efdaee3094d30df557f579651ccfafa91))
- *(runner)* Run commands from repo root; warn on relative path args from a subdir (SEC-02) ([`99e80c8`](https://github.com/s0undt3ch/ToolR/commit/99e80c823b45918cde9e9e64d650cf273cb5975f))
- *(execute)* Pass -P to the runner interpreter (drop implicit CWD from sys.path) ([`cc6fd6c`](https://github.com/s0undt3ch/ToolR/commit/cc6fd6cb9638358bc068fc3cbba7ddc864288bf1))
- *(ci)* Pin & verify action SHAs in one pre-commit pass ([`6ced9bd`](https://github.com/s0undt3ch/ToolR/commit/6ced9bde1c8845e3682220aa1c7def0d6599f7b3))

### <!-- 1 -->🐛 Bug Fixes

- *(renovate)* Match ruff_* crates by depName so grouping fires ([`8ea9fac`](https://github.com/s0undt3ch/ToolR/commit/8ea9faca134df7878b5484284d5bf63d5329d8a4))
- *(test)* Gate Unix-only test helpers/imports for Windows ([`4ccf79a`](https://github.com/s0undt3ch/ToolR/commit/4ccf79ae27a58034adc9ff02a32cd5ce64811e5a))
- *(venv)* Accept real in-tree venvs without a provenance record (validate_venv fallback) ([`9114868`](https://github.com/s0undt3ch/ToolR/commit/9114868af241c863532ec512f0d121968c312734))
- *(test)* Use a platform-absolute path in no_warn_for_absolute_path_arg ([`aa2932d`](https://github.com/s0undt3ch/ToolR/commit/aa2932dba10e7cde677eb1830558f96969c8d726))
- *(install)* Verify SLSA attestation by default (fail closed) ([`886cdef`](https://github.com/s0undt3ch/ToolR/commit/886cdefeb3193648ca5b82b3e11ab72dab8af2c2))
- *(action)* Re-verify checksum + attestation on cache hit (SEC-09) ([`228e51e`](https://github.com/s0undt3ch/ToolR/commit/228e51e7729079f53c5070b3a3a555612c1d74cb))
- *(testing)* Restore sys.modules when CommandsTester exits (SEC-07) ([`b481e53`](https://github.com/s0undt3ch/ToolR/commit/b481e5387531e1f3344bd2d14afa84c274853fbe))
- *(mise)* Lock all CI test-matrix CPythons in mise.lock ([`33d68a3`](https://github.com/s0undt3ch/ToolR/commit/33d68a3fd397e1329db85263a26cc9f4d8025dc2))
- *(ci)* Cache rust toolchain so clippy/rustfmt survive mise cache hits ([`de2046a`](https://github.com/s0undt3ch/ToolR/commit/de2046a107fe4c170a97407aaa3778cc334ce8ee))
- *(context)* Echo ctx.run cmdline literally, not as rich markup ([`4def5ab`](https://github.com/s0undt3ch/ToolR/commit/4def5abab0f9b9ad3b47db05958e1fb8dd57c9f9))
- *(ci)* Distinguish real pin failures from transient ones in verifier ([`2fbd07e`](https://github.com/s0undt3ch/ToolR/commit/2fbd07e2030357f55cb3d9ccf6580061726f1ee2))
- *(ci)* Guarantee clippy/rustfmt via rustup component add, not just caching ([`ec9bfbd`](https://github.com/s0undt3ch/ToolR/commit/ec9bfbdf23d55dcc0ca379b7761f3155380cd852))
- *(ci)* Ensure rust components in setup-pre-commit too ([`f9f6a3f`](https://github.com/s0undt3ch/ToolR/commit/f9f6a3fb7854434b8e3a8653535145e47e94da89))
- *(runner)* Validate spec-file provenance before reading it (SEC-05) ([`01007ff`](https://github.com/s0undt3ch/ToolR/commit/01007ff7644a021bfc9eaeff47f7fb0b0e09f045))
- *(runner)* Unlink the spec file once it has been read (SEC-14) ([`f39bb9c`](https://github.com/s0undt3ch/ToolR/commit/f39bb9c188b91e32490314110069363a285b1d75))

### <!-- 10 -->💼 Other

- *(toolr-py)* Build a single abi3 (cp311) wheel instead of one per CPython ([`fa89b53`](https://github.com/s0undt3ch/ToolR/commit/fa89b5374c39abbddce08e69ca1513577f6674d3))

### <!-- 2 -->🚜 Refactor

- *(manifest)* Remove the dynamic Python-introspection layer ([`cbb5ff1`](https://github.com/s0undt3ch/ToolR/commit/cbb5ff17040f630b761f7c3ef8ec8214d5a72a3d))
- *(core)* Rename the dynamic module to manifest_build ([`bc16f32`](https://github.com/s0undt3ch/ToolR/commit/bc16f3204984b3ea6519b47d6d03d4f05ccc71a9))
- *(sec-02)* Move chdir + relative-path warning to Rust; thin the runner ([`0dc9a93`](https://github.com/s0undt3ch/ToolR/commit/0dc9a93d2e2865a23e493e16f7c72e7dffed5e67))
- *(venv)* Stop running post-sync editable installs ([`6775c1f`](https://github.com/s0undt3ch/ToolR/commit/6775c1f5ddd2e11e456a960a2a117ca0a9bcf044))
- *(venv)* Delete the editable-install module ([`8085b91`](https://github.com/s0undt3ch/ToolR/commit/8085b91289f56534372053aa942348c97c6bdda4))
- *(venv)* Drop the editable-install config field (legacy key ignored, not an error) ([`5c7e7e0`](https://github.com/s0undt3ch/ToolR/commit/5c7e7e03ab70fe837e01c214b2864daef3e49a91))
- *(core)* Remove the dead deps_check preflight and Command.imports ([`4583afa`](https://github.com/s0undt3ch/ToolR/commit/4583afa3df042b4ec844b74e28f7a5b9c496afa2))
- *(toolr-py)* Trim retired argparse machinery from _signature.py ([`be9741a`](https://github.com/s0undt3ch/ToolR/commit/be9741a4860adf72069d9177aa33f24fb1936533))
- *(core)* Replace the zero-migration fragment framework with a version check ([`81c0f98`](https://github.com/s0undt3ch/ToolR/commit/81c0f98b88e22ba3d7de8880be78e741906d6c4e))
- *(cli)* Derive built-in completions from clap instead of hand-mirroring ([`4de07ec`](https://github.com/s0undt3ch/ToolR/commit/4de07ecb6546470ec8dad5bc7d86d7a65bba2b2c))

### <!-- 3 -->📚 Documentation

- *(specs)* Static-only manifest design (SEC-01) ([`e2b7c9e`](https://github.com/s0undt3ch/ToolR/commit/e2b7c9ebf648e0c2b0b9cd4b2cbf4e71a022211c))
- *(specs)* Static-only manifest implementation plan (SEC-01) ([`52e840c`](https://github.com/s0undt3ch/ToolR/commit/52e840c99e37f1181ac0101678b7ae00151d62a1))
- *(authoring)* Document the static-only command-discovery contract ([`c34c13c`](https://github.com/s0undt3ch/ToolR/commit/c34c13c7aab2a935c69064d5d12378276a14fd99))
- *(unreleased)* Note static-only manifest and provenance ([`222f743`](https://github.com/s0undt3ch/ToolR/commit/222f7430ca111d6adfcfd27cc942d2b365a63e6e))
- *(test)* Drop stale toolr._introspect reference from conftest comment ([`4bb1a0b`](https://github.com/s0undt3ch/ToolR/commit/4bb1a0b2457d6566ba35f33bd9863a9da703c672))
- *(specs)* Runner path hygiene design (SEC-02) ([`f3aebf6`](https://github.com/s0undt3ch/ToolR/commit/f3aebf60c0f531bb2b5ee83a10f5ec9219511828))
- *(specs)* Runner path hygiene implementation plan (SEC-02) ([`ca935a2`](https://github.com/s0undt3ch/ToolR/commit/ca935a2590a67e8cb9d03f729802a5446246d205))
- *(authoring)* Commands run from the repo root; path args are repo-root-relative ([`a2482d9`](https://github.com/s0undt3ch/ToolR/commit/a2482d9f6974324be888aa0db2298c0a5db1c983))
- *(unreleased)* Note runner -P + repo-root cwd (SEC-02) ([`d72dcf5`](https://github.com/s0undt3ch/ToolR/commit/d72dcf524feca1373f2e95c515fb69d515f35d8c))
- *(specs)* Note SEC-02 chdir + warning moved to Rust (plan superseded in part) ([`065edc0`](https://github.com/s0undt3ch/ToolR/commit/065edc00e88d5682807bc919579de7df46e19de0))
- *(specs)* Remove editable-install design + plan (SEC-04) ([`8c1f975`](https://github.com/s0undt3ch/ToolR/commit/8c1f9758e690f2cfbef6d6e0fab2f2971952f3c9))
- *(config)* Document editable installs via [tool.uv.sources]; drop editable-install ([`7529dbe`](https://github.com/s0undt3ch/ToolR/commit/7529dbe0f568e5bd0d50999fecaf22d32c4d72f7))
- *(unreleased)* Note removal of editable-install (SEC-04) ([`63b4e15`](https://github.com/s0undt3ch/ToolR/commit/63b4e150d3cbd6c45ee914f805142862e6ea0325))
- *(writing-commands)* Document external command sources [DOC-01] ([`6b03f2f`](https://github.com/s0undt3ch/ToolR/commit/6b03f2f0d18c3cb0259b4240af6be97b90ae844e))
- *(unreleased)* Note runner spec-file hardening (SEC-05)(SEC-14) ([`00277d1`](https://github.com/s0undt3ch/ToolR/commit/00277d10aedafb86bafb6665bd6cd706f9b2a085))

### <!-- 6 -->🧪 Testing

- *(security)* Failing regression for SEC-01 implicit code execution ([`f2b057d`](https://github.com/s0undt3ch/ToolR/commit/f2b057d653afdcd4a91824cedd10ee41e97e0e91))
- *(security)* --help builds statically with no venv (SEC-01 green) ([`13a20e2`](https://github.com/s0undt3ch/ToolR/commit/13a20e2a9d09403acf299b0b8089dd216224ff73))
- *(freshness)* Manifest rebuilds when a venv appears ([`2d9434d`](https://github.com/s0undt3ch/ToolR/commit/2d9434d40c0b3ae64679b8d83ca29255a386f8f7))
- *(coverage)* Cover provenance cache-dir, in-tree slot, bootstrap venv arm, tools walk ([`67f98c9`](https://github.com/s0undt3ch/ToolR/commit/67f98c9dad93c9b56cb93431e3e258cce271711b))
- *(runner)* Document -P shadowing coverage boundary (SEC-02 Task 5) ([`7c3731a`](https://github.com/s0undt3ch/ToolR/commit/7c3731ac5b9ef54c8206b13eb0c0eec62168aa6e))
- *(coverage)* Cover the walk's per-module except + finalize_sync existing-sidecar branch ([`3dd8d2c`](https://github.com/s0undt3ch/ToolR/commit/3dd8d2c991a292152b4b569e2c163874d09e1213))
- *(coverage)* Exercise the warn_relative_paths wrapper in build_spec ([`0d70ffb`](https://github.com/s0undt3ch/ToolR/commit/0d70ffb012e938a47583e112d46e8e589c7d8a79))
- *(docstrings)* Match pyo3 0.29 argument-error message ([`7fbfac9`](https://github.com/s0undt3ch/ToolR/commit/7fbfac97c20b261010aa6f4cf21b30c7a23be0cd))
- *(runner)* Cover all spec-file provenance branches (SEC-05) ([`c03f931`](https://github.com/s0undt3ch/ToolR/commit/c03f931b30484d07032b9fc19abfc570be762469))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Migrate dependency updates from Dependabot to Renovate ([`c59637f`](https://github.com/s0undt3ch/ToolR/commit/c59637f9ce1a6c3b13c482e66a083d9a0cd4a3b1))
- *(prek)* Sync Cargo.lock with Cargo.toml via cargo-lock hook ([`29b3d85`](https://github.com/s0undt3ch/ToolR/commit/29b3d85703bf3d2c43d6ade5d3739b544ff2c5f7))
- Re-trigger Windows distribution (transient mise/cargo-edit webpki-roots download flake) ([`38a4f4f`](https://github.com/s0undt3ch/ToolR/commit/38a4f4fd5492e98710645a3299d9dadc42066103))
- Run benches on Linux by default, with a bench-all-os opt-in ([`9c55002`](https://github.com/s0undt3ch/ToolR/commit/9c550022f333db460d7869b4617a120696007a85))
- *(test)* Verify example-plugin manifest with the shipped toolr binary ([`77f66c6`](https://github.com/s0undt3ch/ToolR/commit/77f66c666540b27a8394ee513d309f41edf59634))
- Add the `full-bench` label to opt PRs into the macOS+Windows benches ([`c9db291`](https://github.com/s0undt3ch/ToolR/commit/c9db291e4f02acc93d86d9d32d36db397d2f7537))
- Route workflow inputs through env vars (CWE-78 hardening) ([`00037f4`](https://github.com/s0undt3ch/ToolR/commit/00037f4388f190c82ef4b051d8a7e01687940e1d))
- Attest wheels on release + main pushes, not just PRs (SEC-11) ([`ecefa3e`](https://github.com/s0undt3ch/ToolR/commit/ecefa3e2d9ca8cfe8cd191349d1b8c1408eb3d50))
- *(mise)* Pin tools via mise.lock and freeze rust to 1.96.0 ([`d63d642`](https://github.com/s0undt3ch/ToolR/commit/d63d642b9ac2eea2cfb99530bb6ab0c82f610b1d))
- Run shellcheck/actionlint/gitleaks from mise, not docker/golang ([`58e9845`](https://github.com/s0undt3ch/ToolR/commit/58e9845e0c75647d6c0a7884138004b6c7bdfbed))

### New Contributors

* @renovate[bot] made their first contribution
## 0.24.1 - 2026-06-06

### Notes

Tab completion now offers flag candidates at every level of the
command tree, not just leaf commands. `toolr --<Tab>` lists the
binary's own root options (`--debug`, `--quiet`, `--timestamps`, …)
alongside `--help`, and `--help` is offered at every group node
(`toolr self --<Tab>`, `toolr self cache --<Tab>`, …) and on every
leaf in addition to its own flags. A bare `--` typed as the
in-progress word is now preserved through the engine — previously
clap consumed it as the end-of-options marker and the shell saw
an empty candidate list.

### <!-- 1 -->🐛 Bug Fixes

- *(complete)* Offer flags at every node and preserve bare `--` ([`446c770`](https://github.com/s0undt3ch/ToolR/commit/446c77042ad5c3c2cf5377f50b1ea0c7a3b761a9))
## 0.24.0 - 2026-06-06

### Notes

Bumped all Rust, Python, and GitHub Actions dependencies in a single
sweep — including `ruff_*` git deps to `0.15.15`, `ruff`/`coverage`/
`hypothesis` on the Python side, and `actions/checkout` and
`actions-cool/check-user-permission` in CI. Transitive lockfile bumps
in `Cargo.lock` and `uv.lock` rolled forward at the same time.

The task-runner startup benchmark moved from `toolr bench compare` to
`python3 scripts/bench.py`. The script now depends only on the Python
standard library (no toolr, no rich), so it can run in a fresh CI job
without bootstrapping a project venv. Output is a markdown table on
stdout; progress lines go to stderr.

`--help` output is now rendered as markdown through `termimad`.
Python docstrings flow through unchanged — headings, sections
(`## Examples`, `## Notes`, `## Warnings`), bullet lists, and
fenced code blocks render as styled markdown in the terminal and
as readable plain text when captured (non-TTY or `NO_COLOR`).
`$COLUMNS` is honored for width control. The page matches clap's
default layout: a `**Usage:**` line, `Arguments:` / `Options:` /
`Commands:` blocks reflecting the actual command shape, possible
values on positionals, and `[default: …]` on options where a
literal default exists (unresolvable Python expressions are
suppressed rather than shown as `<expr>`).

`-h` (short) and `--help` (long) remain distinct: short shows the
first-paragraph `about` and truncates per-option help to its first
line; long shows the full docstring body, Examples/Notes sections,
possible-value lists, and `[default: …]` annotations.

Sphinx-style ``code`` (double backticks) in docstrings is now
normalized to single-backtick markdown at parse time, so RST
syntax no longer leaks into rendered help.

Internal: clap's built-in help is disabled and dispatched to
`crate::help`; the `crate::markdown` pre-render layer, the
`wrap_help` feature on `clap`, and the `clap-help` dependency
are all gone.

### <!-- 0 -->🚀 Features

- *(bench)* Add --toolr-py-find-links to pin the bench fixture to a local wheel ([`d828de7`](https://github.com/s0undt3ch/ToolR/commit/d828de7035dc462aca5a5680814c970f855776ed))
- *(core)* Promote docstring section labels to markdown headings ([`faf704d`](https://github.com/s0undt3ch/ToolR/commit/faf704d0c19217f76d6f8f38740a92d33c5e91e7))
- *(toolr)* Add help module skeleton with TTY-aware skin ([`8534fa4`](https://github.com/s0undt3ch/ToolR/commit/8534fa431f129dcfc2b449ea899f4fc0c033c3cd))
- *(toolr/help)* Resolve render width from $COLUMNS ([`868cfa2`](https://github.com/s0undt3ch/ToolR/commit/868cfa223dcb85aa51b8594394bf6160d031489d))
- *(toolr/help)* Implement render loop with custom width ([`3767ab3`](https://github.com/s0undt3ch/ToolR/commit/3767ab3ebe44c44f68ef91e491dac4333a7035e3))
- *(toolr)* Intercept --help/-h and dispatch to crate::help ([`8b9c900`](https://github.com/s0undt3ch/ToolR/commit/8b9c9008273bb605c51e131012fc3c62057946fb))
- *(toolr/help)* Render Options as a table ([`b9cda44`](https://github.com/s0undt3ch/ToolR/commit/b9cda4469ed49514bd44be3dd0da576a7a8fda37))
- *(toolr/help)* Render Commands as a table ([`f279b52`](https://github.com/s0undt3ch/ToolR/commit/f279b52542c4bc7005a03e113dd7dda663b3bfce))
- *(core/parser)* Resolve same-module constants as parameter defaults ([`db7da61`](https://github.com/s0undt3ch/ToolR/commit/db7da6182bc9780ee964663a714efcd67297ce84))

### <!-- 1 -->🐛 Bug Fixes

- *(tools/bench)* Account for `:` in right-aligned separator width ([`3244f8e`](https://github.com/s0undt3ch/ToolR/commit/3244f8e7580f74f006c2438669aa016917178c9e))
- *(core/docstrings)* Drop spurious newline between body and sections ([`c808b91`](https://github.com/s0undt3ch/ToolR/commit/c808b9178227142138baf7632a1170ab9a95f5d6))
- *(core/docstrings)* Normalize Sphinx double-backticks at parse time ([`ebd2b05`](https://github.com/s0undt3ch/ToolR/commit/ebd2b05f8f1c92aa3d56d54ff03e6998fc48b7e2))
- *(toolr/help)* Collapse blank rows between options ([`c555c7a`](https://github.com/s0undt3ch/ToolR/commit/c555c7a9f82f73963a730f860f005adb7e820016))
- *(toolr/help)* Make USAGE and Options reflect actual command shape ([`107a2d1`](https://github.com/s0undt3ch/ToolR/commit/107a2d1e3a6de0d66f21eeffcc50abb711432a74))
- Gitignore .toolr-manifest.json fixtures at any depth ([`e22d501`](https://github.com/s0undt3ch/ToolR/commit/e22d501f34edb09c3af51ffd886740e53c1b5a01))
- *(toolr/help)* Drop short-flag pad when group has no shorts; add per-cmd --help ([`64324da`](https://github.com/s0undt3ch/ToolR/commit/64324dab3fceb7803ac2dd5ed08da63afc1b1f19))
- *(toolr/help)* Suppress `[default: <expr>]` for unresolvable defaults ([`d9de25d`](https://github.com/s0undt3ch/ToolR/commit/d9de25dd5151d39768b7028f254191491324bc0a))
- *(toolr/help)* Show `-h` on flag-less leaves; match clap's 2-space label gap ([`ca31fd0`](https://github.com/s0undt3ch/ToolR/commit/ca31fd0b0e11a0a8f778fe81007cec5231cdc968))
- *(toolr/help)* Drop the trailing "Report bugs to" footer ([`3eb0ec5`](https://github.com/s0undt3ch/ToolR/commit/3eb0ec50304c50630c466d5aaf025758b48c23f2))
- *(toolr/help)* Restore positional possible-values; render Commands before Options ([`f9942b7`](https://github.com/s0undt3ch/ToolR/commit/f9942b74911df78aa0afeb2a6db8cefda6bdbf69))

### <!-- 10 -->💼 Other

- *(deps)* Bump Rust workspace dependencies ([`32abda3`](https://github.com/s0undt3ch/ToolR/commit/32abda37658ad4a5cbdeeed1500fa465e7a2de21))
- *(deps-dev)* Bump Python dependencies ([`e959296`](https://github.com/s0undt3ch/ToolR/commit/e95929641cc51c9bf3a5073166e91028970543c3))
- *(deps)* Bump GitHub Actions ([`c4b15cc`](https://github.com/s0undt3ch/ToolR/commit/c4b15ccadb1b4d10fcc463c5bddac1b493b4bf71))
- *(toolr)* Add clap-help 1.5 for markdown --help rendering ([`31d0c52`](https://github.com/s0undt3ch/ToolR/commit/31d0c52c33890e2edef918f66981d108f62ea2dc))
- *(clap)* Drop wrap_help feature ([`fcaf48c`](https://github.com/s0undt3ch/ToolR/commit/fcaf48c72a1451b5b2ffafc65f311a5c8bdb9f6b))

### <!-- 2 -->🚜 Refactor

- *(bench)* Move to scripts/ as a stdlib-only script ([`771ecee`](https://github.com/s0undt3ch/ToolR/commit/771ecee44c976a26441ea7a0f1a76ae8ed477d96))
- *(toolr/cli)* Disable clap built-in help, add global help flags ([`7f3dbde`](https://github.com/s0undt3ch/ToolR/commit/7f3dbde819acdad7de3070142c244feae9cdad84))
- *(toolr)* Drop markdown pre-render layer ([`246b435`](https://github.com/s0undt3ch/ToolR/commit/246b4356023d4659902ca24a5ec093a36348268b))
- *(toolr/help)* Match clap's default look; drop clap-help dep ([`a3fe821`](https://github.com/s0undt3ch/ToolR/commit/a3fe821d6925183ab177b004125ddd3b584b8b40))

### <!-- 3 -->📚 Documentation

- *(unreleased)* Note bundled dependency upgrade ([`e9e4d82`](https://github.com/s0undt3ch/ToolR/commit/e9e4d825ac29a5b632df20e80c337fea1e63de4b))
- *(specs)* Design clap-help-driven --help rendering ([`e912e1f`](https://github.com/s0undt3ch/ToolR/commit/e912e1fb12c984516f2cb0a0a6f542251d5970ac))
- *(specs)* Implementation plan for clap-help --help rendering ([`4a1c59e`](https://github.com/s0undt3ch/ToolR/commit/4a1c59e46e240b570595125d11017627ec2828c7))
- Regenerate help snippets for clap-help renderer ([`473f6c6`](https://github.com/s0undt3ch/ToolR/commit/473f6c6d59c71738bbbf2ee7acf62f8160a227c6))
- *(changelog)* Note clap-help-driven --help renderer ([`4ab0c64`](https://github.com/s0undt3ch/ToolR/commit/4ab0c6429de85d8d1ca48b27bbc6ae186cd2f4e6))
- *(specs)* Archive clap-help rendering design + plan ([`e032221`](https://github.com/s0undt3ch/ToolR/commit/e032221528641d34ac705ab4acf178d49f2919c0))
- Standardize example values and relative paths ([`13128e2`](https://github.com/s0undt3ch/ToolR/commit/13128e233ad31e704f42a40e753f7108978efed6))
- *(cli-files)* Regen --help snapshots for 2-space gap and synthetic --help ([`65d9cc3`](https://github.com/s0undt3ch/ToolR/commit/65d9cc3faebf848c390b7157e980c31c23e7fa53))
- *(unreleased)* Refresh help-renderer notes to match shipped behavior ([`490ea49`](https://github.com/s0undt3ch/ToolR/commit/490ea497e7b088d36f1fd461fb6dd4dbd7357251))

### <!-- 6 -->🧪 Testing

- *(toolr)* Update --help assertions for clap-help output ([`5ae0139`](https://github.com/s0undt3ch/ToolR/commit/5ae013930e9bd8fd5408968f2bae22b9175fc286))
- Refresh stale snapshots for new docstring format ([`4464666`](https://github.com/s0undt3ch/ToolR/commit/44646661198372022cd2dbd39825f36fdaf56789))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Bench `toolr` startup against built artifacts on Linux/macOS/Windows ([`2536a9f`](https://github.com/s0undt3ch/ToolR/commit/2536a9fac15042dc4a746f1f07885c184ff9273c))
- *(bench)* Drive bench job through `scripts/bench.py` ([`98a94b2`](https://github.com/s0undt3ch/ToolR/commit/98a94b2365b10ff0f9781f17078aebcef5c8ffd1))
## 0.23.0 - 2026-06-03

### Notes

## Enhancements

### `--help` now renders the full docstring (Examples, Notes, …); `-h` stays short

Building on the title/description split shipped in 0.22.1, both
`command_group(docstring=__doc__)` and `@command`-decorated function
docstrings now feed clap a multi-section render for `long_about`. The
first paragraph still drives the short `about` slot (shown next to the
name in the parent listing and on `-h`), but `--help` now shows the
short summary plus the long body plus any `Examples` / `Notes` /
`Warnings` / `See Also` / `References` / `Todo` / `Deprecated` /
`Version Added` sections from the docstring. `Args:` / `Arguments:`
stay out of the prose — they already appear as per-argument help
blocks.

Side-effects:

- `toolr <group> <cmd> -h` and `toolr <group> <cmd> --help` now differ:
  short form is just the summary, long form is the full render. The
  previous behaviour of always printing the long form on both was a
  paper-over from when `description` carried only the body paragraph.
- The shared rendering logic now lives exclusively in Rust
  (`toolr_core::docstrings::Docstring::full_description`), exposed to
  Python via PyO3. The parallel Python re-implementation in
  `toolr.utils._docstrings.Docstring.full_description` has been removed
  so the two surfaces can't drift.

See [#292](https://github.com/s0undt3ch/ToolR/issues/292).

## Bug fixes

### Tab completion offers `self` / `project` even outside a toolr project

Running `toolr <TAB>` from a directory with no `tools/` ancestor used to
return nothing and exit 1, so the shell fell back to filename
completion. The binary's own `self` / `project` subtree doesn't depend
on a project root, so it should always complete — only user-defined
groups need a discoverable `tools/`. `run_complete` now falls back to
an empty manifest when project discovery fails, then merges in the
built-in completion entries so `self`, `project`, and their children
are offered everywhere.

See [#306](https://github.com/s0undt3ch/ToolR/issues/306).

### <!-- 1 -->🐛 Bug Fixes

- *(parser)* Route description through full_description; --help shows Examples/Notes ([`d86c2bd`](https://github.com/s0undt3ch/ToolR/commit/d86c2bd9ff00cbe7980c8fa0c441cd3e0a45e32f))
- *(coverage)* Rewrite venv paths back to source; ratchet codecov per-crate ([`61a66ea`](https://github.com/s0undt3ch/ToolR/commit/61a66ea1c0f1bca1ef0018c1c9c3d71a6dbda385))
- *(coverage)* Switch source to package name so wheel-installed hits count ([`518b7bb`](https://github.com/s0undt3ch/ToolR/commit/518b7bb510d100530e7e678f470ac2f97ccdaf5f))
- *(complete)* Always offer self/project built-ins, even outside a toolr project ([`e964b0c`](https://github.com/s0undt3ch/ToolR/commit/e964b0cf205714379b231ba07adcedfbf848247a))

### <!-- 3 -->📚 Documentation

- *(agent)* Add CLAUDE.md with workspace facts and agent behavior rules ([`95272c7`](https://github.com/s0undt3ch/ToolR/commit/95272c7f964f039d4818e3e4a526f973a419441d))

### <!-- 6 -->🧪 Testing

- *(docstrings)* Direct coverage for full_description sections ([`88c6811`](https://github.com/s0undt3ch/ToolR/commit/88c68118c647d64a6cf5b160bae9156ad4ef2268))
## 0.22.1 - 2026-06-02

### Notes

## Bug fixes

### `command_group(docstring=...)` now splits the docstring into title + description

`command_group("name", docstring=__doc__)` previously stuffed the entire
docstring into the group's `description` field and left `title` empty,
so the blurb never appeared next to the group in the parent `--help`
listing (clap shows `about`, which is what `title` populates). The
parser now mirrors how `@command` function docstrings are handled — the
first paragraph becomes the group's `title` (clap's `about`) and the
remainder becomes the `description` (clap's `long_about`). An explicit
positional `title=` / `description=` still wins for either side.

Both the static parser (`parser/groups.rs`) and the runtime decorator
(`toolr._decorators.command_group`) now go through the same shared
`parse_docstring` helper, so static-manifest output and runtime
introspection agree. The runtime's old `title = name` fallback —
which would have produced a redundant `dbt-config  dbt-config` in
the parent listing — has been removed; an unset title now stays
empty, matching the static parser.

See [#292](https://github.com/s0undt3ch/ToolR/issues/292).

### <!-- 1 -->🐛 Bug Fixes

- *(parser)* Split command_group docstring into title + description ([`f09fc6c`](https://github.com/s0undt3ch/ToolR/commit/f09fc6c147cf35e3b2c02f23af197e05046b543c))

### <!-- 2 -->🚜 Refactor

- *(tests)* Drop `del fixture` pattern via autouse / usefixtures ([`5589c6f`](https://github.com/s0undt3ch/ToolR/commit/5589c6f05c5877ca3687fa0f4a5b868f7573b266))
## 0.22.0 - 2026-06-02

### Notes

## ⚠ Breaking changes

### `toolr project deps` removed; replaced by `toolr project venv`

- **What changed:** the `toolr project deps` subcommand group has
  been removed. Its two commands moved under `toolr project venv`:
    - `toolr project deps sync` → `toolr project venv sync`
    - `toolr project deps upgrade <pkg>` → `toolr project venv upgrade <pkg>`
- **Behavior change on `sync`:** the new `toolr project venv sync`
  honours the tools venv's freshness stamp by default and no-ops
  (exit 0, no `uv sync`) when the venv is already up to date.
  Use `--force` to re-run unconditionally — that matches what
  `toolr project deps sync` did before.
- **New `--quiet` flag on `sync`:** silent on success and on
  benign unattended-mode exits ("not a toolr repo", "lock missing",
  "uv install needs consent"). Designed for use from a mise
  `[hooks].enter` recipe — see
  [Auto-sync the tools venv on shell-enter](https://toolr.readthedocs.io/latest/installation/mise/#auto-sync-the-tools-venv-on-shell-enter).
- **Migration:** running `toolr project deps <anything>` at 0.22
  prints a tailored error pointing at the new path and exits with
  code 2.
- **Why:** the `deps` group only ever held venv-touching operations;
  collapsing it under `venv` puts every tools-venv operation in one
  place and makes room for future uv-wrapper subcommands (`add`,
  `remove`, `lock`, …) — see
  [#288](https://github.com/s0undt3ch/ToolR/issues/288) — to land in
  the obvious location.

### <!-- 0 -->🚀 Features

- *(venv)* Thread --quiet through run_uv_sync and sync_if_needed ([`114e3bd`](https://github.com/s0undt3ch/ToolR/commit/114e3bd355232582c6b9f40c81fcbe8d99c78af8))
- *(uv)* Add silent_refuse to ConsentMode for unattended callers ([`964921e`](https://github.com/s0undt3ch/ToolR/commit/964921e471b6242d925295ed381d16dce0b71943))
- *(cli)* Move project deps to project venv; flip sync default ([`6d93be1`](https://github.com/s0undt3ch/ToolR/commit/6d93be1e69e9769d103dd6339a082031dd3cb7be))
- *(completions)* Move project venv sync/upgrade; drop deps ([`6617956`](https://github.com/s0undt3ch/ToolR/commit/661795643abecdd01f5cd85c80605ee797cbe5cb))

### <!-- 1 -->🐛 Bug Fixes

- *(hints)* Point users to project venv sync, not project deps sync ([`7053a6b`](https://github.com/s0undt3ch/ToolR/commit/7053a6b8b4325724a8d5a56a25243000c9315fe2))
- *(ci)* Update Python test assertions and changelog link for venv rename ([`699a30b`](https://github.com/s0undt3ch/ToolR/commit/699a30bcb4250827300afa71e155623b15f2c87e))

### <!-- 10 -->💼 Other

- Add design for mise enter-hook auto-sync of tools venv ([`ede2108`](https://github.com/s0undt3ch/ToolR/commit/ede2108af24fce0638154779e2b7c2756826adf2))
- Add implementation plan for mise-enter-hook auto-sync ([`fdb0fb2`](https://github.com/s0undt3ch/ToolR/commit/fdb0fb20384b297dde3414187f6c51c7e90edc1d))
- Link design + plan to follow-up issue #288 ([`c5a8fea`](https://github.com/s0undt3ch/ToolR/commit/c5a8feaaad63a333dd01639e58effe8604e4b699))
- Archive mise-enter-auto-sync design + plan (implemented) ([`e7833a2`](https://github.com/s0undt3ch/ToolR/commit/e7833a2b3b35fc65a892b5728fc11a3fce6e921f))
- Design venv ↔ uv parity (drop venv upgrade, add lock/add/remove) ([`0f4676d`](https://github.com/s0undt3ch/ToolR/commit/0f4676d42fcdc237c645d10c9a0a7cc9daa29dd9))
- Implementation plan for venv ↔ uv parity ([`6eeb250`](https://github.com/s0undt3ch/ToolR/commit/6eeb25093ad42e9912d7a719eb60168d4f785246))
- Introduce UpgradeMode enum ([`051b382`](https://github.com/s0undt3ch/ToolR/commit/051b3829f9f1c728fb4ec47ce92c385e8ac0326e))
- Thread UpgradeMode through run_uv_sync + sync_if_needed ([`2916619`](https://github.com/s0undt3ch/ToolR/commit/291661908992b1a06b86e441065ffe4ae10f5bf9))
- Replace run_uv_lock_upgrade with general run_uv_lock ([`9015bbf`](https://github.com/s0undt3ch/ToolR/commit/9015bbf5b973951ab717d721f27f2395716dfe90))
- Add edit module with run_uv_add + run_uv_remove ([`485f41c`](https://github.com/s0undt3ch/ToolR/commit/485f41c63411f9be6daa2d594a15bcd22d50ee79))
- Thread UpgradeMode through EnsureOpts ([`a8c3322`](https://github.com/s0undt3ch/ToolR/commit/a8c332270e460591c2e78d4fb3dab35ed39f965f))
- Document venv upgrade removal + new lock/add/remove + sync -U/-P ([`5949d96`](https://github.com/s0undt3ch/ToolR/commit/5949d96cdd30892e29648347692f702b0c262657))
- Archive venv-uv-parity design + plan (implemented) ([`1503e53`](https://github.com/s0undt3ch/ToolR/commit/1503e535083bcf4e56302a9fc94a5070ab1c383a))

### <!-- 2 -->🚜 Refactor

- *(venv)* Replace ensure_venv_ready force flag with EnsureOpts ([`af7c111`](https://github.com/s0undt3ch/ToolR/commit/af7c1117f7ef23a7fdcd6aa546b2a680456655e3))

### <!-- 3 -->📚 Documentation

- *(snippets)* Regenerate project venv sync --help capture ([`18561a3`](https://github.com/s0undt3ch/ToolR/commit/18561a3155c2d3ecb33b1eebfe98ec0eae42efc1))
- *(snippets)* Remove obsolete project-deps-sync help capture ([`7bf6aea`](https://github.com/s0undt3ch/ToolR/commit/7bf6aea4464e2c8df0ac302ea38249a10ef50444))
- Rename toolr project deps references to project venv ([`5aa4c42`](https://github.com/s0undt3ch/ToolR/commit/5aa4c42d72c7a42fc4e199c5e298811085c47252))
- *(mise)* Document the enter-hook auto-sync recipe ([`bbce59e`](https://github.com/s0undt3ch/ToolR/commit/bbce59e738bcfd13917523df550254d5832248fd))
- *(unreleased)* Note the project deps → project venv rename ([`c5489b1`](https://github.com/s0undt3ch/ToolR/commit/c5489b10bb8f9da525a74693201672b13e02e33f))
- Regenerate cli-files snippets for venv lock/add/remove + sync -U/-P ([`6c49bd2`](https://github.com/s0undt3ch/ToolR/commit/6c49bd2b443594c93eed862ea99e04e29687cdc9))
- Cover venv lock/add/remove and migrate venv upgrade references ([`fcdbb97`](https://github.com/s0undt3ch/ToolR/commit/fcdbb977b34b76e6319f2ff51d3f285cbb23d1a7))

### <!-- 6 -->🧪 Testing

- *(cli)* Update smoke assertions to project venv sync ([`ea462ce`](https://github.com/s0undt3ch/ToolR/commit/ea462ceae140a17346751dab91b7a1d39a75677d))
- *(cli)* Rename project_deps_upgrade to project_venv_upgrade ([`35405fb`](https://github.com/s0undt3ch/ToolR/commit/35405fb524115ca80b45dfa885dbc888987d9c79))
- *(cli)* Integration coverage for project venv sync flag matrix ([`fb1574f`](https://github.com/s0undt3ch/ToolR/commit/fb1574f451611cb3030d94c675f116ddee08a1d0))
- *(deps-check)* Update missing-deps hint assertion to project venv sync ([`ffb4ca1`](https://github.com/s0undt3ch/ToolR/commit/ffb4ca173d8ca67d9459d809f67609290612832d))
- Cover venv handler success paths via stub-uv + fake-venv fixture ([`6f4fed6`](https://github.com/s0undt3ch/ToolR/commit/6f4fed6e45960b3e1f453b1435bd86fa8ac61459))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Bump throttle action max-sleep default to 15s ([`6c40d5a`](https://github.com/s0undt3ch/ToolR/commit/6c40d5a1139bf73b7e4f783c629d30a63240b4b5))
## Unreleased

### Notes

## ⚠ Breaking changes

### `project venv upgrade` removed in favour of `venv sync -U / -P`

- **What changed:** `toolr project venv upgrade <pkg>` is gone.
  Use `toolr project venv sync -U <pkg>` to upgrade a single package
  (or `-P <pkg>` repeatedly), or `toolr project venv sync -U` to
  re-resolve all packages.
- **Why:** uv expresses upgrades as flags on `lock` and `sync`, not as
  a standalone verb. Aligning toolr with uv's surface removes a
  toolr-specific verb that didn't pull its weight.
- **Migration:** mechanical rename. `venv upgrade foo` → `venv sync -P foo`.

## 🚀 New features

- *(project venv)* `lock` — wrap `uv lock` for refreshing
  `tools/uv.lock` without applying ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv)* `add <package>[@<version>]…` — wrap `uv add` against
  `tools/` ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv)* `remove <package>…` — wrap `uv remove` against
  `tools/` ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv sync)* `-U` / `--upgrade` and `-P` / `--upgrade-package`
  flags mirroring uv ([#288](https://github.com/s0undt3ch/ToolR/issues/288))

## 0.21.1 - 2026-06-01

### <!-- 1 -->🐛 Bug Fixes

- *(dispatch)* Surface arg() metadata TypeError from dispatcher detection ([`415435d`](https://github.com/s0undt3ch/ToolR/commit/415435d41c2cdfa90d2bd5f96781c559df6b0410))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Bump GitHub Action pin SHAs and MISE_VERSION ([`8b1ae08`](https://github.com/s0undt3ch/ToolR/commit/8b1ae08f256c185b05d35eb1b578af786b26126f))
## 0.21.0 - 2026-05-29

### Notes

## ⚠ Breaking changes

### `installation/mise/` plugin removed in favour of the aqua backend

- **What changed:** the bundled asdf-style mise plugin under
  `installation/mise/` has been deleted. mise users now install
  toolr through mise's built-in
  [aqua backend](https://mise.jdx.dev/dev-tools/backends/aqua.html)
  against the
  [aqua-registry entry](https://github.com/aquaproj/aqua-registry/tree/main/pkgs/s0undt3ch/ToolR):

  ```sh
  mise use aqua:s0undt3ch/ToolR@latest
  ```

  No plugin to register, no repository to clone, no `git::<url>//<subdir>`
  syntax to maintain. The aqua-registry entry pulls the same signed
  GitHub release archive the in-tree plugin used to fetch, so the
  installed binary is identical.

- **Why:** the in-tree plugin duplicated logic that mise already
  provides through the aqua backend (release-asset selection,
  SHA-256 verification, archive extraction). Owning that surface
  meant keeping the asdf-protocol hooks in sync with mise's
  evolving subdir-URL semantics and shipping our own attestation
  logic — work that aqua does once for everyone.

- **Migration:** replace the install command everywhere it appears
  (README, quickstart, mise docs page, internal runbooks, CI
  workflows). The old form

  ```sh
  mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise
  mise use --global toolr@latest
  ```

  becomes

  ```sh
  mise plugin uninstall toolr   # if previously installed
  mise use aqua:s0undt3ch/ToolR@latest
  ```

  (Add `--global` if you want toolr available across every
  directory; the project-scoped form above is the default we
  show in the headline docs.)

  The aqua-registry entry requires aqua-registry `v4.518.0` or
  newer (the first release that contains
  `pkgs/s0undt3ch/ToolR/registry.yaml`) and mise's aqua backend,
  which is built in and needs no extra configuration.

### <!-- 0 -->🚀 Features

- *(action)* Install uv via astral-sh/setup-uv when missing or version-pinned ([`8eb6906`](https://github.com/s0undt3ch/ToolR/commit/8eb69066471d8b3d6816f11d9f462e4e87944604))

### <!-- 1 -->🐛 Bug Fixes

- *(uv)* Detect musl libc in auto-installer and add actionable exec-failure hint ([`5af1ca0`](https://github.com/s0undt3ch/ToolR/commit/5af1ca056d8444b53c2c75f31d0c361af02380ca))
- *(action)* Bake released version into action.yml so SHA pins resolve correctly ([`180b3d1`](https://github.com/s0undt3ch/ToolR/commit/180b3d197f6da599c10485eb22b7256b6c430aee))
- Regenerate toolr-ci-setup action.md and widen build-skill-refs trigger ([`71b1830`](https://github.com/s0undt3ch/ToolR/commit/71b183025a0a6157b6a95a1db8f603840064f2bc))
- Regenerate toolr-ci-setup action.md for new `uv-version` input ([`62ae52c`](https://github.com/s0undt3ch/ToolR/commit/62ae52c48a63c1bd01dee03445112a33667c0b95))

### <!-- 10 -->💼 Other

- Drop in-tree asdf plugin, install via aqua backend ([`5f60df4`](https://github.com/s0undt3ch/ToolR/commit/5f60df48949a0f07d8ac5c51025d1ca02dc12330))
- Drop --global from headline install snippets ([`52e7357`](https://github.com/s0undt3ch/ToolR/commit/52e73572ec7834d8a3f1cf0c6649ac8e85690371))
- Leave the uv workspace, resolve toolr-py from PyPI ([`fa9fa20`](https://github.com/s0undt3ch/ToolR/commit/fa9fa20329539bd2374c2ee6cbae6f7a2eeb10d6))

### <!-- 2 -->🚜 Refactor

- *(version)* Write dev versions to action.yml on every push so the bake-in is exercised in CI ([`7995fb0`](https://github.com/s0undt3ch/ToolR/commit/7995fb0bc2cde905448e35cb5c08fe678c029d4f))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Drop self action-version pin and auto-sync rolling tags on release ([`e46c19c`](https://github.com/s0undt3ch/ToolR/commit/e46c19c826d414a828471bbf79775eef3ce01a35))
- *(prepare-release)* Skip tools venv sync (built-in toolr only) ([`c5ef129`](https://github.com/s0undt3ch/ToolR/commit/c5ef129fbd1659e1c149b08a3ce586af12773b2e))
- Format `build-skill-refs` and `regen-doc-snippets` `files:` regexes as verbose multi-line ([`44649d2`](https://github.com/s0undt3ch/ToolR/commit/44649d2e50989e4d73b3075fad946007698d1527))
- Collapse per-alt `$` anchors in `build-skill-refs` regex to a single trailing `)$` ([`563467d`](https://github.com/s0undt3ch/ToolR/commit/563467d3aa046d9e4838d8f19b2192740e50a2fc))
- *(_test)* Pass `--no-sync` to `uv run` so prebuilt toolr-py wheel survives ([`c901231`](https://github.com/s0undt3ch/ToolR/commit/c901231a4a42d2bf84b1f1a8f984e3e7fcb67d90))
## 0.20.1 - 2026-05-28

### Notes

## ⚠ Breaking changes

### `s0undt3ch/ToolR` GitHub Action: binary-only install, no more pipx

- **What changed:** the `Setup ToolR` composite action no longer
  installs toolr via `pipx install toolr==<version>`. The 0.20.0
  release shipped toolr as a binary-only PyPI wheel (no Python
  source), which pipx cannot install — so the old action path was
  already broken at the point this change landed.

  The rewritten action downloads the toolr binary archive directly
  from a GitHub release (`gh release download` with a `curl`
  fallback), cryptographically verifies the SLSA build provenance
  via `gh attestation verify`, caches the result, and puts the
  binary on `PATH`. It also caches `tools/.venv` keyed on
  `tools/pyproject.toml` + `tools/uv.lock` and sets
  `TOOLR_VENV_LOCATION=in-tree` so the cache works out of the box.

- **Minimum version:** the action refuses to install toolr below
  `0.20.0`. Earlier releases used the Python source distribution
  and are not compatible with the binary-only flow.

- **Migration:** remove the deprecated `python-path` and
  `requirements-file` inputs from your workflow's `Setup ToolR`
  step; the action has no use for them now that toolr is a
  standalone binary with its Python deps in the per-project
  `tools/.venv`. New optional inputs: `version` (defaults to the
  action ref, falling back to `latest`), `skip-attestation`
  (defaults to `false`), `cache-prefix` (defaults to `setup-toolr`),
  and `cache-tools-venv` (defaults to `true`).

### `installation/mise/` plugin: minimum toolr `0.20.0`, attests by default

- **What changed:** the bundled mise plugin under
  `installation/mise/` now rejects toolr versions below `0.20.0`
  (matching the action's cutoff) and verifies the SLSA build
  provenance via `gh attestation verify` on every install. Set
  `TOOLR_SKIP_ATTESTATION=1` to bypass — the plugin tells you so
  loudly if `gh` is missing from `PATH`.

### `installation/mise/` plugin: install URL requires mise `v2026.5.11+`

- **What changed:** the documented install command moved from
  `mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise`
  to
  `mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise`.

  The `#<subdir>` form was never a valid mise syntax — mise has
  always interpreted `#` as a git ref selector. The correct
  subdirectory syntax (`git::<git-url>//<subdir>`) only landed in
  mise [v2026.5.11](https://github.com/jdx/mise/pull/9893)
  (May 17, 2026). Workflows pinning `MISE_VERSION` below that
  cutoff need to bump.

- **Migration:** swap the install command everywhere it appears
  (READMEs, CI workflows, internal runbooks) and ensure your mise
  installation is `v2026.5.11` or newer. The plugin source itself
  is unchanged.

## New features

### `TOOLR_VENV_LOCATION` environment variable

The new `TOOLR_VENV_LOCATION` env var overrides the
`[tool.toolr] venv-location` setting in `tools/pyproject.toml`.
Accepts the same `in-tree` / `cache` spellings the TOML key does.
Intended primarily for CI: the `Setup ToolR` action sets it to
`in-tree` automatically so workflows can cache `tools/.venv`
directly without forcing every consumer repo's
`tools/pyproject.toml` to declare `venv-location = "in-tree"`.

### Agent skills

Toolr now ships two in-tree agent skills, installable via
`skillshare` from this repository:

- **`toolr-command-authoring`** — teaches LLM coding assistants
  how to author toolr commands in a project's own `tools/*.py`
  files. Anchored on `toolr project init` and
  `toolr <group> <cmd> --help`; the API surface and docstring
  conventions are regenerated from `toolr-py`'s public surface
  and the parser's section-header table.
- **`toolr-command-packaging`** — teaches LLM coding assistants
  how to ship an existing set of toolr commands as a
  distributable Python plugin. Anchored on the in-tree
  `examples/plugin-package/`; the manifest fragment schema is
  regenerated from `toolr-core`'s serde types.
- **`toolr-ci-setup`** — teaches LLM coding assistants how to
  integrate toolr into a project's GitHub Actions CI. Covers the
  `s0undt3ch/ToolR` GitHub Action: pinning policy, two canonical
  recipes (run a toolr command; gate
  `toolr self build-manifest --check`), and the common failure
  modes a caller hits first. Installable via `skillshare` from
  `skills/toolr-ci-setup/`.

A new maintainer-only `crates/xtask/` workspace crate hosts the
generator (`cargo xtask build-skill-refs`). The `--check` variant
runs in CI on every PR (alongside the existing example-plugin
manifest check) so a public-surface change that forgets to
regenerate the skill references cannot land. A `prek` hook entry
gives the same gate locally.

`cargo xtask build-skill-refs` gains a third generator
(`ci_setup::action`) that rebuilds
`skills/toolr-ci-setup/references/action.md` from the
repository-root `action.yml`. The existing `--check` CI gate
automatically covers the new file.

`docs/skills.md` install instructions now use the `skillshare`
parent-path picker pattern
(`skillshare install s0undt3ch/toolr/skills`) instead of one
command per skill, so the install block does not grow with each
new skill.

See [docs/skills.md](https://toolr.readthedocs.io/latest/skills/)
for the user-facing installation flow.

### <!-- 0 -->🚀 Features

- *(venv)* TOOLR_VENV_LOCATION env var overrides venv-location config ([`5bba1a3`](https://github.com/s0undt3ch/ToolR/commit/5bba1a3e1605ef9cccfbf0fd6780e1d83da9a1bc))
- *(action)* Binary-download + SLSA attest install for setup-toolr ([`ca66e25`](https://github.com/s0undt3ch/ToolR/commit/ca66e251de4c9648e1db91718ee56da3df7285b3))
- *(mise)* Minimum toolr 0.20.0 + verify SLSA build provenance ([`51d38c5`](https://github.com/s0undt3ch/ToolR/commit/51d38c56185b2dbbf313173e1ce3add50064bc62))
- *(mise)* Prefer 'gh' CLI over curl/wget for github.com hits ([`3b2edf0`](https://github.com/s0undt3ch/ToolR/commit/3b2edf0c3b0a66349dfac4d44103e09b1e401d23))
- *(config)* Add .rustfmt.toml + .editorconfig with 100-col policy ([`261e5ea`](https://github.com/s0undt3ch/ToolR/commit/261e5ea6a287be4823768ccc3489ae14ccd8b9eb))
- *(xtask)* Scaffold maintainer-only crates/xtask/ workspace crate ([`b53ebbd`](https://github.com/s0undt3ch/ToolR/commit/b53ebbdfcbc3f1e69a5b8e9d4ff936abeb577c04))
- *(docstrings)* Expose KNOWN_SECTION_HEADERS as introspectable table ([`b8248e3`](https://github.com/s0undt3ch/ToolR/commit/b8248e306b5f9e22521d26ad853051a8e68526d0))
- *(xtask)* Generate authoring skill references from toolr-py source ([`aeef5d2`](https://github.com/s0undt3ch/ToolR/commit/aeef5d284aa1b5e5aaae375156b6433d631f6f3c))
- *(skills)* Author skill prose for toolr-command-authoring ([`1bcb0a3`](https://github.com/s0undt3ch/ToolR/commit/1bcb0a381174607b0b0678629d47910a3d85acee))
- *(xtask)* Generate packaging skill references from toolr-core types ([`fb440b4`](https://github.com/s0undt3ch/ToolR/commit/fb440b4380303f72f461b5c18b9f7f95a4f6b619))
- *(skills)* Author skill prose for toolr-command-packaging ([`41d5a4d`](https://github.com/s0undt3ch/ToolR/commit/41d5a4dc82536af0c9b6873267b8246c1d9ac0ff))
- *(xtask)* Break generator prose at sentence boundaries ([`b85c464`](https://github.com/s0undt3ch/ToolR/commit/b85c464ae99a3a0dff62eb44ec6be9a0d52cd471))

### <!-- 1 -->🐛 Bug Fixes

- *(action)* Run `toolr project deps sync` + cache the resolved venv path ([`153052a`](https://github.com/s0undt3ch/ToolR/commit/153052a8bc94cedae8a01b79890282694de4ab59))
- *(action)* Drive uv sync directly with --no-default-groups + --frozen ([`08aa7db`](https://github.com/s0undt3ch/ToolR/commit/08aa7dbf558da93064e8dc063d912b56db4d2fc7))
- *(install-smoke)* PS7 latest-version resolver + mise plugin link ([`33810bd`](https://github.com/s0undt3ch/ToolR/commit/33810bda27800ec89d196baf0ecad44537a8b7e0))
- *(mise)* Authenticate list-all against api.github.com when possible ([`a4602eb`](https://github.com/s0undt3ch/ToolR/commit/a4602ebef79ebaaa36b24f947b626c40dc32e61e))
- *(mise)* Also authenticate bin/download against github.com ([`3ede133`](https://github.com/s0undt3ch/ToolR/commit/3ede133bdcd3a8e1662e23e0fc1cfdd8969980e1))
- *(lint)* Exclude generated skills/*/references from rumdl ([`0d03a35`](https://github.com/s0undt3ch/ToolR/commit/0d03a351d70e036c830e0b5b1c7dc0bc8e0108d5))
- *(skill-refs)* LF line endings on Windows + show diff in CI on drift ([`42866c8`](https://github.com/s0undt3ch/ToolR/commit/42866c8d09a00596ef0431c2c1dfb71ddee872e1))
- *(skill-refs)* LF line endings on skill example .py files ([`528c8c7`](https://github.com/s0undt3ch/ToolR/commit/528c8c742b7eb54990b2566331752f87a11f322b))
- *(mise)* Correct plugin URL syntax + bump MISE_VERSION ([`eb52b85`](https://github.com/s0undt3ch/ToolR/commit/eb52b859ad1c9c83abaca4faa0ad78f33df25dd1))

### <!-- 10 -->💼 Other

- *(xtask)* Collapse to single debug alias + prek hook regenerates ([`b7f32ab`](https://github.com/s0undt3ch/ToolR/commit/b7f32abe27b808c223b443ae1de62db9d77dc51d))
- Add design for toolr-ci-setup agent skill ([`3e0d9b7`](https://github.com/s0undt3ch/ToolR/commit/3e0d9b7383fff66152bdb5d118628e58308560ea))
- Add implementation plan for toolr-ci-setup skill ([`b8a21f4`](https://github.com/s0undt3ch/ToolR/commit/b8a21f443e98d889bb33cca5d8b6b3b8629bc14a))
- Add serde_yml + indexmap for upcoming action.yml generator ([`701a546`](https://github.com/s0undt3ch/ToolR/commit/701a546f5a890b72e844c989cdd3a61a5242b0f4))
- Add ci_setup generator (renders action.md from action.yml) ([`54234df`](https://github.com/s0undt3ch/ToolR/commit/54234dfb6f6c1d20b91bd99b13fad5e4a5296079))
- Register ci_setup generator; commit generated action.md ([`7179547`](https://github.com/s0undt3ch/ToolR/commit/71795479fd214a2ee014a884d298cd1602022f63))
- Assert action.yml inputs/outputs match references/action.md ([`5c5b722`](https://github.com/s0undt3ch/ToolR/commit/5c5b722db02d00dccd9c70618ee3955e20177480))
- Author SKILL.md body and trigger ([`f9d3c6c`](https://github.com/s0undt3ch/ToolR/commit/f9d3c6ce65f3669bbb27680039645abe62b800fa))
- Add README ([`9e45ac6`](https://github.com/s0undt3ch/ToolR/commit/9e45ac6a35230d48250fb68e6e39baf745c9e6a7))
- Add REVIEW checklist ([`ac6e9d4`](https://github.com/s0undt3ch/ToolR/commit/ac6e9d4d0b07fd27c3e85bf4ff8df68da3e03186))
- Add trigger fixture ([`d6b4e51`](https://github.com/s0undt3ch/ToolR/commit/d6b4e51902bf9f3ab9213216f4d1ecdd7d07c3ee))
- Cross-link to toolr-ci-setup ([`498fa9b`](https://github.com/s0undt3ch/ToolR/commit/498fa9bdb2dbaea66e16aa2e354d533cb1d020e1))
- Cross-link to toolr-ci-setup from rule 3 ([`bb884cb`](https://github.com/s0undt3ch/ToolR/commit/bb884cbdfdc281b73aa0de190a0564c7bb0cca74))
- Note toolr-ci-setup skill and install-pattern change ([`77265bd`](https://github.com/s0undt3ch/ToolR/commit/77265bd6fa0d808e14dbbcf7e71e21b03fb516f1))
- Swap serde_yml for serde_yaml_ng to drop unsound libyml dep ([`e0615de`](https://github.com/s0undt3ch/ToolR/commit/e0615def09bed9f62e296469742f907b858ad8c9))

### <!-- 3 -->📚 Documentation

- *(specs)* Add toolr command-authoring skill design ([`e5e095b`](https://github.com/s0undt3ch/ToolR/commit/e5e095b548fcf79618965eabbd494e66ed61c13f))
- *(specs)* Add toolr command-packaging skill design ([`91ef08e`](https://github.com/s0undt3ch/ToolR/commit/91ef08e49d1088e41c2fb898d8672d3a7cd0ea38))
- *(specs)* Drop python introspection subprocess from skill-refs generator ([`7a5154b`](https://github.com/s0undt3ch/ToolR/commit/7a5154be85f7b1fc609b545c3ae69d371c09f66f))
- *(specs)* Use toolr.__all__ as the public-surface contract for skill-refs ([`273bf9d`](https://github.com/s0undt3ch/ToolR/commit/273bf9d09753f0f002b9b22fb746fdf04d22a72a))
- *(specs)* Add docstring conventions; reuse toolr-core parser/docstrings ([`b90d7c3`](https://github.com/s0undt3ch/ToolR/commit/b90d7c3d4cd2d94869d3fc63494a4dd409ffdb97))
- *(specs)* Packaging skill anchors on examples/plugin-package and new CLI surface ([`5dccaf9`](https://github.com/s0undt3ch/ToolR/commit/5dccaf967eaf6ee4f2f208822219d710a11d880d))
- *(specs)* Consolidated plan for authoring + packaging skills ([`d92167b`](https://github.com/s0undt3ch/ToolR/commit/d92167bf97712554cd2fa891ec2a0b4d0cbf568b))
- *(skills)* Add docs landing page, queue release notes, archive designs ([`5b34278`](https://github.com/s0undt3ch/ToolR/commit/5b3427837bfb06c8fb9f28a501986374f79fe96c))
- *(skills)* Link agent skills from README and docs landing ([`d72bec8`](https://github.com/s0undt3ch/ToolR/commit/d72bec8b5ada26d124446c275b635dc7e5498da0))
- List toolr-ci-setup and consolidate install block ([`b6086fb`](https://github.com/s0undt3ch/ToolR/commit/b6086fbfee1a152cffe740fd3a569ab7f28270f4))

### <!-- 5 -->🎨 Styling

- *(python)* Split ruff line-length (format 100, lint 120) + reformat ([`85dbecc`](https://github.com/s0undt3ch/ToolR/commit/85dbeccc15abe2b9497eca2449fb57442bfcc0f2))
- *(docs)* Enable MD013 with 120-char ceiling + reflow long lines ([`350c88f`](https://github.com/s0undt3ch/ToolR/commit/350c88fcc0ad5d313a1563102dbfcf375cf8d02f))

### <!-- 6 -->🧪 Testing

- *(install-smoke)* Exercise mise plugin via canonical URL form ([`8797fc4`](https://github.com/s0undt3ch/ToolR/commit/8797fc43474aec6934f1994828d0cbbe851b40e1))
- *(skills)* Snapshot manifest over the authoring skill's examples tree ([`f347410`](https://github.com/s0undt3ch/ToolR/commit/f347410e71f7c16c1a6788a8ea04130e34b3852f))
- *(skills)* Refresh authoring-skill manifest snapshot after ruff reformat ([`9a709ba`](https://github.com/s0undt3ch/ToolR/commit/9a709ba0eab746e1505a98fb041ee8d321bc8b23))
- *(xtask)* Use assert_cmd::cargo_bin so subprocess coverage counts ([`2b5e351`](https://github.com/s0undt3ch/ToolR/commit/2b5e35135f6dbe7f181c7293a6036b226963e448))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Update workflows + docs for new setup-toolr action shape ([`a6ec0e8`](https://github.com/s0undt3ch/ToolR/commit/a6ec0e8dd2dfd6085d826e0965faa2c7dfae69f6))
- *(install-smoke)* Add pull_request trigger + gate remote-only jobs ([`b1bad02`](https://github.com/s0undt3ch/ToolR/commit/b1bad021a2eaeb1a75ee5fc7bfcfd9d1de1448d5))
- *(_test)* Regen-and-diff the example-plugin manifest gate too ([`dcf470c`](https://github.com/s0undt3ch/ToolR/commit/dcf470ce7f7199f21e81d6d565cf956d34fa9399))
## 0.20.0 - 2026-05-26

### Notes

This release lands the **Rust front-end rewrite** together with a
**workspace split** and a **distribution-channel reshuffle**. The
argparse-driven Python CLI is fully retired; the `toolr` command is
now a native Rust binary, and the PyPI footprint splits into two
packages so the CLI and the Python runtime can be installed
independently.

If you only invoke `toolr ...` from a project that already ships a
`tools/` directory, the smallest migration is:

1. Install the new CLI binary (`pip install toolr`, `installation/install.sh`,
   mise, or a GitHub release archive — see below).
2. Run `toolr project init` from your repo root. This scaffolds the
   new `tools/pyproject.toml` (with `toolr-py` already declared) and a
   `tools/uv.lock` alongside your existing `tools/*.py` scripts.
3. Move any Python dependencies your `tools/*.py` previously pulled in
   (e.g. via the project's main `pyproject.toml` dev group) into the
   `[project.dependencies]` list inside the new `tools/pyproject.toml`,
   then run `toolr project deps sync`.
4. **Commit `tools/pyproject.toml` and `tools/uv.lock` to git.** Both
   are part of the per-project tools venv contract — without them,
   collaborators and CI can't reproduce your tools venv.

The sections below spell out every other place this rewrite is visible
from the outside.

## ⚠ Breaking changes

### `python -m toolr` is gone

- **What changed:** the `[project.scripts] toolr` console entry
  point and `toolr/__main__.py` have been removed. The Python
  package no longer ships a CLI at all — invoking `python -m toolr`
  fails with `No module named toolr.__main__`.
- **Migration:** install the new Rust CLI (via `pip install toolr`,
  `install.sh`, mise, or a GitHub release archive — see below) and
  invoke it as `toolr <args>`. The argument surface is unchanged;
  only the entry point moved.

### `pip install toolr` no longer makes `import toolr` work

- **What changed:** PyPI now hosts **two** packages. `toolr` is a
  binary-only wheel (`bindings = "bin"`) — it drops the `toolr`
  executable into the wheel's `scripts/` directory and has **no
  Python source**. The Python runtime (`import toolr`, `Context`,
  `command_group`, `toolr.utils`, the `_rust_utils` extension)
  lives in a separate package, `toolr-py`.
- **Migration:** if your project's `tools/*.py` scripts do
  `from toolr import ...`, declare `toolr-py` as a dependency of
  the tools venv. The fastest path is `toolr project init` from
  your repo root — it scaffolds `tools/pyproject.toml` with
  `toolr-py` already declared and a matching `tools/uv.lock`.
  Both files belong in git. The CLI on `PATH` will then find
  `toolr-py` when it shells out to execute a command. See the
  [installation docs](https://toolr.readthedocs.io/en/latest/installation/)
  for the full layout.

### mise plugin: external `mise-toolr` repo retired

- **What changed:** the mise plugin used to be hosted out-of-tree
  at `s0undt3ch/mise-toolr`. It is now self-contained at
  `installation/mise/` inside this repo, and the external
  `mise-toolr` repository is retired. The old
  `mise plugin add toolr https://github.com/s0undt3ch/mise-toolr`
  installation stops working.
- **Migration:**

  ```sh
  mise plugin remove toolr   # if previously installed from the old path
  mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
  mise use --global toolr@latest
  ```

### The argparse Python CLI internals were deleted

- **What changed:** `toolr/__main__.py`, `toolr/_parser.py`, and
  `toolr/_registry.py` have been deleted. Anything that imported
  `Parser`, `CommandRegistry`, or other internals from those
  modules will break.
- **Migration:** the user-facing decorator surface
  (`command_group`, `@command`, `CommandGroup`,
  `MANIFEST_SCHEMA_VERSION`) is preserved. It has moved to
  `toolr._decorators` and is re-exported from the top-level
  package, so the public form continues to work:

  ```python
  from toolr import command_group, command, CommandGroup
  ```

  If you were reaching into `toolr._registry` or `toolr._parser`
  directly, there is no replacement — the Rust binary owns
  manifest discovery, argument parsing, and dispatch now. The
  Python runtime is invoked per-command via a JSON spec file
  (`toolr._runner`).

### `testing.py` import surface tightened

- **What changed:** `toolr.testing` previously re-exposed a few
  helpers that leaned on the now-deleted `_parser` /
  `_registry` modules. Those have been replaced or removed as
  part of the three-way test prune; the supported public surface
  is whatever `toolr.testing` exports today.
- **Migration:** if your test suite imported internal helpers
  from `toolr.testing` and the import now fails, lean on the
  documented `Context` / `command_group` factories or open an
  issue describing the use case.

### `rich-argparse` is no longer pulled in

- **What changed:** `rich-argparse` was only used by the deleted
  `_parser.py`. It has been dropped from `toolr-py`'s
  dependencies. Anything that relied on toolr transitively
  bringing it into the tools venv will need to declare it
  explicitly.
- **Migration:** add `rich-argparse` to your own
  `pyproject.toml` if you depend on it for non-toolr code.

## 🚀 New features

- **Rust CLI binary.** `toolr` is now a native binary built from a
  Cargo workspace. Manifest discovery, argument parsing, help
  rendering, and command dispatch all run in Rust. Python is only
  involved at execution time (per-command subprocess via
  `toolr._runner`), so cold-start latency drops dramatically and
  shell completion is no longer gated on Python import overhead.
- **Three install channels, one source of truth.** The same
  `toolr` binary ships through:
    - `pip install toolr` (a new `py3-none-<plat>` binary wheel),
    - `curl ... | sh` via `installation/install.sh`,
    - mise via `installation/mise/`,
    - GitHub Release archives (`toolr-<version>-<target>.tar.gz`,
      with `.sha256` siblings).

  All four are produced from the same workspace build and share a
  single version number.
- **`toolr-py` PyPI package.** A standalone wheel providing
  `import toolr` for user tool scripts — declared in
  `tools/pyproject.toml`, materialised into the tools venv by
  `uv sync`. Decouples "what CLI you have on PATH" from "what
  Python bindings your tool scripts pin."
- **Python 3.14 support.** Added to the test matrix and the
  `toolr-py` classifier list.
- **Per-project `tools/` venv with uv.** The Rust binary
  materialises (and, if needed, bootstraps) a `tools/` venv via
  `uv` before each execute. Includes missing-dependency
  diagnostics, manifest caching, and cache pruning. See the
  rebuilt [installation /
  usage](https://toolr.readthedocs.io/en/latest/) docs for the
  end-to-end story.
- **Native shell completion.** Generated by the Rust frontend
  (clap-based), available for the usual shells.
- **In-repo mise plugin smoke test.** The plugin lives at
  `installation/mise/` and is covered by the same end-to-end
  smoke harness as the other install channels.
- **SLSA build provenance on every shipped artifact.** Every
  wheel (`toolr-*.whl`, `toolr_py-*.whl`), sdist, per-triple
  binary archive (`toolr-<version>-<triple>.tar.gz` /
  `.zip`), and the release notes / patch files carry a
  cryptographically signed attestation generated by
  `actions/attest-build-provenance`. Verify any of them with:

  ```sh
  gh attestation verify <file> --owner s0undt3ch
  ```

  `install.sh` already passes `--verify-attestation=require` to
  reject any archive whose attestation does not validate. See
  GitHub's [artifact attestations
  docs](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations-to-establish-provenance-for-builds)
  for the full verification model.

## 🗺 Migration cheat-sheet

| If you used... | Replace with... |
|---|---|
| `python -m toolr ...` | `toolr ...` (install the CLI via pip / install.sh / mise / release archive) |
| `pip install toolr` to get `import toolr` | `pip install toolr-py` (or run `toolr project init` to scaffold `tools/pyproject.toml` + `tools/uv.lock` with `toolr-py` declared) |
| `mise plugin add toolr https://github.com/s0undt3ch/mise-toolr` | `mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise` |
| `from toolr._registry import command_group` | `from toolr import command_group` |
| `from toolr._registry import CommandGroup` | `from toolr import CommandGroup` |
| `from toolr._parser import Parser` (and friends) | No replacement — the Rust binary owns parsing now |
| Relying on `rich-argparse` via toolr | Depend on `rich-argparse` directly |

## 🧱 Internal — for contributors

These changes affect anyone hacking on toolr itself but are
invisible to end users:

- **Cargo workspace split.** Three crates under `crates/`:
    - `toolr-core` — private library (no pyo3, no clap). Manifest
      discovery, AST parsing, manifest cache, venv plumbing.
    - `toolr` — the binary crate (`bindings = "bin"`). Depends on
      `toolr-core` plus `clap` and `termimad`.
    - `toolr-py` — the pyo3 dynlib + Python source
      (`bindings = "pyo3"`, `module-name = "toolr.utils._rust_utils"`).
- **Python source location.** Moved from `python/toolr/` to
  `crates/toolr-py/python/toolr/`. The repo-root `python/`
  directory is gone.
- **Two PyPI wheels, one workspace version.** `toolr` and
  `toolr-py` are released together at the same
  `[workspace.package] version`. Both have their own
  `pyproject.toml` and `cibuildwheel` matrices; CI fans the
  wheel builds out per crate and reassembles them at release.
- **Root `pyproject.toml` is dev-tooling only.** It retains
  `[tool.ruff]`, `[tool.mypy]`, `[tool.pytest.ini_options]`,
  `[tool.uv]`, `[tool.uv.workspace]`, `[dependency-groups]` —
  no `[build-system]`, no `[project]`, no `[tool.maturin]`.
- **No `python` feature flag.** The
  `[features] python = ["pyo3"]` dance and the
  `#[cfg(feature = "python")]` annotations are gone. pyo3 lives
  exclusively in `crates/toolr-py/` as a non-optional
  dependency.
- **`tools/version.py` simplified.** Cargo.toml writes go
  through `cargo set-version` (via cargo-edit) instead of
  hand-rolled regex edits.
- **`rich` is a direct `toolr-py` dependency.** Previously
  transitive through `rich-argparse`.
- **`UNRELEASED.md` → release notes pipeline.** The file you're
  reading is now part of every release: `_prepare-release.yml`
  strips this comment header, exports the body as
  `TOOLR_RELEASE_NOTES`, and the cliff template renders it as
  a `### Notes` section in both the GitHub release body and
  `CHANGELOG.md` under the new version's heading.
- **Dogfooding tools venv.** `tools/pyproject.toml` declares
  `toolr-py` as a workspace dependency — the repo's own
  `tools/*.py` scripts run against the same Python runtime
  users get from PyPI.

### Breaking — entry-point plugins removed

The `toolr.commands` entry-point mechanism for registering third-party
plugins is removed. Plugin authors must instead ship a static
`toolr-manifest.json` at the root of their installed Python package.
toolr's dispatch path is now pure Rust and never spawns Python just to
discover commands.

Migrating a plugin:

1. From inside the plugin's repo, run `toolr self build-manifest <pkg>`
   (replace `<pkg>` with the dotted package name). This writes a
   `toolr-manifest.json` next to your package's `__init__.py`.
2. Include the file in your built wheel. For hatchling, add this to
   `pyproject.toml`:

   ```toml
   [tool.hatch.build.targets.wheel]
   include = ["src/<pkg>/toolr-manifest.json"]
   ```

   For setuptools, add `include src/<pkg>/toolr-manifest.json` to
   `MANIFEST.in`.
3. Wire `toolr self build-manifest <pkg> --check` into CI and as a
   pre-commit hook. The `--check` flag exits non-zero when the
   committed `toolr-manifest.json` no longer matches what would be
   generated from current sources.
4. Delete the now-inert `[project.entry-points.'toolr.commands']`
   section from your plugin's `pyproject.toml`.

If you don't ship the file, your plugin's commands will not appear in
`toolr --help` or `toolr <group> --help`.

### Improved — argparse options with underscores accept both spellings

`toolr` normalises the canonical CLI form for argparse-scanned options
to dashes, so `add_argument('--skip_warm_cache', ...)` shows up in
`--help` and shell completion as `--skip-warm-cache`. The original
underscored spelling is now also accepted at parse time, so muscle
memory from the upstream tool (`--skip_warm_cache`) keeps working
without the user having to know about the rewrite.

### Improved — dispatch detects stale manifests automatically

Adding, removing, or editing `tools/*.py` is now reflected on the very
next `toolr <user-cmd>` or `toolr --help` invocation — no
`toolr project manifest rebuild` needed. Installing or upgrading a
third-party plugin that ships its own `toolr-manifest.json` is
similarly picked up automatically. The check is pure Rust and adds
single-digit milliseconds on a warm cache. When a rebuild fails (for
example a syntax error in `tools/foo.py`), toolr serves the cached
manifest with a warning identifying the offending file rather than
blocking dispatch.

### <!-- 0 -->🚀 Features

- *(rust)* Add toolr binary target with placeholder main ([`27d1279`](https://github.com/s0undt3ch/ToolR/commit/27d12794da2a5cdcad3ad9754969906f294a9c63))
- *(rust)* Wire toolr binary to clap for --help and --version ([`bbdbb2b`](https://github.com/s0undt3ch/ToolR/commit/bbdbb2b8e01ac89d49a7e3abfa4647af7d99d5fc))
- *(manifest)* Add manifest data model with serde round-trip tests ([`9b8ecfc`](https://github.com/s0undt3ch/ToolR/commit/9b8ecfc86a95a2e5d498814db68c62af5bd3c471))
- *(manifest)* Add manifest load/write with schema version validation ([`0e6283d`](https://github.com/s0undt3ch/ToolR/commit/0e6283ddd99f3d3fd3ad7b4b3b4db8e15e666b70))
- *(discovery)* Walk upward to locate project root from cwd ([`5108aad`](https://github.com/s0undt3ch/ToolR/commit/5108aad1662e1c9729718500f26934ae134777f9))
- *(hash)* Deterministic hashing over tools/**/*.py contents ([`b7db766`](https://github.com/s0undt3ch/ToolR/commit/b7db766b0947e5201cab07b33bfc9a9386f51849))
- *(parser)* Wire ruff_python_parser for AST access ([`b47a1c1`](https://github.com/s0undt3ch/ToolR/commit/b47a1c176130b2d0d911868f5c57e7857c6ac8a6))
- *(parser)* Extract command_group() calls from module ASTs ([`435ce60`](https://github.com/s0undt3ch/ToolR/commit/435ce60456de562d359218bc8ff74ce8fb2cc509))
- *(parser)* Extract @group.command decorated functions ([`bdb42e9`](https://github.com/s0undt3ch/ToolR/commit/bdb42e9f34d7eb7568fb2b391667d0054c8ee748))
- *(parser)* Extract function signatures into Command arguments ([`41f0cb7`](https://github.com/s0undt3ch/ToolR/commit/41f0cb767febb5b3fbecabab00204ec0da9320df))
- *(parser)* Populate command summary, description, and arg help from docstrings ([`0411a02`](https://github.com/s0undt3ch/ToolR/commit/0411a02184bc9b37a6de511b6e278e86923682d5))
- *(parser)* Extract typing.Literal values into allowed_values ([`14ec3ba`](https://github.com/s0undt3ch/ToolR/commit/14ec3baf27bcfbcbd095996bf0d5d8ccf1218bb4))
- *(parser)* Resolve local enum.Enum members for allowed_values ([`689db73`](https://github.com/s0undt3ch/ToolR/commit/689db73659a3af8d2e549fb71d00045e07c2ac40))
- *(parser)* Build complete static manifest from tools directory ([`cf158c4`](https://github.com/s0undt3ch/ToolR/commit/cf158c49740bf8a5edd6e2f7587deddf154badc0))
- *(cli)* Build subcommand tree from manifest and stub execution ([`bb47f9b`](https://github.com/s0undt3ch/ToolR/commit/bb47f9b4b1d9550677bd03c8f0b530df835f1967))
- *(cli)* Add hidden __build-static-manifest dev command ([`79847b4`](https://github.com/s0undt3ch/ToolR/commit/79847b43073c73be989de54d0a2ee5c7a33e4846))
- *(runner)* Add msgspec schema for toolr._runner spec payload ([`5672c30`](https://github.com/s0undt3ch/ToolR/commit/5672c3073ae5ec114bf989574ea452e8c6b6162d))
- *(runner)* Load + validate spec JSON from TOOLR_SPEC_FILE ([`7b02436`](https://github.com/s0undt3ch/ToolR/commit/7b024362a9e3fa7d9e0ad1fd282cdc9be5829b5a))
- *(runner)* Dispatch into target function via Context ([`ea01f6d`](https://github.com/s0undt3ch/ToolR/commit/ea01f6db928a30562f4d9f975260d7e909ae7943))
- *(execute)* Add ExecutionSpec serde types matching runner schema ([`df94fc4`](https://github.com/s0undt3ch/ToolR/commit/df94fc4c4253344609c3ae11bc3c3573758ed4a5))
- *(execute)* Write spec JSON to private tempfile with drop-cleanup ([`9f1dfa1`](https://github.com/s0undt3ch/ToolR/commit/9f1dfa19301102e1793198010a83b3428f895e33))
- *(execute)* Minimal python interpreter resolver (TOOLR_PYTHON or PATH) ([`a2a45b0`](https://github.com/s0undt3ch/ToolR/commit/a2a45b0a61e7a394b88eff152497b2e50f1426f1))
- *(execute)* Spawn python -m toolr._runner with inherited stdio ([`16dab01`](https://github.com/s0undt3ch/ToolR/commit/16dab01f24d1e1e0421da23ee10620f0e30eea8a))
- *(execute)* Build ExecutionSpec from clap ArgMatches ([`70b9efb`](https://github.com/s0undt3ch/ToolR/commit/70b9efbdf24be7722edf5d450b9c26a78a3feded))
- *(execute)* Wire Python runner into dispatch with exit-code propagation ([`db98c55`](https://github.com/s0undt3ch/ToolR/commit/db98c55060b6cc46e4f23b1110973106ac27644f))
- *(execute)* Forward SIGINT/SIGTERM to Python runner subprocess ([`6541cf9`](https://github.com/s0undt3ch/ToolR/commit/6541cf99ae999176076521587af931d708fe9d6b))
- *(uv)* Add uv module skeleton with discovery types ([`c56cc21`](https://github.com/s0undt3ch/ToolR/commit/c56cc2196da2b64ad6bcbb8a6edd2f846fdcedaa))
- *(uv)* Probe uv --version on PATH and managed paths ([`79cc988`](https://github.com/s0undt3ch/ToolR/commit/79cc9881b57ee820376786a58ac52f81866fa82a))
- *(uv)* Decision logic for consent-based uv install ([`6410d7e`](https://github.com/s0undt3ch/ToolR/commit/6410d7e34c3c2fdc8009adb00b2408967f98b6e9))
- *(uv)* Download and install uv to $XDG_DATA_HOME/toolr/bin ([`d578125`](https://github.com/s0undt3ch/ToolR/commit/d57812594fd3961508d4230863eb5ee3a091632c))
- *(uv)* Unified ensure_uv entrypoint and hidden install command ([`99aed4d`](https://github.com/s0undt3ch/ToolR/commit/99aed4ddc44969a6713ccd8517e0d47c6cf72239))
- *(venv)* Parse [tool.toolr] config from tools/pyproject.toml ([`3fe9fe2`](https://github.com/s0undt3ch/ToolR/commit/3fe9fe20ac5d1ff60cebff8c4340adfdf8b9a279))
- *(venv)* Stable repo-key hash for cache-slot disambiguation ([`9c27607`](https://github.com/s0undt3ch/ToolR/commit/9c276074ee54ab7679839e512e27c5ea3bc5e5c4))
- *(venv)* Resolve cache vs in-tree venv path ([`9a73832`](https://github.com/s0undt3ch/ToolR/commit/9a738322a634994200f6070558cf90b1e4fee280))
- *(venv)* Run uv sync with mtime-based freshness check ([`f9d7e94`](https://github.com/s0undt3ch/ToolR/commit/f9d7e94863d15641f8ce1a6db11902e8eebde839))
- *(venv)* Validate toolr Python package presence in venv ([`8b94b81`](https://github.com/s0undt3ch/ToolR/commit/8b94b812d9667c6e952bebc5fb8c12f28f52dd5e))
- *(venv)* Best-effort editable installs with warn-on-fail ([`e47104c`](https://github.com/s0undt3ch/ToolR/commit/e47104cf5bda2da9d5b8f71be4d5ba96e61e370c))
- *(cli)* Reserve `toolr project` namespace and stub subcommands ([`24aafdb`](https://github.com/s0undt3ch/ToolR/commit/24aafdb60be78b6b718c641ac2875328d8902de5))
- *(project)* Implement `toolr project deps sync` ([`b111f83`](https://github.com/s0undt3ch/ToolR/commit/b111f83d3a21d97691f5488889571f777646e120))
- *(project)* Implement `toolr project venv path` ([`7ce4b36`](https://github.com/s0undt3ch/ToolR/commit/7ce4b3646f86eded9d1b66b3990818980a498630))
- *(project)* Implement `toolr project venv shell` ([`4b3a845`](https://github.com/s0undt3ch/ToolR/commit/4b3a8455dab5da3bee6ee096657ee29eeffa119d))
- *(dispatch)* Spawn the runner via the tools venv python ([`2a000a3`](https://github.com/s0undt3ch/ToolR/commit/2a000a3a83c735b1c2714750b859b4a42dcb78d7))
- *(complete)* Scaffold shell-completion module ([`5badb46`](https://github.com/s0undt3ch/ToolR/commit/5badb4609d8cc969e70748425dd302109348d6c2))
- *(complete)* Prefix-match groups, commands, flags, and allowed values ([`5412dc4`](https://github.com/s0undt3ch/ToolR/commit/5412dc4e31c55132f1ed57a692509d0d9bfae5af))
- *(complete)* Tab-time hash check with cache + fresh-reparse fallback ([`88ee2b3`](https://github.com/s0undt3ch/ToolR/commit/88ee2b301f61a0053f3c2268ff84063d359ad670))
- *(cli)* Add hidden __complete subcommand backing shell scripts ([`b92783e`](https://github.com/s0undt3ch/ToolR/commit/b92783e5b13748d69f066bfc79c79fb0e587dc0f))
- *(complete)* Embed bash completion script with self completion print bash ([`2c31758`](https://github.com/s0undt3ch/ToolR/commit/2c31758efdf7d5ca47b8268389f70cad782720a6))
- *(complete)* Embed zsh completion script ([`4db30ee`](https://github.com/s0undt3ch/ToolR/commit/4db30eefdaf8e2223822825389707c7dc5bf7e34))
- *(complete)* Embed fish completion script ([`f980645`](https://github.com/s0undt3ch/ToolR/commit/f980645ffe1c473b1aa685264cc17200523d4e92))
- *(complete)* Install completion scripts into shell-standard locations ([`c8bbc9f`](https://github.com/s0undt3ch/ToolR/commit/c8bbc9f554e9e4abc1cf1e133fd48931e5d36815))
- *(third_party)* Add fragment model and site-packages glob helper ([`910a73b`](https://github.com/s0undt3ch/ToolR/commit/910a73b7ce1c336fd9a940a9e958c2856c627bf5))
- *(third_party)* Parse + validate manifest fragments with schema-version guard ([`179acd2`](https://github.com/s0undt3ch/ToolR/commit/179acd21467bfa85724579948913d8984a01ba47))
- *(third_party)* Add schema-version migration framework with identity step ([`f5950f9`](https://github.com/s0undt3ch/ToolR/commit/f5950f98334dfc171963a8710a3b598e79651b30))
- *(third_party)* Merge fragments into Manifest with conflict resolution ([`f94f431`](https://github.com/s0undt3ch/ToolR/commit/f94f431ce0e55fdbe600079cc9e0fd7bbc253155))
- *(third_party)* Add discover_and_merge orchestrator with fail-fast on bad fragments ([`89e3fd9`](https://github.com/s0undt3ch/ToolR/commit/89e3fd9699f8c037ddbcb15ee2320d541548ad8b))
- *(parser)* Add build_static_manifest_with_venv merging third-party fragments ([`436ec74`](https://github.com/s0undt3ch/ToolR/commit/436ec74d23a4533c86ca698c4e144d1d8006e6e3))
- *(toolr.build)* Programmatic build_manifest API for third-party packages ([`eb00bc0`](https://github.com/s0undt3ch/ToolR/commit/eb00bc032b948083b19edf2850a6e1f62342c07b))
- *(toolr.build)* Add python -m toolr.build CLI with --check drift detection ([`13604ab`](https://github.com/s0undt3ch/ToolR/commit/13604ab6ed56c6f7a39f10ba202e87774397d2a5))
- *(toolr.build)* Validate fragment schema before writing ([`5348a50`](https://github.com/s0undt3ch/ToolR/commit/5348a50f8d2fff68ad8d273538132afb010a8e45))
- *(cli)* Add toolr self build-manifest Rust wrapper around python -m toolr.build ([`5463e85`](https://github.com/s0undt3ch/ToolR/commit/5463e856b4622bfe3dc5c9e85b6042999cc9a8b1))
- *(dynamic)* Define dynamic-layer payload schema and tests ([`b5cf221`](https://github.com/s0undt3ch/ToolR/commit/b5cf2219385a910b9a58ed68ecd8431ed30eac25))
- *(introspect)* Add dynamic-manifest helper skeleton with empty-payload smoke test ([`f112c3a`](https://github.com/s0undt3ch/ToolR/commit/f112c3a474c0cdcc263379d6ce6643a7e36ce2c5))
- *(introspect)* Walk tools.* and emit registry-derived groups and commands ([`f8ddd35`](https://github.com/s0undt3ch/ToolR/commit/f8ddd353c1d48b14b936c3e51685408ff8d2da15))
- *(introspect)* Load toolr.commands entry points so legacy packages appear in dynamic layer ([`ab81b41`](https://github.com/s0undt3ch/ToolR/commit/ab81b416c7e3bd353ea56c95c2c73995f79f2543))
- *(dynamic)* Compute dynamic-layer hash over venv dist-info names ([`f684346`](https://github.com/s0undt3ch/ToolR/commit/f6843465350c123a6129858ab35c3770f5acc919))
- *(dynamic)* Spawn introspect helper and decode payload with schema check ([`dfb673d`](https://github.com/s0undt3ch/ToolR/commit/dfb673d165fbfaa998b56e87478c000fd1625785))
- *(dynamic)* Merge dynamic payload into static manifest with static-wins policy ([`96fe2f8`](https://github.com/s0undt3ch/ToolR/commit/96fe2f80dc59a82e3e4c3fefdabde31562610701))
- *(dynamic)* Add rebuild_manifest_full and rebuild_dynamic_only orchestration ([`ba62143`](https://github.com/s0undt3ch/ToolR/commit/ba621433ee6348e42710c5a1899d00a76ce150bd))
- *(cli)* Add toolr project manifest rebuild command ([`fb77f69`](https://github.com/s0undt3ch/ToolR/commit/fb77f6980a516ae980e5806d358700c95e634e26))
- *(dispatch)* Auto-rebuild dynamic manifest layer when venv state changes ([`ce239be`](https://github.com/s0undt3ch/ToolR/commit/ce239beb144f119f63ce9641f6739dfcf639ea75))
- *(pre-commit)* Ship toolr-manifest hook config for downstream consumers ([`325c6ac`](https://github.com/s0undt3ch/ToolR/commit/325c6acb929840b9ab50ce370ae94c569f053219))
- *(deps-check)* Add filesystem probe for top-level imports ([`d6f5f7b`](https://github.com/s0undt3ch/ToolR/commit/d6f5f7bb67ab69f0b5c953bd0bbdd74f76726834))
- *(deps-check)* Add pre-flight check_imports with structured error ([`8b80c21`](https://github.com/s0undt3ch/ToolR/commit/8b80c218cdb3b5fe15256f9c402681ec62fe2f4b))
- *(cli)* Run pre-flight missing-deps check before runner spawn ([`f9cccbc`](https://github.com/s0undt3ch/ToolR/commit/f9cccbc737ec723a945b91129502d909f8efe41c))
- *(cli)* Add TOOLR_NO_PREFLIGHT_DEPS escape hatch ([`8968356`](https://github.com/s0undt3ch/ToolR/commit/8968356ac2f5398dc2d97cfbe28bd9ef93ed938e))
- *(deps-check)* Add post-mortem ImportError interceptor ([`12ed37d`](https://github.com/s0undt3ch/ToolR/commit/12ed37d71157016e91a212c696e3ac21af892042))
- *(runner)* Intercept ImportError tracebacks and append sync hint ([`0abbaaa`](https://github.com/s0undt3ch/ToolR/commit/0abbaaacc465bd68c24983f3c0a89c4086505063))
- *(cache)* Add Meta sidecar data model with serde round-trip tests ([`9d156b3`](https://github.com/s0undt3ch/ToolR/commit/9d156b3c6a8cf1291a1609dcba24a3e2ceea86e0))
- *(cache)* Drop meta.json sidecar on venv creation ([`9ee0814`](https://github.com/s0undt3ch/ToolR/commit/9ee08142d38787c33b68ae18acb79c17cfa9e8c6))
- *(cache)* Touch last_used_at on every invocation ([`502b03c`](https://github.com/s0undt3ch/ToolR/commit/502b03c7659de26c9d74f5923d3a53cc2043d571))
- *(cache)* Enumerate cache entries with size accounting ([`cf8cdda`](https://github.com/s0undt3ch/ToolR/commit/cf8cdda54fc529041c7ee6e8edc2e9ef1f2052f5))
- *(cli)* Add toolr self cache list with tabular output ([`f1a5aa6`](https://github.com/s0undt3ch/ToolR/commit/f1a5aa6473c02bee929e8f8ca99432e61ae67089))
- *(cache)* Classify cache entries as keep, orphan, or stale ([`a8c73ee`](https://github.com/s0undt3ch/ToolR/commit/a8c73eebb621de6eab0422fcbf4df24447ada959))
- *(cli)* Add toolr self cache prune with --all and --dry-run ([`9e50b87`](https://github.com/s0undt3ch/ToolR/commit/9e50b877241a92b7f9144855c1b35178eedfb751))
- *(cache)* Emit passive size-hint when cache exceeds thresholds ([`36ebf4e`](https://github.com/s0undt3ch/ToolR/commit/36ebf4e9968fa40d79875420066f8a427e77cdf3))
- *(build)* Pin maturin and add wheel-contents tests ([`a34bd12`](https://github.com/s0undt3ch/ToolR/commit/a34bd12e8643c5bb9c903fa0c281243c7ac607ff))
- *(cli)* Replace python -m toolr with binary-exec deprecation shim ([`31cdb92`](https://github.com/s0undt3ch/ToolR/commit/31cdb92df2bedd59386512f7cfc9be052147f55e))
- *(ci)* Build per-platform Rust binary archives workflow ([`7026d31`](https://github.com/s0undt3ch/ToolR/commit/7026d311305f65787da3b4530d8eeb973da97371))
- *(ci)* Generate release-manifest.json for installer consumption ([`f591157`](https://github.com/s0undt3ch/ToolR/commit/f59115715d53dea6471805d7c8b938ccb068ee68))
- *(install)* Add cross-platform install.sh + install.ps1 scripts ([`ca72140`](https://github.com/s0undt3ch/ToolR/commit/ca721405ab6508047ab2f7765c6bd18326e85ee7))
- *(mise)* Add Rust-binary-fetching mise plugin (staging dir) ([`d163773`](https://github.com/s0undt3ch/ToolR/commit/d1637736d30e08cb4694f00d6a7b642195919ecf))
- *(mise)* Stage mise plugin scripts under dist/mise-plugin/ ([`9af5e82`](https://github.com/s0undt3ch/ToolR/commit/9af5e82234bca4a3135c3b292be102ee6c43cd6d))
- *(cli)* [**breaking**] Remove python -m toolr entry point ([`0f76af7`](https://github.com/s0undt3ch/ToolR/commit/0f76af76905093e32cc4074842d85c7e5ca4b721))
- *(ci)* Always attest archives + verify provenance from install scripts ([`df7c9f3`](https://github.com/s0undt3ch/ToolR/commit/df7c9f3e8928962ca59d1bbb7b7162934466264b))
- *(cli)* Skeleton toolr project init subcommand ([`479c1c7`](https://github.com/s0undt3ch/ToolR/commit/479c1c7b2ae30def8641a078827c256cb3d81f10))
- *(init)* Add scaffold template files and render helpers ([`4b46671`](https://github.com/s0undt3ch/ToolR/commit/4b46671554be40c9f02b49c9df3ed47e8ed049f6))
- *(init)* Fill in example.py with four ctx-exercising commands ([`92966a4`](https://github.com/s0undt3ch/ToolR/commit/92966a4163b8db9476edf9162a226227537d3172))
- *(init)* Scaffold tools/ with atomic file writes and refuse-without-force ([`2c78866`](https://github.com/s0undt3ch/ToolR/commit/2c788662cf1b24868cc65dab3a3c018f3d7b531b))
- *(init)* Auto-run uv sync after scaffolding (skippable via --no-sync) ([`1836631`](https://github.com/s0undt3ch/ToolR/commit/1836631c10ef2507a7eca29e668c6dabbdde2be0))
- *(args)* List[T] / *args support, bool=False flags, hyphenated flags, msgspec coercion ([`325bc00`](https://github.com/s0undt3ch/ToolR/commit/325bc001065835c1cafe6b584c3e0ea309b46e7e))
- *(types)* Toolr.types namespace + rust SupportedType resolver foundation ([`07a3994`](https://github.com/s0undt3ch/ToolR/commit/07a39947b0fe40656d669c61f8d1187ebf48144d))
- *(types)* Clap-side per-type value_parsers + manifest-build type validation ([`7fd99e7`](https://github.com/s0undt3ch/ToolR/commit/7fd99e781b4778e09b0ec66ea9ec16886d57e991))
- *(groups)* Nested command groups (closes #193) ([`5be44dd`](https://github.com/s0undt3ch/ToolR/commit/5be44ddc6a9449c4190d84361fc7f3a0f64a9e52))
- *(complete)* Tab-complete nested groups + their child commands ([`5000a1e`](https://github.com/s0undt3ch/ToolR/commit/5000a1ed70589f438da5e6bcee8ab0ec15a86b25))
- *(types)* Wire heterogeneous tuple[T1, T2, ...] arity into clap ([`9cf6246`](https://github.com/s0undt3ch/ToolR/commit/9cf624655b55a7634fe57f667b2b4cd3d6b9c558))
- *(types)* Arg() Path constraints + msgspec dec_hook for richer types ([`5a53187`](https://github.com/s0undt3ch/ToolR/commit/5a5318793aecb8d5bcdabeb0138c764cd4921477))
- *(types)* Add toolr.types.Version (PEP 440 via pep440_rs / packaging) ([`518416b`](https://github.com/s0undt3ch/ToolR/commit/518416bc1fb504b6a735e881357aaa4f00b49ef7))
- *(types)* Follow module-level type aliases through resolution ([`b70ae97`](https://github.com/s0undt3ch/ToolR/commit/b70ae97f9600aeea53524200a182fdc748bb5bf2))
- *(help)* Render docstring markdown via termimad for --help output ([`355f825`](https://github.com/s0undt3ch/ToolR/commit/355f82586eb8c7336b8397dd4a322f702d16b65e))
- Parent=kwarg nesting + yellow/green help palette ([`1d36f38`](https://github.com/s0undt3ch/ToolR/commit/1d36f38c6af0ac3e5182552bd59f9e4587c0b23c))
- *(parser)* Cross-file group bindings + parent=<var> resolution ([`c75f10e`](https://github.com/s0undt3ch/ToolR/commit/c75f10ebe1233eec8d1fa74877f199c1d90eb65f))
- @command(group="path") string-path attachment + dotted command_group ([`2e31e9f`](https://github.com/s0undt3ch/ToolR/commit/2e31e9f99a235353a4e82ecc5602cc72ae9be7e4))
- ToolrDeprecationWarning on legacy decorators ([`cb81eaa`](https://github.com/s0undt3ch/ToolR/commit/cb81eaabe28f66204a6bdde08fd1080c68bfd698))
- *(arg)* Full arg() metadata + toolr.types.Count (closes #198) ([`eff9c2f`](https://github.com/s0undt3ch/ToolR/commit/eff9c2f6a3df43b916ec199d34c5cfed985b30e7))
- *(cli)* Output Options — timestamps + ctx.run timeout defaults (closes #191) ([`e378635`](https://github.com/s0undt3ch/ToolR/commit/e378635d5ec9ffc386a41b7726729974d1f7b1e2))
- *(retire)* Remove Python CLI frontend (_parser, _registry) ([`7d1c0d4`](https://github.com/s0undt3ch/ToolR/commit/7d1c0d46eb4ffe01836c26c18d3dfab7a1f0fe45))
- *(release)* Wire UNRELEASED.md into cliff via TOOLR_RELEASE_NOTES ([`3a023b2`](https://github.com/s0undt3ch/ToolR/commit/3a023b24ed8fc2e767561fe4b404f1b9e365d179))
- *(manifest)* Add Origin::ThirdParty for glob-merged entries ([`928bede`](https://github.com/s0undt3ch/ToolR/commit/928bede6425c5093efdd4e216005c72560070906))
- *(dynamic)* Hash third-party manifests instead of dist-info ([`7f142c9`](https://github.com/s0undt3ch/ToolR/commit/7f142c9d87cdd2050e30d72c109ce0de03e5e96d))
- *(toolr-py)* [**breaking**] Drop toolr.commands entry-point plugin support ([`28f4cc9`](https://github.com/s0undt3ch/ToolR/commit/28f4cc9ad73f5e849ed32abb5f68f2b7a6589c97))
- *(toolr-core)* Add freshness::compare for shared dispatch/tab logic ([`fa30d18`](https://github.com/s0undt3ch/ToolR/commit/fa30d181ab54d873bb46f3177b570f21225428d4))
- *(toolr)* Add ensure_manifest_fresh bootstrap step ([`3c94f44`](https://github.com/s0undt3ch/ToolR/commit/3c94f44fd5bf4d3f0343c63b6874260ffb6535d5))
- *(toolr)* Refresh manifest before clap parses argv ([`ffe4361`](https://github.com/s0undt3ch/ToolR/commit/ffe43610cd9f91d1eae6cb381832c8d97df80d44))
- *(examples)* Canonical plugin example shipping toolr-manifest.json ([`1984e3d`](https://github.com/s0undt3ch/ToolR/commit/1984e3d8a9115b44faea5a31cdacf439b63d3944))
- *(build_fragment)* Scaffold pure-Rust manifest fragment builder ([`0f61194`](https://github.com/s0undt3ch/ToolR/commit/0f611947525a7a2944ee184a266c221b8066310e))
- *(parser)* Support call-form @group.command and positional description ([`a3c4c3d`](https://github.com/s0undt3ch/ToolR/commit/a3c4c3d348b3e925ceec0d3016498ee00f41ea72))
- *(build_fragment)* Stable JSON serialisation matching Python output ([`07dc407`](https://github.com/s0undt3ch/ToolR/commit/07dc4071b66665baa9437a15738c1db218bf4efa))
- *(parser)* Backfill allowed_values from Literal aliases via resolved_type ([`95d604c`](https://github.com/s0undt3ch/ToolR/commit/95d604cdce380c111f0f2e89d76d8f88b7a4d1a1))
- *(toolr/cli)* Source+package resolution for build-manifest ([`60b60a1`](https://github.com/s0undt3ch/ToolR/commit/60b60a1b91f1d2c1ea6541f8e0c6df474ea057a3))
- *(cli)* Add --source-dir/--package, remove --python from build-manifest ([`35d432c`](https://github.com/s0undt3ch/ToolR/commit/35d432c966c6858e1dc161510d44fe39bd3ba447))
- *(dispatch)* Rewire self build-manifest to pure-Rust path ([`03ce318`](https://github.com/s0undt3ch/ToolR/commit/03ce3183a4e518e94abd50d0f1cb4cbb1ccd0ba3))
- *(toolr-py)* Remove Python toolr.build module ([`9be85fd`](https://github.com/s0undt3ch/ToolR/commit/9be85fdb110db850a23cca146d47682d1ce8dc1d))
- *(dispatch)* Inherit child stderr to preserve TTY; move missing-dep hint into the runner ([`401eb17`](https://github.com/s0undt3ch/ToolR/commit/401eb1734d8213757711deca9d2c1d6a83fadd0c))
- *(argparse)* Accept underscored long-flag spelling as hidden alias ([`6663d39`](https://github.com/s0undt3ch/ToolR/commit/6663d39eaa1716bf0ea3945a6240bc2bcfa5d3ce))
- *(argparse)* `T | None` without default → zero-or-one positional ([`22c7768`](https://github.com/s0undt3ch/ToolR/commit/22c7768e8991d399c8af07936cbdae78292e8b70))
- *(arg)* Validate aliases / conflicts_with / requires kwargs ([`3e28806`](https://github.com/s0undt3ch/ToolR/commit/3e288061c7b937647b2a8ff370cc5c431f56c3a8))

### <!-- 1 -->🐛 Bug Fixes

- *(gitignore)* Anchor MANIFEST pattern to repo root ([`7de3ef9`](https://github.com/s0undt3ch/ToolR/commit/7de3ef952f348db134d34334216c0245fdee4fde))
- *(discovery)* Canonicalize start path and tighten error-payload assertions ([`9f9e1e8`](https://github.com/s0undt3ch/ToolR/commit/9f9e1e8224530af7314219aeada030e7138cf985))
- *(gitignore)* Anchor venv/ pattern to repo root ([`2ff6f30`](https://github.com/s0undt3ch/ToolR/commit/2ff6f3013fc5b034a888b9c9545a639d7b7995f8))
- *(ci)* Resolve Windows compile error and typos CHANGELOG false positive ([`c0f9a02`](https://github.com/s0undt3ch/ToolR/commit/c0f9a020069d3737fef3798e8780795312c994eb))
- *(clippy)* Use sort_by_key with Reverse in self_cache::run_list ([`a60e4df`](https://github.com/s0undt3ch/ToolR/commit/a60e4dffbf585f4ca55da15ea0548c788cbafcee))
- *(parser)* Resolve enum-attribute defaults to their serialised value (#197) ([`8e2889e`](https://github.com/s0undt3ch/ToolR/commit/8e2889eacb17b2a5289c855b3994f9c494a0bb2c))
- *(runner)* Pass dec_hook to *args coercion; document arg() metadata gap ([`448a68a`](https://github.com/s0undt3ch/ToolR/commit/448a68a73873c17e85cf6e082589d61031b7eb6d))
- *(cli)* --help shows summary+description; root-only -d/-q flags ([`6a08334`](https://github.com/s0undt3ch/ToolR/commit/6a083346ec9de7314b0009c7d16e31c15bd64947))
- *(cli)* -h shows full description, matching argparse-era ergonomics ([`4152589`](https://github.com/s0undt3ch/ToolR/commit/4152589d0b47a0a2ad3763284e178496abb5c960))
- *(cli)* Parent listings show summary only; -h on leaves shows the full body ([`b2e6d31`](https://github.com/s0undt3ch/ToolR/commit/b2e6d31c9c11a365edb2fd8afd7029ea53dc5f68))
- *(cli)* Render Output Options help text through termimad ([`83d5604`](https://github.com/s0undt3ch/ToolR/commit/83d5604f70950093ec23dd0b93914f203a8fcb5c))
- *(parser)* Classify `Annotated[bool, arg(...)]` as a Flag ([`8138fd1`](https://github.com/s0undt3ch/ToolR/commit/8138fd19f04e9392091a15f9005078292b06ab58))
- *(ci)* Pin COLUMNS=100 for snippet captures + correct plan-doc links ([`56d3070`](https://github.com/s0undt3ch/ToolR/commit/56d3070a1251162200e1634f41064e0fb87e1ed9))
- *(rust)* Gate unix-only test helpers properly for Windows builds ([`c661e7c`](https://github.com/s0undt3ch/ToolR/commit/c661e7c31b601268b2b5474631071a1fbeca3e92))
- *(rust)* Gate execute::signals test module behind cfg(unix) ([`a7870de`](https://github.com/s0undt3ch/ToolR/commit/a7870debd906f325f27457670e66b02b9c02c3db))
- *(tests)* JSON-encode paths in cache fixture for Windows compatibility ([`0baef9f`](https://github.com/s0undt3ch/ToolR/commit/0baef9fcdf5db07810d07e7fdad2d8be56941abb))
- *(tests)* Canonicalise the in-tree venv-path expected value ([`cb4fa7c`](https://github.com/s0undt3ch/ToolR/commit/cb4fa7cb9bc50494c6dea55abc682a0f8950645f))
- *(tests)* JSON-encode paths in self-cache-prune fixture too ([`b4675ff`](https://github.com/s0undt3ch/ToolR/commit/b4675ff1c9bb26f2e4a6db9897e7017b9bdf262d))
- *(tests)* Skip ancestor-walk tests when host has a tools/ at root ([`bd11a62`](https://github.com/s0undt3ch/ToolR/commit/bd11a620bc0fd0452373f739e10d682198bc34ec))
- *(tests)* Skip silent_failure_when_no_tools_dir_anywhere when host has tools/ ([`2c658a7`](https://github.com/s0undt3ch/ToolR/commit/2c658a7b68e56e2308073ac5a883e6a5468ea83a))
- *(tests)* Serialise execute::python tests that mutate TOOLR_PYTHON ([`ed4d640`](https://github.com/s0undt3ch/ToolR/commit/ed4d640858c4447d1b3bad6abd1feae3d47d2462))
- *(workspace-split)* Stage 7 review fixes ([`68d300a`](https://github.com/s0undt3ch/ToolR/commit/68d300ae97fd12e0a41def8b9da57ccc2f4296b9))
- *(tests)* Patch correct getpass symbol; lift rich<14.3 cap ([`a1a034f`](https://github.com/s0undt3ch/ToolR/commit/a1a034f5c3db99869def3d3e8d254d9d9aa77c4c))
- *(docs)* Update mkdocs.yml paths to crates/toolr-py/python ([`1f6e2dc`](https://github.com/s0undt3ch/ToolR/commit/1f6e2dcd7b30db31318b0c7013bf745ff22d5245))
- *(packaging)* Tac portability + exclude __pycache__ from toolr-py wheel ([`363cb85`](https://github.com/s0undt3ch/ToolR/commit/363cb85e4527c614efaffefe21367b5d8674b44b))
- *(types)* Sync _context.pyi with the .py source (Output Options fields) ([`0ef3972`](https://github.com/s0undt3ch/ToolR/commit/0ef3972fa0fe7f4bcbefcbc9b8d4503800142a69))
- *(types)* Move TypeVar F into TYPE_CHECKING block (ruff TC001) ([`4d58ebf`](https://github.com/s0undt3ch/ToolR/commit/4d58ebffdb737eac593aa72acbebb7dccbade0ae))
- *(version)* Make dev-version cargo-compatible; fix Windows strftime ([`fddb203`](https://github.com/s0undt3ch/ToolR/commit/fddb20325fa0da138c6dc59a6c7d019bda7de5e0))
- *(version)* Realign workspace version with reality; bump patch for dev ([`dc9d459`](https://github.com/s0undt3ch/ToolR/commit/dc9d459b487c911337563cf9da8ad0ee4cb3bbf4))
- *(freshness)* Skip third-party axis when venv_dir is None ([`c3f6146`](https://github.com/s0undt3ch/ToolR/commit/c3f6146d8059422f9d8ecc8f39b98f7d19a7ef7b))
- *(ci)* Declare Rust toolchain for ReadTheDocs maturin build ([`e003c20`](https://github.com/s0undt3ch/ToolR/commit/e003c206795f477a14ff57d7a8c45b8d4f85691b))
- *(ci)* Pin RTD Rust to 1.93+ for ruff workspace deps ([`8019e97`](https://github.com/s0undt3ch/ToolR/commit/8019e972a7f4078d718ad3d178088b266ba9a673))
- *(ci)* Bypass asdf-rust on RTD, install rustup stable directly ([`cd45aef`](https://github.com/s0undt3ch/ToolR/commit/cd45aefe03cd984e771c2778acbe2520695b08bd))
- *(bootstrap)* Merge third-party fragments in rebuild_manifest_full ([`13fc028`](https://github.com/s0undt3ch/ToolR/commit/13fc028cf88a57f80237edb197cce401d4a94a82))
- *(dispatch)* Resolve venv Python on Windows for self build-manifest ([`08c1bdb`](https://github.com/s0undt3ch/ToolR/commit/08c1bdb73a9779ae1300a37ddf3327ed619aebb3))
- *(third-party)* Glob both Lib and lib/python* venv layouts ([`e393c66`](https://github.com/s0undt3ch/ToolR/commit/e393c661920bc6d958a563c5b56716595f9558dd))
- *(parser)* Skip dot-prefixed directories in list_python_files ([`4c7fd88`](https://github.com/s0undt3ch/ToolR/commit/4c7fd883ec294cb8ea0fef8c6a5281eabadaf9ca))
- *(dispatch)* Emit clear error when tools venv python is missing ([`fed5051`](https://github.com/s0undt3ch/ToolR/commit/fed50517c0fb477940e0be5fc5fef0f6b80e218d))
- *(dispatch)* Wire UvError::user_message into production error paths ([`0034e92`](https://github.com/s0undt3ch/ToolR/commit/0034e92b8abec53f9146e0c471dd5d4ed3833ac3))
- *(version)* Drop spurious `= None` default on bump's positional ([`2676213`](https://github.com/s0undt3ch/ToolR/commit/2676213dcffdccfb668bdd7797a65ac9679a90f4))
- *(release)* Pass --new-version as a flag, restore bump's `= None` default ([`27c4c4e`](https://github.com/s0undt3ch/ToolR/commit/27c4c4e3ffbeb66b5c94235e95aec8bd88a9be3b))

### <!-- 10 -->💼 Other

- *(wheels)* Split pyproject.toml; add toolr binary-wheel + toolr-py pyo3-wheel ([`e5d9b8f`](https://github.com/s0undt3ch/ToolR/commit/e5d9b8f8fe03caecb31ccd297f1f2bc9d2a89cde))
- *(tools)* Declare toolr-py as dogfooding-tools dep ([`d4ba5bd`](https://github.com/s0undt3ch/ToolR/commit/d4ba5bde08c2f87181b8fc457847bf27bb484a4c))
- *(tools)* Inherit packaging from toolr-py; widen venv-cache hash ([`89bc57d`](https://github.com/s0undt3ch/ToolR/commit/89bc57d77464b72479411dd5a8375dfacc460ed1))
- Install prebuilt toolr-py wheel instead of rebuilding it ([`393cd25`](https://github.com/s0undt3ch/ToolR/commit/393cd25b4e25a027ebfdc4fea45dffc1836e4dc2))
- Call _test_distribution.yml instead of inlining dist tests ([`9726a8d`](https://github.com/s0undt3ch/ToolR/commit/9726a8da6130473d9dab8c9a35ba9774987e08e5))
- Un-deprecate @group.command, keep parent.command_group on track for 1.0 removal ([`93986dd`](https://github.com/s0undt3ch/ToolR/commit/93986dd11cdce4b75fb598081bb250655a9272fc))
- Exclude superpowers/ subtree from the published site ([`f041644`](https://github.com/s0undt3ch/ToolR/commit/f041644dd119977ca51137bedf4cd337106a25b6))
- Migrate off deprecated codecov/test-results-action ([`9cef749`](https://github.com/s0undt3ch/ToolR/commit/9cef749d02d260a1ad54c0481f6248ecb2376ca1))
- *(deps)* Bump ruff_python_parser from 0.14.0 to 0.15.13 ([`4c167b9`](https://github.com/s0undt3ch/ToolR/commit/4c167b978e358c3a828fe287eaded8ff1795f4fe))
- *(deps)* Bump signal-hook from 0.3.18 to 0.4.4 ([`90e5b95`](https://github.com/s0undt3ch/ToolR/commit/90e5b95840d25daa18362071165f35a5e8dc41a1))
- *(deps)* Bump thiserror from 1.0.69 to 2.0.18 ([`0842727`](https://github.com/s0undt3ch/ToolR/commit/0842727bbdf1b4010985c50ac831e58a8370a38b))
- Upgrade Python and Rust dependencies to latest ([`e2862a0`](https://github.com/s0undt3ch/ToolR/commit/e2862a0e5b7162a99fd45fc13b9b2e6ea581d315))
- Pin ToolR action to a SHA that supports python-path ([`f54a7fe`](https://github.com/s0undt3ch/ToolR/commit/f54a7fee9c66942924ce46e28f2aeeb4e49825e0))
- *(deps-dev)* Bump ruff from 0.15.12 to 0.15.13 ([`7d01305`](https://github.com/s0undt3ch/ToolR/commit/7d01305ad4bd5ad76b1537edda172675ab6c5a19))
- *(deps-dev)* Bump coverage from 7.13.4 to 7.14.0 ([`0e472e3`](https://github.com/s0undt3ch/ToolR/commit/0e472e3d2014666e779fbf449eb6b8424fc7fef1))
- *(deps-dev)* Bump packaging from 26.0 to 26.2 ([`19c60ad`](https://github.com/s0undt3ch/ToolR/commit/19c60ad47b7f77ad69f0159f60bfdb64d7758f6b))
- Bump pre-commit hook revs and drop retired macos-13 runner ([`3969f29`](https://github.com/s0undt3ch/ToolR/commit/3969f2936f38dd2812e35cccd8b561d65f416acd))
- *(deps-dev)* Update mkdocstrings[python] requirement ([`1912ca8`](https://github.com/s0undt3ch/ToolR/commit/1912ca83e0a6735307c9fcf73a4f887c40ea1d28))
- External command sources and dispatchers ([`665fa69`](https://github.com/s0undt3ch/ToolR/commit/665fa69ed1af38a9b9fb4b3bc904d5d9232068fe))
- Refresh uv.lock for packaging>=26.2 in toolr-py ([`5ed62d6`](https://github.com/s0undt3ch/ToolR/commit/5ed62d6663d3338ecaf0eba537e85c874fa36de0))
- Introduce ArgSchema for externally-discovered commands ([`08fbd71`](https://github.com/s0undt3ch/ToolR/commit/08fbd71d0127c1237acb8008a68de4835ec45822))
- Add CommandSchema ([`580045e`](https://github.com/s0undt3ch/ToolR/commit/580045e65431f48367557746e0caf37f63f3c38a))
- Add DispatchCommand ([`533acaf`](https://github.com/s0undt3ch/ToolR/commit/533acaf934fc95fd871a056f2831c33eee52ae0b))
- Implement DispatchCommand.argv reconstruction ([`303a88d`](https://github.com/s0undt3ch/ToolR/commit/303a88d987bd3e120f6a114bc8d6edc75ec8dce9))
- Lock down toolr.sources public surface ([`5be3d57`](https://github.com/s0undt3ch/ToolR/commit/5be3d57ebc9f0150ff93bf240ecfac717c04a2c8))
- Detect dispatcher commands via DispatchCommand annotation ([`aba6995`](https://github.com/s0undt3ch/ToolR/commit/aba6995e64524c67a1ec7720e4cf23c6d4a86c49))
- Add optional dispatched_from field on Command ([`cbf1540`](https://github.com/s0undt3ch/ToolR/commit/cbf1540eb12eb22848bbc9dd84944aca468924cf))
- Scaffold module + parse [tool.toolr.argparse.*] blocks ([`060fac4`](https://github.com/s0undt3ch/ToolR/commit/060fac4c5f49f3208b8d1ac51d3062477da5e2fb))
- AST-extract parser.add_argument calls from a Python source ([`edbe4fe`](https://github.com/s0undt3ch/ToolR/commit/edbe4fefe88f2dba5c4e02f269a75b0d74099c6f))
- Glob-expand scan_paths and scan each file ([`733708b`](https://github.com/s0undt3ch/ToolR/commit/733708bfa696d54569abbfe54940e8c7b85f0aec))
- Apply common_args to each scanned command ([`27dad20`](https://github.com/s0undt3ch/ToolR/commit/27dad20b169a126ceb80f80308752aa95bb218db))
- Validate attachments and detect collisions ([`34b4452`](https://github.com/s0undt3ch/ToolR/commit/34b445273191963791c9c761c7008e3ff9b490f2))
- Graft scanned commands under attached parents ([`f860d6e`](https://github.com/s0undt3ch/ToolR/commit/f860d6eb0f98f4c0a69125a57bcde77fec543633))
- End-to-end run_for_project orchestrator ([`39a63e0`](https://github.com/s0undt3ch/ToolR/commit/39a63e0564dd0f654693158b3b2e5ed18c578674))
- Run the argparse scanner inside build_static_manifest ([`ea60410`](https://github.com/s0undt3ch/ToolR/commit/ea604100d5fcdebc25e69f4391c74081ee92e69f))
- Pack dispatched-child args for runtime injection ([`b4db253`](https://github.com/s0undt3ch/ToolR/commit/b4db253c402f496d486e300b6e87b8b5c20badbb))
- Construct DispatchCommand and invoke parent on dispatched leaves ([`f8713bb`](https://github.com/s0undt3ch/ToolR/commit/f8713bb568491201e44cd951ffd6669a9b41791e))
- Skip DispatchCommand-annotated kwargs in static arg extraction ([`93a5401`](https://github.com/s0undt3ch/ToolR/commit/93a5401d61f8aec09a898e89d8a29dbdcce8991f))
- Handle Annotated[DispatchCommand, ...] and string forward refs ([`e6ace63`](https://github.com/s0undt3ch/ToolR/commit/e6ace6373968f9a9488da354723357029cac7c6c))
- Fill the gaps from Plan A (auto-rebuild + dispatcher-hosts-children) ([`e958a34`](https://github.com/s0undt3ch/ToolR/commit/e958a34bccd27c11f937fe923169f3aabb9403b5))
- Fill the gaps from Plan A (8 tasks across 2 stacks) ([`8209afb`](https://github.com/s0undt3ch/ToolR/commit/8209afbbfc1b42a464f2a64b79e0411f1661b3d3))
- Add should_skip_auto_rebuild argv inspector ([`6ea19dc`](https://github.com/s0undt3ch/ToolR/commit/6ea19dc5a00f1f4cfa28e38615d251d8c68b6502))
- Auto-rebuild manifest when missing for user commands ([`e2cca50`](https://github.com/s0undt3ch/ToolR/commit/e2cca50bfe1d7e4a05ec4832c15f1b7fd7651840))
- Add optional is_dispatcher field on Command ([`31b648c`](https://github.com/s0undt3ch/ToolR/commit/31b648c40d381e4a05998c9427e424332c1ae747))
- Mark dispatcher commands via GraftResult.dispatchers ([`3792fef`](https://github.com/s0undt3ch/ToolR/commit/3792fef6f124d2f05167455e25b7b90461024c19))
- Hoist grafted children into dispatcher subcommands ([`61d8142`](https://github.com/s0undt3ch/ToolR/commit/61d81429a919fb81378e39e0cc6becb15ae7d1f5))
- Widen path lookup to handle 3-segment dispatcher paths ([`a4539a9`](https://github.com/s0undt3ch/ToolR/commit/a4539a97a21ef01bd87599dc16c860b343a6599c))
- Look up grafted children at dispatcher dotted name ([`166413a`](https://github.com/s0undt3ch/ToolR/commit/166413a970d1e74466de1ad2329ec15a74161660))
- Correct Task 6 bucketing to match the working code ([`35d1507`](https://github.com/s0undt3ch/ToolR/commit/35d1507c01f8ca8c018ae396442c3b578462b439))
- Fix rumdl MD007 indent depth in fill-the-gaps plan ([`f830437`](https://github.com/s0undt3ch/ToolR/commit/f830437544d6d532442af986b8f74e11945cd8ed))
- Drop spurious `--` in fish completion script ([`46b652d`](https://github.com/s0undt3ch/ToolR/commit/46b652d662f0938d2100a1c98d3afb28f427c98d))
- Ship completion for built-in `self` and `project` commands ([`1eeb8c2`](https://github.com/s0undt3ch/ToolR/commit/1eeb8c2e9be1938771479feb3732831819ffde51))
- Honour `--force` literally + distinguish install outcomes ([`774a27f`](https://github.com/s0undt3ch/ToolR/commit/774a27f6ed893c3f198718e6fbfa44c759e70099))
- Backfill meta.json on touch when sidecar is missing ([`cc98c18`](https://github.com/s0undt3ch/ToolR/commit/cc98c188271d43b6e640fca1bd3c7c354afaf5a7))
- Additive-merge scaffold — per-file conflict detection and prompts ([`5906b0f`](https://github.com/s0undt3ch/ToolR/commit/5906b0fa39acee0be31a3a1e102ba0221f3fac34))
- Harden scaffold after code review — snapshot rollback, --yes flag, injectable confirm ([`6f373e9`](https://github.com/s0undt3ch/ToolR/commit/6f373e9e4aae0dc9edb42052a733dea80a4b03ca))
- Add build tasks for each package ([`d7cc39a`](https://github.com/s0undt3ch/ToolR/commit/d7cc39adc669e6c1a9319b1dba3249822c8f405b))
- Apply remaining review findings — exit 2 for conflicts, symlink guard, PID .tmp ([`99918a2`](https://github.com/s0undt3ch/ToolR/commit/99918a201bb3b5322d7a915eda1b176c46421e39))
- Fix maturin invocations to use crates/toolr-py/Cargo.toml ([`0c90594`](https://github.com/s0undt3ch/ToolR/commit/0c9059476d8dcefe51ee3da877c3f005c6c7b047))
- Copy build artifacts into dist/ ([`886908c`](https://github.com/s0undt3ch/ToolR/commit/886908c856cd3e8f5b03c7b768686cfc0fd7ef52))
- Add build-clean task to wipe target/ and dist/ ([`68cb2c1`](https://github.com/s0undt3ch/ToolR/commit/68cb2c11118458b5e2ad4b28fde1901151782c1a))
- Extend build-clean to remove .so files, __pycache__, and .egg-info ([`c64476a`](https://github.com/s0undt3ch/ToolR/commit/c64476a4fc5d46c9064f8637b7c0aa5c21b0626c))
- Fix error message to say toolr-py not toolr ([`312a426`](https://github.com/s0undt3ch/ToolR/commit/312a4269a74acddc2902870078a55c3d03f5ac0e))
- Unset VIRTUAL_ENV when invoking uv sync to silence mismatch warning ([`01c0685`](https://github.com/s0undt3ch/ToolR/commit/01c0685b06b8e0c2af667a35a5bc8a552026073d))
- Locate toolr package via .pth files for editable installs ([`233da24`](https://github.com/s0undt3ch/ToolR/commit/233da2481bd830262f67e2bbfb671b14c26d391f))
- Scope build-clean .so removal to crates/ only ([`6129c99`](https://github.com/s0undt3ch/ToolR/commit/6129c999dc0fa1294d2cbef4290d4735398a9054))
- Add develop-toolr-py task for editable installs ([`36b0553`](https://github.com/s0undt3ch/ToolR/commit/36b05539fb0edeea2aa0bbf496724dbf8bf6bd64))
- Add `deps upgrade <pkg>` for bumping a single dependency ([`b9483b7`](https://github.com/s0undt3ch/ToolR/commit/b9483b7cf820bd8c8943a0a153b4e002a54381b6))
- Replace vague schema-mismatch error with copy-pasteable fix ([`fc3c6ee`](https://github.com/s0undt3ch/ToolR/commit/fc3c6ee52cc67e99fe67a0cef316aecff02ae1b3))
- Document bump policy on both sides ([`62f192e`](https://github.com/s0undt3ch/ToolR/commit/62f192e4e1786857d121735afb7cf33b23f9710f))
- Rebuild manifest on `--help` and bare `toolr` ([`f4c7d7a`](https://github.com/s0undt3ch/ToolR/commit/f4c7d7ac97d4a5995ece751bcfbb227a89a2373e))
- Re-export `DispatchCommand` from package root ([`0ee2079`](https://github.com/s0undt3ch/ToolR/commit/0ee2079cba18e594634341c8289c78a4277b5635))
- Skip files with no add_argument calls + warn on intra-source dups ([`25210a2`](https://github.com/s0undt3ch/ToolR/commit/25210a2c2f3dd7fc7ec6c0db2ba292b292e2ba2c))
- Walk through dispatchers so grafted children tab-complete ([`f2390c2`](https://github.com/s0undt3ch/ToolR/commit/f2390c26a5cc6075be3f661122e539d58b448df1))
- Fall back to flags when no positional candidates remain ([`b5c1a17`](https://github.com/s0undt3ch/ToolR/commit/b5c1a17831801784eb9e46059c4538f7b795e264))
- Preserve literal long-flag spelling for dispatch round-trip ([`55b1b2e`](https://github.com/s0undt3ch/ToolR/commit/55b1b2e500a9f61eb1e9b6c6050e0701665e97bb))
- Add similar crate for unified-diff output in --check mode ([`4bbb990`](https://github.com/s0undt3ch/ToolR/commit/4bbb990bdc954eb59416879e0bc5cc8469115006))
- *(bench)* Add task-runner startup-time comparison ([`275e27f`](https://github.com/s0undt3ch/ToolR/commit/275e27f0bd71b3c257314d5723acf36d25b061a9))
- *(deps)* Bump actions-cool/check-user-permission from 2.3.0 to 2.4.0 ([`d9d7960`](https://github.com/s0undt3ch/ToolR/commit/d9d79605ec42861387f10eaa784eb3fd3a5be15a))
- *(deps)* Bump s0undt3ch/ToolR ([`e7e2596`](https://github.com/s0undt3ch/ToolR/commit/e7e2596c99423bbe6ab089361777d07babcf08d9))
- *(deps-dev)* Bump ruff from 0.15.13 to 0.15.14 ([`4831624`](https://github.com/s0undt3ch/ToolR/commit/4831624f4694c2f1f8f5fe494a1167f557c91d54))
- *(deps-dev)* Bump hypothesis from 6.152.8 to 6.152.9 ([`6abb094`](https://github.com/s0undt3ch/ToolR/commit/6abb0941e6b9f918a3086782affcb9c80990bb1e))
- *(deps)* Bump rust dependencies ([`2db3cef`](https://github.com/s0undt3ch/ToolR/commit/2db3cefcd1c5ad7c81e2fc0f37ad9586eb606c03))
- *(deps)* Bump transitive deps to latest compatible versions ([`a532afe`](https://github.com/s0undt3ch/ToolR/commit/a532afe4b82f4641a7089856fd810a50510bfeaf))

### <!-- 2 -->🚜 Refactor

- *(tests)* Convert test_dispatch helpers to pytest factory fixtures ([`8b60d8b`](https://github.com/s0undt3ch/ToolR/commit/8b60d8b437f44143e65829f5d1b4f471fb8a1479))
- *(tests)* Convert test_spec_loader helpers to pytest factory fixtures ([`0856956`](https://github.com/s0undt3ch/ToolR/commit/0856956a1cddf382596ba9113c7a71bd08882c5c))
- *(workspace)* Move toolr-rust-utils crate under crates/toolr-core ([`0e3d91f`](https://github.com/s0undt3ch/ToolR/commit/0e3d91f1d4e5c41de958c46ac4d5cccf4ed6d936))
- *(workspace)* Extract toolr binary into crates/toolr ([`9bd0297`](https://github.com/s0undt3ch/ToolR/commit/9bd0297b9ef7b7ccee27d042630b70586a407d6d))
- *(workspace)* Extract toolr-py pyo3 crate; remove python feature flag ([`00c0463`](https://github.com/s0undt3ch/ToolR/commit/00c04637231566ee4d7b0d19e00e615e165904fe))
- *(workspace)* Move python/toolr/ into crates/toolr-py/python/ ([`533ab81`](https://github.com/s0undt3ch/ToolR/commit/533ab818d9561c81622c74bd94476e9185df62df))
- *(install)* Rename dist/ -> installation/, mise-plugin/ -> mise/ ([`b4f1197`](https://github.com/s0undt3ch/ToolR/commit/b4f119730136405f6c9addffaa781e0e084c4cec))
- *(version)* Delegate Cargo.toml writes to `cargo set-version` ([`18e0e74`](https://github.com/s0undt3ch/ToolR/commit/18e0e740ffbe614ad14a31726d4c7512fc415c1a))
- *(manifest)* Rename dynamic_hash to third_party_hash ([`0ad7df9`](https://github.com/s0undt3ch/ToolR/commit/0ad7df9998a3b267f15f48c44937fd8d1d970460))
- *(dispatch)* Remove execute-time dynamic-layer freshness ([`504075e`](https://github.com/s0undt3ch/ToolR/commit/504075e97655efd10b087fbb57eff2842503691c))
- *(complete)* Use freshness::compare in resolve_manifest_at_tab ([`305f3be`](https://github.com/s0undt3ch/ToolR/commit/305f3bef811c872a65d405a3c9deca9c60b8f7bc))
- *(parser)* Factor list_python_files + module_path_for_prefix for reuse ([`2e998fa`](https://github.com/s0undt3ch/ToolR/commit/2e998fa768ef2a3aae44a549d38a5073a7caae69))
- *(parser)* Extract import-alias tracking into types/imports ([`9c36795`](https://github.com/s0undt3ch/ToolR/commit/9c36795554a6f66489316cf131ffa7016cebb7a8))
- *(parser)* Extract PathConstraints into types/path_constraints ([`b706163`](https://github.com/s0undt3ch/ToolR/commit/b706163383e4c83cfeb84bfd31d64425d86257b9))
- *(parser)* Extract type taxonomy into types/supported ([`57e2001`](https://github.com/s0undt3ch/ToolR/commit/57e2001c488e583e9e1a81cce20243215503c20d))
- *(parser)* Extract small literal extractors into types/literals ([`22020fa`](https://github.com/s0undt3ch/ToolR/commit/22020fa4a517953652cc77290bcf1ed2e5a0397d))
- *(parser)* Extract arg-metadata into types/arg_metadata ([`b92541f`](https://github.com/s0undt3ch/ToolR/commit/b92541f0f2d5c68e1c715abb5e319d4e480b1edc))
- *(parser)* Extract type resolver into types/resolve ([`2c0fd8f`](https://github.com/s0undt3ch/ToolR/commit/2c0fd8f3fe321d861213111749c53d4d573fb2d5))

### <!-- 3 -->📚 Documentation

- *(spec)* Add Rust front-end design ([`1263dec`](https://github.com/s0undt3ch/ToolR/commit/1263dec93f0ef9eec11f46829f95d2c8f176330a))
- *(spec)* Add implementation roadmap ([`ef9eee8`](https://github.com/s0undt3ch/ToolR/commit/ef9eee8f57c44564b0b34b6834e8d85205c4dbdf))
- *(spec)* Reorganize specs/ by topic with sequenced filenames ([`d52994f`](https://github.com/s0undt3ch/ToolR/commit/d52994f25c9e9ae73929d3f8627b25448657a5c6))
- *(spec)* Draft Plan 1 (Rust binary skeleton + static manifest layer) ([`b21b5d3`](https://github.com/s0undt3ch/ToolR/commit/b21b5d3b1c93b4eb34d59c7428c13b5e34d05d71))
- *(roadmap)* Mark Plan 1 as 🔧 In Progress ([`f7728b7`](https://github.com/s0undt3ch/ToolR/commit/f7728b7607a0d1357ddbdadc4b3bc96fe7211f69))
- *(roadmap)* Mark Plan 1 as ✅ Done ([`e08a977`](https://github.com/s0undt3ch/ToolR/commit/e08a9771ae1f1f40f19a0479df0f9fccc95d73aa))
- *(spec)* Draft Plans 2-9 for the Rust front-end rewrite ([`8ec018b`](https://github.com/s0undt3ch/ToolR/commit/8ec018b17af07c7cb48896f1893c08a763a49f24))
- *(roadmap)* Mark Plan 2 as 🔧 In Progress ([`0d1ac8f`](https://github.com/s0undt3ch/ToolR/commit/0d1ac8f3281d25ce0eb924087441265e1d11efac))
- *(spec)* Refactor plan-doc test snippets to use pytest factory fixtures ([`8c513d0`](https://github.com/s0undt3ch/ToolR/commit/8c513d0383f2607b541b14270fc798b104901425))
- *(toolr)* Note toolr._runner module + ship-test ([`7f088b3`](https://github.com/s0undt3ch/ToolR/commit/7f088b3b9cc808eb450ed85ffb31596d45295684))
- *(roadmap)* Mark Plan 2 as ✅ Done ([`c5993fa`](https://github.com/s0undt3ch/ToolR/commit/c5993fa6e493caa0a5f309b4f7e48a4a38d71ee8))
- *(roadmap)* Mark Plan 3 as 🔧 In Progress ([`e1ca4e0`](https://github.com/s0undt3ch/ToolR/commit/e1ca4e059a4d74dd3b32de989dbac00c1582e5fe))
- *(roadmap)* Mark Plan 3 as ✅ Done ([`055cbc0`](https://github.com/s0undt3ch/ToolR/commit/055cbc09d0399080b2ce0734f454a4592bad0530))
- *(roadmap)* Mark Plan 4 as done ([`baf49cd`](https://github.com/s0undt3ch/ToolR/commit/baf49cdf3c33d91229ce2d99ede1f03dea432bcd))
- *(roadmap)* Mark Plan 5 as done ([`93b32e8`](https://github.com/s0undt3ch/ToolR/commit/93b32e85a71a54dc0b72a14f31b107e843fac870))
- *(plans)* Mark all completed steps in plans 4 and 5 as done ([`c3f7af1`](https://github.com/s0undt3ch/ToolR/commit/c3f7af10f4e95374fc8eaf647e5c778fa23a1639))
- *(roadmap)* Mark Plan 6 as done ([`8fe6101`](https://github.com/s0undt3ch/ToolR/commit/8fe61017116334631001c32adae5bf1e37bc8e52))
- *(roadmap)* Mark Plan 7 as done ([`b89c77a`](https://github.com/s0undt3ch/ToolR/commit/b89c77a053795140eebf8fec52d2e29e77bb87ef))
- *(roadmap)* Mark Plan 8 as done ([`ebff6b9`](https://github.com/s0undt3ch/ToolR/commit/ebff6b9f70587370004cc97b99268d05129c1d31))
- *(changelog)* Document Plan 9 distribution + breaking changes ([`fb276c4`](https://github.com/s0undt3ch/ToolR/commit/fb276c47e0d180c07e54ef1495dfac3fd5c1d55c))
- *(readme)* Document new standalone install + pip wheel paths ([`5e3c7f8`](https://github.com/s0undt3ch/ToolR/commit/5e3c7f836ffedab0a3bba3cb50d47a28a8cc3ee6))
- *(roadmap)* Mark Plan 9 as done ([`812de25`](https://github.com/s0undt3ch/ToolR/commit/812de253aae214c0fd014282a493406de7b72d62))
- *(readme)* Document SLSA attestation verification flow ([`7763973`](https://github.com/s0undt3ch/ToolR/commit/77639735de56dbdc83461916467b0743bb8e64fe))
- *(spec)* Brainstorm design for docs overhaul + toolr project init ([`b5e29b2`](https://github.com/s0undt3ch/ToolR/commit/b5e29b2a20c6d1c4181f50a976b5bce61ab1022e))
- *(spec)* Relocate regen-doc-snippets to .pre-commit-hooks/ and link issue #191 ([`c43eec0`](https://github.com/s0undt3ch/ToolR/commit/c43eec07178a5d0e318b67ab3d3e7810a351661e))
- *(plans)* Write Plan 10 (project init) and Plan 11 (docs overhaul) ([`b08e90e`](https://github.com/s0undt3ch/ToolR/commit/b08e90ef34a7482a4e7df7d06035553d0d66b1d8))
- *(roadmap)* Mark Plan 10 as done ([`976e715`](https://github.com/s0undt3ch/ToolR/commit/976e7155a65a8511d7fa0b5f9429f2c095a1efbc))
- Roadmap entry + nav skeleton for the docs overhaul ([`ab9e816`](https://github.com/s0undt3ch/ToolR/commit/ab9e8167b291f85073389bb66d27ab3c7eab85fe))
- *(tooling)* Add regen-doc-snippets script + sample-repo fixture ([`425fcde`](https://github.com/s0undt3ch/ToolR/commit/425fcdee14106b8ff880f11d82f2afdfb9f35ffc))
- Quickstart page (install → init → run) ([`90cb46f`](https://github.com/s0undt3ch/ToolR/commit/90cb46f46cde45296e2b115dbe889f21d5f7e14e))
- Rewrite installation page with new install matrix + SLSA section ([`e4c8332`](https://github.com/s0undt3ch/ToolR/commit/e4c833257a2b2262291d6972637ac4bb00f6cee0))
- Concepts page (orientation tour of toolr's moving pieces) ([`3ae9c2b`](https://github.com/s0undt3ch/ToolR/commit/3ae9c2b3a3d0220b0a93d36171222dd6e7bc6922))
- Writing commands chapter (port old usage + examples, restructure) ([`5a73fae`](https://github.com/s0undt3ch/ToolR/commit/5a73faed105a3c404d1006ce4e560967b0017e2e))
- Project configuration page (tools/pyproject.toml + venv model) ([`591e3da`](https://github.com/s0undt3ch/ToolR/commit/591e3da1dc424b25accff99e74998ff6fb5aade5))
- CLI reference (single page, every subcommand documented uniformly) ([`57f009f`](https://github.com/s0undt3ch/ToolR/commit/57f009f9bdc1c7b0b66c880c2fc0bef34a99a0d9))
- Third-party packages page (static manifest convention + toolr.build) ([`08fd639`](https://github.com/s0undt3ch/ToolR/commit/08fd63925c4c364e34af72f68e69fbecb1083e8f))
- Internals chapter (manifest layers, cache, pre-commit, diagnostics) ([`be3012e`](https://github.com/s0undt3ch/ToolR/commit/be3012eccab2031f58a580f21ac753d940311089))
- *(reference)* Prune private-module pages, keep only the public API surface ([`8fbf2c5`](https://github.com/s0undt3ch/ToolR/commit/8fbf2c50dd38d950b1087f01f89faf66abdef78d))
- Update nav, third-party link, and snippet whitespace handling for the reference rename ([`aac13ab`](https://github.com/s0undt3ch/ToolR/commit/aac13abed7dfcf3fa0ae757fa55854139505d16c))
- Slim home page to a landing + audience-shortcut layout ([`b71fa6d`](https://github.com/s0undt3ch/ToolR/commit/b71fa6d6e5be356d26c7c19b5c77c347bb7363ee))
- *(roadmap)* Mark Plan 11 as done ([`e8cb029`](https://github.com/s0undt3ch/ToolR/commit/e8cb029a9ff325aa7e670fd81818dff57e29da73))
- Rename writing-commands/limitations.md → known-bugs.md ([`62d3282`](https://github.com/s0undt3ch/ToolR/commit/62d3282a26b380dbaad08f7387f1f5edaf2b8c80))
- Supported-types matrix + manifest schema reference ([`158a194`](https://github.com/s0undt3ch/ToolR/commit/158a194b6b47353b2c4b8383377de211e98ce2a6))
- Catch the writing-commands chapter up to the new decorator API ([`61dbb11`](https://github.com/s0undt3ch/ToolR/commit/61dbb119dcaaeb71a2a9babe86418d131999b1da))
- Lead with the string-path API, deprecate legacy in prose ([`b08c273`](https://github.com/s0undt3ch/ToolR/commit/b08c2730ff05accfab68e5d327b7652888aee8d9))
- *(spec)* Add cargo workspace split design ([`8a26dde`](https://github.com/s0undt3ch/ToolR/commit/8a26dde1f6d3b8e7c6f531dc134b31fd3b85f533))
- *(plan)* Add Plan 12 (workspace split) under specs/rust-front-end/ ([`f495f0e`](https://github.com/s0undt3ch/ToolR/commit/f495f0e8db6416a88f895d0037b9db8fefeb14ac))
- *(mise)* Salvage MISE_SUPPORT.md into docs/installation/mise.md ([`24b4a24`](https://github.com/s0undt3ch/ToolR/commit/24b4a242be1ba5ed9244d30158f5dc725b614669))
- *(install)* Clarify the two PyPI packages — toolr (CLI) vs toolr-py (bindings) ([`d07c72d`](https://github.com/s0undt3ch/ToolR/commit/d07c72d4a94f3fca70b51b20739f6cc060003270))
- *(release)* Populate UNRELEASED.md with rearchitecture notes ([`ea7188c`](https://github.com/s0undt3ch/ToolR/commit/ea7188c972803252210834adc68d94621bda70b4))
- *(specs)* Auto-rebuild stale manifest on dispatch, drop entry-point plugins ([`e62577f`](https://github.com/s0undt3ch/ToolR/commit/e62577fbc51a65c0e82c463949c2d0b49ef413ed))
- *(specs)* Add dispatch manifest freshness implementation plan ([`d06c9a0`](https://github.com/s0undt3ch/ToolR/commit/d06c9a06acb7fe5a09389eb748e968cffa540bed))
- *(unreleased)* Note entry-point removal and dispatch freshness ([`15ec310`](https://github.com/s0undt3ch/ToolR/commit/15ec310c648dbfc7ffb2140e6a303ce1aec222f5))
- *(third-party)* Plugin-author guide for shipping toolr-manifest.json ([`2075e49`](https://github.com/s0undt3ch/ToolR/commit/2075e4971cf7f06446dcccff0d9370e79cce5f3d))
- *(specs)* Design for pure-Rust toolr self build-manifest ([`24e74b3`](https://github.com/s0undt3ch/ToolR/commit/24e74b32175212220660d21d7fb44d29f0bc5acd))
- *(specs)* Implementation plan for pure-Rust toolr self build-manifest ([`4234929`](https://github.com/s0undt3ch/ToolR/commit/42349293fdcf24c45d11790cd18de6a6d758f189))
- *(third-party)* Drop Python build references; add static-only contract ([`3cd50e5`](https://github.com/s0undt3ch/ToolR/commit/3cd50e52e8442f027fcff1742336528a22097bdd))
- Clean up stale --python references and refresh CLI help snippets ([`bc46d07`](https://github.com/s0undt3ch/ToolR/commit/bc46d0721150babdda85e32135b8393ce7096226))
- *(reference)* Drop stale build.md link after toolr.build removal ([`908e0c8`](https://github.com/s0undt3ch/ToolR/commit/908e0c80af767db11e5763c663fc3ea73343ec6f))
- *(specs)* Repo presentation pass design ([`b109e51`](https://github.com/s0undt3ch/ToolR/commit/b109e519b51a0f4b1e1c3c52dcc52a2c4adfc1f6))
- *(specs)* Give each install path first-class treatment ([`19a5b36`](https://github.com/s0undt3ch/ToolR/commit/19a5b367a634760ca09a3a34d4451d1afb199868))
- *(specs)* Lead the install section with mise ([`a5a0026`](https://github.com/s0undt3ch/ToolR/commit/a5a0026985803c7e2955f66998fc06b1ac033304))
- *(specs)* Reorder install section to mise, pip, curl|sh, powershell, gh ([`df8b87d`](https://github.com/s0undt3ch/ToolR/commit/df8b87d463a735043562b6811867e6ca34d2008b))
- *(specs)* Repo presentation pass implementation plan ([`8f47ab5`](https://github.com/s0undt3ch/ToolR/commit/8f47ab52fe55a694a4dc7682bed9cfc30b5487e3))
- *(writing-commands)* Scope the deprecation admonition to the subgroup method form ([`7adc1b4`](https://github.com/s0undt3ch/ToolR/commit/7adc1b4d7bdc53678346cedf504982bdfa3debbb))
- *(writing-commands)* Use dotted-string form in the nested-groups bullet ([`f177125`](https://github.com/s0undt3ch/ToolR/commit/f177125f565ca353fa972f6069296cb4db06fbc5))
- *(internals)* Correct the third_party_hash File-shape bullet ([`43831d4`](https://github.com/s0undt3ch/ToolR/commit/43831d4e3378a2464c0c00461d2f0e2f54b5c65e))
- *(internals)* Drop entry-points discovery from the dynamic layer ([`f67d888`](https://github.com/s0undt3ch/ToolR/commit/f67d88829418687aed74cc59d340e31394fb4665))
- *(internals)* Align the hashing-details third_party_hash entry ([`ff659cd`](https://github.com/s0undt3ch/ToolR/commit/ff659cdd07e86ddea122964175db1b879e831a72))
- *(dynamic)* Remove stale entry-points reference in DynamicPayload ([`c7dc16b`](https://github.com/s0undt3ch/ToolR/commit/c7dc16b2859a97c9133dd8130b9d8c35706d6cfc))
- *(urls)* Point at toolr.readthedocs.io/latest for published docs ([`233f7ad`](https://github.com/s0undt3ch/ToolR/commit/233f7ad232a19d1c8a6c2945099750e882e34bb2))
- *(readme)* Rewrite around the Rust front-end value-prop ([`652d1c7`](https://github.com/s0undt3ch/ToolR/commit/652d1c79ae766cca2c83feef9c8a72b516daad6a))
- CONTRIBUTING rewrite + point README links at toolr.readthedocs.io/latest ([`3f707a7`](https://github.com/s0undt3ch/ToolR/commit/3f707a7993bd9c6967fbc6a8079445055d6461f6))
- *(specs)* Document the live-vs-archive split ([`005f2ed`](https://github.com/s0undt3ch/ToolR/commit/005f2ede009c5350a65853b33ae3bdae153de580))
- *(specs)* Codebase audit snapshot (2026-05-23) ([`018ab63`](https://github.com/s0undt3ch/ToolR/commit/018ab63a892a42dbd489942630bf783d29c1230e))
- *(writing-commands)* Document toolr.testing.CommandsTester ([`e6b8362`](https://github.com/s0undt3ch/ToolR/commit/e6b836260c9af3f00030413a689b349115cbbb12))
- *(installation)* Align quickstart + install index with README ([`11d83c6`](https://github.com/s0undt3ch/ToolR/commit/11d83c67223ad8e061f72661ab9d0defa3d7fe95))

### <!-- 6 -->🧪 Testing

- *(project)* Integration tests for venv path resolution ([`8fc400f`](https://github.com/s0undt3ch/ToolR/commit/8fc400fb5508c8f5bdaf6f9d2e1529dd10b184c3))
- *(project)* End-to-end deps-sync + execute smoke (ignored by default) ([`e5ac8ff`](https://github.com/s0undt3ch/ToolR/commit/e5ac8ff3711a0bc2d10fd8fe5a1a7506c88e6fff))
- *(complete)* End-to-end smoke tests against the real toolr binary ([`ac7648c`](https://github.com/s0undt3ch/ToolR/commit/ac7648c7654139cb140ea59bd3de5d4c45dd0a0e))
- *(third_party)* Round-trip Python build_manifest into Rust-readable fragment ([`bf35cc8`](https://github.com/s0undt3ch/ToolR/commit/bf35cc86461d06d6b1457f5055520e11de8ac48e))
- *(dynamic)* End-to-end static-plus-dynamic merge integration test ([`68f7527`](https://github.com/s0undt3ch/ToolR/commit/68f75274d8ec68c61bd4597eab2e55252344dd9a))
- *(cli)* Cover pre-flight vs post-mortem split end-to-end ([`30947b3`](https://github.com/s0undt3ch/ToolR/commit/30947b38fa6bf7308a9e06b20d249d51081059ae))
- *(cache)* End-to-end fixture run of list, prune, prune --all ([`2771a60`](https://github.com/s0undt3ch/ToolR/commit/2771a604713b590c0c642e813adf0b5e32693731))
- *(build)* Add editable-install test (xfail: maturin pyo3 limitation) ([`5a2ef47`](https://github.com/s0undt3ch/ToolR/commit/5a2ef475170a1154d2106367c8e536350cf2a464))
- *(compat)* Add end-to-end command_group compat test (xfail: maturin limitation) ([`81ba0d6`](https://github.com/s0undt3ch/ToolR/commit/81ba0d63ee48b022e587d53e3daf9a6488993393))
- *(init)* End-to-end integration tests for toolr project init ([`a90c788`](https://github.com/s0undt3ch/ToolR/commit/a90c788f913b5955ee1577c48ea9ed592eae86ac))
- *(retire)* Three-way prune of Python frontend tests ([`0d07fd1`](https://github.com/s0undt3ch/ToolR/commit/0d07fd10e0a78a373b3bdf91a5c94881c7bdf90f))
- *(distribution)* Assert wheel shapes and cross-wheel install path ([`b868709`](https://github.com/s0undt3ch/ToolR/commit/b86870908263f03836ebe4fb91bf69aa214d2c37))
- *(distribution)* Add maturin to dev deps so distribution tests run ([`9061022`](https://github.com/s0undt3ch/ToolR/commit/90610224a3639f03012b0ee16d812faa6aa05a84))
- *(distribution)* Consume pre-built wheels in a dedicated CI job ([`7dcb5c1`](https://github.com/s0undt3ch/ToolR/commit/7dcb5c1b42955a9731996ae2969c66b14369b495))
- Update pyo3 TypeError message for pyo3 0.28 ([`b4c8ea3`](https://github.com/s0undt3ch/ToolR/commit/b4c8ea38899b39ff4b4419c7f634993927fd2e19))
- E2e happy path + dispatch fix for parent lookup ([`3d8acb8`](https://github.com/s0undt3ch/ToolR/commit/3d8acb8ee08283435981a3da6a83ec18c8e238c3))
- E2e multi-attach + collision detection (auto-rebuild deferred) ([`ebf4163`](https://github.com/s0undt3ch/ToolR/commit/ebf4163835dadb645c37fa972700827e0664a0b4))
- Unskip auto-rebuild E2E now that bootstrap is wired in ([`d913abf`](https://github.com/s0undt3ch/ToolR/commit/d913abf3136d54a0d09a593f3401099ba426eeaa))
- E2e dispatcher outer flags reachable via grafted child path ([`5bfd5ec`](https://github.com/s0undt3ch/ToolR/commit/5bfd5ec956b36aa4c3eb44035f2edb38f9393eae))
- Update spec-loader assertion to match new schema-mismatch error ([`ed1eb53`](https://github.com/s0undt3ch/ToolR/commit/ed1eb537ea4d8ed690f43d541c03366d2db5dcbf))
- *(toolr)* Regression — added tools/*.py is detected on dispatch ([`a896b2c`](https://github.com/s0undt3ch/ToolR/commit/a896b2c35be5c3825367bba7163c783b6bdda704))
- *(toolr)* Syntax error in tools/ soft-fails with cached fallback ([`ad9faf0`](https://github.com/s0undt3ch/ToolR/commit/ad9faf0179dcb1f3acfd394c1b60a6135b2fd716))
- *(toolr)* Bypass argv (self/project/--version) skips freshness ([`7bce171`](https://github.com/s0undt3ch/ToolR/commit/7bce171d35463186c7f6ac25c799e0243eacbbf3))
- *(toolr)* Tab completion never persists the manifest ([`f76836e`](https://github.com/s0undt3ch/ToolR/commit/f76836ed27ebb4eb1e9bb39211ac89451bab0a31))
- *(distribution)* Pull toolr + toolr-py wheels from wheelhouse ([`3cd75b4`](https://github.com/s0undt3ch/ToolR/commit/3cd75b47692e3f85cde97009d02cb0b41ab13c4c))
- *(distribution)* Make_uv_venv fixture for cross-OS venv setup ([`04d7ff3`](https://github.com/s0undt3ch/ToolR/commit/04d7ff39fe47beba7280cbda7a706dc5860d1ae2))
- *(build_fragment)* Equivalence vs committed example plugin manifest ([`5dd9655`](https://github.com/s0undt3ch/ToolR/commit/5dd9655ad97e4bdf4bcd5a2c4c802c67af21911f))
- *(build_fragment)* Use include_str! for example plugin fixture ([`3761674`](https://github.com/s0undt3ch/ToolR/commit/3761674722f8676f27cea9380d4b077dbc173c06))
- *(build_fragment)* Filtering + edge-case coverage ([`9c2d976`](https://github.com/s0undt3ch/ToolR/commit/9c2d976a3d717b03879ab3624f40f9c0cb505347))
- *(toolr)* Drop obsolete --python error-path test from cli_smoke ([`d0ef6c4`](https://github.com/s0undt3ch/ToolR/commit/d0ef6c4dc97618ca9d08ef64e873d5b4abed13ab))
- *(toolr)* Integration coverage for self build-manifest CLI ([`0bca68d`](https://github.com/s0undt3ch/ToolR/commit/0bca68da84a6ab9167a13fe87e8527498fa0b06e))
- Drop Python build_manifest tests superseded by Rust path ([`2bcf2f8`](https://github.com/s0undt3ch/ToolR/commit/2bcf2f8c0cd807c3ea51ff5334f53542a8c54199))
- *(command)* Resolve python via python3-first lookup, skip if missing ([`90ae2de`](https://github.com/s0undt3ch/ToolR/commit/90ae2ded3be75f615a43225f2c496f51395edce9))
- *(command)* Wall-clock-bound tokio command tests, fix #247 workspace hang ([`da25483`](https://github.com/s0undt3ch/ToolR/commit/da25483cb4153aadc6c3f82a3bd9a4a380196892))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Update ToolR action versions in workflows ([`ba95d8e`](https://github.com/s0undt3ch/ToolR/commit/ba95d8e3b65bdc329de48eb2357d0c94b5a5d0b5))
- Add nightly install-channel smoke tests ([`32bc64f`](https://github.com/s0undt3ch/ToolR/commit/32bc64f4effca285bea846b52ee9ca231a08b884))
- Wire doc-snippet drift check into pre-commit + CI ([`90afb89`](https://github.com/s0undt3ch/ToolR/commit/90afb89e7be0b17e763aa3f61530de28cb031a67))
- Stop tracking tools/.toolr-manifest.json — it's a cache ([`9a3051e`](https://github.com/s0undt3ch/ToolR/commit/9a3051eca29f315d350860e226c993cc45aa0b79))
- *(pre-commit)* Install uv + sync dev venv for regen-doc-snippets hook ([`8dffbda`](https://github.com/s0undt3ch/ToolR/commit/8dffbda447e898a9c7c6b5c559f93a11c769114c))
- *(release)* Bump version to 0.20.0 ([`43dbdaf`](https://github.com/s0undt3ch/ToolR/commit/43dbdafab9ba291765edb92031f2d843fc8e955a))
- *(mise)* Provision all CI tooling via jdx/mise-action ([`ea23f9c`](https://github.com/s0undt3ch/ToolR/commit/ea23f9cffd09fb94e3b8e48240ca1796038f2a9c))
- *(ci)* Prefix reusable workflow filenames with underscore ([`7cdf234`](https://github.com/s0undt3ch/ToolR/commit/7cdf234f769ea55a53f391ad1f75f38fab505240))
- *(cache)* Add CACHE_SEED with single-bump invalidation ([`eee8674`](https://github.com/s0undt3ch/ToolR/commit/eee867420e98bf1a69f7456db59a6eb738b9e3e7))
- *(workspace-split)* Fan out wheel builds; rewire version bump and smoke ([`e325843`](https://github.com/s0undt3ch/ToolR/commit/e325843894ad81426e83e9f0e8f8e5d66fb32a74))
- *(workspace-split)* Post-split carry-over cleanups ([`e03c6b9`](https://github.com/s0undt3ch/ToolR/commit/e03c6b90a781db1c08c25eac2291e52dbf0b3577))
- *(workspace-split)* Final loose ends — roadmap status, __init__ version lookup, binary-archive wiring ([`cadb37e`](https://github.com/s0undt3ch/ToolR/commit/cadb37eaaada48c59dce70be47e32b83e320a9d7))
- *(coverage)* Exclude toolr-py from cargo tarpaulin ([`07ef1bf`](https://github.com/s0undt3ch/ToolR/commit/07ef1bf02ebe127483a7ae21f9461e844f920521))
- *(release)* Replace external ToolR bootstrap with local workspace ([`31d4472`](https://github.com/s0undt3ch/ToolR/commit/31d447226b3b9c581242a4cc19cab9d08a0ec3ea))
- Enforce Rust/Python schema-version lock-step via integration test ([`d08f948`](https://github.com/s0undt3ch/ToolR/commit/d08f9482f4789d47fa645a6e03c859278f7f3e8b))
- Use --source-dir for the example-plugin manifest sync check ([`bf3f53e`](https://github.com/s0undt3ch/ToolR/commit/bf3f53e69a884f8337311811574d2f533fdb67f2))
- Skip toolr-plugin-example in test-job uv sync ([`a16ca1a`](https://github.com/s0undt3ch/ToolR/commit/a16ca1af3e90db9258a1e09f80bdb064b1fb5a81))
- Force LF line endings for *.json across all checkouts ([`4ebe915`](https://github.com/s0undt3ch/ToolR/commit/4ebe9150dfcb11d31cd031621766631f573e7507))
- Build toolr-plugin-example once and share via wheelhouse ([`09e7e62`](https://github.com/s0undt3ch/ToolR/commit/09e7e62e292c2f569bb4962867d53d97e659cac8))
- Some workflow organization ([`6fb509d`](https://github.com/s0undt3ch/ToolR/commit/6fb509d70eb42c4efdaa7c2882fd5b225ec6de72))
- Show OS display name ([`8460eba`](https://github.com/s0undt3ch/ToolR/commit/8460eba8fd65b74a3ce71b2088129c11722ba363))
- *(specs)* Archive shipped rust-front-end design tree ([`870bde9`](https://github.com/s0undt3ch/ToolR/commit/870bde9b37e4e953538f7af18fe22edf251ef42b))
- *(specs)* Archive the nine top-level shipped design and plan files ([`bb77fb0`](https://github.com/s0undt3ch/ToolR/commit/bb77fb0df762020265dc6bafab36f25fb8712bfe))
- *(specs)* Repair in-tree references to moved spec paths ([`db6185b`](https://github.com/s0undt3ch/ToolR/commit/db6185b10b7f00708f723a59e712fc207287b856))
- *(docs)* Delete the dead .nav.yml ([`26a6025`](https://github.com/s0undt3ch/ToolR/commit/26a6025fcd426ed92966932525fba6ba751eba4f))
- *(tools)* Drop tools/__init__.py to match documented PEP 420 model ([`98e6b0c`](https://github.com/s0undt3ch/ToolR/commit/98e6b0c556b0059ab82a6397860c75ee5152390b))
- *(toolr)* Drop rich-argparse comparison comments ([`ba28980`](https://github.com/s0undt3ch/ToolR/commit/ba28980244b3a572dfc8c87ca45255357e8bbc4a))
- Drop dead test helpers and migration-era regression tests ([`f5ee391`](https://github.com/s0undt3ch/ToolR/commit/f5ee391061c34dec907ce11b30715c75e91fd0db))
- Rewrite Plan N / Task N comments + tighten migration prose ([`b7a4a27`](https://github.com/s0undt3ch/ToolR/commit/b7a4a27d7b008d0fee10064ba9ad60861c286acd))
- *(mise)* Add single `test` task aggregating cargo + pytest ([`f0fc8e1`](https://github.com/s0undt3ch/ToolR/commit/f0fc8e1e3009febfac3be68a732690c1020e6c86))
- *(specs)* Archive the repo-presentation-pass design + plan ([`6e4d966`](https://github.com/s0undt3ch/ToolR/commit/6e4d9667f6db1f484ce368e1098389c56942feae))
- *(specs)* Archive the 2026-05-23 codebase audit ([`a3ab98a`](https://github.com/s0undt3ch/ToolR/commit/a3ab98ac567724e5ce4b2ec4919e3d1f4afeeb22))
- *(release)* Use setup-toolr action for matrix generation, drop the workspace build ([`79b76da`](https://github.com/s0undt3ch/ToolR/commit/79b76da9c9444b9399c089a5ce27904350f9c5bb))
- *(build)* Drop `cross`, build musl natively, run full matrix on main ([`c65dd09`](https://github.com/s0undt3ch/ToolR/commit/c65dd095ad79dd272e2df57657172c77ffece2ea))
- *(matrix)* Label-driven opt-in + write the *why* to job summary ([`4ed277a`](https://github.com/s0undt3ch/ToolR/commit/4ed277aa28c18d72d2d6614e75cf99c3db6e6170))
- *(throttle)* Add a GitHub Action to throttle workflow run builds ([`a8b0a87`](https://github.com/s0undt3ch/ToolR/commit/a8b0a87317576cd8e7d60d5adaed07ecb5782b3f))
- Switch from `Literal` to `StrEnum` ([`046e654`](https://github.com/s0undt3ch/ToolR/commit/046e654c83f8210efd4ba0924997043dae893f25))

### <!-- 8 -->🛡️ Security

- *(deps)* Bump step-security/harden-runner from 2.19.3 to 2.19.4 ([`d403382`](https://github.com/s0undt3ch/ToolR/commit/d4033821b9a816ac5a275ea573e9d7c5c8d54b2c))
## 0.11.1 - 2026-05-13

### <!-- 0 -->🚀 Features

- *(logs)* Include extra keywords in logs output ([`695dc58`](https://github.com/s0undt3ch/ToolR/commit/695dc5899d29e7562a3a90850ff458045891c195))
- *(tests)* Fuzzy testing ([`f34c9cd`](https://github.com/s0undt3ch/ToolR/commit/f34c9cd84307c3c00cc7fa7b82ffde9f4b8cfffe))
- *(security)* Add ``SECURITY.md`` file ([`131b75a`](https://github.com/s0undt3ch/ToolR/commit/131b75ab2eab6aecbad36a0f865a0f011fe8e11b))

### <!-- 1 -->🐛 Bug Fixes

- *(pypi)* We can't have local version parts in PyPi ([`fa3c515`](https://github.com/s0undt3ch/ToolR/commit/fa3c515c5690f611b6485bdf108236ed4c7b119d))
- *(signature)* Return `VarArg` for `*args` (VAR_POSITIONAL) parameters ([`f8a967d`](https://github.com/s0undt3ch/ToolR/commit/f8a967d15a80e53f14329468a5382502165facd9))
- *(signature)* Match `KwArg` before `Arg` in `Signature.__call__` ([`1228344`](https://github.com/s0undt3ch/ToolR/commit/1228344b15b3e33296dfaf7bce7ff0f9afe1d212))
- *(docs)* Bump pymdown-extensions to 10.21.2 for pygments 2.20.0 compat ([`7422193`](https://github.com/s0undt3ch/ToolR/commit/7422193e201fd88cb333db33f3425ffab9d4d527))
- *(docs)* Use descriptive link text in SECURITY.md ([`212454a`](https://github.com/s0undt3ch/ToolR/commit/212454a5b4bd02f2ce609e7d2a2344248351181a))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Update ToolR action versions in workflows ([`0d11014`](https://github.com/s0undt3ch/ToolR/commit/0d110142417767c317a9b2fa79608ab32abd1d30))
- *(ci)* Switch to prek ([`64c5997`](https://github.com/s0undt3ch/ToolR/commit/64c599784b7bc3639d94beea71150bc92743576f))
- *(pre-commit)* Update pre-commit hook versions ([`6438077`](https://github.com/s0undt3ch/ToolR/commit/64380774c55a509af331809b861c2cdced949832))
- *(ci)* Switch to `macos-15-intel` to build Intel wheels ([`211b0fa`](https://github.com/s0undt3ch/ToolR/commit/211b0faa402a5304648180ae0fa48059d2eb035a))
- *(ci)* Lock permissions on the build.yml workflow ([`aabe4e5`](https://github.com/s0undt3ch/ToolR/commit/aabe4e59aecf0af7e06bd2c27f10cd1bd96286f3))
- *(ci)* Lock `build.yml` GitHub Actions to SHA hashes ([`08bdda3`](https://github.com/s0undt3ch/ToolR/commit/08bdda3f740fef78db3f2b1b549c2bbf425e11a9))
- *(ci)* Only run attestations on the main repo ([`0d2b361`](https://github.com/s0undt3ch/ToolR/commit/0d2b361c20638d696c0554b956fa4fafd662570b))
- *(ci)* Improve `cibuildwheel`` build performance by adding cache ([`60a438f`](https://github.com/s0undt3ch/ToolR/commit/60a438f5c00b406f170ff7d07e4ac9968984c967))
- *(ci)* Fix the chicken & egg issue with releases. ([`77cc4f6`](https://github.com/s0undt3ch/ToolR/commit/77cc4f636cc60b9f2656932ee3a6a2108e3b8c58))
- *(ci)* Remove no longer required process ([`821592f`](https://github.com/s0undt3ch/ToolR/commit/821592f76fae15ea4ba83525a92fd335c5c59685))
- *(ci)* Update but still lock to the SHA ([`57e3719`](https://github.com/s0undt3ch/ToolR/commit/57e37194bf90de7b9228610d3ba5c9677262bbe2))
- *(ci)* When updating our own usage or toolr in GH Actions, lock it ([`b666088`](https://github.com/s0undt3ch/ToolR/commit/b6660881c6ab62fc7d23fd01799fe5eae679ea3c))
- *(ci)* Restrict GH Actions jobs permissions ([`14bfc6d`](https://github.com/s0undt3ch/ToolR/commit/14bfc6d433b4937d0cb00c07198c0bd7f2e6821b))
- *(pre-commit)* Add pre-commit hook to lock GH Actions steps ([`5d69b7a`](https://github.com/s0undt3ch/ToolR/commit/5d69b7a80743681bc392413f2d4f8175d1c5f14c))
- *(ci)* Switch `prepare-release` to a reusable workflow ([`9e6c657`](https://github.com/s0undt3ch/ToolR/commit/9e6c657d8863f829766f588d45092beea0a5ea47))
- *(ci)* Update actions versions ([`2faf97c`](https://github.com/s0undt3ch/ToolR/commit/2faf97c0018d376a3a8bb01910e4ed7e8cecca57))
- *(ci)* Fix auto version bump ([`2a85e6c`](https://github.com/s0undt3ch/ToolR/commit/2a85e6c577351f6c0987952a5735aa0f9b6fe364))
- *(docs)* Add CONTRIBUTING document ([`7691338`](https://github.com/s0undt3ch/ToolR/commit/769133820372966454c2e013fbb65035cb3661c1))
- *(pre-commit)* Swap markdownlint-cli2 for rumdl, add gitleaks ([`a4f6bc5`](https://github.com/s0undt3ch/ToolR/commit/a4f6bc5e58f12b36dc727c49b53565b81d2b3ce3))

### New Contributors

* @step-security-bot made their first contribution
## 0.11.0 - 2025-09-24

### <!-- 0 -->🚀 Features

- *(docstrings)* We now use a rust extension to parse the docstrings ([`a2744f0`](https://github.com/s0undt3ch/ToolR/commit/a2744f0ff3c4b5c086f780d9f0433fc29c3af832))
- *(commands help)* The command's help message is now formatted with Markdown ([`d786915`](https://github.com/s0undt3ch/ToolR/commit/d786915ab9c92726aff05f79c0a079115dd199f9))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(sync-rolling-tags)* Fix `sync-rolling-tags` workflow trigger ([`889e36f`](https://github.com/s0undt3ch/ToolR/commit/889e36fea9025c584ad3d7ea4173ba09aff2beb8))
- *(ci)* Sync'ing rolling tags is now done on demand ([`773745b`](https://github.com/s0undt3ch/ToolR/commit/773745b4156aecee998f8a2e6e494b1a144704e8))
- *(ci)* Fix sync-rolling-tags command ([`59c2955`](https://github.com/s0undt3ch/ToolR/commit/59c29551a6be607748ab97d12dc9958bfa2fdbe4))
- *(ci)* More fixes to the sync-rolling-tags process ([`45ac806`](https://github.com/s0undt3ch/ToolR/commit/45ac80617a69bec0ba2c32b91deec634a98d6eb1))
- *(release)* Update ToolR action versions in workflows ([`6cc4bde`](https://github.com/s0undt3ch/ToolR/commit/6cc4bdecb59d83d95d52f6a908fb5712a888acd0))
- *(ci)* Final sync-rolling-tags fix ([`c48fcbe`](https://github.com/s0undt3ch/ToolR/commit/c48fcbe64867018814992c2712f38c3648b23e53))
- *(ci)* Refresh some caches ([`a91de3d`](https://github.com/s0undt3ch/ToolR/commit/a91de3d2b2ad13887dd6e19a47570b4a2bb80bcf))

## 0.10.1 - 2025-09-19

### <!-- 1 -->🐛 Bug Fixes

- *(parent)* Fix command nesting ([`c223dfc`](https://github.com/s0undt3ch/ToolR/commit/c223dfc88e2981dbd7cd6aed304d219fc3f8f12a))
- *(command)* We now log the `.run()` cmdline at the `INFO` level ([`1311f8d`](https://github.com/s0undt3ch/ToolR/commit/1311f8d817fd71b4aa2b061c48d3e067b8076486))
- *(tests)* Fix `ctx.which` tests to make them less brittle. ([`8a55cf3`](https://github.com/s0undt3ch/ToolR/commit/8a55cf32e5e793472b5059a5a433b3e6a90cfc38))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Don't update ToolR action usage in workflows ([`fbe4992`](https://github.com/s0undt3ch/ToolR/commit/fbe49927887da44cfbba0e8e4892a672d14837df))
- *(release)* Add workflow that updates ToolR versions in workflows ([`9782974`](https://github.com/s0undt3ch/ToolR/commit/9782974d40ff2d62a183025c9c48dee0d3e92143))

## 0.10.0 - 2025-09-17

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Improve CI build matrix reporting ([`777595c`](https://github.com/s0undt3ch/ToolR/commit/777595c0ce4c32fe0a88e249e49fb36bc54206f9))
- *(ci)* Prevent unnecessary branch builds on PRs ([`4d7e911`](https://github.com/s0undt3ch/ToolR/commit/4d7e911fb7448e22fe9f5b961bfe368b3ef50868))
- *(release)* Update all self ToolR actions usage on release ([`38e2a8b`](https://github.com/s0undt3ch/ToolR/commit/38e2a8b92b7c95431cf3ff05e8af50d020b4f11d))
- *(ci)* Consistent workflow toolr output width ([`4475406`](https://github.com/s0undt3ch/ToolR/commit/447540685a36ddae3936ee4c3232474c42f59748))

## 0.9.0 - 2025-09-13

### <!-- 0 -->🚀 Features

- *(cli)* Provide a `Context` class
- *(registry)* Implemented the registry and it's tests
- *(logging)* Add `toolr.utils.logs` to improve logging support
- *(cli)* Provide the package CLI entry point
- *(help)* We now use ``RichHelpFormatter`` to render the help
- *(docstrings)* Parse docstrings to construct help
- *(docs)* Capture each parameter description from docstrings
- *(coverage)* Upload code coverage to codecov
- *(ci)* Upload test results to codecov
- *(signatures)* Add signature parsing
- *(signature)* Handle append action, including weird boolean append.
- *(nargs)* Support ``nargs`` and ``*variable`` in function signatures
- *(docs)* Documentation!
- *(context)* Implemented prompt support in ``Context``.
- *(github-actions)* Allow setting ToolR from a github-action
- *(signature)* Add support for mutually exclusive groups
- *(logging)* Add `setup_logging` function.

### <!-- 1 -->🐛 Bug Fixes

- *(imports)* Handle import errors when searching for tools
- *(descriptions)* Differentiate descriptions
- *(docstring)* Fix dosctring class reference
- *(decorator)* Fix decorator usage.
- *(help)* Parse each decorated command docstring to provide help
- *(log)* Only log the time on specific occasions.
- *(tests)* Fix tests according to latest code changes
- *(tests)* Fix rust tests on windows
- *(scope)* Let the codecov CLI tool find the coverage files
- *(coverage)* Don't track coverage in ``if TYPE_CHECKING:`` code blocks
- *(signature)* `dest` is always set to the name of the positional parameter
- *(tests)* Small refactor to improve testing
- *(signature)* On positional arguments, the name will always be the first alias
- *(enums)* Handle enums by name instead of by value
- *(cli)* Fix early verbose/debug output CLI parsing logic
- *(tests)* Skip problematic windows test
- *(docs)* Include missing docs examples
- *(docs)* Remove `uv run` prefix from examples
- *(command)* Command names from functions auto-naming
- *(SignatureError)* `SignatureError` exceptions now point to command
- *(pypi)* Fix PyPi packaging uploads

### <!-- 2 -->🚜 Refactor

- *(toolr)* Support 3rd-party commands
- *(consoles)* Name context consoles explicitly
- *(3rd-party)* Fix commands and command groups augment/overrides
- *(consoles)* Refactor consoles setup

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(dependencies)* Add `rich=-argparse` as a dependency
- *(command)* Rename `command.run_command` to `command.run`
- *(context)* Make the ``context`` module "private".
- *(ci)* Define allowed concurrency
- *(requiremenst)* We no longer need to maintain separate requirements files
- *(tools)* Clean up the pre-existing tools directory
- *(pre-commit)* Update pre-commit hook versions
- *(lint)* Fix lint issues found with latest pre-commit hooks versions
- *(cibuildweel)* Bump `MACOSX_DEPLOYMENT_TARGET` to `11.0`
- *(cleanup)* Remove `changelog.d/`, it won't be needed anymore
- *(typing)* Make the typing gods happier
- *(msgspec)* Replaced all usages of ``dataclass`` with ``msgspec.Struct``
- *(pre-commit)* Upgrade some pre-commit hooks
- *(pre-commit)* Add ``codespell`` pre-commit hook
- *(parser)* Use a private method to set the parser instead.
- *(discovery)* Actually start discovering tools when running ``toolr``
- *(typing)* Fix typing
- *(samples)* Fix sample cases to respect the required signature
- *(rust)* Address clippy errors
- *(ci)* Define the pre-commit cache to be inside the workspace
- *(ci)* Parallelize package builds
- *(ci)* Use OIDC to authenticate codecov
- *(tests)* Add default pytest flags to config
- *(dependencies)* Add ``pytest-subtests`` to dev dependencies
- *(tests)* Add ``argv`` tests
- *(logs)* Logging utils module testing
- *(tests)* Add test coverage for the `__main__` module
- *(tests)* Improve test coverage of the context object
- *(README.md)* Fix logo file path
- *(mypy)* Have mypy ignore `tests/support/3rd-party-pkg/.*`
- *(tests)* Add test coverage to ``setup_consoles``
- *(pyproject.toml)* Define the 3rd-party test package as editable
- *(ci)* Improved parallelization
- *(pre-commit)* Update hook versions
- *(tests)* Split `tests/test_context.py` into several test modules
- *(docs)* Add ``ruff`` as a docs dependency
- *(ci)* Add and use ``.github/actions/setup-virtualenv``
- *(ci)* Push built packages to test.pypi.org on the default branch
- *(docs)* Fix logo URL in readme
- *(gitignore)* Ignore `*.code-workspace`
- *(pre-commit)* Upgrade pre-commit hook versions
- *(ConsoleVerbosity)* Move `ConsoleVerbosity` to  `toolr.utils._console`
- *(action)* Simplify action
- *(release)* Update the release process
- *(release)* Separate release workflow
- *(security)* Include build provenance attestations
- *(debug)* Set verbose to true when running in debug mode
- *(oackages)* Stop building for `s390x`.
- *(dependabot)* Add `dependabot` configuration
- *(docs)* Add `.readthedocs.yaml` config file
- *(release)* Fix attestations on release workflow
- *(release)* Fix generate build matrix step
- *(changelog)* Add cliff config file
- *(release)* More release workflow fixes
- *(release)* Use the global permissions
- *(release)* Use GH App to push the tags
- *(release)* The action now just configures git with higher privileges
- *(release)* Just repeat, it's simpler in the end
- *(release)* Use `sdist` to build wheels
- *(release)* Prepare for 0.1.1 release
- *(release)* Publish GH release fixes
- *(release)* Prepare for 0.1.2 release
- *(release)* Change release notes filename name
- *(docs)* Update the docs URL to  the right one
- *(release)* Remove the PyPi url
- *(release)* Revert debug release changes
- *(release)* Fix package name to be compliant with PyPi
- *(changelog)* Fix white-space issues around changelog generation
- *(prepare-release)* Run `pre-commit` against the prepare release changes
- *(ci)* Pre-commit needs to be setup and run in a few places

### New Contributors

- @s0undt3ch-gh-actions-automations[bot] made their first contribution
- @dependabot[bot] made their first contribution
