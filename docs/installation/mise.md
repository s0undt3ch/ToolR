# mise

[mise](https://mise.jdx.dev/) is a polyglot tool-version manager. Every
toolr release from 0.34.0 onward ships a signed
[packslip](https://packslip.dev/) manifest, and mise's packslip backend
installs directly from it — no plugin to register, no repository to
clone. Releases before 0.34.0 predate packslip support and use mise's aqua
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
  of the archive for your platform before unpacking it. Pin that
  workflow from the first install with the
  [`workflow` option](#pin-the-release-workflow).

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

### Pin the release workflow

Pin the workflow that signs ToolR's packslip with mise's per-tool
`workflow` option (mise 2026.10.5 or later):

```toml
[tools]
"packslip:github.com/s0undt3ch/ToolR" = { version = "X.Y.Z", workflow = "release.yml@refs/heads/main" }
```

Without it, mise accepts a packslip signed by any workflow in the ToolR
repository on the first install, then remembers that signer. With it,
mise accepts only a packslip signed by `release.yml` running on `main`
in `s0undt3ch/ToolR`, from the first install on, and refuses anything
else, such as a CI workflow or a copy of `release.yml` on another
branch. The ref is matched exactly.

Keep the `@refs/heads/main` suffix. A bare `workflow = "release.yml"`
means "`release.yml` running on a tag" to mise, and ToolR releases are
signed on `main`, so every install would fail with
`expected an identity starting with ".../release.yml@refs/tags/"`.

What the pin doesn't cover: which workflow built the archives. The
signed packslip lists each archive's digest and size, and mise checks
them, so you get exactly the archives `release.yml` signed for. But
packslip only links the archives' build provenance by digest, and
neither mise nor `packslip verify` checks it. To check that an archive
was built by ToolR's release workflow, verify it with
[`gh attestation verify`](index.md#supply-chain-verification-slsa-attestations).

### Aqua fallback

Releases before 0.34.0 have no packslip manifest to install
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
"packslip:github.com/s0undt3ch/ToolR" = { version = "X.Y.Z", workflow = "release.yml@refs/heads/main" }
```

`"packslip:github.com/s0undt3ch/ToolR" = "X.Y.Z"` works too, without
the [release workflow pin](#pin-the-release-workflow).

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

When `tools/uv.lock`, `tools/pyproject.toml`, `tools/uv.toml` or
`tools/.python-version` changes — say you've pulled a branch that
bumped a dependency — the next `toolr` command you run re-syncs the
tools venv before doing its real work. That sync only
happens when you actually run a `toolr` command, so the latency
shows up at an awkward moment, on a command you ran for a different
reason.

mise's `[hooks].enter` lets you run a command every time the shell
enters this project's directory, before you've typed anything else.
Wiring it to `toolr --quiet project venv sync --if-project` gives you a tools
venv that's already fresh by the time your prompt returns:

```toml
# In this project's mise.toml
[hooks]
enter = "toolr --quiet project venv sync --if-project"
```

That's the entire recipe. No `[tasks]` block, no
`sources`/`outputs` configuration — `toolr project venv sync`
honours its own freshness stamp internally, so when nothing has
changed it exits in tens of milliseconds without spawning uv. When
one of those files has changed, it runs `uv sync --quiet` exactly once and
updates the stamp.

The recipe works identically for every project, regardless of
whether `[tool.toolr] venv-location` is `cache` (the default, under
`$XDG_CACHE_HOME/toolr/<repo-key>/venv/`) or `in-tree`
(`tools/.venv/`) — the freshness stamp lives inside the venv either
way, and the recipe never hard-codes a venv path.

### Unattended-mode guards

`--quiet` suppresses output and tells `toolr` it's running in an unattended
context where blocking on a TTY prompt would freeze the shell. It never turns an
error into a success. `--if-project` does that, exiting 0 silently in three
benign situations:

- The current directory isn't a toolr-using repo (no
  `tools/pyproject.toml`). The hook fires on every `cd` even into
  non-toolr directories; this is normal, not an error.
- toolr can't read where the tools venv lives in a trusted repository,
  for example from a `tools/pyproject.toml` that doesn't parse.
- `uv` isn't installed and `TOOLR_AUTO_SYNC=1` isn't set.
  The hook can't reasonably prompt for consent on every shell
  enter, so it stays out of the way.

Genuine failures — an unparsable lock file, a `uv sync` that exits
non-zero — still print their error to stderr and exit non-zero, so
they aren't silently masked. So does an untrusted repository: the hook
never prompts, so it prints the
[repository trust](../concepts.md#repository-trust) refusal until you
trust the repository.

### First-time bootstrap

The first time you set up a project, run `toolr project trust` once
**without** `--quiet`. It trusts the repository, then asks whether to
sync the tools venv; a `y` installs uv if needed and materialises the
venv. From then on the enter-hook keeps it fresh:

```sh
toolr project trust         # one-time, interactive
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
releases before 0.34.0. Migrate with:

```sh
mise plugin uninstall toolr
mise use packslip:github.com/s0undt3ch/ToolR@latest         # per-project
# or:
mise use -g packslip:github.com/s0undt3ch/ToolR@latest   # machine-wide
```

Both backends install the **same standalone binary** the in-tree
plugin used to fetch (the GitHub release archives), so the runtime
behaviour is identical.
