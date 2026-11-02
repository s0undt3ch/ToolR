# Tester Standalone Group Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Commands declared with the standalone `@command(group=...)` decorator show up in
`CommandsTester.collected_command_groups()`, and an unknown `group=` fails the test the way `toolr` fails the build
(F-19).

**Architecture:** The standalone decorator attaches to the declared group through `group.command(name)(func)`, or parks
the command in a pending registry that `command_group()` drains when it creates the group. `CommandsTester` isolates
the pending registry and reports leftovers.

**Tech Stack:** Python 3.11+, msgspec, attrs, pytest.

**Spec:** [`specs/archive/2026/2026-10-31-tester-standalone-group-design.md`](./2026-10-31-tester-standalone-group-design.md)

## Global Constraints

- Group key is `f"tools.{group}"`, never anything else.
- `CommandGroup.__commands` stays the only place commands live. Attach via `group.command(name)(func)`.
- Leave `discover()`, `_import_tools_modules` and the `sys.path` block of `__enter__` alone (F-09,
  `fix/tester-discover-errors`).
- No new public name; `toolr.testing.__all__` and `toolr.__all__` unchanged.
- Regression fixture uses the audit's exact shape.
- Factory fixtures for `tmp_path` setup; imports at the top of test modules.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **Both import orders.** Same file with the group first (direct attach) and `a_cmds.py` before `b_group.py`
   (pending, drained by `command_group()`). Task 1, `test_standalone_command_in_group_file` and
   `test_standalone_command_before_its_group_module`.
2. **Isolation.** A pending entry from one `CommandsTester` must not reach the next. Task 2,
   `test_pending_commands_do_not_leak_between_testers`.
3. **Unknown group message** matches the manifest build's wording, so users see one error shape. Task 2,
   `test_collected_command_groups_rejects_unknown_group`.
4. **Bare `@command` stays out of scope.** It is not recorded and doesn't raise. No test.

---

### Task 1: Attach standalone commands to their group

**Files:**

- Modify: `crates/toolr-py/python/toolr/_decorators.py` (`command`, `command_group`, new `_get_pending_command_storage`)
- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (new `pending_command_collector` field,
  `pending_command_patcher` field and default, `__enter__` one line, `__exit__`)
- Test: `tests/registry/test_integration.py`, `tests/registry/test_registry_basic.py`

**Interfaces:**

- `_get_pending_command_storage() -> dict[str, list[tuple[str | None, Callable[..., Any]]]]`, keyed by group full name.
- `CommandsTester.pending_command_collector: dict[str, list[tuple[str | None, object]]]` (`init=False`,
  `factory=dict`) and `pending_command_patcher: _patch` patching
  `toolr._decorators._get_pending_command_storage`.
- [ ] **Step 1: Write the tests.** In `tests/registry/test_integration.py`, add a factory fixture
  `tools_repo(tmp_path) -> Callable[[dict[str, str]], Path]` that writes `tools/__init__.py` plus each
  `{relative_path: source}` and returns `tmp_path`. F-09 adds the same fixture to this file; if it landed first, reuse
  it.
    - `test_standalone_command_in_group_file`: the audit's `tools/standalone.py` (`g = command_group("g", "G", "G")`,
      `@command("x", group="g")` documented `def x`, `@g.command` documented `def y`) →
      `set(groups["tools.g"].get_commands()) == {"x", "y"}`.
    - `test_standalone_command_before_its_group_module`: `tools/a_cmds.py` with `@command(group="ci")` documented
      `def run_lint`, `tools/b_group.py` with `command_group("ci", "CI", "CI")` →
      `"run-lint" in groups["tools.ci"].get_commands()`.
    - `test_standalone_command_dotted_group`: `command_group("ci.helm", "Helm", "Helm")` (after `command_group("ci",
      ...)`) and `@command(name="snippet-checker", group="ci.helm")` →
      `"snippet-checker" in groups["tools.ci.helm"].get_commands()`.
    - In `tests/registry/test_registry_basic.py`, `test_standalone_command_attaches_to_existing_group(commands_tester)`:
      `command_group("ci", ...)` then `@command("lint", group="ci")` in the test body →
      `commands_tester.collected_command_groups()["tools.ci"].get_commands()["lint"] is` the function.
- [ ] **Step 2: Run** `uv run pytest tests/registry -k standalone` → all four fail with a missing command.
- [ ] **Step 3: Implement** `_get_pending_command_storage` (same singleton pattern as `_get_command_group_storage`). In
      `command()`'s parameterised `decorator`, after `_require_docstring(func)`, when `group is not None`: look up
      `f"tools.{group}"` in `_get_command_group_storage()`; if found, `existing.command(name)(func)`, else append
      `(name, func)` to the pending list for that key. In `command_group()`, after storing a new group, pop its key from
      the pending registry and attach each entry. Add the two patcher fields to `CommandsTester`, start
      `pending_command_patcher` on the line before `self.command_group_patcher.start()`, and stop it and clear the
      collector in `__exit__`. Update `command()`'s "Returns:" docstring: the function is returned unchanged and also
      recorded under its group.
- [ ] **Step 4: Run** the Step 2 command → PASS. Then `uv run pytest tests/registry tests/decorators` → PASS.
- [ ] **Step 5: Commit.** `fix(testing): record standalone @command(group=...) commands in the registry`

### Task 2: Report unknown group references

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (`collected_command_groups`)
- Test: `tests/registry/test_integration.py`

**Interfaces:**

- `CommandsTester.collected_command_groups() -> dict[str, object]`. Raises `ValueError` when commands remain pending.
- [ ] **Step 1: Write the tests:**
    - `test_collected_command_groups_rejects_unknown_group`: `tools/cmds.py` with `command_group("ci", "CI", "CI")`
      and `@command(group="ci.typo")` documented `def check` → `pytest.raises(ValueError)` whose message contains
      `unknown group references (1):` and
      ``tools.cmds::check: references group `ci.typo` which has no `command_group(...)` declaration.``
    - `test_pending_commands_do_not_leak_between_testers`: a first tester over the typo fixture (exit without reading
      groups), then a second `CommandsTester` over an empty `tmp_path / "other"` → `collected_command_groups() == {}`,
      no raise.
- [ ] **Step 2: Run** `uv run pytest tests/registry -k "unknown_group or leak"` → the unknown-group test fails (no
      raise); the leak test passes already.
- [ ] **Step 3: Implement.** In `collected_command_groups()`, when `self.pending_command_collector` is non-empty, build
      one line per entry, ``  - {func.__module__}::{func.__qualname__}: references group `{group}` which has no
      `command_group(...)` declaration.`` (`group` is the key without the `tools.` prefix), sorted, under
      `unknown group references ({count}):`, and raise `ValueError`. Update its docstring with a `Raises:` section.
- [ ] **Step 4: Run** the Step 2 command → PASS.
- [ ] **Step 5: Commit.** `fix(testing): fail CommandsTester reads on unknown group references`

### Task 3: Docs, skill refs, release notes, verification, archive

**Files:**

- Modify: `docs/writing-commands/testing.md`, `skills/toolr-command-authoring/references/testing.md` (generated),
  `UNRELEASED.md`
- [ ] **Step 1:** `docs/writing-commands/testing.md`, "What you can assert": after the cross-file assertion, say that
      commands declared with standalone `@command(group="…")` appear under their group whatever the file order, and
      that a `group=` naming no declared group makes `collected_command_groups()` raise `ValueError`.
- [ ] **Step 2:** `cargo xtask build-skill-refs` (regenerate, don't hand-merge). Check that `SKILL.md`'s
      `CommandsTester` prose still reads true.
- [ ] **Step 3:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(testing): document standalone commands under CommandsTester`.
- [ ] **Step 4:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green.
- [ ] **Step 5:**
      `git mv specs/2026-10-31-tester-standalone-group-design.md specs/2026-10-31-tester-standalone-group-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive tester standalone group design and plan`.
