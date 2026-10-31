# Record standalone `@command(group=...)` commands in the runtime registry

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-19 (medium).
- **Plan:** [`2026-10-31-tester-standalone-group-plan.md`](./2026-10-31-tester-standalone-group-plan.md)

## Problem

`toolr.command` (`crates/toolr-py/python/toolr/_decorators.py`) has two forms. The bound
`CommandGroup.command` stores the function in the group's `__commands` dict. The standalone parameterised form,
`@command("name", group="ci")`, only runs `_require_docstring(func)` and returns `func`. The name and group are never
recorded. The static parser (`crates/toolr-core/src/parser/commands.rs`) reads them, so `toolr` runs these commands,
but the runtime registry that `toolr.testing.CommandsTester` reads (`_get_command_group_storage()`) never sees them.

The authoring skill and the docs recommend the standalone form for groups that span files.
`docs/writing-commands/testing.md` says `.discover()` registers each `@command` call "exactly as a real
`import tools.*` would", and shows a cross-file assertion.

Repro (the audit's shape), one file `tools/standalone.py`:

```python
from toolr import command, command_group

g = command_group("g", "G", "G")

@command("x", group="g")
def x(ctx):
    """X."""

@g.command
def y(ctx):
    """Y."""
```

`CommandsTester` then `discover()` → `collected_command_groups()` is `{"tools.g": ["y"]}`. `x` is missing. `toolr g
--help` lists both.

## Decision

Make the standalone form register into the same `__commands` dict the bound form uses, so `CommandGroup` stays the
single source of truth.

- Group key: `f"tools.{group}"`, the same key `command_group()` stores under. The static parser matches `group=` against
  the group's full path without the `tools.` prefix, so `group="ci"` and `group="ci.helm"` work and
  `group="tools.ci"` fails in both places.
- **Group already declared** (the usual case: same file, group first, or the group's module imported earlier): look it
  up in `_get_command_group_storage()` and call `group.command(name)(func)`. `name=None` keeps the hyphenated function
  name, as today.
- **Group not declared yet** (`pkgutil` walks modules alphabetically, so `a_cmds.py` runs before `b_group.py`): store
  `(name, func)` in a new pending registry, `_get_pending_command_storage() -> dict[str, list[tuple[str | None,
  Callable[..., Any]]]]`, keyed by group full name. It uses the same function-attribute singleton pattern as
  `_get_command_group_storage`.
- `command_group()`, right after it creates a new `CommandGroup`, pops its own key from the pending registry and
  registers each entry with `group.command(name)(func)`. The early return for an existing group needs nothing: pending
  entries for a declared group can't exist.
- `CommandsTester` patches `_get_pending_command_storage` with its own dict (field `pending_command_collector`, patcher
  `pending_command_patcher`), started just before `command_group_patcher.start()` and stopped and cleared in
  `__exit__`.
- **Leftovers are an error.** `CommandsTester.collected_command_groups()` raises `ValueError` when the pending registry
  is not empty after discovery. The message mirrors the manifest build's `UnknownGroupRefs`:

  ```text
  unknown group references (1):
    - tools.cmds::check: references group `ci.typo` which has no `command_group(...)` declaration.
  ```

  Each line uses `func.__module__::func.__qualname__` and the `group=` string as written. Raising here, not in
  `discover()`, mirrors `toolr` (which fails the build) and keeps this change out of `discover()`, which F-09 rewrites.

Outside `CommandsTester`, nothing reads either registry. The real runner calls the imported function directly, so
runtime dispatch doesn't change. A pending entry whose group's module is never imported in a real run stays in a small
process-local dict.

### Rejected alternatives

- **Always stash, merge on read in `collected_command_groups()`.** Mutates `CommandGroup` objects from a getter and
  needs extra bookkeeping to stay idempotent across reads. Attaching at decoration or group creation time is
  order-independent without it.
- **Attach only when the group exists, silently drop otherwise.** Loses the cross-file case `pkgutil` order produces,
  which is the case the standalone form exists for.
- **Leave unknown `group=` references invisible.** A smaller copy of the false-green problem this fixes: the test
  passes while `toolr` refuses to build.
- **Raise for an unknown group at decoration time.** The group may be declared in a module imported later. Only the
  end of discovery knows.
- **A public lookup API on `CommandGroup` for attaching.** `group.command(name)(func)` already exists and runs the same
  checks.

## Scope and compatibility

- Touches `_decorators.py` (`command`, `command_group`, new `_get_pending_command_storage`) and `_discovery.py` (new
  fields and patcher default, one line in `__enter__` before `command_group_patcher.start()`, `__exit__`,
  `collected_command_groups`). F-09 (`fix/tester-discover-errors`) edits `discover()`, `_import_tools_modules` and the
  `sys.path` block of `__enter__`. The hunks don't overlap; whichever lands second rebases cleanly.
- No runner or manifest schema change. No new public name. The `collected_command_groups()` and `command()` docstrings
  change; `collected_command_groups` is embedded in `skills/toolr-command-authoring/references/testing.md`, so run
  `cargo xtask build-skill-refs`.
- Behaviour change for tests only: a `CommandsTester` test with a typo'd `group=` now fails when it reads the groups.
- Not covered: bare `@command` and `@command("name")` without `group=` are a manifest build error ("missing a
  `group=...` kwarg") but still pass silently under `CommandsTester`. They have no group to attach to; catching them
  needs a decision on import-time errors and is left for a follow-up.
- Docs: `docs/writing-commands/testing.md`, "What you can assert": note that standalone `@command(group=...)` commands
  appear under their group and that an unknown `group=` makes `collected_command_groups()` raise.
- `UNRELEASED.md`:

  > `toolr.testing.CommandsTester` now records commands declared with the standalone `@command(group="…")` decorator,
  > whether the group is declared in the same file or another one. They used to be missing from
  > `collected_command_groups()`, although `toolr` ran them. A `group=` that names no declared group now makes
  > `collected_command_groups()` raise a `ValueError` with the same message the manifest build gives.
