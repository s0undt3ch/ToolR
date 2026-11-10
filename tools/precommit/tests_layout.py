"""
Pre-commit hook command that keeps the ``tests/`` tree to the project's layout rules.
"""

from __future__ import annotations

from pathlib import Path

from rich.markup import escape

from toolr import Context

from ._common import group

from tools.ci import REPO_ROOT

FIXTURES_DIR = "files"


def _test_tree_paths(tests_dir: Path) -> list[Path]:
    """Return the files under ``tests_dir`` relative to it, skipping bytecode caches and dotfiles."""
    return sorted(
        path.relative_to(tests_dir)
        for path in tests_dir.rglob("*")
        if path.is_file()
        and "__pycache__" not in path.parts
        and not path.name.startswith(".")
        and path.suffix != ".pyc"
    )


def layout_problems(tests_dir: Path) -> list[str]:
    """Return one message per breach of the ``tests/`` layout rules.

    1. A directory holding ``.py`` files has an ``__init__.py``.
    2. A non-``.py`` file lives inside a ``files/`` directory.
    3. No ``test_*.py`` sits directly in ``tests/``; tests live in ``tests/<area>/``.

    ``files/`` trees are fixture data: they are exempt from rule 1 and are where rule 2 sends data.
    """
    paths = [path for path in _test_tree_paths(tests_dir) if FIXTURES_DIR not in path.parts[:-1]]
    package_dirs = {path.parent for path in paths if path.suffix == ".py"}
    problems = [
        f"add an `__init__.py` to tests/{directory.as_posix()}"
        for directory in sorted(package_dirs)
        if not (tests_dir / directory / "__init__.py").is_file()
    ]
    problems.extend(
        f"move tests/{path.as_posix()} under a `{FIXTURES_DIR}/` directory next to the tests that use it"
        for path in paths
        if path.suffix != ".py"
    )
    problems.extend(
        f"move tests/{path.as_posix()} into a `tests/<area>/` directory that mirrors the source it covers"
        for path in paths
        if len(path.parts) == 1 and path.suffix == ".py" and path.name.startswith("test_")
    )
    return problems


@group.command
def check_tests_layout(ctx: Context) -> None:
    """Check the ``tests/`` layout rules.

    Every directory holding a ``.py`` file has an ``__init__.py`` (no namespace packages), so test
    modules import as ``tests.…`` and can't shadow real packages such as ``tools``. Every non-``.py``
    file lives in a ``files/`` directory next to the tests that use it, and ``files/`` trees are data,
    not packages. No ``test_*.py`` sits directly in ``tests/``.
    """
    problems = layout_problems(REPO_ROOT / "tests")
    for problem in problems:
        ctx.error(escape(problem))
    if problems:
        ctx.exit(1)
