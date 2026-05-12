"""Runner shim: invoked as ``python -m toolr._runner`` by the toolr binary.

Reads the spec JSON path from ``$TOOLR_SPEC_FILE``, decodes it with
``msgspec.json``, imports the target module, builds a ``Context``, and
calls the target function. Exit code propagates to the parent toolr
process and on to the shell.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

import msgspec

SCHEMA_VERSION: int = 1

_SPEC_ENV_VAR = "TOOLR_SPEC_FILE"


class SpecError(Exception):
    """Raised when the spec file is missing, malformed, or unsupported."""


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


def load_spec(path: str | os.PathLike[str]) -> RunnerSpec:
    """Read the spec at ``path`` and decode it into a :class:`RunnerSpec`.

    Validates the schema version and raises :class:`SpecError` on any
    problem (missing file, malformed JSON, unsupported schema version).
    """
    spec_path = Path(path)
    try:
        data = spec_path.read_bytes()
    except FileNotFoundError as exc:
        msg = f"toolr spec file not found: {spec_path}"
        raise SpecError(msg) from exc
    except OSError as exc:
        msg = f"failed to read toolr spec file {spec_path}: {exc}"
        raise SpecError(msg) from exc
    try:
        spec = msgspec.json.decode(data, type=RunnerSpec)
    except msgspec.DecodeError as exc:
        msg = f"toolr spec file is not valid JSON ({spec_path}): {exc}"
        raise SpecError(msg) from exc
    except msgspec.ValidationError as exc:
        msg = f"toolr spec file failed schema validation ({spec_path}): {exc}"
        raise SpecError(msg) from exc
    if spec.schema_version != SCHEMA_VERSION:
        msg = (
            f"toolr spec file declares schema_version={spec.schema_version}, "
            f"but this toolr Python package only supports {SCHEMA_VERSION}. "
            "Upgrade the toolr package in your tools venv."
        )
        raise SpecError(msg)
    return spec


def load_spec_from_env() -> RunnerSpec:
    """Read ``$TOOLR_SPEC_FILE`` and call :func:`load_spec` on it."""
    spec_path = os.environ.get(_SPEC_ENV_VAR)
    if not spec_path:
        msg = f"{_SPEC_ENV_VAR} is not set. The toolr runner must be invoked by the toolr binary, not directly."
        raise SpecError(msg)
    return load_spec(spec_path)
