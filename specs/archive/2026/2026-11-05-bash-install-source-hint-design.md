# bash install source hint: design

Follow-up FU-19.

## Problem

`toolr self completion install bash` writes the script to
`$XDG_DATA_HOME/bash-completion/completions/toolr`, falling back to
`~/.local/share/bash-completion/completions/toolr`
(`crates/toolr-core/src/complete/install.rs::install_path_for`). Only the
bash-completion package's lazy loader reads that directory. Since #673 the
script itself no longer needs bash-completion, so a user without the package
gets a script that is installed but never loaded. The installer says nothing
about this; only `docs/cli.md` tells the user to source the script from
`~/.bashrc`.

## Decision

When the installer can't find bash-completion, it prints one extra line
telling the user to add `source <installed path>` to their bash startup file
(`~/.bashrc`; login shells such as macOS Terminal's read `~/.bash_profile`,
which the docs and release note mention).

## Detecting bash-completion

The binary runs outside bash, so it can't ask the shell. It probes for the
package's main script, the file `bash_completion` (or the `profile.d` loader
`bash_completion.sh`), never the `bash-completion/` or `completions/`
directories. Other packages create `share/bash-completion/completions/` for
their own scripts without installing the loader, and a directory check would
hide the hint on exactly the host that needs it.

Only bash-completion 2.x loads `~/.local/share/bash-completion/completions/`;
1.x never reads it, so 1.x loaders (`usr/local/etc/bash_completion`,
`opt/homebrew/etc/bash_completion`) are not candidates and a host with only
1.x gets the hint.

Candidates, each relative to a sysroot (`/` in production):

- `usr/share/bash-completion/bash_completion` (most Linux distributions, MSYS2)
- `etc/bash_completion` (a 2.x compatibility shim on Debian and Ubuntu)
- `usr/local/share/bash-completion/bash_completion`
- `usr/local/etc/profile.d/bash_completion.sh` (Intel macOS package managers)
- `opt/homebrew/share/bash-completion/bash_completion`,
  `opt/homebrew/etc/profile.d/bash_completion.sh` (Apple silicon macOS)
- `home/linuxbrew/.linuxbrew/share/bash-completion/bash_completion`,
  `home/linuxbrew/.linuxbrew/etc/profile.d/bash_completion.sh`
- `opt/local/share/bash-completion/bash_completion`,
  `opt/local/etc/profile.d/bash_completion.sh` (MacPorts)

Plus, outside the sysroot:

- `<dir>/bash-completion/bash_completion` for each non-empty `$XDG_DATA_DIRS`
  entry. This covers Nix profiles and any other prefix that publishes its
  data directory there.
- `$HOMEBREW_PREFIX/share/bash-completion/bash_completion` and
  `$HOMEBREW_PREFIX/etc/profile.d/bash_completion.sh`, for custom prefixes.

bash-completion counts as installed if any candidate is a file. Anything not
listed (a custom prefix with no environment hint, a native Windows binary
looking at an MSYS2 or Git for Windows tree) reads as "not installed", so
the hint prints. That is the safe direction: the hint is one line and
following it does no harm even when bash-completion is present, because the
script guards against double registration only as far as `complete -F` goes,
which simply re-registers the same function.

### Testability

The pure layer in `toolr-core` takes the sysroot, the `XDG_DATA_DIRS` value
and the Homebrew prefix as arguments and reads no environment, so unit tests
run over a `TempDir`. The binary fills them from the environment. GitHub's
Ubuntu runners ship bash-completion, so an integration test of the "hint
appears" case needs to move the sysroot too: the binary reads
`TOOLR_BASH_COMPLETION_SYSROOT` (undocumented test seam, like
`TOOLR_NO_CACHE_HINT`) and defaults to `/`. An empty value counts as unset,
so it never probes relative to the working directory.

## Output

- Stream: stdout, like the `wrote …` line and the zsh `fpath` hint that
  follows it.
- When: bash only, after `Wrote` and after `AlreadyInstalled`. A re-run is
  typically how a user debugs "installed but nothing happens", so the hint
  shows there too. Never after `SkippedNeedsForce`, which is an error.
- `--quiet`: the root `toolr --quiet` flag suppresses the hint. `run_self`
  doesn't see root matches today; the flag is threaded through the way
  `project` gets it. The existing `wrote …` line keeps its current behaviour,
  so this change doesn't widen scope.
- Text: `toolr: can't find bash-completion, so bash won't load this script
  by itself; add `source <path>` to your bash startup file (~/.bashrc).` The
  path is single-quoted when it holds anything outside a conservative safe
  set (whitespace, quotes, `$`, backslashes, …). The quoting helper,
  `toolr_core::complete::shell_quote`, is public so the integration tests
  build their expectations through it. No package manager is named: the command runs
  on Linux, macOS and Windows.

## Docs

`docs/cli.md` under `self completion install`: say to source the script when
bash-completion is missing or isn't enabled, that the installer prints the
hint when it can't find the package, and that login shells (macOS Terminal)
read `~/.bash_profile` rather than `~/.bashrc`; make `source <installed path>`
the primary advice with `source <(toolr self completion print bash)` as the
alternative. No doc snippet captures an install run, so
`regen-doc-snippets` should be a no-op.
