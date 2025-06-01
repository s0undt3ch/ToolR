from __future__ import annotations

import argparse
import logging
import pathlib
import sys
from argparse import ArgumentParser
from argparse import _SubParsersAction
from dataclasses import dataclass
from dataclasses import field
from typing import Any

from rich_argparse import RawDescriptionRichHelpFormatter

from toolr import __version__
from toolr._context import ConsoleVerbosity
from toolr._context import Context
from toolr.utils import _logs

log = logging.getLogger(__name__)


@dataclass(frozen=True, slots=True)
class Parser:
    """
    Singleton parser class that wraps argparse.
    """

    repo_root: pathlib.Path = field(default_factory=pathlib.Path.cwd)
    parser: ArgumentParser = field(init=False, repr=False)
    subparsers: _SubParsersAction[ArgumentParser] = field(init=False, repr=False)
    context: Context = field(init=False, repr=False)
    options: argparse.Namespace = field(init=False, repr=False)

    def __post_init__(self) -> None:
        # Let's do a little manual parsing so that we can set debug or quiet early
        verbosity = ConsoleVerbosity.NORMAL
        for arg in sys.argv[1:]:
            if not arg.startswith("-"):
                break
            if arg in ("-q", "--quiet"):
                verbosity = ConsoleVerbosity.QUIET
                break
            if arg in ("-d", "--debug"):
                verbosity = ConsoleVerbosity.VERBOSE
                break

        context = Context(
            parser=self,  # type: ignore[arg-type]
            repo_root=self.repo_root,
            verbosity=verbosity,
        )
        object.__setattr__(self, "context", context)
        parser = argparse.ArgumentParser(
            prog="toolr",
            description="In-project CLI tooling support",
            epilog="More information about ToolR can be found at https://github.com/s0undt3ch/toolr",
            allow_abbrev=False,
            formatter_class=RawDescriptionRichHelpFormatter,
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
        object.__setattr__(self, "parser", parser)

        subparsers = parser.add_subparsers(
            title="Commands",
            dest="command",
            required=True,
            description="These commands are discovered under `<repo-root>/tools` recursively.",
        )
        object.__setattr__(self, "subparsers", subparsers)

    def parse_args(self) -> None:
        """
        Parse CLI.
        """
        # Log the argv getting executed
        self.context.debug(f"Tools executing 'sys.argv': {sys.argv}")
        # Process registered imports to allow other modules to register commands
        # self._process_registered_tool_modules()
        options = self.parser.parse_args()
        if options.quiet:
            logging.root.setLevel(logging.CRITICAL + 1)
        elif options.debug:
            logging.root.setLevel(logging.DEBUG)
        else:
            logging.root.setLevel(logging.INFO)
        if options.timestamps:
            for handler in logging.root.handlers:
                handler.setFormatter(_logs.TIMESTAMP_FORMATTER)
        else:
            for handler in logging.root.handlers:
                handler.setFormatter(_logs.NO_TIMESTAMP_FORMATTER)
        object.__setattr__(self, "options", options)
        if "func" not in options:
            self.context.exit(1, "No command was passed.")
        log.debug("CLI parsed options %s", options)
        options.func(options)

    def __getattr__(self, attr: str) -> Any:
        """
        Proxy unknown attributes to the parser instance.
        """
        if attr == "options":
            return self.__getattribute__(attr)
        return getattr(self.parser, attr)
