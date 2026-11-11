# How toolr is laid out

A short tour of the pieces that show up when you use toolr in a repo.
No deep dives — each item links to its own page.

## The `toolr` binary

A single self-contained Rust executable. It parses your command line,
loads your repo's [manifest](#the-manifest) to know what commands
exist, then spawns a Python subprocess to actually run them. See
[Installation](installation/index.md).

## The `tools/` directory

A directory at the root of your repo where your commands live. Each
`*.py` file under it can register one or more command groups. The
directory is a [PEP 420 namespace package][pep-420] — **no
`__init__.py` is needed**. See [Writing commands](writing-commands/index.md).

toolr reads every `*.py` under `tools/` for commands, so keep test files
out of it. Put a command's tests in your project's own tests tree, e.g.
`tests/tools/<module>/test_<command>.py`, with an `__init__.py` in `tests/`
and each directory below it so pytest doesn't import `tests/tools/` as a
second `tools` package. Fixture data goes in a `files/` directory next to
the tests that use it; see [Testing your commands](writing-commands/testing.md).

A `tools/` is a toolr project only if it contains a `pyproject.toml`
(`toolr project init` creates one). toolr finds the project by walking up
from the current directory to the nearest `tools/pyproject.toml`, so a
`tools/` without one, such as a `src/pkg/tools/` package in your code or a
scripts directory above your repo, is ignored.

Command modules must be regular files under `tools/`. A `*.py` under
`tools/` that is a symlink, including a dangling one, is an error, so toolr
never picks up code from outside `tools/` by surprise: toolr lists every such
link and refuses to build the command list until each is replaced by the
file itself. Symlinked directories under `tools/`, even ones named `*.py`,
are ignored, not followed. To share command code between projects, put it in
a Python package and depend on it from `tools/pyproject.toml`, published or
as a path dependency.

On Windows with `core.symlinks=false`, git checks a link out as a plain
file holding the target path, which then fails to parse instead of
producing this message.

[pep-420]: https://peps.python.org/pep-0420/

## `tools/pyproject.toml`

Declares the Python dependencies your commands need (the `toolr`
package itself, plus anything your `tools/*.py` files import) and
toolr-specific options like the venv layout. Maintained alongside
your code. See [Project configuration](project-config.md).

## The tools venv

A Python virtualenv managed by [uv](https://docs.astral.sh/uv/),
materialised from `tools/pyproject.toml`. By default lives in
`$XDG_CACHE_HOME/toolr/<repo-key>/venv/` (one per repo); opt into
in-tree `tools/.venv/` via `[tool.toolr] venv-location = "in-tree"`.
Created by `toolr project trust`, `toolr project venv sync` or
`toolr project init`. See [Project configuration](project-config.md).

## The manifest

`tools/.toolr-manifest.json` — the cached structure of every group
and command the toolr binary knows about. Toolr regenerates it when
`tools/` or your dependencies change, so tab completion and `--help`
never have to start Python. Has two layers (static + dynamic). It's a pure cache —
**don't commit it to git** (`toolr project init` adds it to
`tools/.gitignore` for you). See
[Internals → Manifest layers](internals/manifest.md).

## Tab completion

Shell scripts for bash, zsh, fish, and PowerShell that call a hidden
`toolr __complete` endpoint on every Tab press. Install once per
shell with `toolr self completion install <shell>`.

Path-typed arguments complete from the file system, using each shell's
own file completion. `--opt=value` completes like `--opt value`, for
choices and paths alike. One catch: the shell doesn't expand `~` after
`--opt=`, so `--opt=~/x` passes a literal `~`; write `--opt ~/x` or
`--opt=$HOME/x` instead. A script installed by an older toolr doesn't do
either: re-run `toolr self completion install <shell> --force`.

Pressing Tab after a bare `-` offers the long forms (`--verbose`) and never
the short aliases (`-v`). This is deliberate. A short alias you type
yourself still works.

## The per-repo cache

`$XDG_CACHE_HOME/toolr/` — one entry per repo, each holding its
venv + a `meta.json` sidecar with timestamps. Inspect with
`toolr self cache list`; prune orphans or stale entries with
`toolr self cache prune`. See [Internals → Cache layout](internals/cache.md).

## Repository trust

toolr refuses to run a repository's code until you have trusted that
repository once. Running a command imports `tools/*.py`, and syncing the
venv runs `uv` against `tools/pyproject.toml`. Both are code the
repository controls.

Trust is keyed on the git repository. A repository and all its worktrees
share one trust record, wherever the worktrees live. Trusting or untrusting
any checkout of a repository trusts or untrusts all of them, and a worktree
made later needs no new prompt. Every separate repository, such as a nested
clone or a submodule, is asked for on its own. A project outside git is
trusted by its own path. Content isn't hashed, so a dependency bump never
asks again.

A few cases differ from what you might expect:

- A home directory under git, such as a dotfiles repository at `~/.git`, is
  not the repository for projects below it. Trusting `~` never trusts
  `~/Downloads/foo`, which asks on its own. A project with its own `.git`
  is unaffected.
- A `.git` that is a symlink, or one owned by another user, is trusted only
  by its own path. On Windows toolr doesn't check who owns `.git`, so only
  the symlink case applies.
- A main checkout made with `git init --separate-git-dir` is trusted
  separately from its worktrees.
- A worktree moved with plain `mv` asks again. If you moved it yourself, run
  `git worktree repair <worktree top>` from the main checkout to restore its
  trust, where the worktree top is the directory holding its `.git` file.
  `git worktree move` keeps it trusted.

Trust a repository in either of two ways:

- Run `toolr project trust [PATH]`. `toolr project untrust [PATH]`
  removes the record.
- Answer `y` at the prompt toolr shows on a terminal. Anything else,
  including bare Enter, refuses. With `toolr --quiet` or no terminal, toolr
  never prompts and refuses.

Trusting a repository is also when toolr offers to build its tools venv.
After `toolr project trust`, or a `y` at the prompt when the venv
is missing, out of date or broken, toolr asks a second question, `Sync it now? [y/N]`.
It warns first that syncing may download uv and a Python, and installs
the dependencies in `tools/pyproject.toml`.

A `y` syncs the venv, installing uv first if needed without asking
again, and the command you ran carries on. Anything else keeps the
trust and leaves the venv alone: `toolr project trust` still exits 0,
and a command run stops with an error. Run `toolr project venv sync`
when you're ready. This prompt replaces the uv install prompt, so it follows
the same rules. With `toolr --quiet` or no terminal
toolr doesn't ask and doesn't sync. `TOOLR_AUTO_SYNC=1` answers `y` without asking,
including with `toolr --quiet` or no terminal, so the sync runs. See the
[environment variables](project-config.md#environment-variables) table.
`toolr project trust --no-sync` only records the trust. If a sync you
agreed to fails, the trust is kept and toolr exits non-zero.

`toolr self trusted` lists the trusted repositories and marks the records
whose repository no longer exists. `toolr self trusted --prune` removes those.
Records live in toolr's state directory, not in the cache, so
`toolr self cache prune` leaves them alone. That's
`$XDG_STATE_HOME/toolr/trust/` when `XDG_STATE_HOME` is an absolute path,
otherwise the platform state directory (`~/.local/state/toolr/trust/` on
Linux), falling back to toolr's data directory where there is none
(`~/Library/Application Support/toolr/trust/` on macOS).

toolr refuses to use a trust store that sits inside the repository being
checked, since that repository could ship its own records. Set
`XDG_STATE_HOME` to a directory outside it. The one exception is a
repository rooted at your home directory, for example because of a
personal `~/tools/`: the default store always lives under it.

Trust starts empty. A venv an older toolr built doesn't make its
repository trusted, and neither does a run under `TOOLR_TRUST_ALL=1`.
`toolr project init` trusts the directory it writes to. Inside a git
repository that's the whole repository, with all its worktrees.

`--help`, bare `toolr` and tab completion need no trust, but in an untrusted
repository they show only toolr's own commands. `--help` and bare `toolr`
print a one-line hint on stderr that names `toolr project trust`. The hint
doesn't appear under `-q` or on help for a built-in command.

These ask for trust first: `toolr <command>` for a command from the
repository, `toolr project manifest rebuild` and
`toolr self build-manifest <package>`. Under `-q` they refuse without asking.
`toolr self build-manifest --source-dir` needs no trust. A mistyped command in
an untrusted repository also asks for trust, because toolr can't tell it from
a repository command. A bad value for a path option is reported after the
prompt.

Until you trust a repository, toolr reads only what it needs to find the
repository and its trust record. It walks up from the current directory to the
first `tools/pyproject.toml`, checking only that the file exists. To find the
git repository the trust record is keyed on, it reads `.git` and, for a linked
worktree, the `gitdir` file that links back to it, at most 64 KiB each. It
writes nothing under `tools/`, and reads nothing else there: not
`tools/pyproject.toml`, not `tools/.toolr-manifest.json`, none of the files in
`tools/`, and nothing inside `tools/.venv`. `toolr project venv path` and the
other `toolr project venv` commands ask for trust before they read
`tools/pyproject.toml`. The one command that reads `tools/` first is
`toolr project init`: you point it at the directory to write, and it compares
each file it would write with one already there, byte for byte, before it
trusts that directory.

Once a repository is trusted, toolr believes a committed
`tools/.toolr-manifest.json` as repository content, like `tools/*.py`. Review
a change to it as you would a change to the code.

### In-tree venvs

After trust, toolr uses an in-tree `tools/.venv` only if it built that venv.
When toolr builds one, it writes a random token into the venv's
`.toolr-sync-stamp` file and records the same token in the trust store, under
`venvs/`. The venv is used only while the two match.

toolr never runs any other in-tree venv, including one an older toolr built or
one made with `uv venv`: it may have come with the repository, and running it
would run code you never approved. The first time a command needs the venv,
plugin commands included, toolr explains this and asks before it removes the
venv and builds a fresh one, since anything installed in it by hand is lost.
Only a `y` on a terminal, or `TOOLR_REPLACE_UNTRUSTED_VENV=1` in the
environment, approves the removal; `TOOLR_AUTO_SYNC` doesn't. With `-q` or no
terminal toolr leaves the venv in place, says why, and names the ways to
approve. A symlinked venv is unlinked, not followed, and toolr refuses to remove
anything when `tools/` is itself a symlink. Plugin manifests from such a venv
aren't read, so `--help` leaves its plugin commands out until the rebuild.
`toolr self build-manifest <package>` refuses against it, and
`toolr project manifest rebuild` warns and builds without the plugins.

Syncs of an in-tree venv take a lock held in the trust store, and say so on
stderr while another toolr holds it, unless run with `-q`. When toolr can't take
that lock, for example because the trust store is read-only, it won't remove a
venv it didn't build: the sync fails and names the store, which couldn't record
a rebuilt venv's token either. Worktrees keep separate tokens, so each one
builds its own venv. Venvs in the cache and runs under `TOOLR_TRUST_ALL=1` skip
the token check, and a sync under `TOOLR_TRUST_ALL=1` keeps the token an in-tree
venv already has. `toolr project untrust` drops the repository's venv records,
and `toolr self trusted --prune` drops records whose venv is gone. On Unix,
each sync also deletes its lock file when it ends, and those two commands delete
the lock files of the records they drop and any no record is left for; a lock
another toolr holds is never deleted.

For automation, set `TOOLR_TRUST_ALL=1` (see the
[environment variables](project-config.md#environment-variables) table for what counts as on).
The `s0undt3ch/ToolR` GitHub Action sets it for you; see
[Project configuration](project-config.md#venv-location). toolr doesn't infer it
from `CI=true`.

!!! warning "Trust is a one-way door"

    Once a repository is trusted, its code runs as you. It can do
    anything you can, including editing the trust store.

Trust doesn't cover:

- A different project placed at the path of a trusted one: a different
  repository cloned to a trusted repository's path, a directory placed at a
  deleted worktree's path before `git worktree prune`, or a non-git
  project at a trusted project's path. Trust follows the path there, so run
  `toolr project untrust` before you delete a checkout you won't recreate.
  A record outlives its repository, and mise's trust works the same way.
  `toolr self trusted --prune` only removes records whose path no longer
  exists, so it can't catch a path that has since been reused.
- A trusted repository that later pulls hostile `tools/*.py` or
  `[tool.uv]` changes.
- Editor and LSP integrations that import `tools/*.py` themselves.
- A compromised package index used by a trusted repository.
- `toolr project init` in a directory that already holds someone else's
  files. It records trust for content you didn't write, and inside a git
  repository it trusts that whole repository.

## Windows notes

There is no Ctrl-C handler on Windows. An interrupt ends toolr at once, so
the command's temporary `toolr-spec-*.json` file stays in `%TEMP%` and the
Python runner may still be running as the prompt returns. Delete leftover
spec files by hand if you interrupt often.
