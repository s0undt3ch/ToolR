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

## Quick Start

1. **Install ToolR**:

   ```bash
   python -m pip install toolr
   ```

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
