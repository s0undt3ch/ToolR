"""`toolr` and `CommandsTester` read every corpus fixture the same way.

Each directory under `files/parity_corpus/` is copied into `tools/`, or into the
project root when it has its own `tools/`, so a module can sit outside `tools/`. An `ok_*` case
must build on both sides with the same groups and commands; an `err_*` case
must fail on both with the same message. An `ok_warn_*` case re-declares a
group with different fields: both sides keep the first and report the same
conflict, which no other `ok_*` case may do.
"""

from __future__ import annotations

import json
import logging
import re
import shutil
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr._decorators import CommandGroup
from toolr._decorators import _group_path
from toolr.testing import CommandsTester

from tests.conftest import ToolsProject

CORPUS = Path(__file__).parent / "files" / "parity_corpus"
CASES = sorted(path.name for path in CORPUS.iterdir() if path.is_dir())
# `toolr` gives the group the description a later declaration passes; the
# runtime keeps the empty one the title-only call created.
NEEDS_LATER_DESCRIPTION = {"ok_group_described_after_title_only"}


def _param(case: str) -> object:
    if case not in NEEDS_LATER_DESCRIPTION:
        return case
    reason = "needs `command_group` to take a later declaration's description"
    return pytest.param(case, marks=pytest.mark.xfail(strict=True, reason=reason))


PARAMS = [_param(case) for case in CASES]


@pytest.fixture
def corpus_project(
    make_tools_project: Callable[..., ToolsProject],
) -> Callable[[str], ToolsProject]:
    """Factory: a project whose `tools/` holds one corpus case."""

    def _make(case: str) -> ToolsProject:
        project = make_tools_project()
        source = CORPUS / case
        target = project.root if (source / "tools").is_dir() else project.root / "tools"
        shutil.copytree(source, target, dirs_exist_ok=True)
        return project

    return _make


def _manifest_commands(project: ToolsProject) -> dict[str, list[str]]:
    """`{group path: sorted command names}` from the manifest `toolr` wrote."""
    manifest = json.loads((project.root / "tools" / ".toolr-manifest.json").read_text())
    found: dict[str, list[str]] = {}
    for group in manifest["groups"]:
        path = f"{group['parent']}.{group['name']}" if group.get("parent") else group["name"]
        found[path] = []
    for command in manifest["commands"]:
        found[command["group"]].append(command["name"])
    return {path: sorted(names) for path, names in found.items()}


def _manifest_groups(project: ToolsProject) -> dict[str, tuple[str, str]]:
    """`{group path: (title, description)}` from the manifest `toolr` wrote."""
    manifest = json.loads((project.root / "tools" / ".toolr-manifest.json").read_text())
    return {
        f"{group['parent']}.{group['name']}" if group.get("parent") else group["name"]: (
            group["title"],
            group["description"],
        )
        for group in manifest["groups"]
    }


def _rust_conflicts(stderr: str) -> list[tuple[str, str]]:
    """`(group path, differing fields)` for each group conflict `toolr` warned about."""
    pattern = r"group `([^`]+)` is declared more than once; .* ignoring the different (.+) in "
    return re.findall(pattern, stderr)


def _python_conflicts(caplog: pytest.LogCaptureFixture) -> list[tuple[str, str]]:
    """`(group path, differing fields)` for each conflict `command_group` logged."""
    pattern = r"Command group 'tools\.([^']+)' is already declared; ignoring the different (.+) of"
    return [
        (match[1], match[2])
        for record in caplog.records
        if (match := re.match(pattern, record.getMessage()))
    ]


def _discover_and_collect(tester: CommandsTester) -> None:
    """Some errors `discover()` raises before importing anything, others show after."""
    tester.discover()
    tester.collected_command_groups()


def test_corpus_covers_both_outcomes() -> None:
    """A case named neither way would be skipped by both branches below."""
    assert all(case.startswith(("ok_", "err_")) for case in CASES), CASES


@pytest.mark.parametrize("case", PARAMS)
def test_parser_and_tester_agree(
    case: str,
    corpus_project: Callable[[str], ToolsProject],
    caplog: pytest.LogCaptureFixture,
) -> None:
    project = corpus_project(case)
    proc = project.run("--help")
    # The level `command_group` logs a conflict at doesn't matter, only that it does.
    caplog.set_level(logging.DEBUG, logger="toolr")
    with CommandsTester(search_path=project.root) as tester:
        if case.startswith("ok_"):
            tester.discover()
            groups = {
                _group_path(key): group
                for key, group in tester.collected_command_groups().items()
                if isinstance(group, CommandGroup)
            }
            assert proc.returncode == 0, proc.stderr
            assert _manifest_commands(project) == {
                path: sorted(group.get_commands()) for path, group in groups.items()
            }
            assert _manifest_groups(project) == {
                path: (group.title, group.description) for path, group in groups.items()
            }
            conflicts = _rust_conflicts(proc.stderr)
            assert conflicts == _python_conflicts(caplog)
            assert bool(conflicts) == case.startswith("ok_warn_"), proc.stderr
            return
        with pytest.raises(ValueError) as excinfo:  # noqa: PT011  # the text is compared below
            _discover_and_collect(tester)
    assert proc.returncode != 0, proc.stdout
    assert f"toolr: {excinfo.value}\n" in proc.stderr


def test_suggestion_tie_picks_the_smallest_name(
    corpus_project: Callable[[str], ToolsProject],
) -> None:
    """`aa` is one edit from `ab`, `ca` and `ac`; both sides suggest `ab`."""
    proc = corpus_project("err_suggestion_tie").run("--help")
    assert "Did you mean `ab`?" in proc.stderr
