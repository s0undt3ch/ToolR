# Installation

`toolr` ships as **two complementary PyPI packages** that live in
different venvs. Install the CLI binary once, globally, and let
`toolr project init` scaffold the per-repo tools venv that pulls in
the Python runtime.

## Two wheels, two roles

| Package    | What it is                                   | Where it lives                  |
| ---------- | -------------------------------------------- | ------------------------------- |
| `toolr`    | The Rust CLI binary you run from the shell.  | On `$PATH`, installed once.     |
| `toolr-py` | The Python runtime your `tools/*.py` import. | In your `tools/pyproject.toml`. |

Most projects want both: the CLI installed globally, `toolr-py`
declared in the per-repo `tools/pyproject.toml` so
`from toolr import Context, command_group` works when your commands
run. The CLI shells out into the tools venv at execute time; it
doesn't share its own venv with your tool scripts.

## Install

Five first-class install paths.

### mise

```sh
mise use packslip:github.com/s0undt3ch/ToolR@latest
```

Installs toolr from its signed [packslip](https://packslip.dev/)
manifest via mise's packslip backend — no plugin to register. mise
verifies the release against the ToolR repository's signing identity
and checks the digest and size of the downloaded archive. Pin the
signer to ToolR's release workflow with
[`workflow = "release.yml@refs/heads/main"`](mise.md#pin-the-release-workflow).
For
projects that already pin tool versions via `.mise.toml`, this
is the most-natural fit — toolr's version becomes part of your
project's reproducible tool set. Releases before 0.34.0
have no manifest to install from; use
`mise use aqua:s0undt3ch/ToolR@<version>` for those. See the
dedicated [mise](mise.md) page for `.mise.toml` / `.tool-versions`
integration and task-runner examples.

### pip

```sh
pip install toolr   # Rust CLI binary
```

This installs the `toolr` binary into whatever venv `pip` is
pointing at. The wheel has no Python source and no `import toolr` —
that's what `toolr-py` is for (see "Two wheels, two roles" above).
**Do not `pip install toolr-py`** into that same venv — `toolr-py`
belongs in the per-repo tools venv that `toolr project init`
scaffolds for you. `python -m toolr` was removed in the rust
front-end rewrite; use the `toolr` executable instead.

### curl | sh (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
```

Verifies the SLSA attestation by default (requires the `gh` CLI, 2.56
or later; pass `--verify-attestation=skip` to bypass). See
[Supply-chain verification](#supply-chain-verification-slsa-attestations).
Pin a version
with `sh -s -- --version X.Y.Z`. Custom prefix:
`sh -s -- --prefix /opt/toolr/bin`. Default prefix is
`$XDG_BIN_HOME` (or `~/.local/bin`).

### PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
```

Verifies the SLSA attestation the same way as `install.sh` (requires
`gh` 2.56 or later; pass `-VerifyAttestation skip` to bypass).

### GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
Windows) from <https://github.com/s0undt3ch/ToolR/releases>, verify
the `.sha256` sibling and the SLSA attestation, drop the binary on
`$PATH`. Useful in locked-down environments that audit binaries
before allowing them on a machine.

## Scaffold your repo

After the binary is on `$PATH`:

```sh
toolr project init                  # writes tools/{pyproject.toml,.gitignore,example.py}
toolr example hello                 # run the generated example
toolr self completion install bash  # or zsh / fish
```

`toolr project init` writes a `tools/pyproject.toml` with
`toolr-py` already declared and runs `uv sync` to materialise the
venv. From here, [Quickstart](../quickstart.md) walks through your
first command edit.

## Requirements

- **Python 3.11 or later** at execute time. The toolr binary itself
  is standalone, but every user command runs inside a Python
  subprocess.
- **[uv](https://docs.astral.sh/uv/)** 0.4.0 or later to materialise
  the tools venv (`uv sync`). If toolr can't find it, it offers to
  install a managed copy. See [uv and Python](#uv-and-python).

## uv and Python {#uv-and-python}

Toolr doesn't pick the tools venv's Python interpreter. It hands the
venv to uv, and uv picks the interpreter.

### Finding uv

Toolr uses the first uv it finds:

1. `uv` in an absolute `PATH` directory. Toolr skips empty and relative
   `PATH` entries (`.`, `bin`, `~/bin`), so a `uv` inside the repository
   never runs by accident.
2. A toolr-managed copy at `$XDG_DATA_HOME/toolr/bin/uv` (`uv.exe` on
   Windows). When `XDG_DATA_HOME` is unset, toolr uses the platform data
   directory: `~/.local/share` on Linux, `~/Library/Application Support`
   on macOS and `%APPDATA%` on Windows.

A uv on `PATH` that toolr can't use is an error: one older than 0.4.0,
one that won't run, or one whose version toolr can't read. Toolr doesn't
fall back to the managed copy, so fix that uv or take it off `PATH`.

### When uv is missing

If toolr finds no uv, it asks whether to download one to the managed
path or leave you to install it yourself. Press Enter or `I` to
install. Any other answer stops toolr with an error.

Toolr only asks when stdin is a terminal. With `--quiet`, at the root
(`toolr --quiet …`) or on a subcommand, it never asks and fails with
an error instead. The one exception is `toolr project venv sync
--quiet`, which exits silently so shell-enter hooks don't break. In CI,
in pipes and in other non-interactive runs toolr also fails, unless you
approve the install in advance:

- Set `TOOLR_AUTO_INSTALL_UV=1` to install without asking. It also
  answers the sync prompt after
  [repository trust](../concepts.md#repository-trust).
- Run [`toolr __install-uv-now`](../cli.md#__install-uv-now), for
  example in a CI setup step. It installs the managed copy when no uv
  is found, and does nothing when a usable one already is.

Toolr downloads prebuilt uv binaries for Linux (x86_64 and aarch64),
macOS (x86_64 and Apple silicon) and Windows (x86_64). On other hosts,
install uv yourself. Toolr unpacks the download itself, so the host
needs no `tar` or `unzip`.

Before unpacking, toolr checks the download against the SHA-256
checksum published with the same uv release. If the checksum is
missing or doesn't match, toolr installs nothing and stops with an
error. Install uv yourself in that case. The checksum catches a
corrupted or altered download, not a compromised uv release.

On Linux, toolr downloads the glibc or musl build of uv to match the
host. If it guesses wrong, set `TOOLR_UV_LIBC=gnu` or
`TOOLR_UV_LIBC=musl`.

### Choosing Python

- If `tools/pyproject.toml` sets
  [`[tool.toolr] python-version`](../project-config.md#python-version),
  toolr passes it to uv as `--python`.
- Otherwise toolr passes no Python request. uv picks an interpreter
  that satisfies `[project] requires-python`, using
  [its own discovery rules](https://docs.astral.sh/uv/concepts/python-versions/).
  Those rules consider uv-managed and system interpreters, and uv can
  download one when nothing installed fits.

uv runs against the `tools/` project, so it honours a
`tools/.python-version` file but not one at the repo root. uv settings
such as `UV_PYTHON`, `UV_PYTHON_PREFERENCE` and `UV_PYTHON_DOWNLOADS`
apply as usual.

Editing `tools/pyproject.toml`, `tools/uv.toml` or
`tools/.python-version` marks the venv out of date, the same as a changed `tools/uv.lock`, so the next
`toolr` command re-syncs it. With the default `venv-location`, the
Python version is part of the venv's cache path. Changing
`python-version`, or `requires-python` when `python-version` is unset,
therefore points toolr at a new venv that doesn't exist yet. Run
`toolr project venv sync` to create it.

`UV_*` environment variables and a user-level `uv.toml` aren't
tracked. After changing them, rebuild the
venv with `toolr project venv sync --force`.

Commands always run in the tools venv; a repo needs
`tools/pyproject.toml` to have one. The only host Python toolr runs is
the `python3` that `toolr project init` asks once to pick the default
`requires-python` it writes.

## Adding `toolr-py` manually

`toolr project init` is the fast path — it declares `toolr-py` for
you. If you'd rather wire it by hand (e.g. you're slotting toolr
into an existing `tools/pyproject.toml`), add it to your project's
tools venv:

```toml title="tools/pyproject.toml"
[project]
name = "my-project-tools"
requires-python = ">=3.11"
dependencies = [
    "toolr-py",
]
```

`uv sync` (or whatever resolver you use for `tools/`) will pull it
in. The `toolr` CLI on PATH will then find `import toolr` inside
your tools venv when it executes commands.

!!! note "Why two packages?"
    The CLI is a Rust binary built with `maturin --bindings bin`,
    which produces a Python-version-independent wheel that ships
    only the executable. The runtime users `import` is a separate
    pyo3-extension wheel built for each (CPython, ABI) pair.
    Splitting them lets the CLI be one global install while the
    runtime tracks each project's tools venv.

## Supply-chain verification (SLSA attestations)

Every release archive is signed with a SLSA build-provenance
attestation produced by the GitHub-hosted release workflow. The
`install.sh` and `install.ps1` scripts verify the attestation **by
default** (`--verify-attestation=require`), which needs the `gh` CLI,
2.56 or later (older `gh` can't load GitHub's current Sigstore trusted
root). If `gh` is missing the install **fails** rather than silently
downgrading to a checksum-only install (the `.sha256` ships from the same
release, so it catches transport corruption, not a tampered release
asset). With `--verify-attestation=auto` (`-VerifyAttestation auto`), a
missing `gh` skips the check with a warning; an outdated one still fails.

The check accepts an archive only when all of these hold:

- The attestation was signed by ToolR's archive-building workflow,
  `_build-binary-archive.yml`, running on `main` on a GitHub-hosted
  runner.
- That run was started by the release workflow, `release.yml`, on
  `main`. CI calls the same archive-building workflow, so this is what
  tells a release from a CI build of some branch.
- One subject of the attestation carries both the archive's exact file
  name and its SHA-256, so a signed archive of another version or
  platform can't be renamed into place.

The Setup ToolR GitHub Action applies the same rules.

`TOOLR_REPO` (`install.sh`) and `-Repo` (`install.ps1`) move the whole
trust anchor to another repository: the downloads, and the workflows the
attestation must come from. Only set them for a fork you trust.

To install without supply-chain verification (accepting the risk — e.g.
on a host where you can't install `gh`):

```sh
sh installation/install.sh --verify-attestation=skip   # POSIX
./installation/install.ps1 -VerifyAttestation skip     # Windows
```

Or verify a downloaded archive manually, from the directory you
downloaded it to:

```sh
export ARCHIVE_NAME=toolr-1.2.3-aarch64-apple-darwin.tar.gz
export RELEASE_WORKFLOW=https://github.com/s0undt3ch/ToolR/.github/workflows/release.yml@refs/heads/main
export ARCHIVE_SHA256=$(sha256sum < "$ARCHIVE_NAME" | cut -d' ' -f1)    # Linux
# export ARCHIVE_SHA256=$(shasum -a 256 < "$ARCHIVE_NAME" | cut -d' ' -f1)  # macOS
gh attestation verify "$ARCHIVE_NAME" \
  --repo s0undt3ch/ToolR \
  --cert-identity https://github.com/s0undt3ch/ToolR/.github/workflows/_build-binary-archive.yml@refs/heads/main \
  --deny-self-hosted-runners \
  --format json \
  --jq '[.[].verificationResult
         | select(.signature.certificate.buildConfigURI == env.RELEASE_WORKFLOW
                  and any(.statement.subject[]; .name == env.ARCHIVE_NAME and .digest.sha256 == env.ARCHIVE_SHA256))]
        | length'
```

It prints `1` for a release archive. A non-zero exit means `gh` found no
valid attestation from ToolR's archive-building workflow on `main`. `0`
means the archive has one, but not from a release build of that exact
file: a CI build, or another release's archive renamed. `gh attestation
verify --repo s0undt3ch/ToolR` alone accepts both. `0` also appears when
`ARCHIVE_SHA256` is empty; check it with `echo $ARCHIVE_SHA256`. The
command is for a POSIX shell; on Windows, use Git Bash or WSL, or let
`install.ps1` do the check.

## Verifying the install

```sh
toolr --version
```

```sh
toolr --help
```

```text
--8<-- "docs/quickstart-files/toolr-help.txt"
```

## Development install

```sh
git clone https://github.com/s0undt3ch/ToolR.git
cd ToolR
uv sync --dev
cargo build --release --bin toolr
```

The `target/release/toolr` binary is what you'll exercise during
development; put `target/release/` on your PATH (or alias `toolr`
to it) while iterating.
