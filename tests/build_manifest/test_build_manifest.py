from __future__ import annotations

import json
import sys
import textwrap
from pathlib import Path
from types import SimpleNamespace
from typing import Literal

import pytest

from toolr._decorators import _get_command_group_storage
from toolr.build import BuildManifestError
from toolr.build import _argument_kind
from toolr.build import _resolve_package_root
from toolr.build import _serialize_default
from toolr.build import _serialize_type
from toolr.build import _validate_fragment
from toolr.build import build_manifest
from toolr.build import main as build_cli
from toolr.utils._signature import KwArg
from toolr.utils._signature import VarArg


@pytest.fixture
def fake_package(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> str:
    """Materialise a tiny third-party package on disk and import it."""
    pkg = tmp_path / "fake_toolr_pkg"
    pkg.mkdir()
    (pkg / "__init__.py").write_text(
        textwrap.dedent(
            '''
            from toolr import command_group

            group = command_group("ext", "External group", description="external")

            @group.command
            def rollout(ctx):
                """Roll out a new build."""
            '''
        ).lstrip()
    )
    monkeypatch.syspath_prepend(str(tmp_path))
    # Ensure a fresh import per test — the registry is process-global.
    sys.modules.pop("fake_toolr_pkg", None)
    # Drop any previously-registered group with the same name so each
    # test runs against its own materialised package.
    storage = _get_command_group_storage()
    for key in list(storage):
        if "ext" in key:
            storage.pop(key)
    return "fake_toolr_pkg"


def test_build_writes_fragment_to_default_path(fake_package: str) -> None:
    result = build_manifest(fake_package)
    assert result.output_path.is_file()
    fragment = json.loads(result.output_path.read_text())
    assert fragment["toolr_schema_version"] == 1
    assert fragment["package"] == fake_package
    names = [c["name"] for c in fragment["commands"]]
    assert "rollout" in names


def test_build_check_mode_detects_drift(fake_package: str, tmp_path: Path) -> None:
    path = tmp_path / "out.json"
    path.write_text("not the current fragment")
    result = build_manifest(fake_package, output_path=path, check=True)
    assert result.drift is True
    # File on disk must not have been overwritten in check mode.
    assert path.read_text() == "not the current fragment"


def test_build_check_mode_no_drift_on_match(fake_package: str, tmp_path: Path) -> None:
    path = tmp_path / "out.json"
    build_manifest(fake_package, output_path=path)
    result = build_manifest(fake_package, output_path=path, check=True)
    assert result.drift is False


def test_build_raises_when_package_declares_no_commands(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    pkg = tmp_path / "empty_pkg"
    pkg.mkdir()
    (pkg / "__init__.py").write_text("")
    monkeypatch.syspath_prepend(str(tmp_path))
    sys.modules.pop("empty_pkg", None)
    with pytest.raises(BuildManifestError):
        build_manifest("empty_pkg")


def test_cli_writes_manifest(fake_package: str, tmp_path: Path) -> None:
    out = tmp_path / "manifest.json"
    rc = build_cli([fake_package, "--output", str(out), "--quiet"])
    assert rc == 0
    assert out.is_file()


def test_cli_check_exits_2_on_drift(fake_package: str, tmp_path: Path) -> None:
    out = tmp_path / "manifest.json"
    out.write_text("stale")
    rc = build_cli([fake_package, "--output", str(out), "--check", "--quiet"])
    assert rc == 2


def test_cli_check_exits_0_when_up_to_date(fake_package: str, tmp_path: Path) -> None:
    out = tmp_path / "manifest.json"
    build_cli([fake_package, "--output", str(out), "--quiet"])
    rc = build_cli([fake_package, "--output", str(out), "--check", "--quiet"])
    assert rc == 0


def test_cli_exits_1_on_missing_package() -> None:
    rc = build_cli(["this_package_does_not_exist_xyz", "--quiet"])
    assert rc == 1


def test_validate_rejects_bad_arg_kind() -> None:
    bad = {
        "toolr_schema_version": 1,
        "package": "p",
        "groups": [],
        "commands": [
            {
                "name": "n",
                "group": "g",
                "module": "m",
                "function": "f",
                "arguments": [{"name": "x", "kind": "bogus"}],
            }
        ],
    }
    with pytest.raises(BuildManifestError):
        _validate_fragment(bad)


# --------------------------------------------------------------------
# _validate_fragment — each rejection branch.
# --------------------------------------------------------------------


def test_validate_rejects_non_int_schema_version() -> None:
    with pytest.raises(BuildManifestError, match="toolr_schema_version"):
        _validate_fragment({"toolr_schema_version": "1", "package": "p"})


def test_validate_rejects_zero_schema_version() -> None:
    with pytest.raises(BuildManifestError, match="toolr_schema_version"):
        _validate_fragment({"toolr_schema_version": 0, "package": "p"})


def test_validate_rejects_non_string_package() -> None:
    with pytest.raises(BuildManifestError, match="package"):
        _validate_fragment({"toolr_schema_version": 1, "package": 42})


def test_validate_rejects_group_missing_name() -> None:
    with pytest.raises(BuildManifestError, match="group missing"):
        _validate_fragment(
            {
                "toolr_schema_version": 1,
                "package": "p",
                "groups": [{"title": "no-name"}],
                "commands": [],
            }
        )


@pytest.mark.parametrize("missing_key", ["name", "group", "module", "function"])
def test_validate_rejects_command_missing_required_field(missing_key: str) -> None:
    cmd: dict[str, str | list[dict[str, str]]] = {
        "name": "n",
        "group": "g",
        "module": "m",
        "function": "f",
    }
    del cmd[missing_key]
    fragment = {
        "toolr_schema_version": 1,
        "package": "p",
        "groups": [],
        "commands": [cmd],
    }
    with pytest.raises(BuildManifestError, match=missing_key):
        _validate_fragment(fragment)


# --------------------------------------------------------------------
# _resolve_package_root — namespace-package rejection.
# --------------------------------------------------------------------


def test_resolve_package_root_rejects_namespace_package() -> None:
    # A SimpleNamespace stand-in for a namespace package: no __file__.
    module = SimpleNamespace()
    with pytest.raises(BuildManifestError, match=r"(?i)namespace"):
        _resolve_package_root(module, "ghost_pkg")


# --------------------------------------------------------------------
# _argument_kind / _serialize_default / _serialize_type — pure helpers.
# --------------------------------------------------------------------


def _make_kwarg(action: str | None):
    return KwArg(
        name="x",
        type=bool if action in ("store_true", "store_false") else str,
        action=action,
        description="",
        aliases=["-x"],
        default=False,
        metavar=None,
        choices=None,
        nargs=None,
        required=False,
        group=None,
    )


def _make_vararg():
    return VarArg(
        name="extra",
        type=str,
        action=None,
        description="",
        aliases=["extra"],
        default=None,
        metavar=None,
        choices=None,
        nargs="*",
    )


@pytest.mark.parametrize("action", ["store_true", "store_false"])
def test_argument_kind_classifies_kwarg_flag_actions(action: str) -> None:

    assert _argument_kind(_make_kwarg(action)) == "flag"


def test_argument_kind_classifies_plain_kwarg_as_optional() -> None:

    assert _argument_kind(_make_kwarg(None)) == "optional"


def test_argument_kind_classifies_vararg_as_positional() -> None:

    assert _argument_kind(_make_vararg()) == "positional"


def test_serialize_default_returns_none_for_none() -> None:

    assert _serialize_default(None) is None


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("hello", "'hello'"),
        (42, "42"),
        (3.14, "3.14"),
        (True, "True"),
        ([1, 2], "[1, 2]"),
    ],
)
def test_serialize_default_uses_repr_for_non_none_values(value: object, expected: str) -> None:

    assert _serialize_default(value) == expected


def test_serialize_type_returns_none_for_none() -> None:

    assert _serialize_type(None) is None


def test_serialize_type_uses_dunder_name_for_plain_types() -> None:

    assert _serialize_type(int) == "int"
    assert _serialize_type(str) == "str"


def test_serialize_type_handles_generic_origin() -> None:

    # `get_origin(Literal["a"])` returns `Literal`; the function pulls
    # the name off the origin so the Rust side recognises it.
    out = _serialize_type(Literal["a", "b"])
    assert out is not None
    assert out == "Literal" or "Literal" in out


def test_serialize_type_falls_back_to_str_for_non_named_annotations() -> None:

    # A plain forward-reference string has neither __name__ nor a
    # `get_origin()` result — falls through to `str(annotation)`.
    out = _serialize_type("MyType")
    assert isinstance(out, str)


# --------------------------------------------------------------------
# CLI — branches not exercised by the existing happy-path tests.
# --------------------------------------------------------------------


def test_cli_quiet_suppresses_check_success_message(
    fake_package: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    out = tmp_path / "manifest.json"
    # Write the up-to-date manifest first.
    rc = build_cli([fake_package, "--output", str(out)])
    assert rc == 0
    capsys.readouterr()  # discard previous output

    rc = build_cli([fake_package, "--output", str(out), "--check", "--quiet"])
    assert rc == 0
    captured = capsys.readouterr()
    assert captured.out == ""  # --quiet suppresses the success message


def test_cli_check_prints_success_when_up_to_date_and_not_quiet(
    fake_package: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    out = tmp_path / "manifest.json"
    build_cli([fake_package, "--output", str(out)])
    capsys.readouterr()

    rc = build_cli([fake_package, "--output", str(out), "--check"])
    assert rc == 0
    captured = capsys.readouterr()
    assert "up to date" in captured.out


def test_cli_quiet_suppresses_wrote_message(
    fake_package: str,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    out = tmp_path / "manifest.json"
    rc = build_cli([fake_package, "--output", str(out), "--quiet"])
    assert rc == 0
    captured = capsys.readouterr()
    assert captured.out == ""


def test_cli_exits_1_with_build_manifest_error_for_empty_package(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    # Materialise a package with no command_group registrations.
    pkg = tmp_path / "empty_toolr_pkg"
    pkg.mkdir()
    (pkg / "__init__.py").write_text("# no commands\n")
    monkeypatch.syspath_prepend(str(tmp_path))
    sys.modules.pop("empty_toolr_pkg", None)

    rc = build_cli(["empty_toolr_pkg"])
    assert rc == 1
    captured = capsys.readouterr()
    assert "no toolr commands" in captured.err
