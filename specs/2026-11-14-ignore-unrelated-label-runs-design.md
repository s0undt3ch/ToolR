# Ignore unrelated label runs in CI (audit C-04)

## Problem

`.github/workflows/ci.yml` triggers on `pull_request` types
`[opened, synchronize, reopened, labeled, unlabeled]`. Renovate opens a PR and adds labels about two
seconds later. Both events land in one concurrency group (`ci.yml` `concurrency.group`, keyed on
`github.event_name` and `github.head_ref`) with `cancel-in-progress` on for PRs. The label run
cancels the real run, and the cancelled run's gate job (`if: always()`) still executes and
publishes a FAILURE check named `Set the CI Workflow(pull_request) Pipeline Exit Status`. PR #746
shows the pattern: runs 43379204378 and 43379207643 started two seconds apart.

Only the `full-build` and `full-bench` labels change what CI does (`FULL_BUILD_LABEL` and
`FULL_BENCH_LABEL` in `tools/ci.py`).

## Constraint

The `main` ruleset requires the check `Set the CI Workflow(pull_request) Pipeline Exit Status`,
produced by the `set-pipeline-exit-status` job. GitHub counts a skipped check as passing
(["Job skipped by a conditional: The job reports Success"][troubleshooting]; `success`, `skipped`
and `neutral` are all successful states). So any design where an ignored-label run publishes a
check under the required name, even a skipped or empty one, lets a label make a red or
in-progress PR look mergeable. The audit's proposed fix (a job-level `if` on the label name alone)
is also unsafe: the run still starts in the shared concurrency group and cancels the real one.

## Design

An "ignored label event" is a `labeled` or `unlabeled` event whose `github.event.label.name` is
neither `full-build` nor `full-bench`.

```text
(github.event.action == 'labeled' || github.event.action == 'unlabeled')
  && github.event.label.name != 'full-build'
  && github.event.label.name != 'full-bench'
```

1. **Concurrency.** For an ignored label event the group key ends in `github.run_id`, so the run
   neither cancels nor is cancelled by the real run. Other events keep the current key.
   The tail of the group becomes
   `${{ <ignored> && github.run_id || github.head_ref || github.run_id }}`. `run_id` is a
   non-zero number, so the `&&`/`||` idiom cannot fall through.
2. **Job skipping.** `prepare-ci` gets `if: ${{ !(<ignored>) }}`. Every job that `needs`
   `prepare-ci` directly or transitively is skipped by Actions when a dependency is skipped.
   `codecov-notify` and the two bench jobs already test `prepare-ci` outputs, which are empty on a
   skipped job, so they skip too. **`pre-commit` has no `needs:`** (the only such job), so it would
   still run. It gets the same `if:` as `prepare-ci`. A task adds a test that fails when a new
   job is neither downstream of `prepare-ci` nor carrying the filter.
3. **Gate.** The gate job cannot be skipped: `toolr pre-commit check-gate-needs`
   (`tools/precommit/workflows.py`, `GATE_JOB_IF`) forces `if: always()` and rewrites anything
   else. So the gate keeps running on ignored runs, finds every need skipped (`pipeline-gate`
   treats `skipped` as acceptable, so it exits 0), and its check must not carry the required
   name. The job `name:` becomes the expression below, where `<ignored>` is the expression above:

   ```text
   ${{ <ignored>
     && format('Ignored label event ({0})', github.event.label.name)
     || format('Set the {0}({1}) Pipeline Exit Status', github.workflow, github.event_name) }}
   ```

   Because the gate runs rather than being skipped, its name is rendered, so the rendered name
   differs from the required one on ignored runs and is unchanged on every other run. Skipping
   the gate and renaming it is not needed and is blocked by the hook.
4. **Single source.** Not possible in full. The context availability table gives
   `concurrency`: `github, inputs, vars`; `jobs.<id>.if`: `github, needs, vars, inputs`;
   `jobs.<id>.name`: `github, needs, strategy, matrix, vars, inputs`. `env` is not available in
   any of the three, so the expression is written out four times (concurrency, `prepare-ci`,
   `pre-commit`, gate name). A repository variable via `vars` would be one source but moves CI
   behaviour out of the repo, so it is rejected. The duplication is guarded by a test that
   extracts the four copies and asserts they are equal, and that the label names match
   `FULL_BUILD_LABEL` and `FULL_BENCH_LABEL`.
5. **Comment.** The trigger comment at the top of `ci.yml` says that only `full-build` and
   `full-bench` label events run CI and that other label events are no-ops.

## Non-changes

- `tools/ci.py` stays as is. `_pr_labels()` reads the PR's current labels from the payload and
  still decides the matrix on real runs. Its docstrings and tests do not describe the trigger.
- `.github/actions/pipeline-gate` is unaffected: its input, its `skipped` handling and the
  `check-gate-needs` invariants are unchanged.
- `run-name` is unchanged.

## Residual behaviour

- A `full-build` or `full-bench` label event still cancels an in-flight real run in the same
  group. That is wanted: the matrix changed.
- A genuine cancellation (a newer push) still lets the old gate publish FAILURE for the old SHA.
  That check belongs to a superseded commit and is out of scope.
- The earlier gate result stays valid across ignored label events because they no longer cancel
  or replace it.

## Checks and sources

Verified (docs, fetched 2026-11-14):

- Context availability table, `contexts` reference: values quoted above.
- Troubleshooting required status checks: skipped-by-conditional job reports Success; a
  dependent of a failed job is skipped and may not block merging; use `always()` with `needs`.
- Concurrency: at most one running job or workflow per group; `cancel-in-progress: true` cancels
  the running one.
- Repository: `pre-commit` is the only job without `needs:`; the gate's `if: always()` is
  enforced by `GATE_JOB_IF`.

Verified: `prek` ran `jactionlint` on the expressions and it accepted them.

Not verified, so the plan tests them live or with actionlint:

- That `github.event.label.name` and `github.event.action` are populated for `labeled` and
  `unlabeled` in a `pull_request` payload (expected from the webhook payload, not fetched).
- That a skipped job still creates a check-run, and under which name. The design does not rely on
  it, because no job whose name is the required one is skipped on an ignored run.
- That Actions evaluates `jobs.<id>.name` with `needs` and `github` for an `always()` gate whose
  needs are all skipped. The docs list `github` as available there; the live test confirms it.
- That the cancelled gate on the real run is the only source of the stale FAILURE. Taken from
  the audit's evidence, not re-queried.

[troubleshooting]: https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/collaborating-on-repositories-with-code-quality-features/troubleshooting-required-status-checks
