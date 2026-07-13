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

### Output Options {#output-options}

Five root-level flags that tweak how toolr's own output renders **and**
the defaults for any `ctx.run(...)` subprocess your command starts.
They're root-only — place them before the subcommand
(`toolr -d --timeout-secs 30 ci hello`, not
`toolr ci hello -d --timeout-secs 30`).

| Flag | What it does |
|---|---|
| `-d` / `--debug` | Increase verbosity. Also enables Python `DEBUG` logging in the runner. Mutually exclusive with `--quiet`. |
| `-q` / `--quiet` | Suppress non-error output. |
| `--timestamps` / `--ts` | Prepend ISO-8601 timestamps to log lines. |
| `--no-timestamps` / `--nts` | Suppress log-line timestamps (default; wins over `--timestamps`). |
| `--timeout-secs SECONDS` / `--timeout` | Default timeout passed to every `ctx.run(...)` call. Per-call `timeout_secs=` still wins. |
| `--no-output-timeout-secs SECONDS` / `--nots` | Default "no output for N seconds" watchdog applied to every `ctx.run(...)`. Per-call `no_output_timeout_secs=` still wins. |

**How the timeouts interact with `ctx.run`:**

```python
@command(group="example")
def slow(ctx):
    """Tools-author writes this, oblivious to root flags."""
    ctx.run("sleep", "30")                       # uses --timeout-secs default if set
    ctx.run("sleep", "30", timeout_secs=60)      # per-call wins; ignores root flag
```

The defaults flow through the runner spec as `ContextSpec.default_timeout_secs`
/ `default_no_output_timeout_secs`. They're plain numbers in seconds;
`None` (the default-default) means "no watchdog unless the caller
opts in."

## `toolr project ...`

Operations on the current repo's `tools/` directory.

### `toolr project init` {#project-init}

Scaffold `tools/` in the current directory.

**Usage:**

```text
toolr project init [--force] [--no-sync] [--venv-location {cache,in-tree}]
                   [--no-example] [--python <version>] [--quiet]
```

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

### `toolr project venv sync` {#project-venv-sync}

Sync the tools venv against `tools/pyproject.toml` and `tools/uv.lock`.

Default behaviour is idempotent — when the freshness stamp says the
venv is up to date, `sync` exits immediately without spawning `uv`.
Pass `--force` to re-run unconditionally. Pass `--quiet` for an
unattended-safe path (silent on success, silent on benign exits like
"not a toolr repo" or "uv install needs consent"); see
[Auto-sync the tools venv on shell-enter](installation/mise.md#auto-sync-the-tools-venv-on-shell-enter)
for the mise enter-hook recipe that builds on `--quiet`.

Pass `-U` to re-resolve every package to its latest allowed version
before syncing, or `-P <pkg>` (repeatable) to re-resolve specific
packages only.

```sh
toolr project venv sync --help
```

```text
--8<-- "docs/cli-files/project-venv-sync-help.txt"
```

Runs the full sync flow (see [Project configuration → sync
interaction](project-config.md#interaction-with-toolr-project-venv-sync)).

### `toolr project venv lock` {#project-venv-lock}

Refresh `tools/uv.lock` without touching the venv (wraps `uv lock`).
Use this when you want to record updated resolutions in the lock file
before deciding whether to apply them.

Pass `-U` to re-resolve every package, or `-P <pkg>` (repeatable) to
re-resolve specific packages only.

```sh
toolr project venv lock --help
```

```text
--8<-- "docs/cli-files/project-venv-lock-help.txt"
```

### `toolr project venv add` {#project-venv-add}

Add one or more packages to `tools/pyproject.toml` and sync the venv
(wraps `uv add`). Package specs follow uv's format:
`name`, `name@version`, `name>=1.2`, etc.

```sh
toolr project venv add --help
```

```text
--8<-- "docs/cli-files/project-venv-add-help.txt"
```

### `toolr project venv remove` {#project-venv-remove}

Remove one or more packages from `tools/pyproject.toml` and sync the
venv (wraps `uv remove`). The package must already appear in
`tools/pyproject.toml` — passing an undeclared name is an error.

```sh
toolr project venv remove --help
```

```text
--8<-- "docs/cli-files/project-venv-remove-help.txt"
```

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

### `toolr project venv run` {#project-venv-run}

Run a command inside the managed tools venv. By default it syncs the venv
first (freshness-gated, exactly like [`venv sync`](#project-venv-sync) and
`venv shell`), then runs the command with `$VIRTUAL_ENV`, `$TOOLR_VENV`, and
`$PATH` set so entry points such as `pytest` resolve from the venv. The child's
stdout, stderr, and exit code pass straight through.

This is the one-liner for running a command-package's tests:

```sh
toolr project venv run -- pytest tools/
```

`--no-sync` never touches the venv — it errors if the venv is missing or stale
instead of syncing. Use it in CI once the venv is known-synced, for
deterministic runs. toolr's own flags must come before the command (or a `--`).

```sh
toolr project venv run --help
```

```text
--8<-- "docs/cli-files/project-venv-run-help.txt"
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

Generate a `toolr-manifest.json` fragment for a third-party package by
AST-walking the installed package source. Pure Rust — no Python
subprocess, no `pip install -e .` required.

**Flags:**

- `<package>` — looked up in the project's tools venv. Mutually
  exclusive with `--source-dir`.
- `--source-dir PATH` — point the tool at a package's source tree
  directly (bypasses the venv lookup). Requires `--package` if the
  leaf directory name isn't the desired package name.
- `--package PKG` — package name to embed in the fragment when using
  `--source-dir`.
- `--check` — verify the on-disk manifest matches what regeneration
  would produce; exit `2` with a unified diff on drift.
- `--output PATH` — write to a specific file instead of the
  package's default location.
- `--schema-version N` — pin the emitted `toolr_schema_version`.

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
by `toolr project venv sync` after a consent flow.
