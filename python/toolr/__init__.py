"""ToolR Python package.

Importable surface: :class:`toolr.Context`, :func:`toolr.command_group`,
:func:`toolr.command`, :func:`toolr.arg`,
:func:`toolr.report_on_import_errors`.

Implementation modules (not part of the user-facing API):

- ``toolr._runner``: invoked by the toolr binary via
  ``python -m toolr._runner``; reads ``$TOOLR_SPEC_FILE`` and dispatches
  into user code.
"""

from __future__ import annotations

try:
    import importlib.metadata

    __version__ = importlib.metadata.version("toolr")
except ImportError:
    __version__ = "0.0.0.not-installed"

from toolr._context import Context
from toolr._registry import MANIFEST_SCHEMA_VERSION
from toolr._registry import command
from toolr._registry import command_group
from toolr.build import BuildManifestError
from toolr.build import BuildResult
from toolr.build import build_manifest
from toolr.utils._imports import report_on_import_errors
from toolr.utils._signature import arg

__all__ = [
    "MANIFEST_SCHEMA_VERSION",
    "BuildManifestError",
    "BuildResult",
    "Context",
    "__version__",
    "arg",
    "build_manifest",
    "command",
    "command_group",
    "report_on_import_errors",
]
