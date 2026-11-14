# Plan: ignore unrelated label runs in CI (audit C-04)

Design: `specs/2026-11-14-ignore-unrelated-label-runs-design.md`. Workflow-only change, so the
verification scope is `prek run --all-files`, not the full `mise run test`, apart from the new
Python test.

## Task 1: Edit `ci.yml`

- Rewrite the trigger comment (top of the file) to say unrelated label events are no-ops.
- Change `concurrency.group` to end in the ignored-event expression described in the design.
- Add `if: ${{ !(<ignored>) }}` to `prepare-ci` and `pre-commit`.
- Template the gate job `name:`.
- Do not touch the gate's `if`, `needs` or steps.

Verify: `prek run --files .github/workflows/ci.yml` (runs `jactionlint` and
`check-gate-needs`). The second must make no change. If `jactionlint` rejects an expression,
record the message in the design's "not verified" list and adjust.

## Task 2: Guard test

`tests/workflows/test_ci_label_filter.py` (`tests/workflows/__init__.py` already exists; use a factory
fixture if one is needed). Parse `ci.yml` with PyYAML and assert:

- The ignored-event expression is identical in `concurrency.group`, `prepare-ci.if`,
  `pre-commit.if` and the gate `name` (normalised whitespace).
- It names exactly `FULL_BUILD_LABEL` and `FULL_BENCH_LABEL` from `tools/ci.py`.
- Every job other than `prepare-ci` and the gate either carries the filter or reaches
  `prepare-ci` through `needs`.
- The gate's `if` is still `always()` and its rendered-name template keeps the required text.

Verify: `uv run pytest tests/workflows` on Python 3.11 and the newest, then
`toolr pre-commit check-tests-layout`.

## Task 3: Commit

Conventional Commit `ci(workflows): ignore unrelated label events`. No release note is needed;
add a `Release-Notes: skip` trailer and check with `toolr release-notes check --base main`.
Stage by path; never `git add -A`.

## Task 4: Live test (needs the owner's go-ahead: GitHub write actions)

On a draft PR from this branch, once CI has finished green on the head commit:

1. Add a dummy label (for example `c04-test`). Expect a new run whose jobs are all skipped except
   a gate check named `Ignored label event (c04-test)`. The earlier run is not cancelled, its
   `Set the CI Workflow(pull_request) Pipeline Exit Status` result is unchanged, and no new check
   appears under that name (`gh pr checks`, and `gh api` on the head SHA's check-runs).
2. Push a commit and add the dummy label while the run is still in progress. The real run must
   not be cancelled.
3. Remove the dummy label (`unlabeled`): same expectations as step 1.
4. Add `full-build`: a full run starts, cancels any in-flight run in the group, and publishes the
   required-name gate. Remove `full-build` afterwards and confirm the same.
5. Record the observed names and whether a skipped job created a check-run, and update the
   design's "not verified" list.

Mergeability check: with a deliberately red real run (or one in progress), add the dummy label
and confirm the PR stays blocked.

## Task 5: Close out

- Move both specs with `git mv` to `specs/archive/2026/` as the final commit before the PR is
  opened.
- Open the PR through `git-spice branch submit --draft`. Watch CI.
