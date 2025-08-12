"""
Complete example.

The purpose is to provide an extensive usage example, kind if like TDD

| Example | Description |
|---------|-------------|
| hello   | Say hello.  |
| goodbye | Say goodbye.|
| multiply| Multiply two numbers.|



"""

from __future__ import annotations

import shutil
from enum import StrEnum
from typing import Annotated
from typing import NoReturn

from toolr import Context
from toolr import arg
from toolr import command_group

group = command_group("example", title="Example", docstring=__doc__)


@group.command
def hello(ctx: Context) -> NoReturn:
    """
    Say hello.

    This is the long description about the hello command.
    """
    ctx.info("Hello, world!")


@group.command("goodbye")
def say_goodbye(ctx: Context, name: str | None = None) -> NoReturn:
    """
    Say goodbye.

    Args:
        name: Name to say goodbye to. If not provided, defaults to "world".
    """
    if name is None:
        name = "world"
    ctx.info(f"Goodbye, {name}!")


@group.command
def multiply(ctx: Context, a: int, b: int, verbose: bool = False) -> NoReturn:
    """
    Multiply two numbers.

    Args:
        a: First number.
        b: Second number.
        verbose: Whether to print the result calculation. Defaults to False, print only the result.
    """
    result = a * b
    if verbose:
        ctx.info(f"{a} * {b} = {result}")
    else:
        ctx.info(result)


class Operation(StrEnum):
    ADD = "add"
    SUBTRACT = "subtract"
    MULTIPLY = "multiply"
    DIVIDE = "divide"


@group.command
def math(
    ctx: Context,
    a: int,
    b: int,
    operation: Annotated[Operation, arg(aliases=["-o", "--op"])] = Operation.ADD,
    verbose: bool = False,
) -> NoReturn:
    """
    Perform a mathematical operation.

    Args:
        a: First number.
        b: Second number.
        operation: Operation to perform.
        verbose: Whether to print the result calculation. Defaults to False, print only the result.
    """
    match operation:
        case Operation.ADD:
            value = a + b
            log_msg = f"{a} + {b} = {value}"
        case Operation.SUBTRACT:
            value = a - b
            log_msg = f"{a} - {b} = {value}"
        case Operation.MULTIPLY:
            value = a * b
            log_msg = f"{a} * {b} = {value}"
        case Operation.DIVIDE:
            if b == 0:
                ctx.error("Division by zero!")
                return
            value = a / b
            log_msg = f"{a} / {b} = {value}"
        case _:
            raise ValueError(f"Invalid operation: {operation}")
    if verbose:
        ctx.info(log_msg)
    else:
        ctx.info(value)


@group.command
def py_version(ctx: Context) -> NoReturn:
    """
    Show Python version.

    This command demonstrates how to run subprocess commands and capture their output.
    """
    python = shutil.which("python")
    ret = ctx.run(python, "--version", capture_output=True, stream_output=False)
    ctx.info("Python version", ret.stdout.read().strip())
