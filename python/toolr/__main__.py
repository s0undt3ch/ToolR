"""
Main CLI entry-point for python tools scripts.
"""

from __future__ import annotations

import logging
import os
import sys
from multiprocessing import freeze_support
from typing import NoReturn

from toolr._parser import Parser
from toolr._registry import CommandRegistry

log = logging.getLogger(__name__)


def main(argv: list[str] | None = None) -> NoReturn:  # type: ignore[misc]
    """
    Main CLI entry-point for python tools scripts.
    """
    parser = Parser()
    log.debug("Searching for tools in %s", parser.repo_root)
    str_repo_root_path = str(parser.repo_root)
    if str_repo_root_path in sys.path:
        sys.path.remove(str_repo_root_path)
    sys.path.insert(0, str_repo_root_path)
    try:
        import tools  # type: ignore[import-not-found]
    except ImportError as exc:
        if os.environ.get("TOOLR_DEBUG_IMPORTS", "0") == "1":
            raise exc from None

    # Let's discover and build the command registry
    registry = CommandRegistry()
    registry.discover_and_build(parser)

    parser.parse_args(argv)
    parser.run()


if __name__ == "__main__":
    freeze_support()
    main()
