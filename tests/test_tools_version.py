"""Tests for ``tools/version.py``.

After delegating Cargo.toml writes to ``cargo set-version`` (cargo-edit), the
remaining Python surface is:

* ``_read_workspace_version`` — a stdlib ``tomllib`` reader.
* ``_compute_dev_version`` — derives an ``X.Y.Z-devN[+SHA]`` string from
  ``git describe``.

We test the reader against a factory-built ``Cargo.toml`` and the dev-version
helper by stubbing the ``Context.run`` calls. ``_set_workspace_version`` is
intentionally not tested directly — it shells out to ``cargo set-version`` and
adds no logic worth covering in isolation.
"""

from __future__ import annotations

import io
import textwrap
from collections.abc import Callable
from pathlib import Path
from typing import Any
from unittest import mock

import pytest
from tools.version import TODAY_VERSION
from tools.version import _compute_dev_version
from tools.version import _read_workspace_version


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


def test_read_workspace_version_happy_path(cargo_toml: Callable[[str], Path]) -> None:
    path = cargo_toml(CARGO_TOML_BASE)
    assert _read_workspace_version(path) == "0.20.0"


def test_read_workspace_version_raises_when_missing_table(
    cargo_toml: Callable[[str], Path],
) -> None:
    path = cargo_toml("[workspace]\nmembers = []\n")
    with pytest.raises(ValueError, match=r"workspace\.package"):
        _read_workspace_version(path)


def test_read_workspace_version_raises_when_missing_key(
    cargo_toml: Callable[[str], Path],
) -> None:
    path = cargo_toml('[workspace.package]\nedition = "2021"\n')
    with pytest.raises(ValueError, match=r"workspace\.package"):
        _read_workspace_version(path)


def test_read_workspace_version_malformed_toml(
    cargo_toml: Callable[[str], Path],
) -> None:
    path = cargo_toml("this is not valid toml = = =\n[[\n")
    with pytest.raises(Exception) as excinfo:  # noqa: PT011 — tomllib.TOMLDecodeError
        _read_workspace_version(path)
    # tomllib.TOMLDecodeError subclasses ValueError; either way the message is
    # not the "No [workspace.package]" one (we want the decode to surface).
    assert "workspace.package" not in str(excinfo.value)


class _FakeRun:
    """Stub for ``ctx.run`` — yields canned stdout for each successive call.

    Each entry in ``outputs`` is the bytes (or str, after encoding) of stdout
    the next ``ctx.run(...)`` should return. ``.stdout.read()`` is called once
    per result by ``_compute_dev_version``.
    """

    def __init__(self, outputs: list[str]) -> None:
        self._outputs = list(outputs)
        self.calls: list[tuple[Any, ...]] = []

    def __call__(self, *args: Any, **_kwargs: Any) -> Any:
        self.calls.append(args)
        if not self._outputs:
            msg = f"Unexpected extra ctx.run call: {args}"
            raise AssertionError(msg)
        out = self._outputs.pop(0)
        return mock.Mock(stdout=io.StringIO(out), stderr=io.StringIO(""), returncode=0)


def _ctx_with_run(run: _FakeRun) -> Any:
    """A minimal Context-shaped object exposing only ``.run``."""
    return mock.Mock(run=run)


def test_compute_dev_version_from_git_describe(monkeypatch: pytest.MonkeyPatch) -> None:
    """`git describe` returns ``v0.20.0-42-gabc1234`` → ``0.20.0-dev42``."""
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    run = _FakeRun(["v0.20.0-42-gabc1234\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == "0.20.0-dev42"


def test_compute_dev_version_pull_request_appends_sha(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Pull-request builds append ``+gSHA`` as semver build metadata."""
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    run = _FakeRun(["v0.11.0-7-gdeadbee\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == "0.11.0-dev7+gdeadbee"


def test_compute_dev_version_no_tags_fallback(monkeypatch: pytest.MonkeyPatch) -> None:
    """Empty `git describe` falls back to ``TODAY_VERSION-dev<count>``."""
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    run = _FakeRun(["\n", "13\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == f"{TODAY_VERSION}-dev13"


def test_compute_dev_version_no_tags_and_no_commits(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """If both `git describe` and `rev-list --count` return empty, count is 0."""
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    run = _FakeRun(["\n", "\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == f"{TODAY_VERSION}-dev0"
