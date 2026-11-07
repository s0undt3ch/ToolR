"""Tests for ``tools/version.py``.

After delegating Cargo.toml writes to ``cargo set-version`` (cargo-edit), the
remaining Python surface is:

* ``_read_workspace_version`` — a stdlib ``tomllib`` reader.
* ``_parse_release_version`` / ``_cargo_version`` — parse a
  version with ``packaging`` and render the SemVer form Cargo needs.
* ``_compute_dev_version`` — derives a dev version from the latest tag, tested
  against real git repositories.
* ``bump`` — validates ``--new-version`` before writing the GitHub Actions files.
* ``_set_action_yml_default_version`` — the action.yml bake-in.

``_set_workspace_version`` is intentionally not tested directly — it shells out
to ``cargo set-version`` and adds no logic worth covering in isolation.
"""

from __future__ import annotations

import os
import re
import textwrap
from collections.abc import Callable
from pathlib import Path
from unittest import mock

import pytest
from packaging.version import Version

from toolr.testing import ContextForTesting
from toolr.testing import make_context

from tools.version import TODAY_VERSION
from tools.version import _cargo_version
from tools.version import _compute_dev_version
from tools.version import _parse_release_version
from tools.version import _read_workspace_version
from tools.version import _set_action_yml_default_version
from tools.version import bump

from tests._git import Git

GITHUB_FILES = ("GITHUB_OUTPUT", "GITHUB_ENV", "GITHUB_STEP_SUMMARY")


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


# --------------------------------------------------------------------
# Parsing and rendering
# --------------------------------------------------------------------


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("0.36.0", "0.36.0"),
        ("v0.36.0", "0.36.0"),
        ("1.0.0-rc1", "1.0.0rc1"),
        ("1.0.0rc1", "1.0.0rc1"),
        ("v1.0.0-rc.1", "1.0.0rc1"),
        ("1.0.0-alpha.2", "1.0.0a2"),
        ("1.0.0-beta.3", "1.0.0b3"),
    ],
)
def test_parse_release_version_accepts(raw: str, expected: str) -> None:
    assert str(_parse_release_version(raw)) == expected


@pytest.mark.parametrize(
    "raw",
    [
        "0.36",
        "0.36.0.1",
        "vv0.36.0",
        "latest",
        "",
        "1.0.0-1",  # PEP 440 reads this as a post-release
        "1.0.0.post1",
        "1.0.0.dev1",
        "1!1.0.0",
        "1.0.0+local",
    ],
)
def test_parse_release_version_rejects(raw: str) -> None:
    with pytest.raises(ValueError, match=re.escape(repr(raw))):
        _parse_release_version(raw)


@pytest.mark.parametrize(
    ("pep440", "cargo"),
    [
        ("0.36.0", "0.36.0"),
        ("1.0.0rc1", "1.0.0-rc.1"),
        ("1.0.0a2", "1.0.0-alpha.2"),
        ("1.0.0b2", "1.0.0-beta.2"),
        ("1.0.1.dev3", "1.0.1-dev3"),
        ("1.0.0rc2.dev3", "1.0.0-rc.2.dev3"),
        ("1.0.1.dev3+gabc1234", "1.0.1-dev3+gabc1234"),
    ],
)
def test_cargo_version_maps_back_to_pep440(pep440: str, cargo: str) -> None:
    """The Cargo form normalises back to the PEP 440 version, as maturin does for the wheels."""
    rendered = _cargo_version(Version(pep440))
    assert rendered == cargo
    assert str(Version(rendered)) == pep440


# --------------------------------------------------------------------
# `_compute_dev_version` and `bump`, against real git repositories
# --------------------------------------------------------------------


@pytest.fixture
def release_repo(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    git: Git,
) -> Callable[..., ContextForTesting]:
    """Factory: a git repo with an optional tag and ``commits`` commits after it.

    Changes into the repo (``ctx.run`` runs git in the current directory) and
    returns a real ``Context`` for it. Every GitHub Actions variable this module
    reads is cleared; tests opt back in.
    """
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", os.devnull)
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    for name in ("GITHUB_EVENT_NAME", *GITHUB_FILES):
        monkeypatch.delenv(name, raising=False)

    def make(*tags: str, commits: int = 0) -> ContextForTesting:
        repo = git.make_repo(
            tmp_path / "repo",
            commits=["initial", *(f"change {index}" for index in range(commits))],
            tags=dict.fromkeys(tags, 0),
        )
        monkeypatch.chdir(repo)
        return make_context(repo)

    return make


@pytest.fixture
def github_files(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, Path]:
    """Point the GitHub Actions output, env and step-summary files into ``tmp_path``."""
    paths = {name: tmp_path / name.lower() for name in GITHUB_FILES}
    for name, path in paths.items():
        monkeypatch.setenv(name, str(path))
    return paths


def _dev_version(ctx: ContextForTesting) -> str:
    """``_compute_dev_version`` in the Cargo form ``bump`` writes."""
    return _cargo_version(_compute_dev_version(ctx))


def _short_sha(git: Git, ctx: ContextForTesting) -> str:
    return git(ctx.repo_root, "rev-parse", "--short", "HEAD").strip()


def test_compute_dev_version_after_final_tag(
    release_repo: Callable[..., ContextForTesting],
) -> None:
    """After ``v0.20.0``, the dev version targets the next patch release."""
    ctx = release_repo("v0.20.0", commits=2)
    assert _dev_version(ctx) == "0.20.1-dev2"


def test_compute_dev_version_pull_request_appends_sha(
    release_repo: Callable[..., ContextForTesting],
    monkeypatch: pytest.MonkeyPatch,
    git: Git,
) -> None:
    """Pull-request builds append ``+g<sha>`` so concurrent PRs don't collide."""
    ctx = release_repo("v0.11.0", commits=7)
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    assert _dev_version(ctx) == f"0.11.1-dev7+g{_short_sha(git, ctx)}"


@pytest.mark.parametrize(
    ("tag", "expected"),
    [
        # The tag spelling the release pipeline produces.
        ("v1.0.0-rc.1", "1.0.0-rc.2.dev3"),
        # A hand-made tag, the shape from the F-26 report: `git describe` gave
        # `v1.0.0-rc1-3-g<sha>`, which split into four parts.
        ("v1.0.0-rc1", "1.0.0-rc.2.dev3"),
        ("v1.0.0-beta.1", "1.0.0-beta.2.dev3"),
    ],
)
def test_compute_dev_version_after_prerelease_tag(
    release_repo: Callable[..., ContextForTesting], tag: str, expected: str
) -> None:
    """After a pre-release, the dev version targets the next pre-release of the same kind."""
    ctx = release_repo(tag, commits=3)
    dev = _dev_version(ctx)
    assert dev == expected
    assert Version(tag) < Version(dev) < Version(tag.removeprefix("v").split("-")[0])


def test_compute_dev_version_prerelease_tag_pull_request(
    release_repo: Callable[..., ContextForTesting],
    monkeypatch: pytest.MonkeyPatch,
    git: Git,
) -> None:
    ctx = release_repo("v1.0.0-rc.1", commits=5)
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    assert _dev_version(ctx) == f"1.0.0-rc.2.dev5+g{_short_sha(git, ctx)}"


def test_compute_dev_version_on_the_tag(release_repo: Callable[..., ContextForTesting]) -> None:
    """Zero commits since the tag still yields a dev version above it."""
    ctx = release_repo("v0.20.0")
    assert _dev_version(ctx) == "0.20.1-dev0"


def test_compute_dev_version_no_tags_fallback(
    release_repo: Callable[..., ContextForTesting],
) -> None:
    """Without a matching tag, fall back to ``TODAY_VERSION`` and the total commit count."""
    ctx = release_repo("not-a-release", commits=2)
    major, minor, _ = TODAY_VERSION.split(".")
    assert _dev_version(ctx) == f"{major}.{minor}.1-dev3"


@pytest.mark.parametrize("tag", ["v1.2.3-foo", "v1.2.3.4"])
def test_compute_dev_version_unparsable_tag(
    release_repo: Callable[..., ContextForTesting], tag: str
) -> None:
    """A tag matching the glob that isn't a release version is an error naming the tag."""
    ctx = release_repo(tag, commits=1)
    with pytest.raises(SystemExit) as excinfo:
        _dev_version(ctx)
    assert excinfo.value.code == 1
    assert tag in ctx.stderr


def test_bump_strips_leading_v(
    release_repo: Callable[..., ContextForTesting], github_files: dict[str, Path]
) -> None:
    """``--new-version v0.36.0`` releases ``0.36.0``; the tag check looks for ``v0.36.0``."""
    # `vnext` and the floating `v0.36` aren't release versions; the tag check skips them.
    ctx = release_repo("v0.35.0", "vnext", "v0.36", commits=1)
    bump(ctx, new_version="v0.36.0", check_existing_tag=True)
    output = github_files["GITHUB_OUTPUT"].read_text()
    assert "release-version=0.36.0\n" in output
    assert "release-patch-name=toolr-0.36.0.patch\n" in output
    assert "release-prerelease=false\n" in output
    assert github_files["GITHUB_ENV"].read_text() == "RELEASE_VERSION=0.36.0\n"
    assert ctx.stdout.rstrip().endswith("0.36.0")


def test_bump_leading_v_finds_existing_tag(
    release_repo: Callable[..., ContextForTesting], github_files: dict[str, Path]
) -> None:
    """With the ``v`` stripped, an existing ``v0.36.0`` tag is found (not ``vv0.36.0``)."""
    ctx = release_repo("v0.36.0", commits=1)
    with pytest.raises(SystemExit) as excinfo:
        bump(ctx, new_version="v0.36.0", check_existing_tag=True)
    assert excinfo.value.code == 1
    assert "v0.36.0 already exists" in ctx.stderr
    assert not any(path.exists() for path in github_files.values())


def test_bump_prerelease_version(
    release_repo: Callable[..., ContextForTesting], github_files: dict[str, Path]
) -> None:
    """A PEP 440 pre-release is released in its Cargo form and flagged as a pre-release."""
    ctx = release_repo("v0.36.0", commits=1)
    bump(ctx, new_version="1.0.0rc1", check_existing_tag=True)
    output = github_files["GITHUB_OUTPUT"].read_text()
    assert "release-version=1.0.0-rc.1\n" in output
    assert "release-patch-name=toolr-1.0.0-rc.1.patch\n" in output
    assert "release-prerelease=true\n" in output
    assert github_files["GITHUB_ENV"].read_text() == "RELEASE_VERSION=1.0.0-rc.1\n"
    assert "`1.0.0-rc.1`" in github_files["GITHUB_STEP_SUMMARY"].read_text()


@pytest.mark.parametrize("new_version", ["0.36", "latest", "1.0.0-1"])
def test_bump_rejects_malformed_version_before_writing(
    release_repo: Callable[..., ContextForTesting],
    github_files: dict[str, Path],
    new_version: str,
) -> None:
    """A malformed ``--new-version`` fails before any GitHub Actions file is written."""
    ctx = release_repo("v0.35.0", commits=1)
    with pytest.raises(SystemExit) as excinfo:
        bump(ctx, new_version=new_version, check_existing_tag=True, include_action=True, write=True)
    assert excinfo.value.code == 1
    assert repr(new_version) in ctx.stderr
    assert "Running" not in ctx.stdout
    assert not any(path.exists() for path in github_files.values())


def test_bump_dev_version_after_prerelease_tag(
    release_repo: Callable[..., ContextForTesting], github_files: dict[str, Path]
) -> None:
    """Without ``--new-version``, a repo tagged ``v1.0.0-rc.1`` gets the next rc's dev version."""
    ctx = release_repo("v1.0.0-rc.1", commits=3)
    bump(ctx)
    output = github_files["GITHUB_OUTPUT"].read_text()
    assert "release-version=1.0.0-rc.2.dev3\n" in output
    assert "release-prerelease=true\n" in output


def test_bump_dev_version_after_final_tag_is_not_prerelease(
    release_repo: Callable[..., ContextForTesting], github_files: dict[str, Path]
) -> None:
    """A dev build after a final release isn't flagged, so CI dry runs keep the final-release steps."""
    ctx = release_repo("v0.20.0", commits=2)
    bump(ctx, check_existing_tag=True)
    output = github_files["GITHUB_OUTPUT"].read_text()
    assert "release-version=0.20.1-dev2\n" in output
    assert "release-prerelease=false\n" in output


@pytest.mark.parametrize("existing", ["v1.0.0-rc1", "v1.0.0rc1", "v1.0.0-rc.1"])
def test_bump_finds_existing_tag_in_any_spelling(
    release_repo: Callable[..., ContextForTesting],
    github_files: dict[str, Path],
    existing: str,
) -> None:
    """``1.0.0-rc1`` is released as ``v1.0.0-rc.1``; any tag for the same version stops it."""
    ctx = release_repo(existing, commits=1)
    with pytest.raises(SystemExit) as excinfo:
        bump(ctx, new_version="1.0.0-rc1", check_existing_tag=True)
    assert excinfo.value.code == 1
    assert f"Tag {existing} already exists for version 1.0.0-rc.1" in ctx.stderr
    assert not any(path.exists() for path in github_files.values())


@pytest.mark.skip_on_windows(reason="the `cargo` stub is a POSIX shell script")
def test_bump_write_sets_cargo_and_action_versions(
    release_repo: Callable[..., ContextForTesting],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """``--write --include-action`` hands the Cargo form to ``cargo set-version`` and ``action.yml``."""
    ctx = release_repo("v0.36.0", commits=1)
    (ctx.repo_root / "action.yml").write_text(textwrap.dedent(ACTION_YML_BASE), encoding="utf-8")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    cargo_args = tmp_path / "cargo-args"
    cargo = bin_dir / "cargo"
    cargo.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > "{cargo_args}"\n')
    cargo.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}{os.pathsep}{os.environ['PATH']}")

    bump(ctx, new_version="1.0.0rc1", include_action=True, write=True)

    assert cargo_args.read_text().splitlines() == ["set-version", "--workspace", "1.0.0-rc.1"]
    assert 'default: "1.0.0-rc.1"' in (ctx.repo_root / "action.yml").read_text(encoding="utf-8")


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


def test_set_action_yml_default_version_writes_dev_versions(
    action_yml: Callable[[str], Path],
) -> None:
    """Dev versions (containing a hyphen) must land in action.yml.

    `toolr version bump --include-action` runs on every push (PRs
    included) via ci.yml's `prepare-release` job. Exercising the
    bake-in regex against a real dev version on each of those runs
    is the point: regressions to the regex surface in regular CI,
    not only when a real release is cut. The dev version never
    escapes the runner because PR jobs cannot push.
    """
    path = action_yml(ACTION_YML_BASE)
    ctx = mock.Mock()
    _set_action_yml_default_version(ctx, "0.21.1-dev42+gabc1234", action_yml_path=path)
    body = path.read_text(encoding="utf-8")
    assert 'default: "0.21.1-dev42+gabc1234"' in body
    assert 'default: "0.20.0"' not in body


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
