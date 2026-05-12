"""Runner shim: invoked as ``python -m toolr._runner`` by the toolr binary.

Reads the spec JSON path from ``$TOOLR_SPEC_FILE``, decodes it with
``msgspec.json``, imports the target module, builds a ``Context``, and
calls the target function. Exit code propagates to the parent toolr
process and on to the shell.
"""

from __future__ import annotations

from typing import Any

import msgspec

SCHEMA_VERSION: int = 1


class ContextSpec(msgspec.Struct, frozen=True):
    """Subset of the ``Context`` reconstructable from the Rust front-end."""

    repo_root: str
    verbosity: str
    timestamps: bool
    log_level: str


class RunnerSpec(msgspec.Struct, frozen=True):
    """Top-level spec written by the Rust binary into ``$TOOLR_SPEC_FILE``."""

    schema_version: int
    group: str
    command: str
    module: str
    function: str
    args: dict[str, Any]
    context: ContextSpec
