from __future__ import annotations

import json
import os
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr._runner import SCHEMA_VERSION
from toolr._runner import SpecError
from toolr._runner import load_spec
from toolr._runner import load_spec_from_env

#: POSIX ownership / permission-bit semantics that the spec-file provenance
#: checks rely on (``os.getuid``, meaningful mode bits, symlinks without
#: elevation). Skipped on Windows, which lacks them.
posix_only = pytest.mark.skipif(
    not hasattr(os, "getuid"), reason="requires POSIX ownership/permission semantics"
)


@pytest.fixture
def spec_file(tmp_path: Path) -> Callable[..., Path]:
    """Factory: write a runner spec JSON to ``tmp_path/spec.json``. Returns its path."""

    def _make(**overrides: object) -> Path:
        payload: dict[str, object] = {
            "schema_version": SCHEMA_VERSION,
            "group": "ci",
            "command": "hello",
            "module": "tools.ci",
            "function": "hello",
            "args": {},
            "context": {
                "repo_root": str(tmp_path),
                "verbosity": "normal",
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        payload.update(overrides)
        spec_path = tmp_path / "spec.json"
        spec_path.write_text(json.dumps(payload))
        return spec_path

    return _make


def test_load_spec_reads_file_and_decodes(spec_file: Callable[..., Path], tmp_path: Path) -> None:
    spec_path = spec_file()
    spec = load_spec(spec_path)
    assert spec.group == "ci"
    assert spec.command == "hello"
    assert spec.context.repo_root == str(tmp_path)


def test_load_spec_rejects_unknown_schema_version(spec_file: Callable[..., Path]) -> None:
    spec_path = spec_file(schema_version=999)
    with pytest.raises(SpecError) as exc_info:
        load_spec(spec_path)
    msg = str(exc_info.value)
    # The error message names both schema numbers ("schema 1, but … schema 999")
    # and points the user at the fix. Assert on the parts users will grep for.
    assert "schema" in msg
    assert "999" in msg


def test_load_spec_raises_when_file_missing(tmp_path: Path) -> None:
    with pytest.raises(SpecError) as exc_info:
        load_spec(tmp_path / "absent.json")
    assert "not found" in str(exc_info.value).lower() or "no such" in str(exc_info.value).lower()


def test_load_spec_raises_on_malformed_json(tmp_path: Path) -> None:
    spec_path = tmp_path / "bad.json"
    spec_path.write_text("{not json")
    with pytest.raises(SpecError):
        load_spec(spec_path)


def test_load_spec_from_env(
    spec_file: Callable[..., Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    spec_path = spec_file()
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))
    spec = load_spec_from_env()
    assert spec.group == "ci"


def test_load_spec_from_env_raises_when_unset(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("TOOLR_SPEC_FILE", raising=False)
    with pytest.raises(SpecError) as exc_info:
        load_spec_from_env()
    assert "TOOLR_SPEC_FILE" in str(exc_info.value)


@posix_only
def test_load_spec_from_env_rejects_group_or_world_writable(
    spec_file: Callable[..., Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A spec file anyone but the owner can write is refused before it is read
    (defense-in-depth: a forgeable spec is a forgeable import target)."""
    spec_path = spec_file()
    spec_path.chmod(0o666)
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))
    with pytest.raises(SpecError) as exc_info:
        load_spec_from_env()
    assert "writable" in str(exc_info.value)


@posix_only
def test_load_spec_from_env_rejects_symlink(
    spec_file: Callable[..., Path],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A symlinked spec path is refused — the binary never writes one, so it
    signals a swapped path."""
    spec_path = spec_file()
    link = tmp_path / "spec-link.json"
    link.symlink_to(spec_path)
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(link))
    with pytest.raises(SpecError) as exc_info:
        load_spec_from_env()
    assert "symlink" in str(exc_info.value)


def test_load_spec_from_env_accepts_private_regular_file(
    spec_file: Callable[..., Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A normal 0600 regular file we own loads cleanly — the guard doesn't get
    in the way of the real dispatch path."""
    spec_path = spec_file()
    spec_path.chmod(0o600)
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))
    spec = load_spec_from_env()
    assert spec.group == "ci"


def test_load_spec_from_env_unlinks_after_reading(
    spec_file: Callable[..., Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Once read, the spec file is removed, so a normal run leaves nothing
    behind in TMPDIR (SEC-14A). The decoded spec is unaffected."""
    spec_path = spec_file()
    monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))
    spec = load_spec_from_env()
    assert spec.group == "ci"
    assert not spec_path.exists()
