"""Guard the ignored-label-event filter in `ci.yml`.

The filter expression is written out four times because the `env` context is unavailable in
`concurrency`, `jobs.<id>.if` and `jobs.<id>.name`. These tests keep the copies identical.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

import pytest
import yaml

from tools.ci import FULL_BENCH_LABEL
from tools.ci import FULL_BUILD_LABEL

CI_YML = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "ci.yml"
GATE = "set-pipeline-exit-status"
ROOT_JOB = "prepare-ci"
FILTERED_JOBS = (ROOT_JOB, "pre-commit")
IGNORED_RE = re.compile(
    r"\(github\.event\.action == 'labeled' \|\| github\.event\.action == 'unlabeled'\)"
    r"(?: && github\.event\.label\.name != '[^']+')+"
)


def _norm(text: str) -> str:
    return " ".join(str(text).split())


@pytest.fixture(scope="module")
def workflow() -> dict[str, Any]:
    return yaml.safe_load(CI_YML.read_text(encoding="utf-8"))


def _ignored(text: str) -> str:
    matches = IGNORED_RE.findall(_norm(text))
    assert len(matches) == 1, f"expected one ignored-event expression in: {text}"
    return matches[0]


def _copies(workflow: dict[str, Any]) -> dict[str, str]:
    jobs = workflow["jobs"]
    return {
        "concurrency.group": _ignored(workflow["concurrency"]["group"]),
        **{f"{job}.if": _ignored(jobs[job]["if"]) for job in FILTERED_JOBS},
        f"{GATE}.name": _ignored(jobs[GATE]["name"]),
    }


def test_filter_copies_are_identical(workflow: dict[str, Any]) -> None:
    copies = _copies(workflow)
    assert len(set(copies.values())) == 1, copies


def test_filter_names_exactly_the_matrix_labels(workflow: dict[str, Any]) -> None:
    expression = next(iter(_copies(workflow).values()))
    assert sorted(re.findall(r"label\.name != '([^']+)'", expression)) == sorted(
        [FULL_BUILD_LABEL, FULL_BENCH_LABEL]
    )


def test_ignored_runs_never_share_the_real_run_group(workflow: dict[str, Any]) -> None:
    group = _norm(workflow["concurrency"]["group"])
    assert group.endswith(
        f"{_copies(workflow)['concurrency.group']} && github.run_id || github.head_ref || github.run_id }}}}"
    )


def test_every_job_is_filtered_or_downstream_of_prepare_ci(workflow: dict[str, Any]) -> None:
    jobs = workflow["jobs"]

    def reaches_root(job: str, seen: frozenset[str] = frozenset()) -> bool:
        if job == ROOT_JOB:
            return True
        needs = jobs[job].get("needs") or []
        needs = [needs] if isinstance(needs, str) else needs
        return any(n not in seen and reaches_root(n, seen | {job}) for n in needs)

    unguarded = [job for job in jobs if job not in (*FILTERED_JOBS, GATE) and not reaches_root(job)]
    assert not unguarded, f"jobs that would run on an ignored label event: {unguarded}"


def test_gate_stays_unskippable_and_keeps_required_name(workflow: dict[str, Any]) -> None:
    gate = workflow["jobs"][GATE]
    assert _norm(gate["if"]) == "always()"
    assert (
        "format('Set the {0}({1}) Pipeline Exit Status', github.workflow, github.event_name)"
        in _norm(gate["name"])
    )
    assert "format('Ignored label event ({0})', github.event.label.name)" in _norm(gate["name"])
