# How to use

It's important to note that ToolR relies on proper typing of the python functions that will become commands.
If fact, it will complain and error out if typing information is missing or unable to parse.

It's also important to note that the function must also have a properly written docstring using
[google style](https://sphinxcontrib-napoleon.readthedocs.io/en/latest/example_google.html) docstrings.

```python
--8<-- "docs/usage/files/example1.py"
```

Let's see it!

```bash
toolr example -h
Usage: toolr example [-h] {echo} ...

Example commands

Options:
  -h, --help  show this help message and exit

Example:
  Example commands

  {echo}
    echo      Command title line.
```

And now the command help:

```bash
toolr example echo -h
Usage: toolr example echo [-h] WHAT

This is the command description, it can span several lines.

Positional Arguments:
  WHAT        What to echo.

Options:
  -h, --help  show this help message and exit
```

## Roundup #1

So far you've seen a few important pieces:

* [``command_group``][toolr._registry.CommandGroup.command_group]
* ``Context``

## Docstrings

Docstrings are really useful and can greatly improve the CLI UX:

```python
--8<-- "docs/usage/files/example2.py"
```

It can even render some markdown tables!

### Module Help

```bash
toolr example -h
Usage: toolr example [-h] {hello,goodbye,multiply,math} ...

Complete example.

Options:
  -h, --help            show this help message and exit

Example:
  The purpose is to provide an extensive usage example, kind if like TDD

    Example    Description
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    hello      Say hello.
    goodbye    Say goodbye.
    multiply   Multiply two numbers.

  {hello,goodbye,multiply,math}
    hello               Say hello.
    goodbye             Say goodbye.
    multiply            Multiply two numbers.
    math                Perform a mathematical operation.
```

### ``math`` command help

```bash
toolr example math -h
Usage: toolr example math [-h] [--operation OPERATION] [--verbose] A B

Perform a mathematical operation.

Positional Arguments:
  A                     First number.
  B                     Second number.

Options:
  -h, --help            show this help message and exit
  --operation, -o, --op OPERATION
                        Operation to perform. Choices: 'add', 'subtract', 'multiply', 'divide'. (default: add)
  --verbose             Whether to print the result calculation. Defaults to False, print only the result. (default: False)
```

## Advanced Topics

### Mutually Exclusive Arguments

ToolR supports [mutually exclusive argument groups][argparse.ArgumentParser.add_argument_group], which allow you to
define sets of arguments where only one can be used at a time.
This is useful for scenarios like verbosity levels, output formats, or alternative processing modes.

#### Basic Usage

Use the `group` parameter in the [`arg()`][toolr.utils._signature.arg] function to specify which mutually exclusive group an argument belongs to:

```python
--8<-- "docs/usage/files/mutually-exclusive-1.py"
```

#### Verbose/Debug Example

Here's a more comprehensive example showing different verbosity and debug levels:

```python
--8<-- "docs/usage/files/mutually-exclusive-2.py"
```

```python
```

#### Command Line Usage

When using the above function, you can only specify one argument from each group:

```bash
# Valid usage - one from each group
toolr analyze-data input.txt --verbose --json --fast

# Invalid usage - multiple from verbosity group
toolr analyze-data input.txt --verbose --quiet  # Error!

# Invalid usage - multiple from format group  
toolr analyze-data input.txt --json --yaml      # Error!

# Valid usage - using defaults for some groups
toolr analyze-data input.txt --debug --csv
```

#### Error Example

```python
# This will raise an error
def invalid_function(
    ctx: Context,
    name: Annotated[str, arg(group="invalid")],  # Positional argument in group - ERROR!
) -> None:
    """This function will fail to parse.

    Args:
        name: The name parameter.
    """
```

This would raise: `SignatureError: Positional parameter 'name' cannot be in a mutually exclusive group.`

### Third-Party Commands

ToolR supports 3rd-party commands from installable Python packages. This allows you to extend ToolR's functionality by installing additional packages that provide their own commands.

#### Creating a 3rd-Party Package

To create a package that contributes commands to ToolR, you need to:

1. **Define your commands** using the standard ToolR API
2. **Register an entry point** in your package's `pyproject.toml`

Here's an example of a 3rd-party package structure:

```python title="thirdparty/commands.py"
from __future__ import annotations

from toolr import Context
from toolr import command_group

third_party_group = command_group("third-party", "Third Party Tools", "Tools from third-party packages")

@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context
        name: Name to greet (default: World)
    """
    ctx.print(f"Hello, {name} from 3rd-party package!")

@third_party_group.command("version")
def version_command(ctx: Context) -> None:
    """Show the version of the 3rd-party package.

    Args:
        ctx: The execution context
    """
    ctx.print("3rd-party package version 1.0.0")
```

#### Entry Point Configuration

In your package's `pyproject.toml`, define the entry point:

```toml
[project.entry-points."toolr.tools"]
<this name is not important> = "<package>.<module calling toolr.command_group()>"
```

For example:

```toml
[project.entry-points."toolr.tools"]
commands = "thirdparty.commands"
```

#### Installation and Discovery

Once installed alongside ToolR, the package will automatically contribute its commands. You can see a complete working example in the [ToolR repository](https://github.com/s0undt3ch/ToolR/tree/main/tests/support/3rd-party-pkg).

#### Command Resolution

When multiple packages provide commands with the same name:

* **Repository commands** (commands defined in your local `tools/` directory) **override** 3rd-party commands
* If the parent command group is shared, 3rd-party commands **augment** the existing command group

This allows for flexible command composition while maintaining local control over command behavior.
