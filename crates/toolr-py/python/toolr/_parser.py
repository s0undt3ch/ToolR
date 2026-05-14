from __future__ import annotations

import argparse
import logging
import pathlib
import sys
from argparse import ArgumentParser
from argparse import _SubParsersAction
from typing import TYPE_CHECKING
from typing import Any

from msgspec import Struct
from msgspec import field
from msgspec import structs
from rich_argparse import ArgumentDefaultsRichHelpFormatter

from toolr import __version__
from toolr._context import Context
from toolr.utils._console import ConsoleVerbosity
from toolr.utils._logs import setup_logging

if TYPE_CHECKING:
    from argparse import Namespace

log = logging.getLogger(__name__)


class Parser(Struct, frozen=True):
    """
    Singleton parser class that wraps argparse.
    """

    repo_root: pathlib.Path = field(default_factory=pathlib.Path.cwd)
    parser: ArgumentParser | None = None
    subparsers: _SubParsersAction[ArgumentParser] | None = None
    context: Context | None = None
    options: Namespace | None = None

    def __post_init__(self) -> None:
        # Let's do a little manual parsing so that we can set debug or quiet early
        verbosity = ConsoleVerbosity.NORMAL
        if any(arg in sys.argv for arg in ["-d", "--debug"]):
            verbosity = ConsoleVerbosity.VERBOSE
        elif any(arg in sys.argv for arg in ["-q", "--quiet"]):
            verbosity = ConsoleVerbosity.QUIET
        else:
            verbosity = ConsoleVerbosity.NORMAL

        setup_logging(verbosity=verbosity)

        # Late import to avoid circular import issues
        from toolr.utils._console import Consoles  # noqa: PLC0415

        consoles = Consoles.setup(verbosity)

        context = Context(
            parser=self,  # type: ignore[arg-type]
            repo_root=self.repo_root,
            verbosity=verbosity,
            _console_stderr=consoles.stderr,
            _console_stdout=consoles.stdout,
        )
        structs.force_setattr(self, "context", context)

        parser = argparse.ArgumentParser(
            prog="toolr",
            description="In-project CLI tooling support",
            epilog="More information about ToolR can be found at https://github.com/s0undt3ch/toolr",
            allow_abbrev=False,
            formatter_class=ArgumentDefaultsRichHelpFormatter,
        )
        parser.add_argument("--version", action="version", version=__version__)
        log_group = parser.add_argument_group("Logging")
        timestamp_meg = log_group.add_mutually_exclusive_group()
        timestamp_meg.add_argument(
            "--timestamps",
            "--ts",
            action="store_true",
            help="Add time stamps to logs",
            dest="timestamps",
        )
        timestamp_meg.add_argument(
            "--no-timestamps",
            "--nts",
            action="store_false",
            default=True,
            help="Remove time stamps from logs",
            dest="timestamps",
        )
        level_group = log_group.add_mutually_exclusive_group()
        level_group.add_argument(
            "--quiet",
            "-q",
            dest="quiet",
            action="store_true",
            default=False,
            help="Disable logging",
        )
        level_group.add_argument(
            "--debug",
            "-d",
            action="store_true",
            default=False,
            help="Show debug messages",
        )
        run_options = parser.add_argument_group(
            "Run Subprocess Options",
            description="These options apply to ctx.run() calls",
        )
        run_options.add_argument(
            "--timeout",
            "--timeout-secs",
            default=None,
            type=int,
            help="Timeout in seconds for the command to finish.",
            metavar="SECONDS",
            dest="timeout_secs",
        )
        run_options.add_argument(
            "--no-output-timeout-secs",
            "--nots",
            default=None,
            type=int,
            help="Timeout if no output has been seen for the provided seconds.",
            metavar="SECONDS",
            dest="no_output_timeout_secs",
        )
        structs.force_setattr(self, "parser", parser)

        subparsers = parser.add_subparsers(
            title="Commands",
            dest="command",
            required=True,
            description="These commands are discovered under `<repo-root>/tools` recursively.",
        )
        structs.force_setattr(self, "subparsers", subparsers)

    def parse_args(self, argv: list[str] | None = None) -> Namespace:
        """
        Parse CLI.
        """
        if TYPE_CHECKING:
            assert self.context is not None
            assert self.parser is not None

        # Log the argv getting executed
        self.context.debug(f"Tools executing 'sys.argv': {sys.argv}")
        # Process registered imports to allow other modules to register commands
        # self._process_registered_tool_modules()
        options = self.parser.parse_args(argv)
        verbosity = ConsoleVerbosity.NORMAL
        if options.quiet:
            verbosity = ConsoleVerbosity.QUIET
        elif options.debug:
            verbosity = ConsoleVerbosity.VERBOSE
        setup_logging(verbosity=verbosity, timestamps=options.timestamps)

        # Late import to avoid circular import issues
        from toolr.utils._console import Consoles  # noqa: PLC0415

        # Reset verbosity and consoles after parsing the CLI
        consoles = Consoles.setup(verbosity)
        structs.force_setattr(self.context, "verbosity", verbosity)
        structs.force_setattr(self.context, "_console_stderr", consoles.stderr)
        structs.force_setattr(self.context, "_console_stdout", consoles.stdout)
        if "func" not in options:
            self.context.exit(1, "No command was passed.")
        structs.force_setattr(self, "options", options)
        log.debug("CLI parsed options %s", options)
        return options

    def run(self) -> None:
        """
        Run the command.
        """
        if self.options is None:
            err_msg = "parser.parse_args() was not called."
            raise RuntimeError(err_msg)
        self.options.func(self.context, self.options)
        self.exit(0)

    def __getattr__(self, attr: str) -> Any:
        """
        Proxy unknown attributes to the parser instance.
        """
        if attr == "options":
            return self.__getattribute__(attr)
        return getattr(self.parser, attr)
