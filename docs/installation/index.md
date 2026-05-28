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
mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise
mise use --global toolr@latest
```

For projects that already pin tool versions via `.mise.toml`, this
is the most-natural fit — toolr's version becomes part of your
project's reproducible tool set. See the dedicated [mise](mise.md)
page for `.mise.toml` / `.tool-versions` integration and
task-runner examples.

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

Verifies the SLSA attestation when `gh` is on PATH. Pin a version
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
- **[uv](https://docs.astral.sh/uv/)** to materialise the tools
  venv (`uv sync`). If it isn't on PATH, toolr installs a managed
  copy on first use.

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
development; put `target/release/` on your PATH (or alias `toolr`
to it) while iterating.
