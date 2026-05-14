<h1 align="center">
  <img width="240px" src="https://raw.githubusercontent.com/s0undt3ch/Toolr/main/docs/imgs/toolr.png" alt="ToolR - AI Generated Logo">
</h1>

<h2 align="center">
  <em>In-project CLI tooling support</em>
</h2>

<p align="center">
  <em>Pronounced <tt>/ˈtuːlər/</tt> (tool-er)</em>
</p>

ToolR is a tool similar to [invoke](https://www.pyinvoke.org/) and the next generation of [python-tools-scripts](https://github.com/saltstack/python-tools-scripts).

The goal is to quickly enable projects to write a Python module under the project's `tools/` sub-directory and it automatically becomes a sub command to the `toolr` CLI.

## Key Features

### Automatic Command Discovery

ToolR automatically discovers and registers commands from your project's `tools/` directory, making it easy to organize and maintain your project's CLI tools.

### Simple Command Definition

Define commands using simple Python functions with type hints. ToolR automatically generates argument parsing based on your function signatures.

### Nested Command Groups

Organize commands into logical groups and subgroups using dot notation, providing a clean and intuitive CLI structure.

### Rich Help System

Built-in support for rich text formatting and automatic help generation from docstrings and type annotations.

### Third-Party Command Support

Extend ToolR's functionality by installing packages that provide additional commands through Python entry points.

## Installation

`toolr` ships in two complementary PyPI packages:

- **`toolr`** — the Rust CLI binary you run as `toolr` from the
  shell. Installs via pip, curl, mise, or a release archive. No
  Python source, no `import toolr`.
- **`toolr-py`** — the Python runtime your `tools/*.py` scripts
  import (`from toolr import Context, command_group`). Provides the
  `toolr` import name and the `_rust_utils` extension module the
  framework runs under the hood.

For a typical project you'll want **both**, in different venvs: the
CLI installed once on PATH, and `toolr-py` added to your
`tools/pyproject.toml` so it's resolved into your tools venv.

### Install the CLI binary (`toolr`)

Pick whichever method matches your environment.

#### `curl ... | sh` (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
```

Pass `--version X.Y.Z` after `sh -s --` to pin a specific release, or
`--prefix /custom/bin` to choose an install directory. Default prefix
is `$XDG_BIN_HOME` (or `~/.local/bin`).

#### PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
```

#### mise

If you use [mise](https://mise.jdx.dev/) for tool version management:

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise install toolr@latest
mise use --global toolr@latest
```

See the [mise installation guide](https://s0undt3ch.github.io/ToolR/installation/mise/)
for `.mise.toml` integration and project-level pinning.

#### pip

```sh
pip install toolr
```

The `toolr` wheel ships only the Rust CLI binary; there's no Python
source and no `import toolr` inside it. `python -m toolr` was removed
in the rust front-end rewrite — use the `toolr` executable instead.

#### GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
Windows) from <https://github.com/s0undt3ch/ToolR/releases> and
extract it onto `$PATH` manually. Each archive ships with a `.sha256`
sibling for verification.

### Enable `import toolr` in your tool scripts (`toolr-py`)

Add `toolr-py` to your project's `tools/pyproject.toml` so the
import surface is available when toolr executes your commands:

```toml
# tools/pyproject.toml
[project]
dependencies = ["toolr-py"]
```

`toolr-py` provides the `toolr` package (`Context`, `command_group`,
helpers in `toolr.utils`, …) and the `_rust_utils` extension module.
See [docs/installation/index.md](https://s0undt3ch.github.io/ToolR/installation/)
for the longer write-up.

### Supply-chain verification (SLSA attestations)

Every release archive is signed with a SLSA build-provenance
attestation produced by the GitHub-hosted release workflow. The
`install.sh` and `install.ps1` scripts will verify the attestation
automatically when the `gh` CLI is on PATH. To require verification:

```sh
sh installation/install.sh --verify-attestation=require   # POSIX
./installation/install.ps1 -VerifyAttestation require     # Windows
```

You can also verify a downloaded archive manually:

```sh
gh attestation verify toolr-1.2.3-aarch64-apple-darwin.tar.gz \
  --repo s0undt3ch/ToolR
```

## Quick Start

1. **Install ToolR** (see [Installation](#installation) above).

2. **Create a tools package** in your project root:

   ```bash
   mkdir tools
   touch tools/__init__.py
   ```

3. **Write your first command** in `tools/example.py`:

   ```python
   from toolr import Context, command_group

   group = command_group("example", "Example Commands", "Example command group")

   @group.command
   def hello(ctx: Context, name: str = "World"):
       """Say hello to someone.

       Args:
         name: The name to say hello to.
       """
       ctx.print(f"Hello, {name}!")
   ```

4. **Run your command**:

   ```bash
   toolr example hello --name Alice
   ```

## Advanced Usage

### Third-Party Commands

ToolR supports 3rd-party commands from installable Python packages. Create packages that extend ToolR's functionality by defining commands and registering them as entry points.

See the [Advanced Topics section](https://s0undt3ch.github.io/ToolR/usage/#advanced-topics) in the documentation for detailed information about creating 3rd-party command packages.

## Testing and Security

ToolR includes comprehensive testing with a focus on security and robustness:

### Property-Based Testing (Fuzzing)

ToolR uses [Hypothesis](https://hypothesis.works/) for property-based testing to automatically discover edge cases and potential vulnerabilities.
Fuzzing tests are integrated into the regular test suite:

```bash
# Run all tests (including fuzzing tests)
uv run pytest

# Run only fuzzing tests
uv run pytest -k "test_fuzz"

# Run with coverage
uv run coverage run -m pytest -ra -s -v
```

### Security Testing Features

- **Automated Fuzzing**: Property-based tests that generate thousands of test cases
- **Unicode Edge Cases**: Comprehensive testing of text processing with various encodings
- **Malformed Input Handling**: Tests ensure graceful handling of invalid input

The integrated fuzzing tests help ensure ToolR can safely handle malformed input, unusual Unicode sequences, and other edge cases that might cause crashes or security vulnerabilities.

## Contributing

We welcome contributions from the community! ToolR is an open-source project and we appreciate:

- 🐛 Bug reports and fixes
- ✨ Feature requests and implementations
- 📖 Documentation improvements
- 🧪 Test coverage enhancements
- 💡 Ideas and suggestions

Please read our [Contributing Guide](CONTRIBUTING.md) for:

- Setting up your development environment
- Coding standards and best practices
- Pull request process
- Commit message conventions
- Testing guidelines

## License

ToolR is licensed under the [Apache License 2.0](LICENSE).
