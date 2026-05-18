"""Test that entry points registered under `toolr.commands` are discovered."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path


def test_entry_point_module_groups_appear(tmp_path: Path) -> None:
    # Install a fake package directly into a tmp sys.path entry, then
    # register it as a `toolr.commands` entry point via a dist-info dir.
    pkg = tmp_path / "fake_toolr_legacy"
    pkg.mkdir()
    (pkg / "__init__.py").write_text(
        textwrap.dedent(
            '''
            from toolr import command_group

            group = command_group("legacy", "Legacy group", description="Legacy.")

            @group.command
            def widget(ctx):
                """Widget command."""
                return 0
            '''
        )
    )

    dist_info = tmp_path / "fake_toolr_legacy-0.0.0.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Metadata-Version: 2.1\nName: fake-toolr-legacy\nVersion: 0.0.0\n")
    (dist_info / "entry_points.txt").write_text("[toolr.commands]\nlegacy = fake_toolr_legacy\n")

    proc = subprocess.run(
        [sys.executable, "-m", "toolr._introspect"],
        capture_output=True,
        text=True,
        check=True,
        env={**os.environ, "PYTHONPATH": str(tmp_path)},
    )
    payload = json.loads(proc.stdout)
    names = {g["name"] for g in payload["groups"]}
    assert "legacy" in names, payload
    cmd_names = {(c["group"], c["name"]) for c in payload["commands"]}
    assert ("legacy", "widget") in cmd_names, payload
