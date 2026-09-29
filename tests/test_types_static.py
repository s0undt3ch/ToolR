"""mypy tells the path types apart from each other and from `pathlib.Path`.

It also pins that a derived path (`f / "x"`, `f.parent`) keeps its type,
which the docs warn about.
"""

from __future__ import annotations

import re
import textwrap
from pathlib import Path

import pytest
from mypy import api as mypy_api

SNIPPET = textwrap.dedent(
    """\
    from pathlib import Path

    from toolr.types import DirectoryPath, FilePath, WritableDirectoryPath


    def wants_path(p: Path) -> None: ...
    def wants_file(p: FilePath) -> None: ...
    def wants_dir(p: DirectoryPath) -> None: ...


    def accepted(f: FilePath, w: WritableDirectoryPath) -> None:
        wants_path(f)
        wants_dir(w)
        child: Path = w / "x"


    def rejected(p: Path) -> None:
        wants_file(p)  # E


    # Pinned, not endorsed: derived paths keep the type (typeshed's `Self`),
    # though nothing checked them. If this starts erroring, update the docs.
    def derived(f: FilePath) -> None:
        wants_file(f / "x")
        wants_file(f.parent)
    """
)


@pytest.fixture
def snippet(tmp_path: Path) -> Path:
    (tmp_path / "mypy.ini").write_text("[mypy]\n")
    path = tmp_path / "snippet.py"
    path.write_text(SNIPPET)
    return path


def test_mypy_distinguishes_path_types(snippet: Path) -> None:
    stdout, stderr, _ = mypy_api.run(
        [
            str(snippet),
            "--config-file",
            str(snippet.parent / "mypy.ini"),
            "--strict",
            "--follow-imports=silent",
            "--no-incremental",
            "--no-error-summary",
        ]
    )
    error_lines = sorted(int(m.group(1)) for m in re.finditer(r":(\d+): error:", stdout))
    expected = [n for n, line in enumerate(SNIPPET.splitlines(), start=1) if line.endswith("# E")]
    assert error_lines == expected, stdout + stderr
