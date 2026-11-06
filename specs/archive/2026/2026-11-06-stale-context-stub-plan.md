# Plan: stale `_context.pyi` stub (FU-25)

Design: [`2026-11-06-stale-context-stub-design.md`](2026-11-06-stale-context-stub-design.md).

All commands run from the worktree root. Spec-only commits carry the `Release-Notes: skip` trailer; the stub
fix ships a release note.

## Task 1: sync the stub

Run `.venv/bin/python -m mypy.stubtest toolr._context` and fix every real report in
`crates/toolr-py/python/toolr/_context.pyi`:

- add `def status_to_exit_code(status: int) -> int: ...` before `class Context`;
- `Context.run`: `stream_output: bool | None = None`, add `interactive: bool = False`;
- `Context.chdir`: add `@contextmanager`.

Only the `Context.__init__` reports (msgspec-generated) may remain.

## Task 2: stubtest test

- `tests/context/files/stubtest_allowlist.txt`: one entry, `toolr\._context\.Context\.__init__`, with a comment
  saying why.
- `tests/context/test_context_stub.py`: one test running
  `sys.executable -m mypy.stubtest toolr._context --allowlist <file>` from the repo root, asserting exit 0 and
  showing stubtest's output on failure. Skipped locally when `toolr.utils._rust_utils` cannot be found; in CI a
  missing extension fails it.

Prove it fails, restoring after each: delete `status_to_exit_code` from the stub; add `def ghost() -> int: ...`;
rename a parameter.

## Task 3: verify

```sh
uv run pytest tests/context -q
prek run mypy --files tests/context/test_exit.py
prek run --all-files
mise run test
```

## Task 4: commit and archive

1. `fix(toolr-py): sync the _context stub with run and chdir` (stub only).
2. `test(toolr-py): check the _context stub with stubtest` (test and allowlist).
3. Last commit before the PR: `git mv` the design and plan into `specs/archive/2026/` (already done on this
   branch; the content is updated in place).
4. `toolr release-notes check --base main` before opening the PR.

Stage files explicitly by path; never `git add -A`.
