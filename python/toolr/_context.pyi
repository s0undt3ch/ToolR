from argparse import ArgumentParser
from collections.abc import Iterator
from pathlib import Path
from typing import Any
from typing import Literal
from typing import NoReturn
from typing import TextIO
from typing import overload

from msgspec import Struct
from rich.console import Console
from rich.console import ConsoleRenderable
from rich.console import RichCast
from rich.text import TextType

from toolr.utils._console import ConsoleVerbosity
from toolr.utils.command import CommandResult

class Context(Struct, frozen=True):
    repo_root: Path
    parser: ArgumentParser
    verbosity: ConsoleVerbosity = ...
    _console_stderr: Console = ...
    _console_stdout: Console = ...

    # Boolean
    @overload
    def prompt(
        self,
        prompt: TextType,
        expected_type: type[bool],
        *,
        default: bool | None = None,
        show_default: bool = True,
    ) -> bool: ...

    # Password string
    @overload
    def prompt(
        self,
        prompt: TextType,
        expected_type: type[str],
        *,
        password: bool = True,
        default: str | None = None,
        case_sensitive: bool = True,
    ) -> str: ...

    # Integer
    @overload
    def prompt(
        self,
        prompt: TextType,
        expected_type: type[int],
        *,
        choices: list[str] | None = None,
        default: int | None = None,
        show_default: bool = True,
        show_choices: bool = True,
    ) -> int: ...

    # Float
    @overload
    def prompt(
        self,
        prompt: TextType,
        expected_type: type[float],
        *,
        choices: list[str] | None = None,
        default: float | None = None,
        show_default: bool = True,
        show_choices: bool = True,
    ) -> float: ...

    # Generic string (default when expected_type is None)
    @overload
    def prompt(
        self,
        prompt: TextType,
        expected_type: None = None,
        *,
        choices: list[str] | None = None,
        default: None = None,
        case_sensitive: bool = True,
        show_default: bool = True,
        show_choices: bool = True,
    ) -> str: ...
    def _prompt(
        self,
        prompt: TextType,
        expected_type: type[str | int | float | bool] | None = None,
        *,
        password: bool = False,
        case_sensitive: bool = True,
        choices: list[str] | None = None,
        default: str | float | bool | None = None,
        show_default: bool = True,
        show_choices: bool = True,
        console: Console | None = None,
        stream: TextIO | None = None,
    ) -> str | int | float | bool: ...
    def print(
        self,
        *args: ConsoleRenderable | RichCast | str,
        sep: str = " ",
        end: str = "\n",
        style: str | None = None,
        justify: Literal["default", "left", "center", "right", "full"] | None = None,
        overflow: Literal["fold", "crop", "ellipsis", "ignore"] | None = None,
        no_wrap: bool | None = None,
        emoji: bool | None = None,
        markup: bool | None = None,
        highlight: bool | None = None,
        width: int | None = None,
        height: int | None = None,
        crop: bool = True,
        soft_wrap: bool | None = None,
        new_line_start: bool = False,
    ) -> None: ...
    def debug(
        self,
        *objects: ConsoleRenderable | RichCast | str,
        sep: str = " ",
        end: str = "\n",
        style: str | None = None,
        justify: Literal["default", "left", "center", "right", "full"] | None = None,
        overflow: Literal["fold", "crop", "ellipsis", "ignore"] | None = None,
        markup: bool | None = None,
        highlight: bool | None = None,
        width: int | None = None,
        height: int | None = None,
        crop: bool = True,
        soft_wrap: bool | None = None,
        new_line_start: bool = False,
        log_locals: bool = False,
    ) -> None: ...
    def info(
        self,
        *objects: ConsoleRenderable | RichCast | str,
        sep: str = " ",
        end: str = "\n",
        style: str | None = None,
        justify: Literal["default", "left", "center", "right", "full"] | None = None,
        overflow: Literal["fold", "crop", "ellipsis", "ignore"] | None = None,
        markup: bool | None = None,
        highlight: bool | None = None,
        width: int | None = None,
        height: int | None = None,
        crop: bool = True,
        soft_wrap: bool | None = None,
        new_line_start: bool = False,
        log_locals: bool = False,
    ) -> None: ...
    def warn(
        self,
        *objects: ConsoleRenderable | RichCast | str,
        sep: str = " ",
        end: str = "\n",
        style: str | None = None,
        justify: Literal["default", "left", "center", "right", "full"] | None = None,
        overflow: Literal["fold", "crop", "ellipsis", "ignore"] | None = None,
        markup: bool | None = None,
        highlight: bool | None = None,
        width: int | None = None,
        height: int | None = None,
        crop: bool = True,
        soft_wrap: bool | None = None,
        new_line_start: bool = False,
        log_locals: bool = False,
    ) -> None: ...
    def error(
        self,
        *objects: ConsoleRenderable | RichCast | str,
        sep: str = " ",
        end: str = "\n",
        style: str | None = None,
        justify: Literal["default", "left", "center", "right", "full"] | None = None,
        overflow: Literal["fold", "crop", "ellipsis", "ignore"] | None = None,
        markup: bool | None = None,
        highlight: bool | None = None,
        width: int | None = None,
        height: int | None = None,
        crop: bool = True,
        soft_wrap: bool | None = None,
        new_line_start: bool = False,
        log_locals: bool = False,
    ) -> None: ...
    def exit(self, status: int = 0, message: str | None = None) -> NoReturn: ...
    def run(
        self,
        *cmdline: str,
        stream_output: bool = True,
        capture_output: bool = False,
        timeout_secs: float | None = None,
        no_output_timeout_secs: float | None = None,
        **kwargs: Any,
    ) -> CommandResult[str] | CommandResult[bytes]: ...
    def chdir(self, path: str | Path) -> Iterator[Path]: ...
