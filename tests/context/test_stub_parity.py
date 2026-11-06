"""Guard the hand-written `_context.pyi` stub against drifting from `_context.py`.

mypy prefers a `.pyi` over its `.py`, so a name missing from the stub is invisible to a
run that omits the `.py` source, even though the runtime module defines it.
"""

from __future__ import annotations

import ast
from dataclasses import dataclass
from pathlib import Path

import pytest

PKG = Path(__file__).parents[2] / "crates/toolr-py/python/toolr"


@dataclass(frozen=True)
class DeclaredNames:
    top_level: set[str]
    context_members: set[str]


@pytest.fixture
def stub_names() -> tuple[DeclaredNames, DeclaredNames]:
    """Parse the `.py` and the `.pyi` once per test, with the same rules for both."""

    def parse(path: Path) -> DeclaredNames:
        tree = ast.parse(path.read_text(encoding="utf-8"))
        top_level: set[str] = set()
        context_members: set[str] = set()
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.ClassDef)):
                top_level.add(node.name)
            elif isinstance(node, ast.Assign):
                top_level.update(t.id for t in node.targets if isinstance(t, ast.Name))
            elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
                top_level.add(node.target.id)
            if isinstance(node, ast.ClassDef) and node.name == "Context":
                for member in node.body:
                    if isinstance(member, ast.FunctionDef):
                        context_members.add(member.name)
                    elif isinstance(member, ast.AnnAssign) and isinstance(member.target, ast.Name):
                        context_members.add(member.target.id)
        return DeclaredNames(top_level, context_members)

    return parse(PKG / "_context.py"), parse(PKG / "_context.pyi")


def test_context_stub_declares_every_top_level_name(
    stub_names: tuple[DeclaredNames, DeclaredNames],
) -> None:
    source, stub = stub_names
    assert source.top_level - stub.top_level == set()


def test_context_stub_declares_every_context_member(
    stub_names: tuple[DeclaredNames, DeclaredNames],
) -> None:
    source, stub = stub_names
    assert source.context_members - stub.context_members == set()
