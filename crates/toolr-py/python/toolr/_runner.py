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

import contextlib
import datetime
import importlib
import inspect
import ipaddress
import os
import pathlib
import stat
import sys
import traceback
import uuid
import warnings
from argparse import ArgumentParser
from pathlib import Path
from types import UnionType
from typing import TYPE_CHECKING
from typing import Annotated
from typing import Any
from typing import Union
from typing import get_args
from typing import get_origin
from typing import get_type_hints

import msgspec
from packaging.version import Version

from toolr._context import Context
from toolr._exc import ToolrDeprecationWarning
from toolr.sources import CommandSchema
from toolr.sources import DispatchCommand
from toolr.utils._console import Consoles
from toolr.utils._console import ConsoleVerbosity
from toolr.utils._signature import detect_dispatch_parameter

if TYPE_CHECKING:
    from collections.abc import Callable

# Promote toolr's own deprecation warnings to visible-by-default. The
# stdlib silences DeprecationWarning by default for non-__main__ code,
# which means user `tools/*.py` files would never surface the
# `parent.command_group("child", ...)` deprecation. Filter is
# per-location ("default") so each call site fires once per process —
# keeps output bounded across runs with many deprecated call sites.
warnings.simplefilter("default", ToolrDeprecationWarning)

SCHEMA_VERSION: int = 1
"""Schema version of the toolr ↔ toolr-py dispatch protocol.

This constant **must** match ``RUNNER_SCHEMA_VERSION`` in
``crates/toolr-core/src/execute/spec.rs`` exactly; a CI gate enforces
the lock-step.

**When to bump**: bump this **and** the Rust counterpart by ``+1``
together when the two sides would no longer understand each other:

* Add, remove, or rename any field on ``RunnerSpec`` (or any struct it
  nests) on either side.
* Change the meaning of any existing field (e.g. string-keyed to
  int-keyed, units changed, encoding changed).
* Add, remove, or rename a required field on the manifest JSON that the
  runner consumes.
* Change the env-var / stdin / stdout / exit-code conventions between
  the toolr binary and the Python runner.

**When *not* to bump**:

* Adding a new optional field on either side with a safe default
  (``serde(default)`` on Rust, ``msgspec`` default on Python) — old
  spec files still decode and old runners still work against new
  binaries.
* Internal refactors that don't change the serialised shape.

Toolr is pre-1.0, so bumps are monotonic integers tied 1:1 to "the
protocol changed in a way an older peer can't handle".
"""

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
        # The binary and toolr-py must agree on the dispatch wire format.
        # Direct the user at the exact command that brings the venv back
        # in sync; mention the pin-down-the-binary escape hatch for the
        # rarer case where they need to stay on the older toolr-py.
        msg = (
            f"toolr-py in this tools venv speaks schema {SCHEMA_VERSION}, "
            f"but the toolr binary emitted schema {spec.schema_version}. "
            "The venv is out of sync with the binary.\n\n"
            "Run:\n"
            "  toolr project venv upgrade toolr-py\n\n"
            "Or pin the toolr binary to a version compatible with "
            f"toolr-py schema {SCHEMA_VERSION}."
        )
        raise SpecError(msg)
    return spec


def _validate_spec_file(path: str) -> None:
    """Refuse a spec file that isn't a private, we-own-it regular file.

    Defense-in-depth (SEC-05). The toolr binary writes the spec to a 0600
    ``O_EXCL`` tempfile it owns and hands us the path via
    ``$TOOLR_SPEC_FILE``; ``_import_target`` then imports whatever
    ``spec.module`` says. That chain is trusted today, but this guards a
    future regression where the spec path becomes attacker-influenceable:
    a symlink, a file owned by another user, or a group/world-writable
    file is rejected with :class:`SpecError` rather than read-from (and
    imported-from). If the spec can't be forged, the import target can't
    be either.
    """
    try:
        info = os.lstat(path)
    except FileNotFoundError as exc:
        # Keep the wording load_spec() uses for a missing file, so the
        # validation step doesn't change the message users (and tests) see.
        msg = f"toolr spec file not found: {path}"
        raise SpecError(msg) from exc
    except OSError as exc:
        msg = f"toolr spec file is not accessible: {path} ({exc})"
        raise SpecError(msg) from exc
    if stat.S_ISLNK(info.st_mode):
        msg = f"toolr spec file must not be a symlink: {path}"
        raise SpecError(msg)
    if not stat.S_ISREG(info.st_mode):
        msg = f"toolr spec file is not a regular file: {path}"
        raise SpecError(msg)
    # POSIX-only: the binary creates the spec 0600 and owned by us. Windows
    # lacks these ownership/permission semantics, so the not-a-symlink +
    # regular-file checks above are what we can portably assert there.
    if hasattr(os, "getuid"):  # pragma: no cover - POSIX guard; the skip is Windows-only
        _check_spec_file_owner_and_mode(info, path)


def _check_spec_file_owner_and_mode(info: os.stat_result, path: str) -> None:
    """Refuse a spec file owned by another user or group/world-writable.

    The POSIX half of :func:`_validate_spec_file` (the classic tmp-swap
    scenarios); guarded there behind ``hasattr(os, "getuid")``.
    """
    if info.st_uid != os.getuid():
        msg = f"toolr spec file is not owned by the current user: {path}"
        raise SpecError(msg)
    if info.st_mode & 0o022:
        msg = f"toolr spec file is group/world-writable; refusing to read it: {path}"
        raise SpecError(msg)


def load_spec_from_env() -> RunnerSpec:
    """Read ``$TOOLR_SPEC_FILE`` and call :func:`load_spec` on it."""
    spec_path = os.environ.get(_SPEC_ENV_VAR)
    if not spec_path:
        msg = f"{_SPEC_ENV_VAR} is not set. The toolr runner must be invoked by the toolr binary, not directly."
        raise SpecError(msg)
    _validate_spec_file(spec_path)
    spec = load_spec(spec_path)
    # SEC-14(A): we are the last reader, so unlink the spec now rather than
    # waiting for the binary to drop its NamedTempFile handle after we exit.
    # That shrinks the window in which the 0600 spec JSON (which can carry
    # CLI argument values) lingers in TMPDIR if the process is SIGKILLed.
    # Best-effort: on Unix the unlink succeeds (the binary's open handle
    # keeps the inode alive until it drops, and its drop tolerates the
    # already-removed file); on Windows the binary still holds the file open
    # so the unlink may fail — harmless, the binary cleans up on drop.
    with contextlib.suppress(OSError):
        os.unlink(spec_path)
    return spec


def _build_context(spec: RunnerSpec) -> Context:
    """Construct a minimal :class:`toolr.Context` from a :class:`RunnerSpec`."""
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
    try:
        # `spec.module` comes from the toolr-controlled manifest/spec (written by
        # the toolr binary), not untrusted external input. SEC-05 tracks
        # defense-in-depth confinement of the import target.
        module = importlib.import_module(spec.module)  # nosemgrep
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


def _is_optional(hint: Any) -> bool:
    """True if `hint` is `T | None` (PEP 604) or `Optional[T]`/`Union[T, None]`.

    Handles `Annotated[T | None, ...]` by peeling the wrapper first.
    """
    inner = _unwrap_annotated(hint)
    if get_origin(inner) not in (UnionType, Union):
        return False
    return type(None) in get_args(inner)


def _dec_hook(target_type: type, obj: Any) -> Any:  # noqa: PLR0911
    """Coerce values msgspec doesn't know about natively.

    The rust binary serialises everything that needs validation as a
    string (Path, DateTime, UUID, IPv*, Email, Version …) —
    pre-validated by the clap value-parser. This hook turns that string
    into the matching Python type so the command function receives the
    expected type.
    """
    if isinstance(obj, str):
        if isinstance(target_type, type) and issubclass(target_type, pathlib.PurePath):
            return target_type(obj)
        if target_type is datetime.datetime:
            return datetime.datetime.fromisoformat(obj)
        if target_type is datetime.date:
            return datetime.date.fromisoformat(obj)
        if target_type is datetime.time:
            return datetime.time.fromisoformat(obj)
        if target_type is uuid.UUID:
            return uuid.UUID(obj)
        if target_type is ipaddress.IPv4Address:
            return ipaddress.IPv4Address(obj)
        if target_type is ipaddress.IPv6Address:
            return ipaddress.IPv6Address(obj)
        if target_type is Version:
            return Version(obj)
    msg = f"toolr runner: don't know how to coerce {type(obj).__name__} → {target_type!r}"
    raise TypeError(msg)


def _coerce_args(
    target: Callable[..., Any], raw: dict[str, Any]
) -> tuple[list[Any], dict[str, Any]]:
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
                    positional = [
                        msgspec.convert(elem, type=hint, strict=False, dec_hook=_dec_hook)
                        for elem in value
                    ]
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

    # Zero-or-one positionals (`new_version: str | None`) are absent from
    # `raw` when the user didn't pass them — clap accepts the omission
    # (the type is Optional, so `is_optional_wrapper` flips `required` to
    # false) but the python function has no default to fall back on. Fill
    # `None` for each such missing parameter so the call doesn't blow up
    # with "missing required positional argument".
    for param_name, param in sig.parameters.items():
        if param_name == "ctx":
            continue
        if param.kind == param.VAR_POSITIONAL:
            continue
        if param_name in keyword:
            continue
        if param.default is not param.empty:
            # Function has its own default — let it apply.
            continue
        if _is_optional(hints.get(param_name)):
            keyword[param_name] = None

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


def _print_missing_dep_hint(exc: ImportError, stream: Any) -> None:
    """Append the styled "run `toolr project venv sync`" hint to ``stream``.

    The hint replaces what the Rust-side post-mortem stderr capture used
    to do — now that the runner inherits stderr (so Rich's TTY detection
    works), the runner has to emit the hint itself.
    """
    missing = getattr(exc, "name", None) or "this module"
    print(
        f"\ntoolr: import `{missing}` failed at runtime. "
        "A dependency may be missing - run `toolr project venv sync` "
        "and check tools/pyproject.toml.",
        file=stream,
    )


def _append_repo_root(repo_root: str, path_list: list[str] | None = None) -> None:
    """Append ``repo_root`` to ``sys.path`` so ``import tools.*`` resolves.

    Append (not insert) so stdlib and site-packages win — only ``tools.*``,
    which nothing else provides, resolves from the repo. Idempotent.

    This is the one cwd/path concern that must stay in the runner: it needs
    *append* semantics (repo_root last), which `PYTHONPATH` cannot express
    (it prepends, ahead of stdlib + site-packages). The chdir and the
    relative-path warning both live on the Rust side.
    """
    target = sys.path if path_list is None else path_list
    if repo_root not in target:
        target.append(repo_root)


def run(spec: RunnerSpec) -> int:  # noqa: PLR0911
    """Execute the command described by ``spec``. Returns a process exit code.

    ``ctx.exit(status, ...)`` raises :class:`SystemExit`; we honor its code.
    Any other uncaught exception is logged to stderr and returns 1.
    """
    repo_root = Path(spec.context.repo_root)
    try:
        ctx = _build_context(spec)
        # `''` is gone from sys.path (the interpreter ran with `-P`), so make
        # `import tools.*` resolve regardless of where toolr was invoked. The
        # working directory is already repo_root (the Rust side spawned the
        # runner with `current_dir(repo_root)`); the relative-path warning also
        # lives on the Rust side, which knows the cwd, arg types, and values.
        _append_repo_root(str(repo_root))
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
        print(code, file=sys.stderr)  # noqa: T201
        return 1
    except SpecError as exc:
        print(f"toolr runner: {exc}", file=sys.stderr)  # noqa: T201
        # `_import_target` wraps an ImportError thrown while loading the
        # user's command module into a SpecError. Surface the missing-dep
        # hint when that's the case so a top-level or transitive import
        # failure still gets the styled "run venv sync" guidance.
        if isinstance(exc.__cause__, ImportError):
            _print_missing_dep_hint(exc.__cause__, sys.stderr)
        return 2
    except ImportError as exc:
        # An ImportError raised from inside the command function body
        # (e.g. `def cmd(ctx): import yaml; ...` where yaml is missing)
        # bypasses `_import_target` entirely. Print the traceback and
        # add the same hint so the user gets the same affordance as a
        # top-level import failure.
        traceback.print_exc(file=sys.stderr)
        _print_missing_dep_hint(exc, sys.stderr)
        return 1
    except Exception:  # noqa: BLE001
        traceback.print_exc(file=sys.stderr)
        return 1
    return 0


def main() -> int:
    """Module entry point — invoked by ``python -m toolr._runner``."""
    try:
        spec = load_spec_from_env()
    except SpecError as exc:
        print(f"toolr runner: {exc}", file=sys.stderr)  # noqa: T201
        return 2
    return run(spec)


if __name__ == "__main__":
    sys.exit(main())
