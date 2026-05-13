# Quickstart

The fastest path from zero to a running command. About two minutes
end-to-end. For a full install matrix, see [Installation](installation/index.md).

## 1. Install the toolr binary

On Linux or macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.sh | sh
```

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.ps1 | iex
```

Verify:

```sh
toolr --version
```

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

Open the generated file. Each function decorated with
`@command(group="example")` becomes a CLI subcommand. The first
argument is a [`Context`][toolr.Context] object; the rest become CLI
arguments inferred from your type hints and docstring.

For the full authoring guide, head to [Writing commands](writing-commands/index.md).

## Optional next steps

- **Install shell tab completion:**
  `toolr self completion install bash` (or `zsh` / `fish`).
- **Learn the mental model:** [How toolr is laid out](concepts.md) — one
  page covering the binary, the tools venv, the manifest, and the cache.
- **Ship your tools with a third-party PyPI package:** see
  [Third-party packages](third-party.md).
