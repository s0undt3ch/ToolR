"""`toolr` and `CommandsTester` read every corpus fixture the same way.

Each directory under `files/parity_corpus/` is copied into `tools/`, or into the
project root when it has its own `tools/`, so a module can sit outside `tools/`. An `ok_*` case
must build on both sides with the same groups and commands; an `err_*` case
must fail on both with the same message.
"""

from __future__ import annotations

import json
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


def test_corpus_covers_both_outcomes() -> None:
    """A case named neither way would be skipped by both branches below."""
    assert all(case.startswith(("ok_", "err_")) for case in CASES), CASES


@pytest.mark.parametrize("case", CASES)
def test_parser_and_tester_agree(case: str, corpus_project: Callable[[str], ToolsProject]) -> None:
    project = corpus_project(case)
    proc = project.run("--help")
    with CommandsTester(search_path=project.root) as tester:
        tester.discover()
        if case.startswith("ok_"):
            groups = tester.collected_command_groups()
            assert proc.returncode == 0, proc.stderr
            assert _manifest_commands(project) == {
                _group_path(key): sorted(group.get_commands())
                for key, group in groups.items()
                if isinstance(group, CommandGroup)
            }
            return
        with pytest.raises(ValueError) as excinfo:  # noqa: PT011  # the text is compared below
            tester.collected_command_groups()
    assert proc.returncode != 0, proc.stdout
    assert f"toolr: {excinfo.value}\n" in proc.stderr


def test_suggestion_tie_picks_the_smallest_name(
    corpus_project: Callable[[str], ToolsProject],
) -> None:
    """`aa` is one edit from `ab`, `ca` and `ac`; both sides suggest `ab`."""
    proc = corpus_project("err_suggestion_tie").run("--help")
    assert "Did you mean `ab`?" in proc.stderr
