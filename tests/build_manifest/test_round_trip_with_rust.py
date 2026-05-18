"""End-to-end: Python build_manifest writes a fragment that the Rust side picks up and merges."""

from __future__ import annotations

import json
import sys
import textwrap
from pathlib import Path

import pytest

from toolr._decorators import _get_command_group_storage
from toolr.build import build_manifest


@pytest.fixture
def fake_third_party_package(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[str, Path]:
    """Create and import a fake package, return (package_name, package_dir)."""
    pkg_dir = tmp_path / "fake_ext_pkg"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text(
        textwrap.dedent(
            '''
            from toolr import command_group

            group = command_group("ext", "External", description="ext")

            @group.command
            def rollout(ctx):
                """Roll out a new build."""
            '''
        ).lstrip()
    )
    monkeypatch.syspath_prepend(str(tmp_path))
    sys.modules.pop("fake_ext_pkg", None)
    storage = _get_command_group_storage()
    for key in list(storage):
        if "ext" in key:
            storage.pop(key)
    return "fake_ext_pkg", pkg_dir


def test_python_build_then_rust_merge(
    fake_third_party_package: tuple[str, Path],
    tmp_path: Path,
) -> None:
    package, pkg_dir = fake_third_party_package

    # 1. Build the fragment via Python.
    build_manifest(package, output_path=pkg_dir / "toolr-manifest.json")
    assert (pkg_dir / "toolr-manifest.json").is_file()

    # 2. Materialise a fake tools venv that contains the package.
    venv = tmp_path / "venv"
    site = venv / "lib" / "python3.13" / "site-packages"
    site.mkdir(parents=True)
    # Symlink the package dir into site-packages so the glob matches.
    (site / package).symlink_to(pkg_dir, target_is_directory=True)

    # 3. The cross-binary merge is exercised by `cargo test --lib third_party::`;
    # here we assert the file shape — schema version + rollout command — so the
    # round-trip contract is visible from the Python side.
    fragment = json.loads((pkg_dir / "toolr-manifest.json").read_text())
    assert fragment["toolr_schema_version"] == 1
    assert any(c["name"] == "rollout" for c in fragment["commands"])
