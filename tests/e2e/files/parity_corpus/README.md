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
