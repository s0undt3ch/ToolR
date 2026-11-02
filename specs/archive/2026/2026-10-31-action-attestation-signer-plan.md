# Action Attestation Signer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Setup ToolR action rejects toolr archives whose attestation wasn't produced by a `release.yml` run
(F-23).

**Architecture:** The "Verify SLSA build provenance" step adds `--signer-workflow` and `--deny-self-hosted-runners`,
then counts verified attestations whose `buildConfigURI` is `release.yml` and fails on zero. An offline bash harness
stubs `gh` and runs the step's script.

**Tech Stack:** composite GitHub Action (bash, `gh attestation verify`), pytest + PyYAML, `jq` (tests only).

**Spec:** [`specs/archive/2026/2026-10-31-action-attestation-signer-design.md`](./2026-10-31-action-attestation-signer-design.md)

## Global Constraints

- Branch stacks on `fix/action-cache-key` → `fix/action-latest-repo`. Touch only the "Verify SLSA build provenance"
  step in `action.yml`. Don't edit `tests/action_yml/test_resolve_version.py` or `test_tools_venv_cache.py`.
- No schema change. No input description change, so no skill-ref regen.
- Factory fixtures for `tmp_path` setup, imports top-of-file.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The filter reads only verified results.** The count comes from `gh attestation verify --format json`, which
   prints attestations that passed `gh`'s checks, never from a separate unverified fetch.
2. **Match is exact.** Round-1 review replaced the original `startswith("…/release.yml@")` prefix with an exact match
   on `release.yml@refs/heads/main` and on the signer `_build-binary-archive.yml@refs/heads/main`, so an edited
   `release.yml` on another branch or PR ref is rejected. `test_rejects_other_workflow_or_ref`,
   `test_rejects_signer_off_main`. Round 2 added the archive-name subject match
   (`test_rejects_attestation_for_another_archive`), a fail-closed count check and a gh 2.56 floor.
   Round 3 pinned the signer with `--cert-identity` (exact) in place of `--signer-workflow` plus a `--jq` SAN check.
3. **Every real release still passes,** across all triples. Task 2, Step 3.
4. **CI-built archives fail,** checked against a real artifact, not only the stub. Task 2, Step 3.

---

### Task 1: Reproduce, then bind the check to the release workflow

**Files:**

- Modify: `action.yml` ("Verify SLSA build provenance": add `id: verify-attestation`, rewrite the `run:` body)
- Modify: `skills/toolr-ci-setup/SKILL.md` ("Common failure modes")
- Create: `tests/action_yml/test_verify_attestation.py`

**Interfaces:** fixture `verify_runner(tmp_path) -> Callable[..., subprocess.CompletedProcess[str]]`, called as
`verify_runner(*, build_configs: list[str], gh_rc: int = 0)`. It extracts the `verify-attestation` step's `run:` body
from `action.yml` (module helper `_step_script(step_id: str) -> str`, local to this module), sets
`ARCHIVE_PATH=<tmp>/toolr.tar.gz`, and defines a `gh` stub function that:

- appends its argument list to `tmp_path / "gh_calls"`;
- returns `gh_rc` straight away when non-zero;
- otherwise builds `[{"verificationResult": {"signature": {"certificate": {"buildConfigURI": c}}}} for c in
  build_configs]` and, when `--jq <expr>` is present, pipes it through `jq -c "<expr>"`, else prints it.

Module-level autouse fixture skips when `bash` or `jq` is missing.

- [ ] **Step 1: Write the failing tests** (reported shape: a CI-built archive with a valid in-repo attestation).
    - `test_rejects_ci_built_archive`:
      `build_configs=["https://github.com/s0undt3ch/ToolR/.github/workflows/ci.yml@refs/heads/main"]` →
      `returncode != 0`, stderr contains `not built by the ToolR release workflow`.
    - `test_accepts_release_built_archive`:
      `build_configs=["https://github.com/s0undt3ch/ToolR/.github/workflows/release.yml@refs/heads/main"]` →
      `returncode == 0`.
    - `test_accepts_when_any_attestation_is_release`: one `ci.yml` entry plus one `release.yml` entry → `returncode ==
      0`.
    - `test_rejects_lookalike_workflow`: `…/workflows/release.yml-copy@refs/heads/main` → `returncode != 0`.
    - `test_gh_verify_failure_fails`: `gh_rc=1` → `returncode != 0`.
    - `test_passes_signer_and_hosted_runner_flags`: after an accepted run, the `gh_calls` line contains
      `--repo s0undt3ch/ToolR`,
      `--signer-workflow s0undt3ch/ToolR/.github/workflows/_build-binary-archive.yml` and
      `--deny-self-hosted-runners`.

- [ ] **Step 2: Run and apply the close criterion.** `uv run pytest tests/action_yml/test_verify_attestation.py -v`.
    - Before Step 3 the step has no `id`, so add only `id: verify-attestation` to `action.yml` first, then run.
    - Expected against the current `run:` body: `test_rejects_ci_built_archive`, `test_rejects_lookalike_workflow` and
      `test_passes_signer_and_hosted_runner_flags` fail (the old command exits 0 and ignores `buildConfigURI`).
    - **Close criterion:** if `test_rejects_ci_built_archive` passes against the old command, or if a real CI-built
      archive fails `gh attestation verify <archive> --repo s0undt3ch/ToolR` (Task 2, Step 3's negative check run
      with the old command), the gap doesn't exist: drop the branch, mark F-23 closed with that evidence, and stop.
      On 2026-10-31 the real CI artifact passed, so the expectation is to proceed.

- [ ] **Step 3: Implement.** Rewrite the `run:` body as in the design: keep the `gh`-missing check and its messages;
      capture `count` from the `gh attestation verify … --format json --jq '…'` call (under `set -euo pipefail`, a
      `gh` failure still aborts); if `count` is empty or `< 1`, print
      `setup-toolr: ${ARCHIVE_PATH##*/} has a valid ToolR attestation but was not built by the ToolR release
      workflow` and a second line pointing at `skip-attestation: true` as the explicit opt-out, then exit 1. Update the
      step comment: what each flag binds, and why the `buildConfigURI` filter is needed (signer is the reusable
      workflow, shared with `ci.yml`).

- [ ] **Step 4: Skill prose.** In `SKILL.md` "Common failure modes", add a bullet: "`… was not built by the ToolR
      release workflow`": the archive has a valid attestation from the ToolR repo but came from a CI or branch build,
      not a release. Install a published release version; don't reach for `skip-attestation` to get around it.

- [ ] **Step 5: Run.** `uv run pytest tests/action_yml -v` → all pass. `cargo xtask build-skill-refs --check` →
      clean.

- [ ] **Step 6: Commit.** `fix(action): only accept archives attested by the release workflow`

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(unreleased): note the action's release-workflow attestation check`.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green (`shellcheck` /
      `actionlint` over the new step).
- [ ] **Step 3: Real-attestation checks** (scratch directory outside the repo; paste the outputs into the PR body).
      Use the step's exact flags and `--jq` expression:
    - Positive: `gh release download v<latest> --repo s0undt3ch/ToolR --pattern 'toolr-*'` (archives only), then run
      the command on every archive → each prints `1`. Repeat for one archive of `v0.20.0` → `1`.
    - Negative: `gh run download <recent ci.yml run on main> --repo s0undt3ch/ToolR --name
      toolr-archive-x86_64-unknown-linux-musl` → the old `--repo`-only command exits 0, the new one prints `0`.
- [ ] **Step 4: Actions check.** toolr's own CI runs `uses: ./` with `version: latest` on every PR job that sets up
      toolr. On this PR, those jobs' "Verify SLSA build provenance" step must pass on Linux, and the `_test`/`_bench`
      jobs that use the action on macOS and Windows (if any) must pass too. That is the in-Actions proof that real
      release archives clear the new filter.
- [ ] **Step 5:**
      `git mv specs/2026-10-31-action-attestation-signer-design.md specs/2026-10-31-action-attestation-signer-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive action attestation signer design and plan`.
