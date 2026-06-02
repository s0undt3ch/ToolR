"""Unit-level coverage for `toolr._introspect`.

The existing tests in this directory (`test_introspect_empty.py`,
`test_introspect_tools_walk.py`)
all spawn `python -m toolr._introspect` as a subprocess, which is a
faithful test of the wire format but doesn't credit any coverage to
the in-process test runner. This file calls the module's helpers
directly so the coverage counter actually moves.

Tests run against a clean copy of the command-group registry per
test (opt in via ``@pytest.mark.usefixtures("clean_registry")``) so
user-defined tools fixtures from one test don't leak into another.
"""

from __future__ import annotations

import importlib
import json
import subprocess
import sys
import textwrap
from collections.abc import Iterator
from pathlib import Path

import pytest

from toolr._decorators import _get_command_group_storage
from toolr._introspect import PAYLOAD_SCHEMA_VERSION
from toolr._introspect import _command_entry
from toolr._introspect import _ensure_tools_on_syspath
from toolr._introspect import _import_tools_modules
from toolr._introspect import _split_leaf
from toolr._introspect import _walk_registry
from toolr._introspect import build_payload
from toolr._introspect import main

# --------------------------------------------------------------------
# Test isolation helpers.
# --------------------------------------------------------------------


@pytest.fixture
def clean_registry() -> Iterator[None]:
    """Snapshot + restore the command-group registry across each test.

    `command_group(...)` calls inside test fixtures register into a
    process-wide dict. Without this fixture, registrations leak into
    later tests and `_walk_registry` produces non-deterministic output.

    Apply via ``@pytest.mark.usefixtures("clean_registry")`` — the
    fixture has no value to inject so consumers don't need to name it
    as a parameter.
    """
    storage = _get_command_group_storage()
    saved = dict(storage)
    storage.clear()
    yield
    storage.clear()
    storage.update(saved)


@pytest.fixture
def isolated_sys_path() -> Iterator[None]:
    """Snapshot + restore `sys.path` and any `tools*` modules in `sys.modules`.

    `_ensure_tools_on_syspath` mutates `sys.path`; `_import_tools_modules`
    imports `tools` and submodules. We undo both so tests are independent.

    Crucially, we also *evict* any `tools.*` modules already loaded by an
    earlier test (or by the dev workspace's real `tools` package) before
    yielding — otherwise `importlib.import_module("tools")` returns the
    pre-loaded one and the test's tmp_path scaffold is silently ignored.
    """
    saved_path = list(sys.path)
    saved_modules = {
        name: sys.modules[name]
        for name in list(sys.modules)
        if name == "tools" or name.startswith("tools.")
    }
    for name in saved_modules:
        del sys.modules[name]
    yield
    sys.path[:] = saved_path
    for name in list(sys.modules):
        if (name == "tools" or name.startswith("tools.")) and name not in saved_modules:
            del sys.modules[name]
    for name, module in saved_modules.items():
        sys.modules[name] = module


@pytest.fixture
def tools_fixture(tmp_path: Path, isolated_sys_path: None) -> Path:
    """Scaffold a tools/ directory with a simple demo module and return its path."""
    del isolated_sys_path  # ensure the path/module state is restored after the test
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    # Uses the legacy `@group.command` bound decorator; emits a deprecation
    # warning at import time, which is the very behavior we want covered.
    (tools / "demo.py").write_text(
        textwrap.dedent(
            '''
            """Demo dynamic-layer module."""
            from toolr import command_group

            group = command_group("demo", "Demo", description="demo group")

            @group.command
            def hello(ctx) -> None:
                """Say hello.

                Detailed description spans
                multiple lines.
                """
                ctx.print("hi")
            '''
        )
    )
    return tools


# --------------------------------------------------------------------
# _ensure_tools_on_syspath
# --------------------------------------------------------------------


@pytest.mark.usefixtures("isolated_sys_path")
def test_ensure_tools_on_syspath_noop_when_path_is_empty() -> None:
    before = list(sys.path)
    _ensure_tools_on_syspath(None)
    _ensure_tools_on_syspath("")
    assert sys.path == before


@pytest.mark.usefixtures("isolated_sys_path")
def test_ensure_tools_on_syspath_inserts_parent_of_tools_root(tmp_path: Path) -> None:
    tools_root = tmp_path / "tools"
    tools_root.mkdir()
    _ensure_tools_on_syspath(str(tools_root))
    assert str(tmp_path) in sys.path
    # Calling again is idempotent — the entry is only inserted once.
    before_second = list(sys.path)
    _ensure_tools_on_syspath(str(tools_root))
    assert sys.path == before_second


# --------------------------------------------------------------------
# _split_leaf
# --------------------------------------------------------------------


def test_split_leaf_returns_none_parent_for_top_level() -> None:
    leaf, parent = _split_leaf("toplevel")
    assert leaf == "toplevel"
    assert parent is None


def test_split_leaf_returns_parent_path_for_nested() -> None:
    leaf, parent = _split_leaf("a.b.c")
    assert leaf == "c"
    assert parent == "a.b"


# --------------------------------------------------------------------
# _command_entry
# --------------------------------------------------------------------


def test_command_entry_extracts_summary_and_description() -> None:
    def fn() -> None:
        """Summary line.

        Multi-line description
        with a wrap.
        """

    entry = _command_entry("ci", "hello", fn)
    assert entry["name"] == "hello"
    assert entry["group"] == "ci"
    assert entry["summary"] == "Summary line."
    assert "Multi-line description" in entry["description"]
    assert entry["arguments"] == []
    assert entry["imports"] == []
    assert entry["origin"] == "dynamic"


def test_command_entry_handles_undocumented_callable() -> None:
    def fn() -> None: ...

    entry = _command_entry("ci", "bare", fn)
    assert entry["summary"] == ""
    assert entry["description"] == ""


def test_command_entry_falls_back_to_command_name_when_callable_has_no_dunder_name() -> None:
    # Plain instance: inherits __module__ from its class, but has no __name__
    # of its own — exercises the `getattr(func, "__name__", cmd_name)` branch.
    class _Bare:
        pass

    entry = _command_entry("ci", "weird", _Bare())
    assert entry["function"] == "weird"
    # `__module__` is inherited from the defining class — for a test-local class
    # that's this test file's dotted path. We don't assert the exact value (it
    # changes if the file moves), only that the field exists as a non-None str.
    assert isinstance(entry["module"], str)


def test_command_entry_falls_back_to_empty_string_when_module_is_falsy() -> None:
    # `getattr(..., "__module__", "") or ""` collapses None to "".
    class _NoModule:
        __module__ = None  # type: ignore[assignment]

    entry = _command_entry("ci", "weird", _NoModule())
    assert entry["module"] == ""


# --------------------------------------------------------------------
# _import_tools_modules
# --------------------------------------------------------------------


@pytest.mark.usefixtures("isolated_sys_path")
def test_import_tools_modules_returns_silently_when_no_tools_package(tmp_path: Path) -> None:
    # Place a directory that is NOT named `tools` on sys.path — import
    # of `tools` will hit ModuleNotFoundError and bail without errors.
    sys.path.insert(0, str(tmp_path))
    warnings: list[str] = []
    _import_tools_modules(warnings)
    assert warnings == []


@pytest.mark.usefixtures("clean_registry")
def test_import_tools_modules_walks_user_tools(tools_fixture: Path) -> None:
    _ensure_tools_on_syspath(str(tools_fixture))
    warnings: list[str] = []
    _import_tools_modules(warnings)
    assert warnings == []
    # The walk imported tools.demo, which registered the `demo` group.
    storage = _get_command_group_storage()
    assert any(name.endswith(".demo") or name == "tools.demo" for name in storage)


@pytest.mark.usefixtures("isolated_sys_path", "clean_registry")
def test_import_tools_modules_collects_warnings_for_broken_modules(tmp_path: Path) -> None:
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "broken.py").write_text("raise RuntimeError('boom')\n")
    _ensure_tools_on_syspath(str(tools))
    warnings: list[str] = []
    _import_tools_modules(warnings)
    assert any("broken" in w for w in warnings), f"warnings: {warnings}"


# --------------------------------------------------------------------
# _walk_registry + build_payload
# --------------------------------------------------------------------


@pytest.mark.usefixtures("clean_registry")
def test_walk_registry_returns_empty_for_unpopulated_registry() -> None:
    groups, commands = _walk_registry()
    assert groups == []
    assert commands == []


@pytest.mark.usefixtures("clean_registry")
def test_walk_registry_yields_groups_and_commands(tools_fixture: Path) -> None:
    _ensure_tools_on_syspath(str(tools_fixture))
    _import_tools_modules([])
    groups, commands = _walk_registry()
    assert any(g["name"] == "demo" for g in groups)
    assert any(c["name"] == "hello" and c["group"] == "demo" for c in commands)
    # Top-level groups serialise `parent` as None.
    demo = next(g for g in groups if g["name"] == "demo")
    assert demo["parent"] is None
    assert demo["origin"] == "dynamic"


@pytest.mark.usefixtures("clean_registry")
def test_build_payload_returns_schema_and_collected_state(tools_fixture: Path) -> None:
    payload = build_payload(str(tools_fixture))
    assert payload["payload_schema_version"] == PAYLOAD_SCHEMA_VERSION
    assert any(g["name"] == "demo" for g in payload["groups"])
    assert any(c["name"] == "hello" for c in payload["commands"])
    assert payload["warnings"] == []


@pytest.mark.usefixtures("clean_registry", "isolated_sys_path")
def test_build_payload_with_no_tools_root_returns_empty_state(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # `build_payload(None)` still tries to import `tools`. The dev workspace
    # has a real `tools` package on sys.path that would register groups and
    # invalidate the "empty state" assertion — stub the import out.
    real_import_module = importlib.import_module

    def fake_import_module(name: str, package: str | None = None) -> object:
        if name == "tools" or name.startswith("tools."):
            raise ModuleNotFoundError(name)
        return real_import_module(name, package)

    monkeypatch.setattr("toolr._introspect.importlib.import_module", fake_import_module)
    payload = build_payload(None)
    assert payload["payload_schema_version"] == PAYLOAD_SCHEMA_VERSION
    assert payload["groups"] == []
    assert payload["commands"] == []


# --------------------------------------------------------------------
# main() — argparse + JSON output
# --------------------------------------------------------------------


@pytest.mark.usefixtures("clean_registry")
def test_main_emits_json_payload_to_stdout(
    capsys: pytest.CaptureFixture[str],
    tools_fixture: Path,
) -> None:
    rc = main(["--tools-root", str(tools_fixture)])
    assert rc == 0
    captured = capsys.readouterr()
    payload = json.loads(captured.out)
    assert payload["payload_schema_version"] == PAYLOAD_SCHEMA_VERSION
    assert any(g["name"] == "demo" for g in payload["groups"])


@pytest.mark.usefixtures("clean_registry", "isolated_sys_path")
def test_main_emits_empty_payload_when_no_tools_root(
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    real_import_module = importlib.import_module

    def fake_import_module(name: str, package: str | None = None) -> object:
        if name == "tools" or name.startswith("tools."):
            raise ModuleNotFoundError(name)
        return real_import_module(name, package)

    monkeypatch.setattr("toolr._introspect.importlib.import_module", fake_import_module)
    rc = main([])
    assert rc == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["groups"] == []
    assert payload["commands"] == []


# --------------------------------------------------------------------
# Module-as-script invocation. Kept as a subprocess test because
# `if __name__ == "__main__": raise SystemExit(main())` only fires
# when the file is executed via `python -m toolr._introspect` — there
# is no in-process way to exercise that block without manually
# importing-as-main. The test is fast (sub-second), and it preserves
# coverage credit for the trailing two lines of the module under
# tarpaulin's `--follow-exec` configuration that the rust tests use.
# --------------------------------------------------------------------


def test_introspect_module_runs_as_script() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "toolr._introspect"],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(result.stdout)
    assert payload["payload_schema_version"] == PAYLOAD_SCHEMA_VERSION
