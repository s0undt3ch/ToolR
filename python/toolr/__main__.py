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

log = logging.getLogger(__name__)


def main() -> NoReturn:  # type: ignore[misc]
    """
    Main CLI entry-point for python tools scripts.
    """
    parser = Parser()
    log.debug("Searching for tools in %s", parser.repo_root)
    if parser.repo_root in sys.path:
        sys.path.remove(parser.repo_root)
    sys.path.insert(0, parser.repo_root)
    try:
        import tools  # noqa: F401
    except ImportError as exc:
        if os.environ.get("TOOLR_DEBUG_IMPORTS", "0") == "1":
            raise exc from None

    parser.parse_args()


if __name__ == "__main__":
    freeze_support()
    main()
