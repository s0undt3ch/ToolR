from __future__ import annotations

from typing import TYPE_CHECKING

try:
    import importlib.metadata

    __version__ = importlib.metadata.version("toolr")
except ImportError:
    __version__ = "0.0.0.not-installed"

from toolr._context import Context
from toolr._registry import registry
from toolr.utils._signature import arg

if TYPE_CHECKING:
    from toolr._registry import CommandRegistry

    assert registry is not None
    assert isinstance(registry, CommandRegistry)

# Create a command group alias
command_group = registry.command_group

__all__ = ["Context", "__version__", "arg", "command_group", "registry"]
