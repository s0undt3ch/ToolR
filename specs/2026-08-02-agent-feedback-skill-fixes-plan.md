# Plan: agent-feedback skill fixes

See design doc: `2026-08-02-agent-feedback-skill-fixes-design.md`.

- [ ] 1. Fix `docs/writing-commands/context.md`: drop `check=True` from the
      `ctx.run` prose/example, state explicitly that nonzero exit does not
      raise — inspect `.returncode` on the result.
- [ ] 2. Delete `--force` from `skills/toolr-command-authoring/SKILL.md:184`.
- [ ] 3. Soften `CommandGroup.command`'s docstring in
      `crates/toolr-py/python/toolr/_decorators.py` (cross-file usage works,
      just less idiomatic).
- [ ] 4. Extend `crates/xtask/src/build_skill_refs/authoring.rs`'s `ClassDef`
      arm to render public methods (and simple annotated fields) nested under
      the class entry. Regenerate `skills/*/references/*.md` via
      `cargo xtask build-skill-refs`. Verify `--check` passes.
- [x] 5. Ship `toolr._pytest_plugin` (`crates/toolr-py/python/toolr/`),
      registered via `[project.entry-points.pytest11]` in
      `crates/toolr-py/pyproject.toml`. Appends repo root to `sys.path` in
      `pytest_configure` (discovery mirrors
      `toolr_core::discovery::discover_project_root`), plus a session-scoped
      `repo_root` fixture. No scaffold template needed — activates
      automatically for every `tools/` package via its `toolr-py` dependency.
      Tests live at `tools/tests/` (blessed convention). Unit tests in
      `tests/test_pytest_plugin.py`.
- [ ] 6. Add `toolr.testing.make_context(repo_root, **overrides)` — minimal
      `Context` builder with in-memory consoles, real `ArgumentParser`. Unit
      tests in the Python test suite.
- [ ] 7. Add a multi-file, multi-subgroup worked example under
      `skills/toolr-command-authoring/examples/tools/` (parent group +
      2 subgroup files), regenerate the committed
      `examples/toolr-manifest.json`, add a short SKILL.md subsection
      ("Package-shaped groups") noting `__init__.py` is fine in *subpackages*
      of `tools/`, just not in `tools/` itself.
- [ ] 8. Add a short "Testing your commands" section to
      `skills/toolr-command-authoring/SKILL.md` covering `CommandsTester` and
      `make_context`.
- [ ] 9. Run `mise run test` (full umbrella — Rust + Python changed).
- [ ] 10. Archive this design+plan pair to `specs/archive/2026/` as the final
      commit before opening the PR, per CLAUDE.md convention. Delete
      `AGENT_FEEDBACK.md` (scratch note, not meant to persist — its useful
      content is now folded in here).
