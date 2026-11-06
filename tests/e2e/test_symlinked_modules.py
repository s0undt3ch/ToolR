"""`toolr` and `CommandsTester.discover()` refuse symlinked `tools/` modules alike (F-17)."""

from __future__ import annotations

from collections.abc import Callable

import pytest

from toolr._exc import ToolrError
from toolr.testing import CommandsTester

from tests.conftest import ToolsProject

pytestmark = pytest.mark.skip_on_windows(reason="symlinks need a privilege on Windows")

DEMO_MODULE = '''
from toolr import command_group

group = command_group("demo", "Demo", "Demo commands")


@group.command
def hello(ctx):
    """Say hello."""
'''
LINKED_MODULE = DEMO_MODULE.replace("demo", "linked").replace("hello", "hi")


def test_binary_and_discovery_report_the_same_links(
    make_tools_project: Callable[..., ToolsProject],
) -> None:
    project = make_tools_project(demo=DEMO_MODULE)
    tools = project.root / "tools"
    shared = project.root / "shared"
    shared.mkdir()
    (shared / "linked.py").write_text(LINKED_MODULE)
    (tools / "linked.py").symlink_to("../shared/linked.py")
    (tools / "sub").mkdir()
    (tools / "sub" / "other.py").symlink_to("../../shared/linked.py")

    result = project.run("--help")
    assert result.returncode != 0, result.stdout
    _, found, body = result.stderr.partition("symlinked modules are not supported")
    assert found, result.stderr

    with CommandsTester(search_path=project.root) as tester, pytest.raises(ToolrError) as excinfo:
        tester.discover()

    assert found + body.rstrip("\n") == str(excinfo.value)
    assert str(excinfo.value) == (
        "symlinked modules are not supported (2):\n"
        "  - tools/linked.py\n"
        "  - tools/sub/other.py\n"
        "Put the file itself under tools/, or share it as a Python package."
    )
