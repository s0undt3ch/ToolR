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
Created by `toolr project deps sync` (or automatically by
`toolr project init`). See [Project configuration](project-config.md).

## The manifest

`tools/.toolr-manifest.json` — the cached structure of every group
and command the toolr binary knows about. Toolr regenerates it when
`tools/` or your dependencies change, so tab completion and `--help`
stay sub-50ms. Has two layers (static + dynamic). It's a pure cache —
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
