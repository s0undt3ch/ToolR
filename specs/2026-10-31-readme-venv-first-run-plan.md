# README Venv First Run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The README no longer claims toolr creates the tools venv on first invocation, and it tells users of a cloned
repo to run `toolr project venv sync` (F-32).

**Architecture:** Two edits to `README.md`, both below line 24 so the `docs/index.md` include is unaffected.

**Tech Stack:** Markdown, mkdocs.

**Spec:** [`specs/2026-10-31-readme-venv-first-run-design.md`](./2026-10-31-readme-venv-first-run-design.md)

## Global Constraints

- Doc-only: verification is `prek run --all-files` and `uv run mkdocs build --strict`. No `mise run test`.
- No regression test: the finding is a wrong sentence, and no test reads README prose.
- Don't name the files that make the venv stale (F-04 changes that list on `fix/venv-sync-pyproject`).
- Don't insert or remove lines at or above README line 24 (`docs/index.md` uses `{!README.md!lines=5-24}`).
- British English. Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The new wording matches dispatch.** Dispatch syncs a stale or broken venv but never creates a missing one
   (`resync_if_needed`, `Freshness::Missing` → early return).
2. **`docs/index.md` renders the same home page** before and after (`lines=5-24` unchanged).

---

### Task 1: Fix the README, verify, archive

**Files:**

- Modify: `README.md` ("Why ToolR" list, "No system-Python dependency" bullet; "Scaffold your repo" section)
- [ ] **Step 1: Edit the bullet.** Replace "Toolr resolves a per-repo Python venv via `uv` on first invocation." with a
  sentence to this effect, wrapped at 100 columns like its neighbours:

    ```markdown
    - **No system-Python dependency.** `toolr project init` (or `toolr project venv sync` in an existing checkout)
      builds a per-repo Python venv with `uv`, and toolr re-syncs it when it goes out of date. The host OS doesn't
      need Python at all to install toolr — it's a single static binary.
    ```

  Keep the "See [How toolr finds uv and Python]…" line after it.
- [ ] **Step 2: Add the clone case.** After the "Scaffold your repo" code block, before "The full install matrix…",
  add:

    ```markdown
    In a clone of a repo that already has `tools/`, run `toolr project venv sync` once instead of `toolr project init`.
    ```

- [ ] **Step 3: Verify.** `sed -n 5,24p README.md` is identical before and after (`git diff -U0 README.md` shows no
      hunk starting at or above line 24). `prek run --all-files` → green. `uv run mkdocs build --strict` → green.
- [ ] **Step 4: Commit.** `docs(readme): say which command creates the tools venv`
- [ ] **Step 5: Archive.**
      `git mv specs/2026-10-31-readme-venv-first-run-design.md specs/2026-10-31-readme-venv-first-run-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive README venv first run design and plan`.
