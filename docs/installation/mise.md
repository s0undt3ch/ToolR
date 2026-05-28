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

```sh
# Install whatever the latest GitHub release is
mise use --global aqua:s0undt3ch/ToolR@latest

# Install (and pin) a specific version
mise use --global aqua:s0undt3ch/ToolR@0.20.0

# Pin a version to the current directory
mise use aqua:s0undt3ch/ToolR@0.20.0
```

Verify:

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
mise use --global aqua:s0undt3ch/ToolR@latest
```

The aqua backend installs the **same standalone binary** the
in-tree plugin used to fetch (the GitHub release archives), so the
runtime behaviour is identical.
