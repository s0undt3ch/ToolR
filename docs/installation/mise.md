# mise

[mise](https://mise.jdx.dev/) is a polyglot tool-version manager. Every
toolr release ships a signed
[packslip](https://packslip.dev/) manifest, and mise's packslip backend
installs directly from it — no plugin to register, no repository to
clone. Releases published before packslip support use mise's aqua
backend instead; see "Aqua fallback" below.

## Why

- **Version pinning per project.** Different repos can use different
  `toolr` versions without manual `PATH` juggling.
- **Reproducibility for teams.** Commit a `.mise.toml` or
  `.tool-versions` and everyone on the project ends up on the same
  `toolr` release.
- **Multi-version side-by-side.** Install several releases at once;
  switch with `mise use packslip:github.com/s0undt3ch/ToolR@X.Y.Z`.
- **Supply-chain verified.** mise verifies the release against the
  ToolR repository's signing identity, remembering the release
  workflow after the first install, and checks the digest and size
  of the archive for your platform before unpacking it.

## Install toolr

### Pin per project (recommended)

`mise use` without a scope flag writes to the current directory's
`.mise.toml`. Run it inside the repo you're scaffolding toolr for:

```sh
# Latest release
mise use packslip:github.com/s0undt3ch/ToolR@latest

# Pin a specific version
mise use packslip:github.com/s0undt3ch/ToolR@X.Y.Z
```

This is the form the README and quickstart show. It matches
toolr's design as a project-level tool — every repo declares its
own `toolr` version, so `.mise.toml` is the single source of truth
for "which toolr does this project run with?".

mise defaults to a 24-hour minimum release age when selecting new
versions (configurable per tool with `minimum_release_age`; set it to
`"0"` on the tool to bypass), so a version tagged moments ago may not
resolve immediately — that's expected, not a broken pin.

### Install machine-wide

If you'd rather have one `toolr` available across every directory
without per-project pinning, add `-g`:

```sh
mise use -g packslip:github.com/s0undt3ch/ToolR@latest
```

`-g` (`--global`) writes to `~/.config/mise/config.toml` (or whatever
mise resolves for your platform). Per-project `.mise.toml` pins
still override the global entry when present, so this is a safe
"have toolr on PATH everywhere" knob — it just clutters the
global config with a tool you mostly use inside specific repos.

### Verify

```sh
toolr --version
```

### Aqua fallback

Releases before packslip support have no packslip manifest to install
from. For those, use mise's aqua backend, unchanged from before
packslip landed:

```sh
mise use aqua:s0undt3ch/ToolR@<version>
```

The aqua registry entry pulls the same signed GitHub release archives
with SHA-256 verification built in; it just can't check the packslip
signature a pre-packslip release never published.

## Project configuration

### `.mise.toml` (recommended)

```toml
[tools]
"packslip:github.com/s0undt3ch/ToolR" = "X.Y.Z"
```

Then run `mise install` from the project root. mise resolves the
version from `.mise.toml` and installs it on demand.

### `.tool-versions` (asdf-style, legacy)

```text
packslip:github.com/s0undt3ch/ToolR X.Y.Z
```

mise also reads asdf's `.tool-versions` files, so existing asdf
users can keep their pin format unchanged.

## Combining with mise tasks

`mise` can run repo-scoped tasks. Once `toolr` is on PATH via the
packslip backend, wire it into tasks like any other binary:

```toml
[tools]
"packslip:github.com/s0undt3ch/ToolR" = "X.Y.Z"

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
mise ls-remote packslip:github.com/s0undt3ch/ToolR

# List locally installed versions
mise ls packslip:github.com/s0undt3ch/ToolR

# Show the active version in the current directory
mise current packslip:github.com/s0undt3ch/ToolR

# Show the install dir for a version
mise where packslip:github.com/s0undt3ch/ToolR

# Uninstall a version
mise uninstall packslip:github.com/s0undt3ch/ToolR@X.Y.Z
```

## Troubleshooting

### `toolr: command not found`

Make sure mise's shim/activate hook is wired into your shell:

```sh
eval "$(mise activate bash)"   # or zsh / fish
```

Or invoke through mise directly:

```sh
mise exec packslip:github.com/s0undt3ch/ToolR -- toolr --help
```

### `no aqua-registry found for s0undt3ch/ToolR`

This applies to the aqua fallback only. mise's aqua backend resolves
entries against the latest published aqua-registry release, not
against `main`. If the entry was added recently it may not yet be in
a release tag. Check
[aqua-registry releases](https://github.com/aquaproj/aqua-registry/releases)
and bump mise (or wait for its registry cache to refresh) once a
release containing the entry has shipped.

### Debug an install

```sh
mise --verbose use packslip:github.com/s0undt3ch/ToolR
```

### Reinstall

```sh
mise uninstall packslip:github.com/s0undt3ch/ToolR@X.Y.Z
mise install packslip:github.com/s0undt3ch/ToolR@X.Y.Z
```

## Migrating from the in-tree plugin

Earlier toolr revisions shipped an asdf-style plugin at
`installation/mise/` that was installed via
`mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise`.
That plugin has been **removed** in favour of the backends described
above: packslip for current releases, aqua as the fallback for
releases before packslip support. Migrate with:

```sh
mise plugin uninstall toolr
mise use packslip:github.com/s0undt3ch/ToolR@latest         # per-project
# or:
mise use -g packslip:github.com/s0undt3ch/ToolR@latest   # machine-wide
```

Both backends install the **same standalone binary** the in-tree
plugin used to fetch (the GitHub release archives), so the runtime
behaviour is identical.
