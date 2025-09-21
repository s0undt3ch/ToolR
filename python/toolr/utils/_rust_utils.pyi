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
class Example(TypedDict):
    description: str
    snippet: str | None
    syntax: str | None

class VersionChanged(TypedDict):
    version: str
    description: str

class ParsedDocstring(TypedDict, total=False):
    short_description: str
    long_description: str | None
    params: dict[str, str | None]
    examples: list[Example]
    notes: list[str]
    warnings: list[str]
    see_also: list[str]
    references: list[str]
    todo: list[str]
    deprecated: str | None
    version_added: str | None
    version_changed: list[VersionChanged]

class DocstringParser:
    def __init__(self) -> None: ...
    @staticmethod
    def strict() -> DocstringParser: ...
    def parse(self, docstring: str) -> ParsedDocstring: ...
