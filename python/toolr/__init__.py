from __future__ import annotations

try:
    import importlib.metadata

    __version__ = importlib.metadata.version("toolr")
except ImportError:
    __version__ = "0.0.0.not-installed"

from toolr._context import Context
from toolr._registry import command_group
from toolr.utils._signature import arg

__all__ = ["Context", "__version__", "arg", "command_group"]
