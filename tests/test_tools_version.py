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
from tools.version import _bump_patch
from tools.version import _compute_dev_version
from tools.version import _read_workspace_version
from tools.version import _set_action_yml_default_version


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


def test_bump_patch() -> None:
    """`_bump_patch` increments the patch component only."""
    assert _bump_patch("0.11.1") == "0.11.2"
    assert _bump_patch("0.20.0") == "0.20.1"
    assert _bump_patch("1.2.9") == "1.2.10"


def test_compute_dev_version_from_git_describe(monkeypatch: pytest.MonkeyPatch) -> None:
    """`git describe` returns ``v0.20.0-42-gabc1234`` → ``0.20.1-dev42``.

    The patch is bumped relative to the tag base so the result is semver-greater
    than the tag (cargo set-version refuses to "downgrade", and a pre-release of
    the SAME version is semver-wise a downgrade).
    """
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    run = _FakeRun(["v0.20.0-42-gabc1234\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == "0.20.1-dev42"


def test_compute_dev_version_pull_request_appends_sha(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Pull-request builds append ``+gSHA`` as semver build metadata."""
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    run = _FakeRun(["v0.11.0-7-gdeadbee\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == "0.11.1-dev7+gdeadbee"


def test_compute_dev_version_no_tags_fallback(monkeypatch: pytest.MonkeyPatch) -> None:
    """Empty `git describe` falls back to ``bump(TODAY_VERSION)-dev<count>``."""
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    run = _FakeRun(["\n", "13\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == f"{_bump_patch(TODAY_VERSION)}-dev13"


def test_compute_dev_version_no_tags_and_no_commits(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """If both `git describe` and `rev-list --count` return empty, count is 0."""
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)
    run = _FakeRun(["\n", "\n"])
    ctx = _ctx_with_run(run)
    assert _compute_dev_version(ctx) == f"{_bump_patch(TODAY_VERSION)}-dev0"


# --------------------------------------------------------------------
# `_set_action_yml_default_version`
# --------------------------------------------------------------------


@pytest.fixture
def action_yml(tmp_path: Path) -> Callable[[str], Path]:
    """Factory: write an action.yml with the given body. Returns its path."""

    def _make(body: str) -> Path:
        path = tmp_path / "action.yml"
        path.write_text(textwrap.dedent(body), encoding="utf-8")
        return path

    return _make


ACTION_YML_BASE = """\
name: "Setup ToolR"
description: "test action"

inputs:
  version:
    description: |
      ToolR version to install.
      Multi-line description body.
    default: "0.20.0"

  skip-attestation:
    description: "skip attestation"
    default: "false"

  cache-prefix:
    description: "cache prefix"
    default: "setup-toolr"
"""


def test_set_action_yml_default_version_bumps_only_version_input(
    action_yml: Callable[[str], Path],
) -> None:
    """The bake-in must rewrite ONLY the `version:` input's default.

    Sibling inputs (`skip-attestation`, `cache-prefix`) have their own
    `default:` lines at the same indent — those must not be touched.
    """
    path = action_yml(ACTION_YML_BASE)
    ctx = mock.Mock()
    _set_action_yml_default_version(ctx, "0.21.5", action_yml_path=path)

    body = path.read_text(encoding="utf-8")
    # version default updated
    assert 'default: "0.21.5"' in body
    assert 'default: "0.20.0"' not in body
    # sibling defaults preserved
    assert 'default: "false"' in body
    assert 'default: "setup-toolr"' in body


def test_set_action_yml_default_version_skips_dev_versions(
    action_yml: Callable[[str], Path],
) -> None:
    """Dev versions (containing a hyphen) must not land in action.yml.

    `toolr version bump` runs on every push (computing dev versions
    like ``0.21.1-dev42``). Writing those to action.yml would set the
    SHA-pin fallback to a nonexistent release.
    """
    path = action_yml(ACTION_YML_BASE)
    original = path.read_text(encoding="utf-8")
    ctx = mock.Mock()
    _set_action_yml_default_version(ctx, "0.21.1-dev42", action_yml_path=path)
    assert path.read_text(encoding="utf-8") == original


def test_set_action_yml_default_version_hard_fails_on_missing_block(
    action_yml: Callable[[str], Path],
) -> None:
    """If the `version:` input vanishes from action.yml, fail loudly.

    Silently skipping would let the bake-in regress whenever the
    file's structure changes — defeating the whole point of the
    release-prep automation. Bail with a non-zero exit instead so the
    release workflow halts.
    """
    path = action_yml("""\
name: "Setup ToolR"
description: "no version input here"

inputs:
  cache-prefix:
    default: "setup-toolr"
""")
    ctx = mock.Mock()
    _set_action_yml_default_version(ctx, "0.21.0", action_yml_path=path)
    # ctx.exit must have been called with a non-zero code.
    ctx.error.assert_called()
    ctx.exit.assert_called_once()
    exit_code = ctx.exit.call_args.args[0]
    assert exit_code != 0


def test_set_action_yml_default_version_round_trip(
    action_yml: Callable[[str], Path],
) -> None:
    """Re-running with the same version is a no-op (idempotent)."""
    path = action_yml(ACTION_YML_BASE)
    ctx = mock.Mock()
    _set_action_yml_default_version(ctx, "0.20.0", action_yml_path=path)
    # The body should be identical apart from the (unchanged) default.
    # We assert byte-for-byte equivalence here as a strong idempotency
    # check; if the regex starts to over-match, this test will catch it.
    assert path.read_text(encoding="utf-8") == textwrap.dedent(ACTION_YML_BASE)
