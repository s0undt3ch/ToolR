"""Runner shim: invoked as ``python -m toolr._runner`` by the toolr binary.

Reads the spec JSON path from ``$TOOLR_SPEC_FILE``, decodes it with
``msgspec.json``, imports the target module, builds a ``Context``, and
calls the target function. Exit code propagates to the parent toolr
process and on to the shell.

The Rust side serialises CLI argument values as strings (or arrays of
strings for collection-typed args, or bools for `Flag` kinds). The
runner then uses :func:`msgspec.convert` against the target function's
actual type hints to coerce each value into its declared type — int,
float, list[T], tuple[T1, T2], Enum, Literal, Path, datetime, and so
on are all supported "for free" via msgspec's coercion rules.
"""

from __future__ import annotations

import inspect
import os
import warnings
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Annotated
from typing import Any
from typing import get_args
from typing import get_origin
from typing import get_type_hints

import msgspec

from toolr._exc import ToolrDeprecationWarning
from toolr.sources import CommandSchema
from toolr.sources import DispatchCommand
from toolr.utils._signature import detect_dispatch_parameter

if TYPE_CHECKING:
    from collections.abc import Callable

    from toolr._context import Context

# Promote toolr's own deprecation warnings to visible-by-default. The
# stdlib silences DeprecationWarning by default for non-__main__ code,
# which means user `tools/*.py` files would never surface the legacy
# decorator warnings. Filter is per-location ("default") so each call
# site fires once per process — keeps output bounded across runs with
# many legacy decorators.
warnings.simplefilter("default", ToolrDeprecationWarning)

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
    # `None` means "no default — `ctx.run` doesn't time out unless the
    # caller asks for it." Plumbed from `toolr --timeout-secs N` /
    # `toolr --no-output-timeout-secs N` on the Rust side.
    default_timeout_secs: float | None = None
    default_no_output_timeout_secs: float | None = None


class DispatchPayloadSpec(msgspec.Struct, frozen=True):
    """Dispatch payload written by the Rust binary for dispatched leaves.

    Mirrors `DispatchSpec` in `crates/toolr-core/src/execute/spec.rs`.
    When set on a :class:`RunnerSpec`, the runner constructs a
    `toolr.sources.DispatchCommand` from these fields and calls
    :func:`invoke_dispatcher` instead of running the spec as a normal
    command invocation.
    """

    command: str
    command_args: dict[str, Any]
    schema: CommandSchema


class RunnerSpec(msgspec.Struct, frozen=True):
    """Top-level spec written by the Rust binary into ``$TOOLR_SPEC_FILE``."""

    schema_version: int
    group: str
    command: str
    module: str
    function: str
    args: dict[str, Any]
    context: ContextSpec
    dispatch: DispatchPayloadSpec | None = None


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
    except msgspec.ValidationError as exc:
        # `ValidationError` is a subclass of `DecodeError` in msgspec, so
        # the more-specific clause must come first; otherwise the broader
        # "not valid JSON" message swallows real type/schema mismatches.
        msg = f"toolr spec file failed schema validation ({spec_path}): {exc}"
        raise SpecError(msg) from exc
    except msgspec.DecodeError as exc:
        msg = f"toolr spec file is not valid JSON ({spec_path}): {exc}"
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
        default_timeout_secs=spec.context.default_timeout_secs,
        default_no_output_timeout_secs=spec.context.default_no_output_timeout_secs,
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


def _unwrap_annotated(hint: Any) -> Any:
    """Strip a `typing.Annotated[T, ...]` wrapper down to its underlying T."""
    if get_origin(hint) is Annotated:
        return get_args(hint)[0]
    return hint


def _dec_hook(target_type: type, obj: Any) -> Any:  # noqa: PLR0911
    """Coerce values msgspec doesn't know about natively.

    The rust binary serialises everything that needs validation as a
    string (Path, DateTime, UUID, IPv*, Email, Version …) —
    pre-validated by the clap value-parser. This hook turns that string
    into the matching Python type so the command function receives the
    expected type.
    """
    import datetime as _dt  # noqa: PLC0415
    import ipaddress as _ip  # noqa: PLC0415
    import pathlib as _path  # noqa: PLC0415
    import uuid as _uuid  # noqa: PLC0415

    from packaging.version import Version as _PkgVersion  # noqa: PLC0415

    if isinstance(obj, str):
        if isinstance(target_type, type) and issubclass(target_type, _path.PurePath):
            return target_type(obj)
        if target_type is _dt.datetime:
            return _dt.datetime.fromisoformat(obj)
        if target_type is _dt.date:
            return _dt.date.fromisoformat(obj)
        if target_type is _dt.time:
            return _dt.time.fromisoformat(obj)
        if target_type is _uuid.UUID:
            return _uuid.UUID(obj)
        if target_type is _ip.IPv4Address:
            return _ip.IPv4Address(obj)
        if target_type is _ip.IPv6Address:
            return _ip.IPv6Address(obj)
        if target_type is _PkgVersion:
            return _PkgVersion(obj)
    msg = f"toolr runner: don't know how to coerce {type(obj).__name__} → {target_type!r}"
    raise TypeError(msg)


def _coerce_args(target: Callable[..., Any], raw: dict[str, Any]) -> tuple[list[Any], dict[str, Any]]:
    """Coerce `raw` against `target`'s actual type hints.

    Returns a ``(positional_args, keyword_args)`` pair. Positional args come
    from any parameter declared as ``*args`` in the target's signature — the
    rust side emitted them under that parameter's name as a list of strings,
    which we coerce element-wise and then splat positionally.

    Every keyword goes through :func:`msgspec.convert` with ``strict=False``
    so str→int, str→float, str→Enum, str→Path, etc. all do the right thing.
    Unknown keys (i.e. parameters that aren't on the function — shouldn't
    happen with a well-formed manifest, but defensive) pass through
    untouched so the function can raise a clear ``TypeError`` itself.
    """
    try:
        hints = get_type_hints(target, include_extras=False)
    except Exception:  # noqa: BLE001 — best-effort; fall back to raw values.
        hints = {}
    sig = inspect.signature(target)
    var_positional_name = next(
        (name for name, p in sig.parameters.items() if p.kind == p.VAR_POSITIONAL),
        None,
    )

    positional: list[Any] = []
    keyword: dict[str, Any] = {}
    for name, value in raw.items():
        hint = _unwrap_annotated(hints.get(name)) if name in hints else None
        if name == var_positional_name:
            # `*args: T` — `hint` is the *element* type, value is a list.
            if not isinstance(value, list):
                msg = f"toolr runner: expected a list for variadic positional `*{name}`, got {type(value).__name__}"
                raise SpecError(msg)
            if hint is not None:
                try:
                    positional = [msgspec.convert(elem, type=hint, strict=False, dec_hook=_dec_hook) for elem in value]
                except msgspec.ValidationError as exc:
                    msg = f"toolr runner: invalid value for `{name}`: {exc}"
                    raise SpecError(msg) from exc
            else:
                positional = list(value)
            continue
        if hint is None:
            keyword[name] = value
            continue
        try:
            keyword[name] = msgspec.convert(value, type=hint, strict=False, dec_hook=_dec_hook)
        except msgspec.ValidationError as exc:
            msg = f"toolr runner: invalid value for `--{name.replace('_', '-')}`: {exc}"
            raise SpecError(msg) from exc
    return positional, keyword


def invoke_dispatcher(
    *,
    ctx: Any,
    func: Callable[..., Any],
    parent_kwargs: dict[str, Any],
    child_name: str,
    child_args: dict[str, Any],
    child_schema: CommandSchema,
) -> Any:
    """Call ``func(ctx, **parent_kwargs, <dispatch_param>=DispatchCommand(...))``.

    Raises ``RuntimeError`` if ``func`` doesn't have a DispatchCommand-
    annotated parameter (manifest builder should have caught this at
    build time; this is a defensive guard against a stale manifest).
    """
    param = detect_dispatch_parameter(func)
    if param is None:
        msg = f"invoke_dispatcher: {func.__qualname__!r} has no DispatchCommand parameter (manifest out of sync?)"
        raise RuntimeError(msg)
    dispatched = DispatchCommand(
        command=child_name,
        command_args=child_args,
        schema=child_schema,
    )
    return func(ctx, **parent_kwargs, **{param: dispatched})


def run(spec: RunnerSpec) -> int:
    """Execute the command described by ``spec``. Returns a process exit code.

    ``ctx.exit(status, ...)`` raises :class:`SystemExit`; we honor its code.
    Any other uncaught exception is logged to stderr and returns 1.
    """
    try:
        ctx = _build_context(spec)
        target = _import_target(spec)
        if spec.dispatch is not None:
            # Dispatched leaf: `target` is the parent dispatcher, `args`
            # carries the parent's own kwargs, `dispatch` carries the
            # child name/args/schema. Coerce the parent kwargs against
            # the parent's hints — `invoke_dispatcher` injects the
            # `DispatchCommand` keyword on top of those.
            _, parent_kwargs = _coerce_args(target, spec.args)
            invoke_dispatcher(
                ctx=ctx,
                func=target,
                parent_kwargs=parent_kwargs,
                child_name=spec.dispatch.command,
                child_args=spec.dispatch.command_args,
                child_schema=spec.dispatch.schema,
            )
        else:
            var_args, kw_args = _coerce_args(target, spec.args)
            target(ctx, *var_args, **kw_args)
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
