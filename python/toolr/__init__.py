from __future__ import annotations

try:
    import importlib.metadata

    __version__ = importlib.metadata.version("toolr")
except ImportError:
    __version__ = "0.0.0.not-installed"

from toolr._context import Context

__all__ = ["Context"]
