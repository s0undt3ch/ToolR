# Installation

`toolr` is distributed as **two complementary PyPI packages** that
live in different venvs:

| Package    | What it is                                            | How you use it |
|------------|-------------------------------------------------------|----------------|
| `toolr`    | The Rust CLI binary — `toolr ...` on the shell.       | Install once, globally, on PATH (pip, install.sh, mise, release archive). |
| `toolr-py` | The Python runtime: `import toolr`, `Context`, `command_group`, `_rust_utils`. | Add to your project's `tools/pyproject.toml` so it resolves into the tools venv. |

A typical project setup uses **both**:

1. Install the `toolr` CLI on PATH once (any method below).
2. Add `toolr-py` to `tools/pyproject.toml` so `tools/*.py` scripts can
   `from toolr import Context, command_group`.

The CLI shells out into a per-project tools venv at execute time; it
doesn't share its own venv with your tool scripts.

## Requirements

- **Python 3.11 or later** is needed at execute time. The toolr binary
  itself is standalone, but every user command runs inside a Python
  subprocess.
- **[uv](https://docs.astral.sh/uv/)** is needed to materialise the
  tools venv (`uv sync`). If you don't have it on PATH, toolr can
  install a managed copy on first use.

## Install the CLI binary (`toolr`)

Pick whichever method matches your environment.

### `curl ... | sh` (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
```

Pass `--version X.Y.Z` after `sh -s --` to pin a specific release, or
`--prefix /custom/bin` to choose an install directory. Default prefix
is `$XDG_BIN_HOME` (or `~/.local/bin`).

### PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
```

### mise

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise use --global toolr@latest
```

The plugin source ships in this repo under `installation/mise/`. See
the dedicated [mise](mise.md) page for `.mise.toml` / `.tool-versions`
integration and task-runner examples.

### pip

```sh
pip install toolr
```

This installs **only** the Rust CLI binary. The wheel has no Python
source and no `import toolr` — that's what `toolr-py` is for (see
below). `python -m toolr` was removed in the rust front-end rewrite;
use the `toolr` executable instead.

### GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
Windows) from <https://github.com/s0undt3ch/ToolR/releases> and
extract it onto `$PATH` manually. Each archive ships with a `.sha256`
sibling for verification.

## Enable `import toolr` in your tool scripts (`toolr-py`)

The `toolr-py` wheel provides the Python runtime your `tools/*.py`
scripts use — the `toolr` package (`Context`, `command_group`,
helpers under `toolr.utils`) and the `_rust_utils` extension module
the framework calls internally.

Add it as a dependency of your project's tools venv:

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
    pyo3-extension wheel built for each (CPython, ABI) pair. Splitting
    them lets the CLI be one global install while the runtime tracks
    each project's tools venv.

## Supply-chain verification (SLSA attestations)

Every release archive is signed with a SLSA build-provenance
attestation produced by the GitHub-hosted release workflow. The
`install.sh` and `install.ps1` scripts verify the attestation
automatically when the `gh` CLI is on PATH. To require verification:

```sh
sh installation/install.sh --verify-attestation=require   # POSIX
./installation/install.ps1 -VerifyAttestation require     # Windows
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
development; put `target/release/` on your PATH (or alias `toolr` to
it) while iterating.
