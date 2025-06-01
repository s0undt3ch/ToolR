from __future__ import annotations

try:
    import importlib.metadata

    __version__ = importlib.metadata.version("toolr")
except ImportError:
    __version__ = "0.0.0.not-installed"

from toolr._context import Context
from toolr._registry import registry

# Create a command group alias
command_group = registry.command_group

__all__ = ["Context", "command_group", "registry"]
