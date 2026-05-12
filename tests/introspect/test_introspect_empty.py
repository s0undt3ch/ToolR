"""Smoke tests for the dynamic-manifest introspection helper."""

from __future__ import annotations

import json
import subprocess
import sys


def test_empty_project_emits_valid_payload() -> None:
    """`python -m toolr._introspect` with no tools_root produces a parseable empty payload."""
    proc = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr._introspect"],
        capture_output=True,
        text=True,
        check=True,
    )
    payload = json.loads(proc.stdout)
    assert payload["payload_schema_version"] == 1
    assert payload["groups"] == []
    assert payload["commands"] == []
    assert payload["warnings"] == []


def test_help_flag_exits_zero() -> None:
    proc = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr._introspect", "--help"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert proc.returncode == 0
    assert "Dump toolr dynamic-layer manifest" in proc.stdout
