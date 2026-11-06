# Plan: stale `_context.pyi` stub (FU-25)

Design: [`2026-11-06-stale-context-stub-design.md`](2026-11-06-stale-context-stub-design.md).

All commands run from the worktree root. Commits use the `Release-Notes: skip` trailer.

## Task 1: failing guard test

File: `tests/test_context_stub_parity.py` (new; `tests/__init__.py` already exists).

- Module constants: `PKG = Path(__file__).parents[1] / "crates/toolr-py/python/toolr"`.
- Helper `_declared_names(path: Path) -> tuple[set[str], set[str]]`: parse with `ast`, return
  `(top_level_names, context_member_names)`. Top-level covers `FunctionDef`, `ClassDef` and `Assign` / `AnnAssign`
  targets that are `ast.Name`. Members come from the `Context` class body: `FunctionDef` names and `AnnAssign`
  targets. Import top-of-file; no deferred imports.
- `test_context_stub_declares_every_top_level_name`: assert `top(py) - top(pyi) == set()`.
- `test_context_stub_declares_every_context_member`: assert `members(py) - members(pyi) == set()`.

Assertions compare against the empty set so a failure prints the missing names.

Verify the test fails first:

```sh
uv run pytest tests/test_context_stub_parity.py -q
```

Expected: `test_context_stub_declares_every_top_level_name` fails with `{'status_to_exit_code'}`; the member test
passes.

## Task 2: fix the stub

File: `crates/toolr-py/python/toolr/_context.pyi`: add `def status_to_exit_code(status: int) -> int: ...`
immediately before `class Context(Struct, frozen=True):`.

## Task 3: verify

```sh
uv run pytest tests/test_context_stub_parity.py tests/context/test_exit.py -q
prek run mypy --files tests/context/test_exit.py      # was failing, must pass
prek run mypy --all-files
uv run ruff format tests/test_context_stub_parity.py
prek run --all-files
mise run test
```

`mise run test` is required because Python files changed. `cargo xtask build-skill-refs --check` runs inside it;
this change touches no `__all__`, so it must stay green without a regen.

## Task 4: commit and archive

1. `fix(toolr-py): declare status_to_exit_code in the _context stub`, with the stub and the test in one commit,
   trailer `Release-Notes: skip`.
2. Last commit before the PR: `git mv` the design and plan into `specs/archive/2026/`.
3. `toolr release-notes check --base main` to confirm the skip is accepted.

Stage files explicitly by path; never `git add -A`.
