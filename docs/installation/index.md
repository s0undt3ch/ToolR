# Installation

## Requirements

- Python 3.11 or higher

## Using pip

```bash
python -m pip install toolr
```

## Project Setup

After installation, create a `tools/` package in your project root:

```bash
mkdir tools
touch tools/__init__.py
```

This directory will contain all your CLI commands. ToolR will automatically discover and register any Python modules in this directory.

## Verification

To verify the installation, run:

```bash
toolr --help
```

You should see the ToolR help output with available commands.

```console
Usage: toolr [-h] [--version] [--timestamps | --no-timestamps] [--quiet | --debug] [--timeout SECONDS] [--no-output-timeout-secs SECONDS] {} ...

In-project CLI tooling support

Options:
  -h, --help            show this help message and exit
  --version             show program's version number and exit

Logging:
  --timestamps, --ts    Add time stamps to logs (default: False)
  --no-timestamps, --nts
                        Remove time stamps from logs (default: True)
  --quiet, -q           Disable logging (default: False)
  --debug, -d           Show debug messages (default: False)

Run Subprocess Options:
  These options apply to ctx.run() calls

  --timeout, --timeout-secs SECONDS
                        Timeout in seconds for the command to finish. (default: None)
  --no-output-timeout-secs, --nots SECONDS
                        Timeout if no output has been seen for the provided seconds. (default: None)

Commands:
  These commands are discovered under `<repo-root>/tools` recursively.

  {}

More information about ToolR can be found at https://github.com/s0undt3ch/toolr
```

## Development Installation

For development or to use the latest version:

```bash
# Clone the repository
git clone https://github.com/s0undt3ch/toolr.git
cd toolr

# Install in development mode
uv sync --dev
```

## Third-Party Command Packages

ToolR supports 3rd-party command packages that extend its functionality. These packages are automatically discovered when installed alongside ToolR.

To install a 3rd-party command package:

```bash
python -m pip install <package-name>
```

The package's commands will be automatically available in the ToolR CLI. See the [Advanced Topics section](../usage/index.md#advanced-topics) for information about creating your own 3rd-party command packages.
