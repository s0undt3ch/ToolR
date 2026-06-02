"""Unit + subprocess coverage for `toolr._runner` branches.

`tests/runner/test_dispatch.py` covers the happy-path subprocess shape
(spec → import → invoke → exit). This file fills in the per-function
error / coercion branches that didn't yet have direct tests.

Most cases call the runner's functions directly rather than spawning a
subprocess — much faster, and each test pins a specific branch. The
remaining subprocess cases exist to verify the top-level `main()`
swallows-and-formats-stderr behaviour for unhandled exit codes.
"""

from __future__ import annotations

import datetime as dt
import ipaddress
import json
import os
import pathlib
import subprocess
import sys
import textwrap
import uuid
from collections.abc import Callable
from pathlib import Path
from typing import Annotated
from typing import Literal

import pytest
from packaging.version import Version

from toolr._runner import SCHEMA_VERSION
from toolr._runner import ContextSpec
from toolr._runner import RunnerSpec
from toolr._runner import SpecError
from toolr._runner import _build_context
from toolr._runner import _coerce_args
from toolr._runner import _dec_hook
from toolr._runner import _import_target
from toolr._runner import _unwrap_annotated
from toolr._runner import load_spec
from toolr._runner import load_spec_from_env
from toolr._runner import main
from toolr._runner import run

# --------------------------------------------------------------------------
# Factory fixtures (mirrored from test_dispatch.py for test isolation).
# --------------------------------------------------------------------------


@pytest.fixture
def make_spec_file(tmp_path: Path) -> Callable[..., Path]:
    """Factory: write a serialised RunnerSpec to disk; return the path."""

    def _make(
        *,
        schema_version: int = SCHEMA_VERSION,
        group: str = "demo",
        command: str = "noop",
        module: str = "tools.demo",
        function: str = "noop",
        args: dict[str, object] | None = None,
        verbosity: str = "normal",
        repo_root: Path | None = None,
        contents: str | None = None,
    ) -> Path:
        spec_path = tmp_path / "spec.json"
        if contents is not None:
            spec_path.write_text(contents)
            return spec_path
        payload = {
            "schema_version": schema_version,
            "group": group,
            "command": command,
            "module": module,
            "function": function,
            "args": args or {},
            "context": {
                "repo_root": str(repo_root or tmp_path),
                "verbosity": verbosity,
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        spec_path.write_text(json.dumps(payload))
        return spec_path

    return _make


def _runner_spec(
    *,
    module: str = "tools.demo",
    function: str = "noop",
    args: dict[str, object] | None = None,
    verbosity: str = "normal",
    repo_root: str | Path = "/var/empty",
) -> RunnerSpec:
    """Build a RunnerSpec directly (skip the JSON round-trip)."""
    return RunnerSpec(
        schema_version=SCHEMA_VERSION,
        group="demo",
        command="noop",
        module=module,
        function=function,
        args=args or {},
        context=ContextSpec(
            repo_root=str(repo_root),
            verbosity=verbosity,
            timestamps=False,
            log_level="INFO",
        ),
    )


# --------------------------------------------------------------------------
# load_spec / load_spec_from_env
# --------------------------------------------------------------------------


def test_load_spec_missing_file_raises_spec_error(tmp_path: Path) -> None:
    missing = tmp_path / "does-not-exist.json"
    with pytest.raises(SpecError, match=r"not found"):
        load_spec(missing)


def test_load_spec_invalid_json_raises_spec_error(make_spec_file: Callable[..., Path]) -> None:
    spec_path = make_spec_file(contents="{not valid json")
    with pytest.raises(SpecError, match=r"not valid JSON"):
        load_spec(spec_path)


def test_load_spec_type_mismatch_raises_schema_validation(
    tmp_path: Path, make_spec_file: Callable[..., Path]
) -> None:
    # `context.verbosity` is required and typed `str`; an int trips msgspec's
    # struct decoder, which surfaces as `msgspec.ValidationError` (the
    # subclass of `DecodeError`). The runner reports it under the more
    # specific "failed schema validation" message.
    spec_path = make_spec_file(
        contents=json.dumps(
            {
                "schema_version": SCHEMA_VERSION,
                "group": "demo",
                "command": "noop",
                "module": "tools.demo",
                "function": "noop",
                "args": {},
                "context": {
                    "repo_root": str(tmp_path),
                    "verbosity": 99,  # wrong type
                    "timestamps": False,
                    "log_level": "INFO",
                },
            }
        )
    )
    with pytest.raises(SpecError, match=r"failed schema validation"):
        load_spec(spec_path)


def test_load_spec_rejects_unknown_schema_version(make_spec_file: Callable[..., Path]) -> None:
    spec_path = make_spec_file(schema_version=SCHEMA_VERSION + 99)
    with pytest.raises(SpecError) as exc_info:
        load_spec(spec_path)
    msg = str(exc_info.value)
    # The error must tell the user exactly which command to run, and must
    # name both schema versions so they understand which side is stale.
    assert "toolr project venv upgrade toolr-py" in msg, msg
    assert f"schema {SCHEMA_VERSION}" in msg, msg
    assert f"schema {SCHEMA_VERSION + 99}" in msg, msg


def test_load_spec_round_trip_preserves_fields(make_spec_file: Callable[..., Path]) -> None:
    spec_path = make_spec_file(args={"name": "alice"}, verbosity="quiet")
    spec = load_spec(spec_path)
    assert spec.schema_version == SCHEMA_VERSION
    assert spec.context.verbosity == "quiet"
    assert spec.args == {"name": "alice"}


def test_load_spec_from_env_unset_raises(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("TOOLR_SPEC_FILE", raising=False)
    with pytest.raises(SpecError, match=r"TOOLR_SPEC_FILE"):
        load_spec_from_env()


def test_load_spec_from_env_empty_string_raises(monkeypatch: pytest.MonkeyPatch) -> None:
    # The `if not spec_path` guard covers both unset and empty-string —
    # without this case the empty-string branch stays unhit.
    monkeypatch.setenv("TOOLR_SPEC_FILE", "")
    with pytest.raises(SpecError, match=r"TOOLR_SPEC_FILE"):
        load_spec_from_env()


def test_load_spec_from_env_happy_path(
    monkeypatch: pytest.MonkeyPatch,
    make_spec_file: Callable[..., Path],
) -> None:
    spec_path = make_spec_file()
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))
    spec = load_spec_from_env()
    assert spec.command == "noop"


# --------------------------------------------------------------------------
# _build_context
# --------------------------------------------------------------------------


@pytest.mark.parametrize("verbosity", ["quiet", "normal", "verbose"])
def test_build_context_accepts_known_verbosity(tmp_path: Path, verbosity: str) -> None:
    ctx = _build_context(_runner_spec(verbosity=verbosity, repo_root=tmp_path))
    assert ctx.repo_root == tmp_path


def test_build_context_rejects_unknown_verbosity(tmp_path: Path) -> None:
    spec = _runner_spec(verbosity="loud", repo_root=tmp_path)
    with pytest.raises(SpecError, match=r"unknown verbosity"):
        _build_context(spec)


# --------------------------------------------------------------------------
# _import_target
# --------------------------------------------------------------------------


def test_import_target_missing_module_raises_spec_error() -> None:
    spec = _runner_spec(module="nonexistent.module.path", function="anything")
    with pytest.raises(SpecError, match=r"failed to import"):
        _import_target(spec)


def test_import_target_missing_attribute_raises_spec_error() -> None:
    # `os` definitely exists; `__definitely_missing__` does not.
    spec = _runner_spec(module="os", function="__definitely_missing__")
    with pytest.raises(SpecError, match=r"has no attribute"):
        _import_target(spec)


def test_import_target_returns_callable_attribute() -> None:
    spec = _runner_spec(module="os", function="getcwd")
    target = _import_target(spec)
    assert callable(target)
    assert target is os.getcwd


# --------------------------------------------------------------------------
# _unwrap_annotated
# --------------------------------------------------------------------------


def test_unwrap_annotated_strips_metadata() -> None:
    hint = Annotated[int, "some metadata", 42]
    assert _unwrap_annotated(hint) is int


def test_unwrap_annotated_returns_bare_type_untouched() -> None:
    assert _unwrap_annotated(int) is int
    assert _unwrap_annotated(str | None) == (str | None)


# --------------------------------------------------------------------------
# _dec_hook (per-type coverage)
# --------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("target_type", "value", "expected"),
    [
        (pathlib.Path, "/example/path", pathlib.Path("/example/path")),
        (pathlib.PurePosixPath, "x/y", pathlib.PurePosixPath("x/y")),
        # Intentionally naive datetime: the runner's `_dec_hook` calls
        # `datetime.fromisoformat(obj)` which preserves the input's tzinfo
        # (None when the string has no offset). The test asserts the
        # round-trip — adding tzinfo here would only mask whether the
        # hook preserves the original value.
        (dt.datetime, "2026-01-02T03:04:05", dt.datetime(2026, 1, 2, 3, 4, 5)),  # noqa: DTZ001
        (dt.date, "2026-01-02", dt.date(2026, 1, 2)),
        (dt.time, "03:04:05", dt.time(3, 4, 5)),
        (
            uuid.UUID,
            "12345678-1234-5678-1234-567812345678",
            uuid.UUID("12345678-1234-5678-1234-567812345678"),
        ),
        (ipaddress.IPv4Address, "10.0.0.1", ipaddress.IPv4Address("10.0.0.1")),
        (ipaddress.IPv6Address, "2001:db8::1", ipaddress.IPv6Address("2001:db8::1")),
        (Version, "1.2.3", Version("1.2.3")),
    ],
)
def test_dec_hook_coerces_known_types(target_type: type, value: str, expected: object) -> None:
    assert _dec_hook(target_type, value) == expected


def test_dec_hook_raises_typeerror_on_unsupported() -> None:
    # `complex` is not in the hook's special-case table — fall through to raise.
    with pytest.raises(TypeError, match=r"don't know how to coerce"):
        _dec_hook(complex, "1+2j")


def test_dec_hook_rejects_non_string_value() -> None:
    # The hook only handles `isinstance(obj, str)` — int input falls through.
    with pytest.raises(TypeError, match=r"don't know how to coerce"):
        _dec_hook(pathlib.Path, 42)


# --------------------------------------------------------------------------
# _coerce_args
# --------------------------------------------------------------------------


def _no_hints(ctx, x, y):  # pragma: no cover - hint-less helper for coverage tests
    return ctx, x, y


def _scalar_hints(ctx, name: str, count: int, ratio: float, enable: bool) -> None:
    """Helper with primitive type hints for coercion tests."""


def _path_hint(ctx, where: pathlib.Path) -> None:
    """Helper with a Path-typed kwarg."""


def _literal_hint(ctx, level: Literal["debug", "info", "warning"]) -> None:
    """Helper with a Literal-typed kwarg."""


def _variadic_int(ctx, *values: int) -> None:
    """Helper with `*args: int` for variadic coercion tests."""


def _variadic_no_hint(ctx, *values) -> None:
    """Helper with `*args` and no element type hint."""


def test_coerce_args_passes_through_when_no_hints() -> None:
    raw = {"x": "1", "y": "two"}
    positional, keyword = _coerce_args(_no_hints, raw)
    assert positional == []
    assert keyword == {"x": "1", "y": "two"}


def test_coerce_args_coerces_scalar_types() -> None:
    raw = {"name": "alice", "count": "7", "ratio": "1.25", "enable": "true"}
    positional, keyword = _coerce_args(_scalar_hints, raw)
    assert positional == []
    assert keyword == {"name": "alice", "count": 7, "ratio": 1.25, "enable": True}


def test_coerce_args_routes_path_through_dec_hook() -> None:
    raw = {"where": "/example/destination"}
    _, keyword = _coerce_args(_path_hint, raw)
    assert keyword["where"] == pathlib.Path("/example/destination")


def test_coerce_args_validates_literal() -> None:
    raw = {"level": "info"}
    _, keyword = _coerce_args(_literal_hint, raw)
    assert keyword == {"level": "info"}


def test_coerce_args_raises_spec_error_for_invalid_literal() -> None:
    raw = {"level": "trace"}
    with pytest.raises(SpecError, match=r"invalid value for `--level`"):
        _coerce_args(_literal_hint, raw)


def test_coerce_args_handles_variadic_with_element_type() -> None:
    raw = {"values": ["1", "2", "3"]}
    positional, keyword = _coerce_args(_variadic_int, raw)
    assert positional == [1, 2, 3]
    assert keyword == {}


def test_coerce_args_handles_variadic_without_element_type() -> None:
    raw = {"values": ["a", "b"]}
    positional, keyword = _coerce_args(_variadic_no_hint, raw)
    # No hint → list passes through untouched.
    assert positional == ["a", "b"]
    assert keyword == {}


def test_coerce_args_raises_when_variadic_value_is_not_a_list() -> None:
    raw = {"values": "1,2,3"}
    with pytest.raises(SpecError, match=r"expected a list for variadic"):
        _coerce_args(_variadic_int, raw)


def test_coerce_args_passes_unknown_kwarg_through_untouched() -> None:
    # If the manifest sends a kwarg whose name isn't on the function (e.g.
    # a parameter that's been removed but the cached manifest is stale),
    # the runner shouldn't crash — it just forwards the raw value and lets
    # Python raise its own clearer TypeError at call time.
    raw = {"name": "x", "count": "1", "ratio": "0.5", "enable": "false", "leftover": "stale"}
    _, keyword = _coerce_args(_scalar_hints, raw)
    assert keyword["leftover"] == "stale"


def test_coerce_args_strips_annotated_metadata() -> None:
    def _fn(ctx, port: Annotated[int, "via clap"]) -> None: ...

    _, keyword = _coerce_args(_fn, {"port": "8080"})
    assert keyword == {"port": 8080}


def test_coerce_args_falls_back_when_get_type_hints_raises() -> None:
    # A forward reference to an undefined name makes `get_type_hints` raise
    # NameError; the runner's `except Exception` swallows that and falls
    # back to raw values.
    def _fn(ctx, x: DoesNotExist) -> None: ...  # type: ignore[name-defined]  # noqa: F821

    _, keyword = _coerce_args(_fn, {"x": "raw"})
    assert keyword == {"x": "raw"}


def test_coerce_args_fills_none_for_absent_optional_positional() -> None:
    """`T | None` without a default → runner injects `None` when clap omits the slot.

    The rust front-end accepts `T | None` positionals as zero-or-one, so when
    the user doesn't pass the trailing arg, the value is absent from `raw`.
    Without this fill-in, the python function call blows up with
    "missing 1 required positional argument".
    """

    def _fn(ctx, new_version: str | None) -> None: ...

    _, keyword = _coerce_args(_fn, {})
    assert keyword == {"new_version": None}


def test_coerce_args_does_not_overwrite_supplied_optional_positional() -> None:
    """When the value IS in `raw`, the fill-in must not clobber it."""

    def _fn(ctx, new_version: str | None) -> None: ...

    _, keyword = _coerce_args(_fn, {"new_version": "0.20.0"})
    assert keyword == {"new_version": "0.20.0"}


def test_coerce_args_skips_fill_in_when_parameter_has_default() -> None:
    """`T | None = None` is a flag; missing key means "use the function's own default", not "inject None"."""

    def _fn(ctx, name: str | None = None) -> None: ...

    _, keyword = _coerce_args(_fn, {})
    # No injection — the function's own default takes over.
    assert keyword == {}


def test_coerce_args_skips_fill_in_for_non_optional_missing_params() -> None:
    """Plain `str` (no default, not Optional) must NOT be auto-filled.

    The call should still fail with the user's own clearer TypeError,
    not silently receive `None`.
    """

    def _fn(ctx, name: str) -> None: ...

    _, keyword = _coerce_args(_fn, {})
    assert keyword == {}


# --------------------------------------------------------------------------
# run() — non-subprocess exit-code shapes
# --------------------------------------------------------------------------


def test_run_returns_zero_on_systemexit_none(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    captured: dict[str, object] = {}

    def fake_target(ctx, **_kw):  # pragma: no cover - executed via run()
        captured["called"] = True
        raise SystemExit  # SystemExit(None)

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    rc = run(_runner_spec(repo_root=tmp_path))
    assert rc == 0
    assert captured == {"called": True}


def test_run_treats_string_exit_code_as_failure(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def fake_target(ctx, **_kw):  # pragma: no cover - executed via run()
        msg = "bailing out"
        raise SystemExit(msg)

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    rc = run(_runner_spec(repo_root=tmp_path))
    assert rc == 1
    assert "bailing out" in capsys.readouterr().err


def test_run_returns_2_for_spec_error_from_coercion(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    # Force `_coerce_args` to raise `SpecError` via a target that takes a
    # Literal-typed kwarg the spec value can't satisfy.
    def fake_target(
        ctx, level: Literal["debug", "info"]
    ) -> None:  # pragma: no cover - run via runner
        pass

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    spec = _runner_spec(args={"level": "trace"}, repo_root=tmp_path)
    rc = run(spec)
    assert rc == 2
    assert "invalid value for `--level`" in capsys.readouterr().err


def test_run_returns_0_on_clean_completion(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    def fake_target(ctx, **_kw) -> None: ...

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    assert run(_runner_spec(repo_root=tmp_path)) == 0


def test_run_returns_integer_exit_code_from_systemexit(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    def fake_target(ctx, **_kw) -> None:
        raise SystemExit(7)

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    assert run(_runner_spec(repo_root=tmp_path)) == 7


def test_run_returns_1_on_unhandled_exception(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def fake_target(ctx, **_kw) -> None:
        msg = "kaboom"
        raise RuntimeError(msg)

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    assert run(_runner_spec(repo_root=tmp_path)) == 1
    err = capsys.readouterr().err
    assert "RuntimeError" in err
    assert "kaboom" in err
    # Non-import errors must not trigger the missing-dep hint, otherwise
    # the hint would be noise on every command failure.
    assert "toolr project venv sync" not in err


def test_run_emits_missing_dep_hint_for_function_body_importerror(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A bare `ImportError` inside the command body must surface the
    styled "run `toolr project venv sync`" hint on stderr.

    This is the case the old Rust-side post-mortem used to handle by
    parsing captured stderr; now the runner emits the hint itself
    because stderr is inherited and there's no capture path.
    """

    def fake_target(ctx, **_kw) -> None:
        # Simulate `import optional_pkg` failing inside the function body.
        msg = "No module named 'optional_pkg'"
        raise ImportError(msg, name="optional_pkg")

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    assert run(_runner_spec(repo_root=tmp_path)) == 1
    err = capsys.readouterr().err
    assert "ImportError" in err
    assert "`optional_pkg`" in err
    assert "toolr project venv sync" in err
    assert "tools/pyproject.toml" in err


def test_run_emits_missing_dep_hint_for_transitive_importerror_via_spec_error(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """When a top-level import fails (`_import_target` wraps it as
    SpecError-from-ImportError), the hint must still fire so transitive
    missing deps get the same affordance as function-body ones.
    """

    def fake_import_target(_spec):
        # Mimic `_import_target`'s wrapping: SpecError chained from
        # an underlying ImportError. The hint lookup uses `__cause__`.
        inner_msg = "No module named 'transitive_dep'"
        cause = ImportError(inner_msg, name="transitive_dep")
        msg = f"failed to import tools.demo: {inner_msg}"
        raise SpecError(msg) from cause

    monkeypatch.setattr("toolr._runner._import_target", fake_import_target)
    assert run(_runner_spec(repo_root=tmp_path)) == 2
    err = capsys.readouterr().err
    assert "toolr runner: failed to import tools.demo" in err
    assert "`transitive_dep`" in err
    assert "toolr project venv sync" in err


def test_run_falls_back_to_generic_hint_when_importerror_has_no_name(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """`from pkg import thing` raises a bare `ImportError` whose `name`
    attribute is `None`. The hint must still render with a generic
    placeholder rather than crashing on a `None` interpolation.
    """

    def fake_target(ctx, **_kw) -> None:
        msg = "cannot import name 'thing' from 'pkg'"
        raise ImportError(msg)

    monkeypatch.setattr("toolr._runner._import_target", lambda _spec: fake_target)
    assert run(_runner_spec(repo_root=tmp_path)) == 1
    err = capsys.readouterr().err
    assert "this module" in err
    assert "toolr project venv sync" in err


# --------------------------------------------------------------------------
# Remaining error branches — direct unit coverage to avoid subprocess cost.
# --------------------------------------------------------------------------


def test_load_spec_other_oserror_raises_spec_error(tmp_path: Path) -> None:
    # Reading a directory (rather than a regular file) raises `IsADirectoryError`
    # (a subclass of `OSError` that is NOT `FileNotFoundError`) — the only
    # convenient way to hit the generic OSError branch without root.
    dir_path = tmp_path / "i-am-a-dir"
    dir_path.mkdir()
    with pytest.raises(SpecError, match=r"failed to read"):
        load_spec(dir_path)


def test_coerce_args_validation_error_inside_variadic_raises_spec_error() -> None:
    raw = {"values": ["1", "two", "3"]}  # `two` won't coerce to int
    with pytest.raises(SpecError, match=r"invalid value for `values`"):
        _coerce_args(_variadic_int, raw)


def test_main_exits_2_when_spec_env_unset(monkeypatch: pytest.MonkeyPatch) -> None:
    # Direct unit-style invocation: covers the `except SpecError` branch
    # of main() without paying for a subprocess. The existing subprocess
    # test verifies stderr text; this one verifies the return code path.
    monkeypatch.delenv("TOOLR_SPEC_FILE", raising=False)
    assert main() == 2


def test_main_delegates_to_run_on_valid_spec(
    monkeypatch: pytest.MonkeyPatch,
    make_spec_file: Callable[..., Path],
) -> None:
    spec_path = make_spec_file()
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))

    captured: dict[str, object] = {}

    def fake_run(spec: RunnerSpec) -> int:
        captured["spec"] = spec
        return 0

    monkeypatch.setattr("toolr._runner.run", fake_run)
    assert main() == 0
    assert isinstance(captured["spec"], RunnerSpec)


# --------------------------------------------------------------------------
# Subprocess: top-level main() formatting on a malformed spec file.
# --------------------------------------------------------------------------


def test_main_prints_spec_error_and_exits_2_for_missing_spec_file(tmp_path: Path) -> None:
    env = os.environ.copy()
    missing = tmp_path / "no-such-file.json"
    env["TOOLR_SPEC_FILE"] = str(missing)
    result = subprocess.run(
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 2
    assert "toolr runner:" in result.stderr
    assert "not found" in result.stderr


def test_main_prints_spec_error_for_invalid_json(tmp_path: Path) -> None:
    spec_path = tmp_path / "spec.json"
    spec_path.write_text("{not valid json")
    env = os.environ.copy()
    env["TOOLR_SPEC_FILE"] = str(spec_path)
    result = subprocess.run(
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 2
    assert "not valid JSON" in result.stderr


def test_main_module_invocation_clean_exit(tmp_path: Path) -> None:
    """End-to-end: well-formed spec + ctx.exit(None) → exit 0."""
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "demo.py").write_text(
        textwrap.dedent(
            """
            from toolr import command_group

            group = command_group("demo", "Demo", description="demo group")

            @group.command
            def noop(ctx) -> None:
                pass
            """
        )
    )
    spec_payload = {
        "schema_version": SCHEMA_VERSION,
        "group": "demo",
        "command": "noop",
        "module": "tools.demo",
        "function": "noop",
        "args": {},
        "context": {
            "repo_root": str(tmp_path),
            "verbosity": "normal",
            "timestamps": False,
            "log_level": "INFO",
        },
    }
    spec_path = tmp_path / "spec.json"
    spec_path.write_text(json.dumps(spec_payload))
    env = os.environ.copy()
    env["TOOLR_SPEC_FILE"] = str(spec_path)
    env["PYTHONPATH"] = str(tmp_path) + os.pathsep + env.get("PYTHONPATH", "")
    result = subprocess.run(
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, f"stderr:\n{result.stderr}"
