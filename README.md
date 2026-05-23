<h1 align="center">
  <img width="240px" src="https://raw.githubusercontent.com/s0undt3ch/Toolr/main/docs/imgs/toolr.png" alt="ToolR - AI Generated Logo">
</h1>

<h2 align="center">
  <em>In-project CLI tooling, with a Rust front-end.</em>
</h2>

<p align="center">
  <em>Pronounced <tt>/ˈtuːlər/</tt> (tool-er)</em>
</p>

ToolR is a Python task runner that boots in milliseconds because the front-end is a Rust binary. Python only runs when you invoke a command, inside a per-repo `uv`-managed venv.

| Tool runner          | `-h` steady-state | First run | Second run |
| -------------------- | ----------------: | --------: | ---------: |
| **toolr**            |       **10.4 ms** |  277.0 ms |    21.8 ms |
| doit                 |           83.4 ms |  143.0 ms |    88.9 ms |
| invoke               |           84.6 ms |  101.7 ms |    77.4 ms |
| nox                  |           91.8 ms |  395.9 ms |   115.3 ms |
| duty                 |          166.4 ms |  188.5 ms |   252.4 ms |
| python-tools-scripts |          252.2 ms |  340.3 ms |   189.0 ms |

`<tool> -h`, 20 runs, steady-state = mean of last 18. Measured on Apple M3 Pro / macOS 26.5 / arm64. Reproduce locally with `toolr bench compare` (add `--markdown` for the table above).

## Why ToolR

- **Sub-millisecond discovery.** The CLI is a Rust binary. `--help` and Tab completion read a cached static manifest; Python never boots for non-execute paths.
- **No system-Python dependency.** Toolr resolves a per-repo Python venv via `uv` on first invocation. The host OS doesn't need Python at all to install toolr — it's a single static binary.
- **Write Python, not framework boilerplate.** Drop a `tools/*.py` file with a `command_group` and a `@command` decorator. Type hints become CLI arguments; Google-style docstrings become `--help` text.
- **First-class third-party command packages.** Plugins ship a static `toolr-manifest.json` inside the wheel. Discovery is a glob + JSON parse; no Python import to find them.
- **Signed releases.** Every release archive ships with a SLSA build-provenance attestation. The install scripts verify it automatically when `gh` is on PATH.

## Two wheels, two roles

| Package    | What it is                                   | Where it lives                  |
| ---------- | -------------------------------------------- | ------------------------------- |
| `toolr`    | The Rust CLI binary you run from the shell.  | On `$PATH`, installed once.     |
| `toolr-py` | The Python runtime your `tools/*.py` import. | In your `tools/pyproject.toml`. |

Most projects want both: the CLI installed globally, `toolr-py` declared in the per-repo `tools/pyproject.toml` so `from toolr import Context, command_group` works when your commands run.

## Install

Five first-class install paths.

### mise

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise use --global toolr@latest
```

For projects that already pin tool versions via `.mise.toml`, this is the most-natural fit — toolr's version becomes part of your project's reproducible tool set. See [docs/installation/mise/](https://toolr.readthedocs.io/latest/installation/mise/).

### pip

```sh
pip install toolr   # Rust CLI binary
```

This installs the `toolr` binary into whatever venv `pip` is pointing at. **Do not `pip install toolr-py`** into that same venv — `toolr-py` is the Python runtime your `tools/*.py` files import, and it belongs in the per-repo tools venv that `toolr project init` scaffolds for you (where it's declared in `tools/pyproject.toml` and materialised via `uv sync`). See "Two wheels, two roles" above for the split.

### curl | sh (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
```

Verifies the SLSA attestation when `gh` is on PATH. Pin a version with `sh -s -- --version X.Y.Z`. Custom prefix: `sh -s -- --prefix /opt/toolr/bin`.

### PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
```

### GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for Windows) from <https://github.com/s0undt3ch/ToolR/releases>, verify the `.sha256` sibling and the SLSA attestation, drop the binary on `$PATH`. Useful in locked-down environments that audit binaries before allowing them on a machine.

### Scaffold your repo

After the binary is on `$PATH`:

```sh
toolr project init                  # writes tools/{pyproject.toml,.gitignore,example.py}
toolr example hello                 # run the generated example
toolr self completion install bash  # or zsh / fish
```

The full install matrix (per-OS notes, attestation flags, prefix overrides) lives in [docs/installation/](https://toolr.readthedocs.io/latest/installation/).

## What you write

```python
# tools/example.py
"""Example commands."""
from toolr import Context, command_group

example = command_group("example", title="Example", description=__doc__)


@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Say hello to <name>.

    Args:
        name: who to greet.
    """
    ctx.print(f"Hello, {name}!")
```

```sh
$ toolr example hello --name Pedro
Hello, Pedro!
```

`toolr project init` writes a richer four-command starter than this two-liner — open it and edit, or delete it and start from scratch.

## Where to go next

- [Quickstart](https://toolr.readthedocs.io/latest/quickstart/)
- [Writing commands](https://toolr.readthedocs.io/latest/writing-commands/)
- [Third-party packages](https://toolr.readthedocs.io/latest/third-party/)
- [Internals (manifest, freshness, cache)](https://toolr.readthedocs.io/latest/internals/)
- [CLI reference](https://toolr.readthedocs.io/latest/cli/)

## Project status

ToolR is pre-1.0. The on-disk manifest is versioned (`schema_version` in `tools/.toolr-manifest.json`); the binary refuses to load a higher version than it understands. The public Python surface is `toolr.__all__`; anything not listed there is implementation detail. Backwards-incompatible changes will be explicit in the changelog (generated by `git-cliff` on release).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[Apache-2.0](LICENSE).
