# Parity corpus

Each directory is one case that `toolr` and `CommandsTester` must read the same
way (see `tests/e2e/test_parser_tester_parity.py`).

- `ok_<shape>`: builds on both sides with the same groups and commands.
- `err_<shape>`: fails on both sides with the same message.

A case is laid out in one of two ways:

- Plain: its files sit directly in the case directory and are copied into the
  project's `tools/`.
- With its own `tools/`: the case holds a `tools/` directory plus root-level
  helper modules, for shapes that need a module outside `tools/`
  (for example `ok_reexport_outside_tools`).

The test copies the case into a temporary project, so cases never run in place.

## Naming `command_group` cases

- `err_group_in_<construct>`: a `command_group(...)` call inside that construct (`if`, `try`, `with`,
  `function`), which the parser can't read without running it.
- `err_group_source_order_<form>`: two offending calls in one statement of that form; the message must
  list them in source order, not evaluation order.
- `err_group_name_only_*`: a call that passes little more than the name. `command_group` creates the
  group when none is registered, so it isn't a harmless lookup and is reported like any other.
- `ok_group_shadowed_*`, `ok_group_foreign_*`: calls to something that isn't toolr's `command_group`
  (a parameter or local `def` of that name, a foreign import, another object's method). Not reported.
- `err_group_ordering_groups_first`: across two modules, every group-binding problem comes before every
  command problem.
