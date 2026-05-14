# Installation

`toolr` ships as a single self-contained binary. Pick the install method
that matches your environment.

## Requirements

- **Python 3.11 or later** is needed at execute time. The toolr binary
  itself is standalone, but every user command runs inside a Python
  subprocess.
- **[uv](https://docs.astral.sh/uv/)** is needed to materialise the
  tools venv (`uv sync`). If you don't have it on PATH, toolr can
  install a managed copy on first use.

## `curl ... | sh` (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.sh | sh
```

Pass `--version X.Y.Z` after `sh -s --` to pin a specific release, or
`--prefix /custom/bin` to choose an install directory. Default prefix
is `$XDG_BIN_HOME` (or `~/.local/bin`).

## PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.ps1 | iex
```

## mise

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise use --global toolr@latest
```

The plugin source ships in this repo under `installation/mise/`. See
the dedicated [mise](mise.md) page for `.mise.toml` / `.tool-versions`
integration and task-runner examples.

## pip

```sh
pip install toolr
```

The wheel installs the Python runtime support — the `toolr` package and
the `_rust_utils` extension module — needed by `tools/*.py` commands at
execute time. The `toolr` **binary** is not bundled in the wheel
(maturin pyo3-bindings limitation); install it via one of the methods
above. `python -m toolr` was removed in the rust front-end rewrite.

## GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
Windows) from <https://github.com/s0undt3ch/ToolR/releases> and
extract it onto `$PATH` manually. Each archive ships with a `.sha256`
sibling for verification.

## Supply-chain verification (SLSA attestations)

Every release archive is signed with a SLSA build-provenance
attestation produced by the GitHub-hosted release workflow. The
`install.sh` and `install.ps1` scripts verify the attestation
automatically when the `gh` CLI is on PATH. To require verification:

```sh
sh dist/install.sh --verify-attestation=require   # POSIX
./dist/install.ps1 -VerifyAttestation require     # Windows
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
