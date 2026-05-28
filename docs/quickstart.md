# Quickstart

The fastest path from zero to a running command. About two minutes
end-to-end. For a full install matrix, see [Installation](installation/index.md).

## 1. Install the toolr binary

toolr ships as two complementary packages:

| Package    | What it is                                   | Where it lives                  |
| ---------- | -------------------------------------------- | ------------------------------- |
| `toolr`    | The Rust CLI binary you run from the shell.  | On `$PATH`, installed once.     |
| `toolr-py` | The Python runtime your `tools/*.py` import. | In your `tools/pyproject.toml`. |

This step installs the CLI binary. `toolr-py` lands automatically in
step 2 when `toolr project init` scaffolds `tools/pyproject.toml` and
runs `uv sync`.

Five first-class install paths — pick whichever matches your environment.

### mise

```sh
mise use aqua:s0undt3ch/ToolR@latest
```

Pulls toolr from the
[aqua registry](https://github.com/aquaproj/aqua-registry/tree/main/pkgs/s0undt3ch/ToolR)
via mise's built-in aqua backend — no plugin to register. For
projects that already pin tool versions via `.mise.toml`, this is
the most-natural fit. See [installation/mise](installation/mise.md).

### pip

```sh
pip install toolr   # Rust CLI binary
```

Installs the `toolr` binary into whatever venv `pip` is pointing at.
**Do not `pip install toolr-py`** into that same venv — `toolr-py`
belongs in the per-repo tools venv that `toolr project init`
scaffolds for you.

### curl | sh (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
```

Verifies the SLSA attestation when `gh` is on PATH. Pin a version with
`sh -s -- --version X.Y.Z`. Custom prefix: `sh -s -- --prefix /opt/toolr/bin`.

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

### Verify

```sh
toolr --version
```

The full install matrix (per-OS notes, attestation flags, prefix
overrides) lives in [Installation](installation/index.md).

## 2. Scaffold `tools/` in your repo

From your repo root:

```sh
toolr project init
```

This writes three files under `tools/`:

- `pyproject.toml` — declares the venv's dependencies and project options.
- `.gitignore` — ignores in-tree venvs.
- `example.py` — four sample commands you can run, edit, or delete.

It then runs `uv sync` to materialise the tools venv. The next command
you run uses that venv automatically.

## 3. Run your first command

```sh
toolr example hello --name you
```

```text
--8<-- "docs/quickstart-files/example-hello.txt"
```

Inspect the help to see what else the example file gives you:

```sh
toolr example --help
```

```text
--8<-- "docs/quickstart-files/example-help.txt"
```

## 4. Edit `tools/example.py` (or replace it)

Open the generated file. The scaffold declares a group with
`example = command_group("example", ...)` and attaches commands by
decorating functions with `@example.command`. Each decorated
function becomes a CLI subcommand. The first argument is a
[`Context`][toolr.Context] object; the rest become CLI arguments
inferred from your type hints and docstring.

When your tools grow past a single file and you want commands in one
file to attach to a group declared in another, switch to the
string-keyed form: `@command(group="example")`. See
[*Scaling command groups across files*](writing-commands/across-files.md).

For the full authoring guide, head to [Writing commands](writing-commands/index.md).

## Optional next steps

- **Install shell tab completion:**
  `toolr self completion install bash` (or `zsh` / `fish`).
- **Learn the mental model:** [How toolr is laid out](concepts.md) — one
  page covering the binary, the tools venv, the manifest, and the cache.
- **Ship your tools with a third-party PyPI package:** see
  [Third-party packages](third-party.md).
