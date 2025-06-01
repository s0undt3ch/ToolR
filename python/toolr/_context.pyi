from argparse import ArgumentParser
from collections.abc import Iterator
from dataclasses import dataclass
from enum import IntEnum
from pathlib import Path
from typing import Any
from typing import Literal
from typing import NoReturn

from rich.console import Console
from rich.console import ConsoleRenderable
from rich.console import RichCast

from toolr.utils.command import CommandResult

class ConsoleVerbosity(IntEnum):
    QUIET = 0
    NORMAL = 1
    VERBOSE = 2

@dataclass(frozen=True, slots=True)
class Context:
    repo_root: Path
    parser: ArgumentParser
    verbosity: ConsoleVerbosity = ...
    console: Console = ...
    console_stdout: Console = ...

    def __post_init__(self) -> None: ...
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
