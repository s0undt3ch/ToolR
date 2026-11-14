# Python hygiene (audit WP5)

Five small, independent clean-ups from the 2026-11-13 audit. No user-visible behaviour change.

## P-02: type-check `tools/`

- Evidence: the `mypy` hook in `.pre-commit-config.yaml` selects `^(crates/toolr-py/python/|tests/).*\.py$`, so
  the project-local pre-commit commands under `tools/` are never type-checked.
- Fix: add `tools/` to the hook's `files`, fix what mypy reports without blanket ignores.
- Tests: `_find_capture_toolr` in `tools/precommit/docs.py` has no tests; add
  `tests/tools/precommit/docs/test_find_capture_toolr.py` (env override, newest of release/debug, PATH
  fallback, `None`).

## M-04: cross-boundary runner spec shape test

- Evidence: `ExecutionSpec` / `ContextSpec` (Rust, `crates/toolr-core/src/execute/spec.rs`) and `RunnerSpec` /
  `ContextSpec` (`crates/toolr-py/python/toolr/_runner.py`) are kept in step by comment only; only the version
  constants are gated.
- Fix: one Python test that parses the Rust struct field names and asserts the Python twins carry the same
  set. The spec itself does not change, so no schema bump.

## M-05: runner test fixtures copied across modules

- Evidence: `tools_module`, `spec_file`/`spec_payload` and `run_runner` are defined in `test_current_context.py`,
  `test_dispatch.py` and `test_spec_loader.py`, beside `tests/runner/conftest.py`.
- Fix: one copy of each in that conftest (`spec_file` gains `drop=` to replace the loader's `DROP` sentinel).

## P-04 / U-22 / M-08: dead code

- Evidence: a garbled duplicate comment in `toolr/__init__.py` `__getattr__`; `SignatureParameterError` and
  `SignatureError` in `_exc.py` have no users outside archived specs and the changelog, and are not in any
  `__all__`.
- Fix: delete them.

## U-19: skill reference tables cite private modules

- Evidence: generated `skills/*/references/*.md` name private modules (`toolr._...`) as the source of public names.
- Fix: in the generator under `crates/xtask/src/build_skill_refs/`, then regenerate.
