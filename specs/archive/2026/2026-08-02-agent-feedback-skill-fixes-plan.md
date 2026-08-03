# Plan: agent-feedback skill fixes

See design doc: `2026-08-02-agent-feedback-skill-fixes-design.md`.

- [x] 1. Fix `docs/writing-commands/context.md`: drop `check=True` from the
      `ctx.run` prose/example, state explicitly that nonzero exit does not
      raise — inspect `.returncode` on the result.
- [x] 2. Delete `--force` from `skills/toolr-command-authoring/SKILL.md:184`.
- [x] 3. Soften `CommandGroup.command`'s docstring in
      `crates/toolr-py/python/toolr/_decorators.py` (cross-file usage works,
      just less idiomatic).
- [x] 4. Extend `crates/xtask/src/build_skill_refs/authoring.rs`'s `ClassDef`
      arm to render public methods nested under the class entry. Regenerated
      `skills/*/references/*.md` via `cargo xtask build-skill-refs`. `--check`
      passes. (Fields not rendered — deferred, not required to close the gap:
      `Context`'s methods, `ctx.run` in particular, were the actual complaint.)
- [x] 5. Shipped `toolr.testing._pytest_plugin` (moved under a new
      `toolr.testing` package, not top-level `toolr._pytest_plugin` — see
      item 6), registered via `[project.entry-points.pytest11]` in
      `crates/toolr-py/pyproject.toml`. Appends repo root to `sys.path` in
      `pytest_configure` (discovery mirrors
      `toolr_core::discovery::discover_project_root`), plus a session-scoped
      `repo_root` fixture. No scaffold template needed — activates
      automatically for every `tools/` package via its `toolr-py` dependency.
      Tests live at `tools/tests/` (blessed convention, one subdirectory per
      module for multi-command files). Unit tests in `tests/test_pytest_plugin.py`.
- [x] 6. Added `toolr.testing.make_context(repo_root, **overrides)` —
      `Context` builder with in-memory (`rich.Console(file=...)`) consoles
      sharing the real `TOOLR_THEME`, a real `ArgumentParser`. Restructured
      `toolr.testing` from a single module into a package
      (`_discovery.py` for `CommandsTester`, `_make_context.py`,
      `_pytest_plugin.py`) so all test-support code lives under one
      namespace. Unit tests in `tests/test_make_context.py`.
- [ ] 7. Multi-file, multi-subgroup worked example — **deferred, not done**.
      Scope-cut to keep this PR to the verified feedback points; worth a
      follow-up.
- [x] 8. Added a "Testing your commands" section covering `CommandsTester`,
      `make_context`, the pytest plugin, and the `tools/tests/` convention to
      both `skills/toolr-command-authoring/SKILL.md` and
      `docs/writing-commands/testing.md`.
- [x] 9. Ran the full umbrella: `cargo test --workspace` (30 binaries, 0
      failed), `cargo xtask build-skill-refs --check`, `uv run pytest`
      (390 passed), `uv run mkdocs build --strict`, `prek run --all-files`
      (all green except the pre-existing, already-tracked local mypy noise —
      see the test commit's body and
      `memory/project_local_mypy_msgspec_failure.md`).
- [x] 10. Archived this design+plan pair to `specs/archive/2026/` as the
      final commit before opening the PR. Deleted `AGENT_FEEDBACK.md`.
