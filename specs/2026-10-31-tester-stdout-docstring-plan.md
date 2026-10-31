# Tester Stdout Docstring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ContextForTesting.stdout`/`stderr` docstrings and the testing docs say which `Context` method writes where
(F-42).

**Architecture:** Documentation only, pinned by one test in `tests/test_make_context.py`.

**Tech Stack:** Python, rich, pytest, mkdocs.

**Spec:** [`specs/2026-10-31-tester-stdout-docstring-design.md`](./2026-10-31-tester-stdout-docstring-design.md)

## Global Constraints

- No behaviour change to `Context` or `make_context`.
- Imports at the top of test modules. Don't import `RunMock` or `Mock` at module level in `tests/test_make_context.py`;
  its doc-snippet tests import them locally on purpose. Use a plain stub function for `run=`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The `ctx.run` echo** is the case users trip on. Task 1 asserts it is in `stderr` and not in `stdout`.
2. **Docstring wording matches the test**, method by method.

---

### Task 1: Pin and document the stream split

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_make_context.py` (`ContextForTesting.stdout`,
  `ContextForTesting.stderr`)
- Modify: `docs/writing-commands/testing.md`, `skills/toolr-command-authoring/references/testing.md` (generated)
- Test: `tests/test_make_context.py`

**Interfaces:** none.

- [ ] **Step 1: Write the test** `test_make_context_stream_split(tmp_path)`: `make_context(tmp_path,
      prompt_input="bob\n", run=fake_run)` where `fake_run(*args, **kwargs)` returns `make_command_result()` (add
      `make_command_result` to the existing top-level `from toolr.testing import ...`). Call `ctx.print("to-stdout")`,
      `ctx.info("info-line")`, `ctx.warn("warn-line")`, `ctx.prompt("Name?")`, `ctx.run("echo", "sub")`, and
      `ctx.exit(1, "exit-message")` under `pytest.raises(SystemExit)`. Assert `"to-stdout"` and `"Name?"` are in
      `ctx.stdout`; `"info-line"`, `"warn-line"`, `"Running 'echo sub'"` and `"exit-message"` are in `ctx.stderr` and
      not in `ctx.stdout`.
- [ ] **Step 2: Run** `uv run pytest tests/test_make_context.py -k stream_split` → PASS (it pins current behaviour; the
      bug is in the docs).
- [ ] **Step 3:** Rewrite the `stdout` and `stderr` docstrings as the spec words them. In
      `docs/writing-commands/testing.md`, replace "`ctx.stdout`/`ctx.stderr` capture everything written through
      `ctx.print`/`ctx.info`/`ctx.error`/etc." with: `ctx.stdout` captures `ctx.print` and the question text of
      `ctx.prompt`; `ctx.stderr` captures `ctx.info`/`ctx.warn`/`ctx.error`/`ctx.debug`, the `ctx.exit` message and the
      `Running '…'` line `ctx.run` prints; a subprocess's own streamed output is not captured.
- [ ] **Step 4:** `cargo xtask build-skill-refs` (regenerate, don't hand-merge), then
      `cargo xtask build-skill-refs --check` → clean.
- [ ] **Step 5: Commit.** `docs(testing): say which ContextForTesting stream each method writes to`

### Task 2: Verification and archive

- [ ] **Step 1:** No `UNRELEASED.md` entry (see the spec).
- [ ] **Step 2:** `mise run test` (a `.py` file changed), polling every 30-60 s → green. `prek run --all-files` →
      green. `uv run mkdocs build --strict` → green.
- [ ] **Step 3:**
      `git mv specs/2026-10-31-tester-stdout-docstring-design.md specs/2026-10-31-tester-stdout-docstring-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive tester stdout docstring design and plan`.
