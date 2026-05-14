"""Tests for tools/version.py — version-bump implementation.

After the workspace split (Plan 12 Stage 7), `toolr version bump --write`
writes the new release version to `[workspace.package] version` in the
repository's root `Cargo.toml` (the new single source of truth for the
release) rather than `[project] version` in `pyproject.toml` (which no
longer exists at the root).
"""

from __future__ import annotations

import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest
from packaging.version import Version
from tools.version import _read_workspace_version
from tools.version import _write_workspace_version


@pytest.fixture
def cargo_toml(tmp_path: Path) -> Callable[[str], Path]:
    """Factory: write a Cargo.toml with the given body. Returns its path."""

    def _make(body: str) -> Path:
        path = tmp_path / "Cargo.toml"
        path.write_text(textwrap.dedent(body), encoding="utf-8")
        return path

    return _make


CARGO_TOML_BASE = """\
[workspace]
members = ["crates/toolr-core", "crates/toolr", "crates/toolr-py"]
resolver = "2"

[workspace.package]
version = "0.20.0"
edition = "2021"
authors = ["X <x@x.com>"]
license = "Apache-2.0"
repository = "https://github.com/example/example"

[workspace.dependencies]
serde = "1"
"""


def test_read_workspace_version(cargo_toml: Callable[[str], Path]) -> None:
    path = cargo_toml(CARGO_TOML_BASE)
    assert _read_workspace_version(path) == "0.20.0"


def test_write_workspace_version_updates_in_place(
    cargo_toml: Callable[[str], Path],
) -> None:
    path = cargo_toml(CARGO_TOML_BASE)
    _write_workspace_version("0.21.0", path)
    assert _read_workspace_version(path) == "0.21.0"

    # Confirm we did not touch unrelated keys around it.
    text = path.read_text(encoding="utf-8")
    assert 'edition = "2021"' in text
    assert 'authors = ["X <x@x.com>"]' in text
    assert "[workspace.dependencies]" in text
    assert 'serde = "1"' in text


def test_write_workspace_version_does_not_match_project_version(
    cargo_toml: Callable[[str], Path],
) -> None:
    """A `[project] version` lookalike inside a comment must not be matched."""
    body = CARGO_TOML_BASE.replace(
        "[workspace]",
        '# [project]\n# version = "0.99.0"\n\n[workspace]',
    )
    path = cargo_toml(body)
    _write_workspace_version("0.21.0", path)

    text = path.read_text(encoding="utf-8")
    # Comment is untouched.
    assert '# version = "0.99.0"' in text
    # Real workspace version was updated.
    assert _read_workspace_version(path) == "0.21.0"


def test_read_workspace_version_raises_when_missing(
    cargo_toml: Callable[[str], Path],
) -> None:
    path = cargo_toml("[workspace]\nmembers = []\n")
    with pytest.raises(ValueError, match=r"workspace\.package"):
        _read_workspace_version(path)


def test_write_workspace_version_raises_when_missing(
    cargo_toml: Callable[[str], Path],
) -> None:
    path = cargo_toml("[workspace]\nmembers = []\n")
    with pytest.raises(ValueError, match=r"workspace\.package"):
        _write_workspace_version("0.21.0", path)


def test_commented_section_header_is_ignored(
    cargo_toml: Callable[[str], Path],
) -> None:
    """A commented `# [workspace.package]` header (with a commented version line
    beneath it) must not be picked up by the regex when a real
    `[workspace.package]` block follows.
    """
    body = """\
    [workspace]
    members = ["crates/toolr-core", "crates/toolr", "crates/toolr-py"]
    resolver = "2"

    # [workspace.package]
    # version = "0.99.0"

    [workspace.package]
    version = "0.20.0"
    edition = "2021"
    """
    path = cargo_toml(body)
    assert _read_workspace_version(path) == "0.20.0"

    _write_workspace_version("0.21.0", path)
    text = path.read_text(encoding="utf-8")
    # The commented line must be untouched.
    assert '# version = "0.99.0"' in text
    # The real version must be updated.
    assert _read_workspace_version(path) == "0.21.0"


def test_commented_version_line_inside_real_section_is_ignored(
    cargo_toml: Callable[[str], Path],
) -> None:
    """A commented `# version = "..."` line inside the real
    `[workspace.package]` block must not be picked up; the live `version =`
    line below must be the one matched.
    """
    body = """\
    [workspace]
    members = ["crates/toolr-core", "crates/toolr", "crates/toolr-py"]
    resolver = "2"

    [workspace.package]
    # version = "0.99.0"
    version = "0.20.0"
    edition = "2021"
    """
    path = cargo_toml(body)
    assert _read_workspace_version(path) == "0.20.0"

    _write_workspace_version("0.21.0", path)
    text = path.read_text(encoding="utf-8")
    # Commented version line stays exactly as it was.
    assert '# version = "0.99.0"' in text
    # Live version was updated.
    assert _read_workspace_version(path) == "0.21.0"
    # And we did not produce two live `version = ...` lines.
    live_version_lines = [
        line for line in text.splitlines() if line.strip().startswith("version =") and not line.strip().startswith("#")
    ]
    assert len(live_version_lines) == 1
    assert '"0.21.0"' in live_version_lines[0]


def test_write_workspace_version_preserves_hyphenated_dev_version(
    cargo_toml: Callable[[str], Path],
) -> None:
    """Hyphenated dev versions (semver pre-release form) must round-trip
    verbatim — packaging.Version would normalize `0.11.0-dev42` to
    `0.11.0.dev42`, which cargo then rejects.
    """
    path = cargo_toml(CARGO_TOML_BASE)
    _write_workspace_version("0.11.0-dev42", path)
    # The literal hyphenated form must end up in the file.
    text = path.read_text(encoding="utf-8")
    assert 'version = "0.11.0-dev42"' in text
    assert _read_workspace_version(path) == "0.11.0-dev42"
    # And it still parses as a valid PEP 440 version (normalized internally).
    assert Version(_read_workspace_version(path)) == Version("0.11.0.dev42")
