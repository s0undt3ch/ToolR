# Design: fixing skill/doc gaps found by AGENT_FEEDBACK.md

Source: `AGENT_FEEDBACK.md` (a real session's account of building a multi-file
`tools/ritual/` package), cross-checked against current source, plus a
fable-model brainstorm that found an additional, worse bug.

## Verified findings

1. **`docs/writing-commands/context.md:30`** teaches
   `ctx.run(*cmd, capture_output=True, check=True)`. `Context.run` forwards
   `**kwargs` to `toolr.utils.command.run`, which has no `check` parameter —
   this line is a `TypeError` waiting to happen, and plausibly the direct
   cause of the feedback's `check=False` mistake. **P0.**
2. **`skills/toolr-command-authoring/SKILL.md:184`** documents
   `toolr project manifest rebuild --force`. `crates/toolr/src/cli.rs`'s
   `rebuild` subcommand takes no args at all — `rebuild` already rebuilds
   unconditionally, so `--force` isn't a missing flag, it's a nonsense one.
   Delete it from the doc. **P0.**
3. **`crates/xtask/src/build_skill_refs/authoring.rs`**'s `ClassDef` arm
   (`render_entry`) emits only `class Name` + the class docstring, discarding
   the body — so `Context`, the most-used object in the whole surface, has no
   documented methods in the generated skill reference
   (`skills/toolr-command-authoring/references/commands.md`). Fix: walk
   `def.body`, render public methods with the existing `function_signature`/
   `function_docstring` helpers, nested under the class entry. **P1, moderate
   Rust, drift-gated by `build-skill-refs --check` forever after.**
4. **`tools/pyproject.toml` scaffold** doesn't make `tools.*` importable under
   the docs' own recommended `toolr project venv run -- pytest tools/`.
   Feedback proposed `[tool.pytest.ini_options] pythonpath = [".."]` — this is
   the wrong fix (inverts the runner's deliberate append-only `sys.path`
   precedence in `_runner.py::_append_repo_root`, is config-resolution
   fragile, wrong-relative outside the `tools/` rootdir). A per-project
   scaffolded `tools/tests/conftest.py` was considered next, but the actual
   fix landed one layer up: a real pytest plugin,
   `toolr._pytest_plugin`, registered via a `pytest11` entry point in
   `crates/toolr-py/pyproject.toml`. Every `tools/` package already depends
   on `toolr-py`, so this activates automatically for every toolr project
   with zero scaffolding — one fix in `toolr-py`, not a template copied into
   every repo that can drift. `pytest_configure` discovers repo root the
   same way `toolr_core::discovery::discover_project_root` does (nearest
   ancestor containing `tools/`) and calls the existing
   `_runner._append_repo_root`. A session-scoped `repo_root` fixture is
   included as a bonus for tests that want the path directly. Tests live at
   `tools/tests/` (this repo's blessed convention going forward — mirrors
   `tests/` at the repo root), so `pytest tools/` collects everything under
   the tree regardless of nesting. **P1, done.**
5. **No `make_context` testing helper.** `toolr.testing.CommandsTester`
   covers command *discovery* only; nothing constructs a `Context` to unit
   test a decorated function's body. Add a minimal
   `toolr.testing.make_context(repo_root, **overrides)` returning a real
   `Context` with in-memory consoles. **P2, small-medium.**
6. **`CommandGroup.command`'s docstring** (`_decorators.py`) reads like
   cross-file `@group.command` is unsupported; it works fine. Add one
   clarifying sentence. **P2, trivial.**
7. **No multi-file, multi-subgroup worked example.** Add one alongside
   `examples/tools/greet.py` / `db.py`, regenerate the committed
   `examples/toolr-manifest.json`, document that the "don't add `__init__.py`
   to `tools/`" rule doesn't extend to subpackages. **P2, small.**
8. **`toolr.testing` isn't mentioned in either SKILL.md.** Add a short
   "Testing your commands" section once (5) exists, covering both
   `CommandsTester` and `make_context`.

## Preventing recurrence

Two of the three drift classes found here already have a gate — the fix is
using it, not inventing a new one:

- **Generated reference drift** (item 3): `cargo xtask build-skill-refs
  --check` fails CI when `skills/*/references/*.md` diverges from source
  docstrings. Once `Context`'s methods render there, they can't silently rot.
- **CLI behaviour drift** (item 2, `--force`): `toolr pre-commit
  regen-doc-snippets` runs the real binary and captures real output into
  `docs/**/*.txt`. A `--force` example captured this way would have failed
  the moment the flag stopped existing, instead of quietly going stale.

The `check=True` bug was neither — `docs/writing-commands/files/example.py`
already calls `ctx.run` correctly and is embedded via `--8<--`; the bug was a
**hand-typed prose sentence next to the real snippet, describing the same
call slightly wrong**. That's the pattern with no mechanical gate: prose
asserting API shape (a kwarg, a flag, a return value) written free-hand
beside — but not derived from — the thing it describes.

**Rule going forward:** when documentation states what a function accepts or
a command supports, prefer embedding a real, type-checked/executed snippet
over describing it in prose. Where prose is unavoidable (semantics, not
syntax — e.g. "nonzero exit does not raise"), keep it adjacent to the real
snippet so a reviewer reads both at once, and flag doc PRs that add a bare
code fence (not an `--8<--` include) showing a function signature or CLI
invocation as needing double-checking against source before merge.

## Non-goals

- Adding a `check=` kwarg to `Context.run` itself (a real design option the
  brainstorm raised) — out of scope here; this pass fixes docs to match
  current behaviour, doesn't change the API.
- The `.pth`-in-venv follow-up for repo-root import contract — bigger,
  deserves its own spec if pursued later.
