# mise

[mise](https://mise.jdx.dev/) is a polyglot tool-version manager.
`toolr` is published in the
[aqua registry](https://github.com/aquaproj/aqua-registry/tree/main/pkgs/s0undt3ch/ToolR)
and mise installs it directly via its built-in aqua backend — no
plugin to register, no repository to clone.

## Why

- **Version pinning per project.** Different repos can use different
  `toolr` versions without manual `PATH` juggling.
- **Reproducibility for teams.** Commit a `.mise.toml` or
  `.tool-versions` and everyone on the project ends up on the same
  `toolr` release.
- **Multi-version side-by-side.** Install several releases at once;
  switch with `mise use toolr@X.Y.Z`.
- **Supply-chain verified.** The aqua registry entry pulls the
  signed GitHub release archives with SHA-256 verification built in.

## Install toolr

### Pin per project (recommended)

`mise use` without a scope flag writes to the current directory's
`.mise.toml`. Run it inside the repo you're scaffolding toolr for:

```sh
# Latest release
mise use aqua:s0undt3ch/ToolR@latest

# Pin a specific version
mise use aqua:s0undt3ch/ToolR@0.20.0
```

This is the form the README and quickstart show. It matches
toolr's design as a project-level tool — every repo declares its
own `toolr` version, so `.mise.toml` is the single source of truth
for "which toolr does this project run with?".

### Install machine-wide

If you'd rather have one `toolr` available across every directory
without per-project pinning, add `--global`:

```sh
mise use --global aqua:s0undt3ch/ToolR@latest
```

`--global` writes to `~/.config/mise/config.toml` (or whatever
mise resolves for your platform). Per-project `.mise.toml` pins
still override the global entry when present, so this is a safe
"have toolr on PATH everywhere" knob — it just clutters the
global config with a tool you mostly use inside specific repos.

### Verify

```sh
toolr --version
```

## Project configuration

### `.mise.toml` (recommended)

```toml
[tools]
"aqua:s0undt3ch/ToolR" = "0.20.0"
```

Then run `mise install` from the project root. mise resolves the
version from `.mise.toml` and installs it on demand.

### `.tool-versions` (asdf-style, legacy)

```text
aqua:s0undt3ch/ToolR 0.20.0
```

mise also reads asdf's `.tool-versions` files, so existing asdf
users can keep their pin format unchanged.

## Combining with mise tasks

`mise` can run repo-scoped tasks. Once `toolr` is on PATH via the
aqua backend, wire it into tasks like any other binary:

```toml
[tools]
"aqua:s0undt3ch/ToolR" = "0.20.0"

[tasks.test]
description = "Run tests"
run = "toolr test run"

[tasks.lint]
description = "Run linters"
run = "toolr lint check"

[tasks.ci]
description = "Run CI checks"
depends = ["lint", "test"]
```

Then:

```sh
mise run ci
```

## Auto-sync the tools venv on shell-enter

When `tools/pyproject.toml` or `tools/uv.lock` changes — say you've
pulled a branch that bumped a dependency — `toolr`'s next invocation
re-syncs the tools venv before doing its real work. That sync only
happens when you actually run a `toolr` command, so the latency
shows up at an awkward moment, on a command you ran for a different
reason.

mise's `[hooks].enter` lets you run a command every time the shell
enters this project's directory, before you've typed anything else.
Wiring it to `toolr project venv sync --quiet` gives you a tools
venv that's already fresh by the time your prompt returns:

```toml
# In this project's mise.toml
[hooks]
enter = "toolr project venv sync --quiet"
```

That's the entire recipe. No `[tasks]` block, no
`sources`/`outputs` configuration — `toolr project venv sync`
honours its own freshness stamp internally, so when nothing has
changed it exits in tens of milliseconds without spawning uv. When
the lock file has moved, it runs `uv sync --quiet` exactly once and
updates the stamp.

The recipe works identically for every project, regardless of
whether `[tool.toolr] venv-location` is `cache` (the default, under
`$XDG_CACHE_HOME/toolr/<repo-key>/venv/`) or `in-tree`
(`tools/.venv/`) — the freshness stamp lives inside the venv either
way, and the recipe never hard-codes a venv path.

### Unattended-mode guards

`--quiet` does more than suppress output: it also tells `toolr` that
it's running in an unattended context where blocking on a TTY prompt
would freeze the shell. To honour that, `--quiet` exits 0 silently
in three benign situations:

- The current directory isn't a toolr-using repo (no
  `tools/pyproject.toml`). The hook fires on every `cd` even into
  non-toolr directories; this is normal, not an error.
- `tools/uv.lock` is missing. The user probably hasn't run
  `toolr project init` yet — that's their next step, not something
  the hook should report.
- `uv` isn't installed and `TOOLR_AUTO_INSTALL_UV=1` isn't set.
  The hook can't reasonably prompt for consent on every shell
  enter, so it stays out of the way.

Genuine failures — an unparsable lock file, a `uv sync` that exits
non-zero — still print their error to stderr and exit non-zero, so
they aren't silently masked.

### First-time bootstrap

The first time you set up a project, run `toolr project venv sync`
once **without** `--quiet`. That's the run that will install uv if
needed (with a normal consent prompt) and materialise the venv. From
then on the enter-hook keeps it fresh:

```sh
toolr project venv sync     # one-time, interactive
cd ..; cd back              # enter-hook now keeps it fresh
```

### Project- vs. machine-scoped

The hook lives in the project's own `mise.toml`. It is **not** a
global setting — every project that wants this behaviour opts in by
adding the line. That keeps non-toolr projects free of unexpected
post-`cd` work.

## Common commands

```sh
# List all upstream versions
mise ls-remote aqua:s0undt3ch/ToolR

# List locally installed versions
mise ls aqua:s0undt3ch/ToolR

# Show the active version in the current directory
mise current aqua:s0undt3ch/ToolR

# Show the install dir for a version
mise where aqua:s0undt3ch/ToolR

# Uninstall a version
mise uninstall aqua:s0undt3ch/ToolR@0.20.0
```

## Troubleshooting

### `toolr: command not found`

Make sure mise's shim/activate hook is wired into your shell:

```sh
eval "$(mise activate bash)"   # or zsh / fish
```

Or invoke through mise directly:

```sh
mise exec aqua:s0undt3ch/ToolR -- toolr --help
```

### `no aqua-registry found for s0undt3ch/ToolR`

mise's aqua backend resolves entries against the latest published
aqua-registry release, not against `main`. If the entry was added
recently it may not yet be in a release tag. Check
[aqua-registry releases](https://github.com/aquaproj/aqua-registry/releases)
and bump mise (or wait for its registry cache to refresh) once a
release containing the entry has shipped.

### Debug an install

```sh
mise --verbose use aqua:s0undt3ch/ToolR
```

### Reinstall

```sh
mise uninstall aqua:s0undt3ch/ToolR@0.20.0
mise install aqua:s0undt3ch/ToolR@0.20.0
```

## Migrating from the in-tree plugin

Earlier toolr revisions shipped an asdf-style plugin at
`installation/mise/` that was installed via
`mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise`.
That plugin has been **removed** in favour of the aqua-backed install
described above. Migrate with:

```sh
mise plugin uninstall toolr
mise use aqua:s0undt3ch/ToolR@latest         # per-project
# or:
mise use --global aqua:s0undt3ch/ToolR@latest   # machine-wide
```

The aqua backend installs the **same standalone binary** the
in-tree plugin used to fetch (the GitHub release archives), so the
runtime behaviour is identical.
