# mise

[mise](https://mise.jdx.dev/) is a polyglot tool-version manager. The
`toolr` repository ships an asdf-style plugin under `installation/mise/`
that lets mise install and manage `toolr` binaries.

## Why

- **Version pinning per project.** Different repos can use different
  `toolr` versions without manual `PATH` juggling.
- **Reproducibility for teams.** Commit a `.mise.toml` or
  `.tool-versions` and everyone on the project ends up on the same
  `toolr` release.
- **Multi-version side-by-side.** Install several releases at once;
  switch with `mise use toolr@X.Y.Z`.
- **CI-tested.** The plugin is exercised by
  [`install-smoke.yml`](https://github.com/s0undt3ch/ToolR/blob/main/.github/workflows/install-smoke.yml)
  on every release, so the version you install is the version we tested.

## Install the plugin

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
```

The `#installation/mise` suffix tells mise to use the asdf-plugin layout
inside the `installation/mise/` subdirectory of the toolr repo.

## Install toolr

```sh
# Install a specific version
mise install toolr@0.11.0

# Install whatever the latest GitHub release is
mise install toolr@latest

# Use a version globally
mise use --global toolr@0.11.0

# Pin a version to the current directory
mise use toolr@0.11.0
```

Verify:

```sh
toolr --version
```

## Project configuration

### `.mise.toml` (recommended)

```toml
[tools]
toolr = "0.11.0"
```

Then run `mise install` from the project root. mise resolves the version
from `.mise.toml` and installs it on demand.

### `.tool-versions` (asdf-style, legacy)

```text
toolr 0.11.0
```

mise also reads asdf's `.tool-versions` files, so existing asdf users
can keep their pin format unchanged.

## Combining with mise tasks

`mise` can run repo-scoped tasks. Once `toolr` is on PATH via the
plugin, wire it into tasks like any other binary:

```toml
[tools]
toolr = "0.11.0"

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
mise list-all toolr

# List locally installed versions
mise list toolr

# Show the active version in the current directory
mise current toolr

# Show the install dir for a version
mise where toolr

# Uninstall a version
mise uninstall toolr@0.11.0
```

## Troubleshooting

### `toolr: command not found`

Make sure mise's shim/activate hook is wired into your shell:

```sh
eval "$(mise activate bash)"   # or zsh / fish
```

Or invoke through mise directly:

```sh
mise exec toolr -- toolr --help
```

### Debug an install

```sh
MISE_DEBUG=1 mise install toolr@0.11.0
```

### Reinstall

```sh
mise uninstall toolr@0.11.0
mise install toolr@0.11.0
```

## Plugin internals

The plugin's source is in
[`installation/mise/`](https://github.com/s0undt3ch/ToolR/tree/main/installation/mise)
inside the toolr repo. The `bin/` directory contains the asdf-style
hooks (`list-all`, `download`, `install`) that mise invokes:

1. **`list-all`** queries the GitHub releases API for available
   `toolr` versions.
2. **`download`** fetches the archive for the host's target triple.
3. **`install`** extracts the binary into the mise-managed install
   directory.

The plugin installs the standalone `toolr` binary — the same artifact
shipped by [`installation/install.sh`](https://github.com/s0undt3ch/ToolR/blob/main/installation/install.sh)
and the GitHub release archives.
