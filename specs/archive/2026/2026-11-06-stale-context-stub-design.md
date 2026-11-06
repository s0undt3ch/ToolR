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
- Nothing guards the pair: no `stubtest` (it needs the built `_rust_utils` extension to import the module),
  and no test compares the files.

An AST diff of the two files shows the missing function is the only drift: top-level names differ by
`status_to_exit_code` alone, and `Context` members match.

## Design

1. Add `def status_to_exit_code(status: int) -> int: ...` to `_context.pyi`, placed before `class Context` to
   mirror the `.py` order. Stub files carry no docstrings in this repo, so none is added.
2. Add a drift guard: a pytest module at `tests/context/test_stub_parity.py` that parses `_context.py` and `_context.pyi` with `ast` and asserts every
   top-level name and every `Context` member in the `.py` is declared in the `.pyi`.

Why the guard is worth it: it is about 30 lines, needs no built extension or mypy run, and the drift is
already a recurring failure with a confusing symptom (CI green, commit hook red). It checks names only, not
signatures. Signature drift stays with mypy and review, which keeps the test cheap and unlikely to flake.
The check is one-directional (`.py` names must appear in the `.pyi`), because a stub-only name is a different
bug that mypy surfaces in the stub's own callers.

Rejected:

- Delete the `.pyi` and rely on the annotated `.py` plus `py.typed`. It removes the drift class entirely, but
  the stub exists on purpose (overloads and `...` bodies for the type surface, asserted by
  `tests/distribution/test_toolr_py_wheel.py`) and removing it changes the shipped wheel. Out of scope for a
  stale-name fix; raise separately if wanted.
- `stubtest` in CI. It needs the compiled extension, which the pre-commit environment does not have.
- Make the mypy hook always pass `_context.py`. It hides the stub from the hook, so it would leave users
  type-checking against a stale stub unnoticed.

## Scope and risk

Type-only change plus one test file; no runtime behaviour changes. The stub affects downstream type checking
only, so the user-visible effect is that `from toolr._context import status_to_exit_code` now type-checks. That
is a private module (`_context`), so no release note: use a `Release-Notes: skip` trailer on the implementation
commits.

## Open questions

- Whether to drop the `.pyi` altogether (see Rejected). Default: no.
