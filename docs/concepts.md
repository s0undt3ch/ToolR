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
Created by `toolr project venv sync` (or automatically by
`toolr project init`). See [Project configuration](project-config.md).

## The manifest

`tools/.toolr-manifest.json` — the cached structure of every group
and command the toolr binary knows about. Toolr regenerates it when
`tools/` or your dependencies change, so tab completion and `--help`
never have to start Python. Has two layers (static + dynamic). It's a pure cache —
**don't commit it to git** (`toolr project init` adds it to
`tools/.gitignore` for you). See
[Internals → Manifest layers](internals/manifest.md).

## Tab completion

Shell scripts for bash, zsh, and fish that call a hidden
`toolr __complete` endpoint on every Tab press. Install once per
shell with `toolr self completion install <shell>`.

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

Trust is keyed on the repository's canonical root path. Content isn't
hashed, so a dependency bump never asks again.

Trust a repository in either of two ways:

- Answer `y` at the prompt toolr shows on a terminal. Anything else,
  including bare Enter, refuses. With `--quiet` or no terminal, toolr
  never prompts and refuses.
- Run `toolr project trust [PATH]`. `toolr project untrust [PATH]`
  removes the record.

`toolr self trusted` lists the trusted repositories and marks the ones
whose path no longer exists. `toolr self trusted --prune` removes those.
Records live in toolr's state directory (`$XDG_STATE_HOME/toolr/trust/`),
not in the cache, so `toolr self cache prune` leaves them alone.

A repository where toolr already built a venv is trusted automatically
the first time. `toolr project init` trusts the directory it writes to.

These never need trust, because nothing executes: `--help`, bare
`toolr`, tab completion, `toolr project manifest rebuild` and
`toolr self build-manifest`.

For automation, set `TOOLR_TRUST_ALL=1`. The value must be exactly `1`.
The `s0undt3ch/ToolR` GitHub Action sets it for you; see
[Project configuration](project-config.md#venv-location). toolr doesn't infer it
from `CI=true`.

!!! warning "Trust is a one-way door"

    Once a repository is trusted, its code runs as you. It can do
    anything you can, including editing the trust store.

Trust doesn't cover:

- A trusted repository that later pulls hostile `tools/*.py` or
  `[tool.uv]` changes.
- Editor and LSP integrations that import `tools/*.py` themselves.
- A compromised package index used by a trusted repository.
- `toolr project init` in a directory that already holds someone else's
  files. It records trust for content you didn't write.
