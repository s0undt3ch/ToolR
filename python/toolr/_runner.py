"""Runner shim: invoked as ``python -m toolr._runner`` by the toolr binary.

Reads the spec JSON path from ``$TOOLR_SPEC_FILE``, decodes it with
``msgspec.json``, imports the target module, builds a ``Context``, and
calls the target function. Exit code propagates to the parent toolr
process and on to the shell.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Any

import msgspec

if TYPE_CHECKING:
    from toolr._context import Context

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


def _build_context(spec: RunnerSpec) -> Context:
    """Construct a minimal :class:`toolr.Context` from a :class:`RunnerSpec`."""
    # Late imports — keep module-load fast and avoid pulling rich into pure
    # spec-decoding code paths.
    import pathlib  # noqa: PLC0415
    from argparse import ArgumentParser  # noqa: PLC0415

    from toolr._context import Context  # noqa: PLC0415
    from toolr.utils._console import Consoles  # noqa: PLC0415
    from toolr.utils._console import ConsoleVerbosity  # noqa: PLC0415

    verbosity_map = {
        "quiet": ConsoleVerbosity.QUIET,
        "normal": ConsoleVerbosity.NORMAL,
        "verbose": ConsoleVerbosity.VERBOSE,
    }
    try:
        verbosity = verbosity_map[spec.context.verbosity]
    except KeyError as exc:
        msg = f"unknown verbosity {spec.context.verbosity!r} in spec; expected one of {sorted(verbosity_map)}"
        raise SpecError(msg) from exc

    consoles = Consoles.setup(verbosity)
    # ArgumentParser is required by Context for ctx.exit() — it calls
    # parser.exit(status). A bare parser is sufficient.
    parser = ArgumentParser(prog=f"toolr {spec.group} {spec.command}", add_help=False)
    return Context(
        repo_root=pathlib.Path(spec.context.repo_root),
        parser=parser,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )


def _import_target(spec: RunnerSpec) -> Any:
    """Import ``spec.module`` and return the attribute named ``spec.function``."""
    import importlib  # noqa: PLC0415

    try:
        module = importlib.import_module(spec.module)
    except ImportError as exc:
        msg = f"failed to import {spec.module}: {exc}"
        raise SpecError(msg) from exc
    try:
        return getattr(module, spec.function)
    except AttributeError as exc:
        msg = f"module {spec.module!r} has no attribute {spec.function!r}"
        raise SpecError(msg) from exc


def run(spec: RunnerSpec) -> int:
    """Execute the command described by ``spec``. Returns a process exit code.

    ``ctx.exit(status, ...)`` raises :class:`SystemExit`; we honor its code.
    Any other uncaught exception is logged to stderr and returns 1.
    """
    try:
        ctx = _build_context(spec)
        target = _import_target(spec)
        target(ctx, **spec.args)
    except SystemExit as exc:
        code = exc.code
        if code is None:
            return 0
        if isinstance(code, int):
            return code
        # str / other: print and return 1
        import sys  # noqa: PLC0415

        print(code, file=sys.stderr)  # noqa: T201
        return 1
    except SpecError as exc:
        import sys  # noqa: PLC0415

        print(f"toolr runner: {exc}", file=sys.stderr)  # noqa: T201
        return 2
    except Exception:  # noqa: BLE001
        import sys  # noqa: PLC0415
        import traceback  # noqa: PLC0415

        traceback.print_exc(file=sys.stderr)
        return 1
    return 0


def main() -> int:
    """Module entry point — invoked by ``python -m toolr._runner``."""
    try:
        spec = load_spec_from_env()
    except SpecError as exc:
        import sys  # noqa: PLC0415

        print(f"toolr runner: {exc}", file=sys.stderr)  # noqa: T201
        return 2
    return run(spec)


if __name__ == "__main__":
    import sys

    sys.exit(main())
