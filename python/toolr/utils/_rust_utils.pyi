from collections.abc import Sequence
from typing import TypedDict

# Command execution types
class CommandError(Exception):
    def __init__(self, message: str) -> None: ...

class CommandTimeoutError(CommandError):
    def __init__(self, message: str) -> None: ...

class CommandTimeoutNoOutputError(CommandError):
    def __init__(self, message: str) -> None: ...

def run_command_impl(
    args: Sequence[str],
    cwd: str | None = ...,
    env: dict[str, str] = ...,
    input: bytes | None = ...,
    stdout_fd: int | None = ...,
    stderr_fd: int | None = ...,
    sys_stdout_fd: int | None = ...,
    sys_stderr_fd: int | None = ...,
    timeout_secs: float | None = ...,
    no_output_timeout_secs: float | None = ...,
) -> int: ...

# Docstring parsing types
class Parameter(TypedDict, total=False):
    param_type: str | None
    description: str
    is_optional: bool
    default_value: str | None

class Return(TypedDict, total=False):
    return_type: str | None
    description: str

class Yield(TypedDict, total=False):
    yield_type: str | None
    description: str

class Example(TypedDict, total=False):
    description: str | None
    snippet: str

class Raise(TypedDict, total=False):
    exception_type: str
    description: str

class Attribute(TypedDict, total=False):
    name: str
    attr_type: str | None
    description: str

class ParsedDocstring(TypedDict, total=False):
    short_description: str
    long_description: str
    params: dict[str, Parameter]
    returns: Return | None
    yields: Yield | None
    examples: list[Example]
    notes: list[str]
    raises: dict[str, Raise]
    attributes: dict[str, Attribute]
    warnings: list[str]
    see_also: list[str]
    references: list[str]
    todo: list[str]
    deprecated: str | None
    version_added: str | None
    version_changed: list[dict[str, str]]

class DocstringParser:
    def __init__(self) -> None: ...
    @staticmethod
    def strict() -> DocstringParser: ...
    def parse(self, docstring: str) -> ParsedDocstring: ...
