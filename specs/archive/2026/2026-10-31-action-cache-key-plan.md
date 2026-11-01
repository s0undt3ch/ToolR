# Action Cache Key Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A change to a repository's root `uv.lock` no longer busts the Setup ToolR action's `tools/.venv` cache
(F-39).

**Architecture:** The `venv-cache` step's key hashes `tools/pyproject.toml` and `tools/uv.lock` only. A static pytest
check pins the key's hash inputs.

**Tech Stack:** composite GitHub Action, pytest + PyYAML.

**Spec:** [`specs/archive/2026/2026-10-31-action-cache-key-design.md`](./2026-10-31-action-cache-key-design.md)

## Global Constraints

- Branch stacks on `fix/action-latest-repo` (F-22). Touch only the `venv-cache` step in `action.yml`; don't edit the
  `resolve-version` step or `tests/action_yml/test_resolve_version.py`.
- No schema change. No `cache-prefix` default change.
- Imports top-of-file in the new test module.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The key, not the comment, is the fix.** Task 1, `test_tools_venv_cache_key_hashes_only_tools_files`.
2. **The comment matches toolr's own freshness rule** (`check_freshness` in `crates/toolr-core/src/venv/sync.rs` reads
   `tools/uv.lock` only). No claim that `tools/` is a workspace member.
3. **Skill prose matches the key.** `SKILL.md`'s cache-miss bullet names two files.
4. **Actions-only behaviour.** That a root-only `uv.lock` change now hits the cache is only observable on a real run;
   see Task 2, Step 3.

---

### Task 1: Drop the root `uv.lock` from the cache key

**Files:**

- Modify: `action.yml` (step id `venv-cache`: `key` and its comment)
- Modify: `skills/toolr-ci-setup/SKILL.md` ("Common failure modes", the "`tools/.venv` cache misses every run" bullet)
- Create: `tests/action_yml/test_tools_venv_cache.py`

**Interfaces:** `_step(step_id: str) -> dict[str, object]` module helper loading `action.yml` with `yaml.safe_load` and
returning the step with that `id` (raise `AssertionError` when missing, as `test_resolve_version.py` does). Keep it
local to this module; don't share it with `test_resolve_version.py`.

- [ ] **Step 1: Write the test.**
    - `test_tools_venv_cache_key_hashes_only_tools_files`: take `_step("venv-cache")["with"]["key"]`, extract the
      `hashFiles(...)` argument list with a regex over `'([^']+)'` inside `hashFiles\((.*?)\)` → equals
      `["tools/pyproject.toml", "tools/uv.lock"]`.
    - `test_tools_venv_cache_key_keeps_prefix_and_triple`: the key starts with
      `${{ inputs.cache-prefix }}-tools-venv|` and contains `${{ steps.archive.outputs.triple }}`, so the fix doesn't
      drop the other key parts.

- [ ] **Step 2: Run and confirm the failure.** `uv run pytest tests/action_yml/test_tools_venv_cache.py -v` → the
      first test fails with `['tools/pyproject.toml', 'tools/uv.lock', 'uv.lock'] != [...]`; the second passes.

- [ ] **Step 3: Implement.** Remove `'uv.lock'` from the `hashFiles` call. Replace the comment block above `key:` with:
      the venv is invalidated by `tools/pyproject.toml` or `tools/uv.lock` (the same file toolr's freshness check
      reads), not by a toolr release or the repo's root lock; the triple stays so an OS or arch change also misses.

- [ ] **Step 4: Fix the skill prose.** In `SKILL.md`, change the bullet to say the key hashes `tools/pyproject.toml`
      and `tools/uv.lock`, and that both must be committed. Keep the rest of the bullet.

- [ ] **Step 5: Run.** `uv run pytest tests/action_yml -v` → all pass. `cargo xtask build-skill-refs --check` → clean
      (no input description changed).

- [ ] **Step 6: Commit.** `fix(action): key the tools venv cache on tools/ files only`

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(unreleased): note the action's tools venv cache key change`.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 3: Actions check.** On the PR's CI, the "Cache tools venv" step of any job using `uses: ./` logs the key
      it looked up. First run: a miss with a hash different from `main`'s. Re-run the job: a hit on the same key.
      The "root lock change no longer busts it" half is a one-off observation on the next PR after merge that touches
      only the root `uv.lock` (Renovate opens these regularly): its "Cache tools venv" step must report a hit. Note the
      result on this PR once seen.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-action-cache-key-design.md specs/2026-10-31-action-cache-key-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive action cache key design and plan`.
