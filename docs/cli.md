# CLI reference

Every `toolr` subcommand documented in one place. Top-level flags
first, then each subcommand under its namespace. Each section is
anchored for grepping.

## Top-level

```sh
toolr --help
```

```text
--8<-- "docs/cli-files/toolr-help.txt"
```

- `toolr --version` — print the binary's version.
- `toolr --help` — print top-level help. On a leaf command the
  long form prints the full docstring (rendered via termimad);
  `-h` prints the same content as a one-line summary.
- `-d` / `--debug` — increase verbosity. **Root-only**: place it
  before the subcommand (`toolr -d example hello`, not
  `toolr example -d hello`). Mutually exclusive with `--quiet`.
- `-q` / `--quiet` — suppress non-error output. Root-only, same
  placement rule as `--debug`.

## `toolr project ...`

Operations on the current repo's `tools/` directory.

### `toolr project init` {#project-init}

Scaffold `tools/` in the current directory.

**Usage:** `toolr project init [--force] [--no-sync] [--venv-location {cache,in-tree}] [--no-example] [--python <version>] [--quiet]`

**Flags:**

- `--force` — overwrite an existing `tools/` directory.
- `--no-sync` — skip the automatic `uv sync` after scaffolding.
- `--venv-location` — `cache` (default) or `in-tree`. Sets the
  `[tool.toolr] venv-location` value in the generated `pyproject.toml`.
- `--no-example` — skip generating `tools/example.py`.
- `--python` — `requires-python` value for `tools/pyproject.toml`
  (defaults to the running Python's `>=major.minor`).
- `--quiet` — suppress informational output.

```sh
toolr project init --help
```

```text
--8<-- "docs/cli-files/project-init-help.txt"
```

See also: [Quickstart](quickstart.md), [Project configuration](project-config.md).

### `toolr project deps sync` {#project-deps-sync}

Materialise (or refresh) the tools venv via `uv sync`.

```sh
toolr project deps sync --help
```

```text
--8<-- "docs/cli-files/project-deps-sync-help.txt"
```

Runs the full sync flow (see [Project configuration → sync
interaction](project-config.md#interaction-with-toolr-project-deps-sync)).

### `toolr project venv path` {#project-venv-path}

Print the absolute path to the resolved tools venv. Useful for
shell-scripting or sanity-checking `venv-location`.

```sh
toolr project venv path --help
```

```text
--8<-- "docs/cli-files/project-venv-path-help.txt"
```

### `toolr project venv shell` {#project-venv-shell}

Spawn a subshell with the tools venv activated. `$VIRTUAL_ENV` is set,
`$PATH` is prepended with the venv's `bin/`, and `$TOOLR_VENV` carries
the venv path for prompt customisation.

```sh
toolr project venv shell --help
```

```text
--8<-- "docs/cli-files/project-venv-shell-help.txt"
```

### `toolr project manifest rebuild` {#project-manifest-rebuild}

Regenerate the static + dynamic manifest in place. Equivalent to what
toolr does automatically when it detects drift, but explicit and
reportable.

```sh
toolr project manifest rebuild --help
```

```text
--8<-- "docs/cli-files/project-manifest-rebuild-help.txt"
```

## `toolr self ...`

Operations on toolr itself (not on the project).

### `toolr self completion print <shell>` {#self-completion-print}

Print the completion script for `<shell>` to stdout. Useful for piping
into your own dotfiles management.

**`<shell>`:** `bash`, `zsh`, or `fish`.

```sh
toolr self completion print --help
```

```text
--8<-- "docs/cli-files/self-completion-print-help.txt"
```

### `toolr self completion install <shell> [--force]` {#self-completion-install}

Write the completion script for `<shell>` into its standard location
(`~/.bash_completion.d/`, `~/.zsh/completions/`, or
`~/.config/fish/completions/`). `--force` overwrites an existing file
without prompting.

```sh
toolr self completion install --help
```

```text
--8<-- "docs/cli-files/self-completion-install-help.txt"
```

### `toolr self cache list` {#self-cache-list}

Tabular listing of every cached per-repo venv: source repo, size,
last-used timestamp.

```sh
toolr self cache list --help
```

```text
--8<-- "docs/cli-files/self-cache-list-help.txt"
```

### `toolr self cache prune` {#self-cache-prune}

Remove orphan and stale cache entries.

**Flags:**

- `--all` — remove every cache entry (with confirmation).
- `--yes` / `-y` — skip the confirmation prompt when used with `--all`.
- `--dry-run` — show what would be deleted without deleting.
- `--stale-after-days <DAYS>` — override the default staleness
  threshold (30 days).

```sh
toolr self cache prune --help
```

```text
--8<-- "docs/cli-files/self-cache-prune-help.txt"
```

### `toolr self build-manifest <package>` {#self-build-manifest}

Generate a `toolr-manifest.json` fragment for a third-party package.
Wraps `python -m toolr.build`; locates a working Python automatically.

**Flags:**

- `--check` — verify the on-disk manifest matches what regeneration
  would produce; exit non-zero on drift.
- `--output PATH` — write to a specific file instead of the
  package's default location.
- `--python PATH` — use a specific Python interpreter.

```sh
toolr self build-manifest --help
```

```text
--8<-- "docs/cli-files/self-build-manifest-help.txt"
```

See also: [Third-party packages](third-party.md).

## Internal subcommands

Used by other tooling (shell completion scripts, CI). Not part of the
user-facing surface; documented here for completeness.

### `toolr __complete <cwd> <args...>` {#__complete}

Completion endpoint called by the installed shell scripts. Reads the
current repo's manifest, prefix-matches against subcommands and arg
values, writes candidates to stdout. Sub-50ms.

### `toolr __build-static-manifest` {#__build-static-manifest}

Regenerate only the static layer of the manifest (no Python
introspection). Faster than `toolr project manifest rebuild` but
won't pick up dynamically-registered commands.

### `toolr __install-uv-now` {#__install-uv-now}

Force-install a managed copy of uv into `$XDG_DATA_HOME/toolr/bin/`,
bypassing the usual consent prompt. Used by the install scripts and
by `toolr project deps sync` after a consent flow.
