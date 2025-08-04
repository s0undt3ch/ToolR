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
