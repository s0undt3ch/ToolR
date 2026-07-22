"""
Pre-commit hook commands that keep the GitHub workflows wired correctly.
"""

from __future__ import annotations

from ruamel.yaml import YAML

from toolr import Context

from ._common import group

# The in-repo composite action that marks a job as a pipeline gate. Using
# the action (rather than any expression in the job) as the marker means a
# gate whose wiring was mangled or dropped is still detected — and repaired.
GATE_ACTION = "./.github/actions/pipeline-gate"

# Without `if: always()` the gate job is skipped the moment an upstream job
# fails, and a skipped check can satisfy branch protection.
GATE_JOB_IF = "always()"

# Composite actions can't read the caller's `needs` context, so the gate
# step must hand it over; the action scans it for failed/cancelled results
# and sets the exit status. The gate's `needs` must list every other job
# for that scan to be meaningful.
GATE_JOBS_INPUT = "${{ toJSON(needs) }}"


def _gate_steps(job: dict) -> list[dict]:
    """The job's steps that use the pipeline-gate action."""
    return [
        step
        for step in job.get("steps") or []
        if str(step.get("uses", "")).partition("@")[0] == GATE_ACTION
    ]


def _normalised(expression: object) -> str:
    """Collapse whitespace so folded/multi-line YAML scalars compare equal."""
    return " ".join(str(expression).split())


def _set_job_key(job: dict, key: str, value: object) -> None:
    """Set a job mapping key, inserting missing keys before `steps`.

    ruamel appends new keys at the end of a mapping, which for a job means
    after `steps` — valid YAML, unreadable layout.
    """
    if key in job:
        job[key] = value
    else:
        job.insert(list(job).index("steps"), key, value)


def _round_trip_yaml() -> YAML:
    """A ruamel round-trip loader/dumper matching this repo's workflow style."""
    yaml = YAML(typ="rt")
    yaml.preserve_quotes = True
    # Never reflow long lines (run-name expressions, comments, ...).
    yaml.width = 100_000
    # GitHub Actions house style: 2-space mappings, sequence dashes
    # indented 2 past their key.
    yaml.indent(mapping=2, sequence=4, offset=2)
    return yaml


@group.command
def check_gate_needs(ctx: Context, *, check: bool = False) -> None:
    """Keep pipeline gate jobs depending on every other job in their workflow.

    Each workflow that wants a single required status check ends with a gate
    job that fails when any of its ``needs`` reports ``failure`` or
    ``cancelled``. GitHub only exposes results for *direct* dependencies: a
    job skipped because an upstream dependency failed reports ``skipped``,
    not ``failure``, so a gate that misses even one job can go green while
    that job burns.

    Gates are detected by the action they use, not by job name: any job with
    a step that uses the in-repo ``pipeline-gate`` composite action is a
    gate. Beyond the ``needs`` list, this command enforces (and repairs) the
    rest of the gate wiring: the job-level ``if: always()``, the step's
    ``jobs: ${{ toJSON(needs) }}`` input, and that no step-level ``if``
    stops the gate from running. Workflows without a gate job are ignored.

    By default this command rewrites each gate's ``needs`` to the full job
    set (sorted, so reordering jobs in the file causes no churn; comments
    preserved), so adding a job without gating it can't land. Wired up as a
    pre-commit hook in auto-fix style: prek sees the modified workflow and
    fails the commit until the fresh file is staged.

    Args:
        check: Report drift and exit non-zero instead of rewriting files.
    """
    yaml = _round_trip_yaml()
    drifted = False
    for workflow in sorted((ctx.repo_root / ".github" / "workflows").glob("*.yml")):
        data = yaml.load(workflow.read_text())
        jobs = data.get("jobs") or {}
        gates = {job_id: job for job_id, job in jobs.items() if _gate_steps(job)}
        if not gates:
            continue
        # Gates never need each other: two gates each needing "everything
        # but me" would form a dependency cycle GitHub rejects.
        expected = sorted(job_id for job_id in jobs if job_id not in gates)
        problems = []
        for gate_id, gate in sorted(gates.items()):
            needs = gate.get("needs") or []
            needs = [needs] if isinstance(needs, str) else list(needs)
            if needs != expected:
                problems.extend(
                    f"job `{job_id}` is missing from `{gate_id}.needs` — "
                    "its failure would not fail the gate"
                    for job_id in sorted(set(expected) - set(needs))
                )
                problems.extend(
                    f"`{gate_id}.needs` lists unknown job `{job_id}`"
                    for job_id in sorted(set(needs) - set(expected))
                )
                _set_job_key(gate, "needs", expected)
            if _normalised(gate.get("if")) != GATE_JOB_IF:
                problems.append(
                    f"`{gate_id}` must carry `if: {GATE_JOB_IF}` — without it the gate "
                    "is skipped when an upstream job fails"
                )
                _set_job_key(gate, "if", GATE_JOB_IF)
            for step in _gate_steps(gate):
                if _normalised((step.get("with") or {}).get("jobs")) != GATE_JOBS_INPUT:
                    problems.append(
                        f"the `{gate_id}` gate step must pass `jobs: {GATE_JOBS_INPUT}` — "
                        "the action cannot read the `needs` context itself"
                    )
                    step.setdefault("with", {})["jobs"] = GATE_JOBS_INPUT
                if "if" in step:
                    problems.append(
                        f"the `{gate_id}` gate step must not have an `if` — "
                        "the gate must always run"
                    )
                    del step["if"]
        if problems:
            drifted = True
            for message in problems:
                ctx.error(f"{workflow.name}: {message}")
            if check:
                continue
            with workflow.open("w") as wfh:
                yaml.dump(data, wfh)
            ctx.info(f"{workflow.name}: gate wiring rewritten")
    if drifted and check:
        ctx.exit(1)
    if not drifted:
        ctx.info("All pipeline gate jobs are correctly wired.")
