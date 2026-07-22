#!/usr/bin/env -S uv run --script --quiet
# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml"]
# ///
"""Verify pipeline gate jobs depend on every other job in their workflow.

Each workflow that wants a single required status check ends with a gate
job (id `set-pipeline-exit-status`) that fails when any of its `needs`
reports `failure` or `cancelled`. GitHub only exposes results for *direct*
dependencies: a job skipped because an upstream dependency failed reports
`skipped`, not `failure`, so a gate that misses even one job can go green
while that job burns. This hook fails when a gate's `needs` list drifts
from the full job set, so adding a job without gating it can't land.

Invoked from a local pre-commit hook (and therefore CI's pre-commit job).
"""

from __future__ import annotations

import sys
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = REPO_ROOT / ".github" / "workflows"
GATE_JOB_ID = "set-pipeline-exit-status"


def main() -> int:
    errors: list[str] = []
    for workflow in sorted(WORKFLOWS.glob("*.yml")):
        jobs = yaml.safe_load(workflow.read_text()).get("jobs") or {}
        gate = jobs.get(GATE_JOB_ID)
        if gate is None:
            continue
        needs = gate.get("needs") or []
        needs = {needs} if isinstance(needs, str) else set(needs)
        expected = set(jobs) - {GATE_JOB_ID}
        for job_id in sorted(expected - needs):
            errors.append(
                f"{workflow.name}: job `{job_id}` is missing from "
                f"`{GATE_JOB_ID}.needs` — its failure would not fail the gate"
            )
        for job_id in sorted(needs - expected):
            errors.append(f"{workflow.name}: `{GATE_JOB_ID}.needs` lists unknown job `{job_id}`")
    for error in errors:
        print(error, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
