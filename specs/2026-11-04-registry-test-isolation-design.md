# Registry test isolation: design

## Problem

Run `tests/registry` in reverse order and two tests fail with `DID NOT RAISE ValueError`:
`test_command_group_without_description_or_docstring` and
`test_command_group_with_both_docstring_and_description` (`tests/registry/test_command_group_errors.py`).

Two causes share one root:

1. **Test leak.** `test_registry_basic.py` (five calls) and `test_command_group_errors.py:58` call
   `command_group("test", ...)` with no `CommandsTester` around them. They write into the real
   process-wide registry, a dict stored as a function attribute of `_get_command_group_storage`
   (`crates/toolr-py/python/toolr/_decorators.py`). Only `tests/decorators/test_decorators_unit.py`
   isolates it, with a module-local autouse fixture.
2. **Product bug.** `command_group` returns an existing group before it checks its arguments, so
   `command_group("test", "T")` with no description returns the cached group instead of raising. A
   user hits the same hole when two modules declare one group and one call is malformed.

## Goals

- Every Python test starts with an empty command-group registry and leaves the old one in place.
- `command_group` raises `ValueError` for invalid arguments whether or not the group exists.

## Non-goals

- The pending-command list. `_get_pending_command_storage` returns a fresh list per call outside
  `CommandsTester`, so it holds no process-wide state.
- Other order dependencies (see below).

## Design

### Fixture

Move the autouse `_clean_registry` fixture from `tests/decorators/test_decorators_unit.py` into
`tests/conftest.py` and delete the module-local copy. It snapshots the dict, clears it, yields, then clears
and restores the snapshot. Root scope covers every directory that calls `command_group` in-process
(`registry`, `decorators`, `runner`, `sources`, `e2e`) and any later one. `CommandsTester` still patches
the storage function per test: the autouse fixture sets up first and tears down last, so the patch
never sees it. No new directory or data file, so the `__init__.py` and `files/` layout rules hold.

### Argument checks

Split the validation and field resolution out of the body into a helper,
`_resolve_group_fields(title, description, long_description, docstring)`, returning the final
`(title, description, long_description)`. It holds the stripped-docstring check, the
docstring-versus-description conflict, the missing-description error and the docstring parse.
`command_group` normalises `name` and `parent`, calls the helper, and only then looks the group up.

### Re-declaring an existing group

A second valid call with the same full name returns the first group unchanged. If its resolved title,
description or long description differ from the first group's, it logs a warning naming the group and the
fields. It never raises.

Why not raise: a plugin and a local `tools/` module may both declare a group so `@command(group=...)` has a
home whichever imports first. Raising would break that. The static parser (`parser/groups.rs`) reads each
binding on its own and does not reject duplicates, so returning the first group, as `CommandsTester` and
the runtime do, is the closest match. Plan Task 2 confirms how the manifest treats two bindings of one
group and records any gap as a follow-up rather than changing the parser here.

## Other order dependencies

Reversing the whole suite (1026 tests; `pytest-randomly` is not installed) also fails four tests that pass
alone: `tests/context/test_chdir.py` (two, `FileNotFoundError`: a prior test leaves a deleted cwd),
`tests/utils/logs/test_include_timestamps.py`, `tests/runner/test_path_hygiene.py`
(`No module named 'tools.probe'`), plus one pytester error ("no `tools/pyproject.toml` found"), probably the same
cwd leak. They have different roots from the registry leak. Out of scope; file a separate follow-up.

## Testing

- `tests/registry/test_registry_isolation.py`: two tests in fixed order in one module. The first registers a
  group, the second asserts the registry is empty. It fails without the fixture.
- `test_command_group_errors.py`: for an existing group, a call with no description or docstring, and a call
  with both, still raise. This is the reported shape.
- A re-declaration with identical arguments returns the same object with no warning. One with a different
  description returns the same object and logs a warning (`caplog`).
- Real calls into the real registry, no mocks. All changed `toolr` lines are covered.

## Release note

User-visible: `command_group` with invalid arguments now raises even if a group of that name exists, and a
conflicting re-declaration logs a warning. Queue it with `toolr release-notes add --text`.
