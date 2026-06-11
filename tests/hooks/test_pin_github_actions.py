"""Tests for the ``pin-github-actions`` pre-commit hook.

The hook locks `uses:` refs to SHAs and verifies already-pinned ones against
their tag comment in a single default pass. It is a standalone script under
``.pre-commit-hooks/`` (not an importable package), loaded via
``importlib.util.spec_from_file_location``. No test touches the network: the
per-line verify tests inject a fake resolver into ``verify_action_line``, and
the ``process_file`` / cache tests stub the ``gh`` subprocess via ``gh_stub``.
"""

from __future__ import annotations

import importlib.util
import json
from collections.abc import Callable
from pathlib import Path
from types import ModuleType

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[2]
_HOOK_PATH = _REPO_ROOT / ".pre-commit-hooks" / "pin-github-actions.py"

# A pretend "good" pin: this SHA is what tag v6.0.3 resolves to.
GOOD_SHA = "df4cb1c069e1874edd31b4311f1884172cec0e10"
TAG = "v6.0.3"


def _load_hook() -> ModuleType:
    spec = importlib.util.spec_from_file_location("pin_github_actions", _HOOK_PATH)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def hook() -> ModuleType:
    """The loaded ``pin-github-actions`` hook module."""
    return _load_hook()


@pytest.fixture
def resolver() -> Callable[[str, str, str], str | None]:
    """Fake resolver: only ``actions/checkout@v6.0.3`` resolves, to GOOD_SHA."""

    def resolve(owner: str, repo: str, ref: str) -> str | None:
        if (owner, repo, ref) == ("actions", "checkout", TAG):
            return GOOD_SHA
        return None

    return resolve


@pytest.fixture
def workflow_factory(tmp_path: Path) -> Callable[[str], Path]:
    """Factory writing a workflow file with the given body, returning its path."""
    created: list[Path] = []

    def make(body: str) -> Path:
        path = tmp_path / f"workflow_{len(created)}.yml"
        path.write_text(body)
        created.append(path)
        return path

    return make


def test_doctored_sha_fails_verify(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """A line whose pinned SHA disagrees with its tag comment is rejected."""
    line = "      - uses: actions/checkout@0000000000000000000000000000000000000000 # v6.0.3"
    error = hook.verify_action_line(line, resolve=resolver)
    assert error is not None
    assert "mismatch" in error
    assert GOOD_SHA in error  # expected SHA surfaced
    assert "0000000000000000000000000000000000000000" in error  # actual SHA surfaced


def test_correctly_pinned_line_passes(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """A line whose pinned SHA matches the resolved tag comment passes."""
    line = f"      - uses: actions/checkout@{GOOD_SHA} # {TAG}"
    assert hook.verify_action_line(line, resolve=resolver) is None


def test_sha_pin_without_comment_fails(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """A SHA pin with no ``# <tag>`` comment cannot be verified -> error."""
    line = f"      - uses: actions/checkout@{GOOD_SHA}"
    error = hook.verify_action_line(line, resolve=resolver)
    assert error is not None
    assert "no '# <tag>' comment" in error


def test_unresolvable_tag_comment_fails(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """A tag comment the resolver can't map to a SHA is an error, not a pass."""
    line = f"      - uses: actions/checkout@{GOOD_SHA} # v9.9.9"
    error = hook.verify_action_line(line, resolve=resolver)
    assert error is not None
    assert "could not resolve tag" in error


def test_unpinned_tag_ref_is_out_of_scope(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """A not-yet-pinned tag ref is the autofix hook's job; verify ignores it."""
    line = "      - uses: actions/checkout@v6.0.3"
    assert hook.verify_action_line(line, resolve=resolver) is None


def test_non_uses_line_ignored(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """Lines that aren't ``uses:`` declarations return None."""
    assert hook.verify_action_line("      - run: echo hello", resolve=resolver) is None


def test_repointed_tag_is_caught(
    hook: ModuleType, resolver: Callable[[str, str, str], str | None]
) -> None:
    """A tag that upstream now resolves to a new SHA is caught — verification
    compares against the comment's tag, not the SHA already in the file."""

    def repointed(owner: str, repo: str, ref: str) -> str | None:
        # v6.0.3 now points at a different SHA than what the file pins.
        return "ffffffffffffffffffffffffffffffffffffffff"

    line = f"      - uses: actions/checkout@{GOOD_SHA} # {TAG}"
    error = hook.verify_action_line(line, resolve=repointed)
    assert error is not None
    assert "mismatch" in error


@pytest.fixture
def gh_stub(hook: ModuleType, monkeypatch: pytest.MonkeyPatch) -> Callable[[str], list[list[str]]]:
    """Factory: stub the ``gh`` subprocess so the real (cached)
    ``get_commit_sha`` resolves every ref to the given SHA.

    Clears the per-process resolution cache on each install (so one test's
    stub never leaks into the next) and returns the list of recorded ``gh``
    invocations, letting a test assert how many times ``gh`` was called.
    """

    def install(sha: str) -> list[list[str]]:
        calls: list[list[str]] = []

        def fake_run(argv: list[str], **kwargs: object) -> object:
            calls.append(argv)

            class _Result:
                returncode = 0
                stdout = json.dumps({"sha": sha})

            return _Result()

        hook.get_commit_sha.cache_clear()
        monkeypatch.setattr(hook.subprocess, "run", fake_run)
        return calls

    return install


def test_process_file_verifies_existing_pins(
    hook: ModuleType,
    gh_stub: Callable[[str], list[list[str]]],
    workflow_factory: Callable[[str], Path],
) -> None:
    """Default ``process_file`` verifies an already-pinned ref without
    rewriting it: a matching pin is clean, a mismatch is reported with a
    ``file:line`` prefix and leaves the file untouched."""
    gh_stub(GOOD_SHA)

    ok = workflow_factory(f"      - uses: actions/checkout@{GOOD_SHA} # {TAG}\n")
    assert hook.process_file(ok) == (False, [])

    bad = workflow_factory(
        "name: demo\n"
        "jobs:\n"
        "  build:\n"
        "    steps:\n"
        f"      - uses: actions/checkout@{'0' * 40} # {TAG}\n"
    )
    modified, errors = hook.process_file(bad)
    assert modified is False  # already pinned -> verified, never rewritten
    assert len(errors) == 1
    assert errors[0].startswith(f"{bad}:5:")
    assert "mismatch" in errors[0]


def test_get_commit_sha_is_memoised(
    hook: ModuleType, gh_stub: Callable[[str], list[list[str]]]
) -> None:
    """The same owner/repo@ref resolves once per run — a repeated lookup reuses
    the in-process cache instead of re-querying ``gh``."""
    calls = gh_stub(GOOD_SHA)
    assert hook.get_commit_sha("actions", "checkout", TAG) == GOOD_SHA
    assert hook.get_commit_sha("actions", "checkout", TAG) == GOOD_SHA
    assert len(calls) == 1


def test_missing_gh_is_a_hard_error(hook: ModuleType, monkeypatch: pytest.MonkeyPatch) -> None:
    """Without ``gh`` and without ``--exit-zero``, the hook hard-errors (exit
    2) rather than silently passing as if everything verified."""
    monkeypatch.setattr(hook, "check_gh_cli", lambda: False)
    monkeypatch.setattr(hook.sys, "argv", ["pin-github-actions", "x.yml"])
    assert hook.main() == 2


def test_exit_zero_softens_missing_gh(hook: ModuleType, monkeypatch: pytest.MonkeyPatch) -> None:
    """``--exit-zero`` downgrades a missing ``gh`` to a warning + exit 0."""
    monkeypatch.setattr(hook, "check_gh_cli", lambda: False)
    monkeypatch.setattr(hook.sys, "argv", ["pin-github-actions", "--exit-zero", "x.yml"])
    assert hook.main() == 0
