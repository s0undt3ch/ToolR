"""Tests for ``toolr pre-commit check-gate-needs``, the merge gate's wiring guard."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import REPO_ROOT
from tools.precommit.workflows import check_gate_needs

GATE_USES = "./.github/actions/pipeline-gate"

CORRECT = f"""\
name: ci
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: make
  test:
    runs-on: ubuntu-latest
    steps:
      - run: make test
  gate:
    # keep me
    needs: [build, test]
    if: always()
    runs-on: ubuntu-latest
    steps:
      - uses: {GATE_USES}
        with:
          jobs: ${{{{ toJSON(needs) }}}}
"""


@pytest.fixture
def make_workflows(tmp_path: Path) -> Callable[[dict[str, str]], Path]:
    """Factory: write ``{name: text}`` under ``tmp_path/.github/workflows``. Returns the repo root."""

    def factory(files: dict[str, str]) -> Path:
        directory = tmp_path / ".github" / "workflows"
        directory.mkdir(parents=True)
        for name, text in files.items():
            (directory / name).write_text(text, encoding="utf-8", newline="\n")
        return tmp_path

    return factory


def test_correct_gate_is_left_untouched(make_workflows: Callable[[dict[str, str]], Path]) -> None:
    root = make_workflows({"ci.yml": CORRECT})
    ctx = make_context(root)
    check_gate_needs(ctx)
    assert (root / ".github/workflows/ci.yml").read_text() == CORRECT
    assert "correctly wired" in ctx.stderr


def test_workflow_without_a_gate_is_ignored(
    make_workflows: Callable[[dict[str, str]], Path],
) -> None:
    text = "name: x\non: push\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: 'true'\n"
    root = make_workflows({"x.yml": text})
    check_gate_needs(make_context(root))
    assert (root / ".github/workflows/x.yml").read_text() == text


def test_missing_need_is_added_sorted_and_comments_survive(
    make_workflows: Callable[[dict[str, str]], Path],
) -> None:
    drifted = CORRECT.replace("needs: [build, test]", "needs: [build]")
    root = make_workflows({"ci.yml": drifted})
    ctx = make_context(root)
    check_gate_needs(ctx)
    fixed = (root / ".github/workflows/ci.yml").read_text()
    assert "job `test` is missing from `gate.needs`" in ctx.stderr
    assert "needs:\n      - build\n      - test\n" in fixed
    assert "# keep me" in fixed
    check_gate_needs(make_context(root), check=True)


def test_unknown_need_is_removed(make_workflows: Callable[[dict[str, str]], Path]) -> None:
    root = make_workflows({"ci.yml": CORRECT.replace("[build, test]", "[build, test, ghost]")})
    ctx = make_context(root)
    check_gate_needs(ctx)
    assert "lists unknown job `ghost`" in ctx.stderr
    assert "ghost" not in (root / ".github/workflows/ci.yml").read_text()


def test_missing_need_is_inserted_before_steps(
    make_workflows: Callable[[dict[str, str]], Path],
) -> None:
    text = CORRECT.replace("    needs: [build, test]\n", "")
    root = make_workflows({"ci.yml": text})
    check_gate_needs(make_context(root))
    fixed = (root / ".github/workflows/ci.yml").read_text()
    assert fixed.index("needs:") < fixed.index("steps:", fixed.index("gate:"))


def test_wrong_job_if_is_repaired(make_workflows: Callable[[dict[str, str]], Path]) -> None:
    root = make_workflows({"ci.yml": CORRECT.replace("if: always()", "if: success()")})
    ctx = make_context(root)
    check_gate_needs(ctx)
    assert "must carry `if: always()`" in ctx.stderr
    assert "if: always()" in (root / ".github/workflows/ci.yml").read_text()


def test_step_level_if_is_removed(make_workflows: Callable[[dict[str, str]], Path]) -> None:
    text = CORRECT.replace(
        f"- uses: {GATE_USES}\n", f"- uses: {GATE_USES}\n        if: github.event_name == 'push'\n"
    )
    root = make_workflows({"ci.yml": text})
    ctx = make_context(root)
    check_gate_needs(ctx)
    assert "gate step must not have an `if`" in ctx.stderr
    assert "github.event_name" not in (root / ".github/workflows/ci.yml").read_text()


def test_wrong_jobs_input_is_repaired(make_workflows: Callable[[dict[str, str]], Path]) -> None:
    root = make_workflows({"ci.yml": CORRECT.replace("toJSON(needs)", "toJSON(inputs)")})
    ctx = make_context(root)
    check_gate_needs(ctx)
    assert "must pass `jobs:" in ctx.stderr
    assert "toJSON(needs)" in (root / ".github/workflows/ci.yml").read_text()


def test_gate_is_found_by_its_action_not_its_name(
    make_workflows: Callable[[dict[str, str]], Path],
) -> None:
    text = CORRECT.replace("  gate:", "  pipeline-status:").replace("[build, test]", "[build]")
    root = make_workflows({"ci.yml": text})
    check_gate_needs(make_context(root))
    assert "- test" in (root / ".github/workflows/ci.yml").read_text()


def test_two_gates_never_need_each_other(make_workflows: Callable[[dict[str, str]], Path]) -> None:
    second = CORRECT[CORRECT.index("  gate:") :].replace("gate:", "gate-two:", 1)
    root = make_workflows({"ci.yml": CORRECT + second})
    ctx = make_context(root)
    check_gate_needs(ctx)
    assert "correctly wired" in ctx.stderr


def test_check_mode_reports_and_exits_without_rewriting(
    make_workflows: Callable[[dict[str, str]], Path],
) -> None:
    drifted = CORRECT.replace("[build, test]", "[build]")
    root = make_workflows({"ci.yml": drifted})
    ctx = make_context(root)
    with pytest.raises(SystemExit) as exc:
        check_gate_needs(ctx, check=True)
    assert exc.value.code == 1
    assert "`test` is missing" in ctx.stderr
    assert (root / ".github/workflows/ci.yml").read_text() == drifted


def test_rewrite_keeps_non_ascii_text_and_lf_endings(
    make_workflows: Callable[[dict[str, str]], Path],
) -> None:
    drifted = "# Runs the tests \u2014 every PR\n" + CORRECT.replace("[build, test]", "[build]")
    root = make_workflows({"ci.yml": drifted})
    check_gate_needs(make_context(root))
    fixed = (root / ".github/workflows/ci.yml").read_bytes()
    assert "\u2014 every PR".encode() in fixed
    assert b"\r\n" not in fixed


def test_repo_workflows_are_wired() -> None:
    check_gate_needs(make_context(REPO_ROOT), check=True)
