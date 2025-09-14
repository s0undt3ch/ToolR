"""
Imports related utilities.
"""

from __future__ import annotations

import logging
from collections.abc import Iterator
from contextlib import contextmanager
from typing import TYPE_CHECKING

log = logging.getLogger(__name__)


@contextmanager
def report_on_import_errors(message: str) -> Iterator[None]:
    """
    Catch import errors and raise a CommandDependencyNotFoundError.
    """
    try:
        yield
    except ModuleNotFoundError as exc:
        # Suppress the current frame (the yield line) from the traceback
        # We just want to show the user the import error from the code that actually uses the command
        if TYPE_CHECKING:
            assert exc.__traceback__ is not None
        exc.__traceback__ = exc.__traceback__.tb_next
        log.warning(message, exc_info=exc)
