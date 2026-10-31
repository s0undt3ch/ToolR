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
and checks the digest and size of the downloaded archive. For
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

Verifies the SLSA attestation by default (requires the `gh` CLI; pass
`--verify-attestation=skip` to bypass). Pin a version
with `sh -s -- --version X.Y.Z`. Custom prefix:
`sh -s -- --prefix /opt/toolr/bin`. Default prefix is
`$XDG_BIN_HOME` (or `~/.local/bin`).

### PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
```

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

1. `uv` on `PATH`.
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

- Set `TOOLR_AUTO_INSTALL_UV=1` to install without asking.
- Run [`toolr __install-uv-now`](../cli.md#__install-uv-now), for
  example in a CI setup step. It installs the managed copy when no uv
  is found, and does nothing when a usable one already is.

Toolr downloads prebuilt uv binaries for Linux (x86_64 and aarch64),
macOS (x86_64 and Apple silicon) and Windows (x86_64). On other hosts,
install uv yourself. Toolr unpacks the download itself, so the host
needs no `tar` or `unzip`.

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

Editing `tools/pyproject.toml` or `tools/.python-version` marks the
venv out of date, the same as a changed `tools/uv.lock`, so the next
`toolr` command re-syncs it. With the default `venv-location`, the
Python version is part of the venv's cache path. Changing
`python-version`, or `requires-python` when `python-version` is unset,
therefore points toolr at a new venv that doesn't exist yet. Run
`toolr project venv sync` to create it.

The `UV_*` settings aren't tracked. After changing them, rebuild the
venv with `toolr project venv sync --force`.

Two cases don't go through uv:

- A repo with no `tools/pyproject.toml` has no tools venv. Toolr runs
  commands with `$TOOLR_PYTHON` if set, else `python3`, else `python`
  from `PATH`. `$TOOLR_PYTHON` takes an absolute path or a bare name,
  which toolr looks up on `PATH`. A `PATH` interpreter older than
  Python 3.11 is skipped. The interpreter must have `toolr-py` installed.
- `toolr project init` runs the host `python3` once to pick the default
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
default** (`--verify-attestation=require`), which needs the `gh` CLI: if
`gh` is missing the install **fails** rather than silently downgrading to
a checksum-only install (the `.sha256` ships from the same release, so it
catches transport corruption, not a tampered release asset).

To install without supply-chain verification (accepting the risk — e.g.
on a host where you can't install `gh`):

```sh
sh installation/install.sh --verify-attestation=skip   # POSIX
./installation/install.ps1 -VerifyAttestation skip     # Windows
```

Or verify a downloaded archive manually:

```sh
gh attestation verify toolr-1.2.3-aarch64-apple-darwin.tar.gz \
  --repo s0undt3ch/ToolR
```

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
