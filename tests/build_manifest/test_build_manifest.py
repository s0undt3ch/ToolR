from __future__ import annotations

import json
import sys
import textwrap
from pathlib import Path

import pytest

from toolr._registry import _get_command_group_storage
from toolr.build import BuildManifestError
from toolr.build import build_manifest
from toolr.build import main as build_cli


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
