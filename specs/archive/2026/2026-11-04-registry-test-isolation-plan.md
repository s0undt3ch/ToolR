# Registry test isolation: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every Python test a fresh command-group registry, and make `command_group` validate its
arguments before returning an existing group.

**Architecture:** A root autouse fixture isolates the registry. A `_resolve_group_fields` helper runs before
the existing-group lookup in `command_group`.

**Tech Stack:** Python 3.11+, pytest, `caplog`.

**Spec:** `specs/2026-11-04-registry-test-isolation-design.md`

## Global Constraints

- Branch `fix/registry-test-isolation`, worktree `.worktrees/registry-test-isolation`, off `main`.
- Stage files by path. Never `git add -A`. Never stage `audit/`.
- Conventional Commits. No Co-Authored-By trailer. British English.
- Test layout (PR #672): every `tests/` directory holding `.py` has an `__init__.py`; data goes in `files/`.
- Imports at the top of test modules. Real tests, no mocks. Changed `toolr` lines need 99% patch coverage.

## Review Focus

- A dotted name (`command_group("ci.helm", ...)`) and `parent=` both resolve to the same registry key as before.
- `-OO` (stripped docstrings) still raises from `_reject_stripped_docstrings` before the lookup.
- `CommandsTester` runs still pass: its patched storage is untouched by the autouse fixture.

---

### Task 1: Isolate the registry in tests

**Files:**

- Modify: `tests/conftest.py`, `tests/decorators/test_decorators_unit.py`
- Create: `tests/registry/test_registry_isolation.py`
- [ ] Write `test_registry_isolation.py` with `test_registers_a_group` (calls `command_group("leak", "L", "d")`)
  followed by `test_registry_starts_empty` (asserts `_get_command_group_storage() == {}`). Run it: the second
  test fails.
- [ ] Move `_clean_registry` into `tests/conftest.py` (imports at the top) and delete the copy and its now
  unused imports from `test_decorators_unit.py`.
- [ ] Run `uv run pytest tests/registry tests/decorators -p no:randomly`: green.
- [ ] Reverse run: `uv run pytest --co -q -p no:randomly tests/registry | grep :: | tail -r > ids.txt` in the
  scratchpad, then `uv run pytest -p no:cacheprovider @ids.txt`: green.
- [ ] Commit: `test(registry): isolate the command-group registry for every test`.

### Task 2: Validate arguments before returning an existing group

**Files:**

- Modify: `crates/toolr-py/python/toolr/_decorators.py`
- Modify: `tests/registry/test_command_group_errors.py`

**Interfaces:**

- Produces: `_resolve_group_fields(title, description, long_description, docstring) -> tuple[str, str, str | None]`.
- [ ] Add failing tests: with `command_group("dup", "T", "d")` already registered, `command_group("dup", "T")`
  and `command_group("dup", "T", description="d", docstring="x")` raise `ValueError`; identical re-declaration
  returns the same object and logs nothing; a different description returns the same object and logs one
  warning naming `tools.dup` and `description`.
- [ ] Extract `_resolve_group_fields` from the body of `command_group` (stripped-docstring check, conflict check,
  missing-description error, docstring parse). Call it after name and parent normalisation and before
  `collector.get(...)`.
- [ ] On an existing group, compare the resolved fields; `log.warning` if any differ; return the group.
- [ ] Parity: read how `crates/toolr-core/src/manifest/` merges two bindings of one group. If it differs from
  first-wins, note it in the PR description as a follow-up; do not change the parser.
- [ ] Check `docs/` and the skills for text on re-declaring a group (`git grep -n "already exists"`); update prose
  if any. Run `cargo xtask build-skill-refs --check`.
- [ ] Run `uv run pytest tests/registry tests/decorators tests/testing` and `uv run coverage run -m pytest
  tests/registry && uv run coverage report --include='*/_decorators.py'`: changed lines covered.
- [ ] Commit: `fix(decorators): validate command_group arguments before returning an existing group`.

### Task 3: Release note and verification

- [ ] `toolr release-notes add --text "..."` describing both user-visible changes (raise on invalid arguments
  even when the group exists; warning on a conflicting re-declaration). Commit as
  `docs(release-notes): command_group argument checks`.
- [ ] `toolr release-notes check --base main`, `prek run --all-files`, `mise run test`.
- [ ] Archive both spec files with `git mv` into `specs/archive/2026/` as the last commit before the PR.
