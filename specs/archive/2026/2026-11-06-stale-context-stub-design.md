# Stale `_context.pyi` stub (FU-25)

## Problem

A staged-only mypy pre-commit run fails with:

```text
tests/context/test_exit.py:9: error: Module "toolr._context" has no attribute "status_to_exit_code"  [attr-defined]
```

while `prek run mypy --all-files` passes. Reported from PR #679; still reproduces on `main` (f65b07a2).

## Findings

Reproduced in this worktree with the pinned hook environment:

| Command | Result |
| --- | --- |
| `prek run mypy --files tests/context/test_exit.py` | fails, the error above |
| `prek run mypy --all-files` | passes |
| `mypy tests/context/test_exit.py crates/toolr-py/python/toolr/_runner.py` | fails, one error per importer |
| `mypy tests/context/test_exit.py crates/toolr-py/python/toolr/_context.py` | passes |
| `mypy crates/toolr-py/python/toolr/_context.pyi tests/context/test_exit.py` | fails |

The local msgspec-related mypy failures did not appear in these runs, so this is not that issue.

Root cause:

- `toolr._context` is a pure-Python module, not a Rust one. `status_to_exit_code` is a plain function in
  `crates/toolr-py/python/toolr/_context.py` (added in ca24a526), imported by `_runner.py` and
  `tests/context/test_exit.py`.
- `crates/toolr-py/python/toolr/_context.pyi` is a hand-written stub shipped beside the `.py` (the wheel test
  asserts both files). The stub was never updated, so it lacks that one name.
- mypy prefers a `.pyi` over its `.py` when resolving an import, so every importer sees the stub.
- `--all-files` passes only because the hook then lists `_context.py` as an explicit source file. An explicit
  source is checked as the module itself, and the stub is ignored. A staged-only run that omits `_context.py`
  resolves the import through the stub and reports the gap.
- Nothing guards the pair: the earlier draft of this spec assumed `stubtest` could not run because it needs the
  built `_rust_utils` extension, but the dev `.venv` has it built and CI runs pytest against a prebuilt toolr-py
  wheel.

`mypy.stubtest toolr._context` shows the stub had drifted by more than the one missing name:

- `status_to_exit_code` was absent.
- `Context.run` lacked `interactive: bool = False` and typed `stream_output` as `bool = True` where the runtime
  is `bool | None = None`.
- `Context.chdir` is `@contextmanager` at runtime but was a plain `def` in the stub.
- Ten `Context.__init__` errors, which are noise: msgspec generates `__init__` and stubtest cannot see its
  signature.

## Design

1. Sync the stub with the runtime: declare `status_to_exit_code`, add `interactive` and fix `stream_output` on
   `run`, and decorate `chdir` with `@contextmanager`.
2. Replace the drift guard with `tests/context/test_stub_parity.py`, which runs
   `python -m mypy.stubtest toolr._context --allowlist tests/context/files/stubtest_allowlist.txt` in a
   subprocess from the repo root and asserts exit 0, printing stubtest's output on failure. The allowlist holds
   only the msgspec `__init__` noise. The test is skipped when `toolr.utils._rust_utils` is not importable, since
   stubtest could then only report an import error.

Why stubtest and not an `ast` comparison: the first implementation compared declared names only. Review found it
missed `async def`, nested bodies, re-exports, stub-only names and every signature difference, and it let the
`run` and `chdir` drift through. stubtest imports the runtime and compares names, signatures, defaults and
decorators in both directions, so none of those need bespoke code. Its limit is that it does not compare
parameter annotations on their own; a pure annotation change with an unchanged default is not reported.

CI: `_test.yml` runs `uv run --no-sync coverage run -m pytest` against the dev group (which includes mypy) plus
the prebuilt toolr-py wheel, so the extension is present. `tests/distribution/` jobs run only that directory and
never collect this test.

Rejected:

- Delete the `.pyi` and rely on the annotated `.py` plus `py.typed`. It removes the drift class entirely, but
  the stub exists on purpose (overloads and `...` bodies for the type surface, asserted by
  `tests/distribution/test_toolr_py_wheel.py`) and removing it changes the shipped wheel. Out of scope; raise
  separately if wanted.
- An `ast` name comparison (the first implementation); see above.
- Make the mypy hook always pass `_context.py`. It hides the stub from the hook, so it would leave users
  type-checking against a stale stub unnoticed.

## Scope and risk

Type-only change plus one test file; no runtime behaviour changes. The stub affects downstream type checking
only: `from toolr._context import status_to_exit_code` now type-checks, `ctx.run(..., interactive=True)` no longer
fails a type check, and `ctx.chdir(...)` is typed as a context manager. No release note: use a `Release-Notes: skip` trailer on the implementation
commits.

## Open questions

- Whether to drop the `.pyi` altogether (see Rejected). Default: no.
