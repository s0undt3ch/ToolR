# Drop `RunMock.register(...)`

**Status:** Approved (2026-08-16)
**Topic:** Remove `RunMock.register(...)` and `_Registration` from `toolr.testing`, added in
`specs/archive/2026/2026-08-14-testing-run-mock-design.md`, before that design's PR (#437) merged.
**Supersedes:** the `RunMock.register(...)`/`_Registration` sections of
`specs/archive/2026/2026-08-14-testing-run-mock-design.md` (declarative registration API,
longest-prefix matching, `occurrences` semantics) and the matching sections of
`specs/archive/2026/2026-08-14-testing-run-mock-plan.md` (Task 4's `.register()` steps). Those
documents remain the accurate historical record of what was originally built and why; this document
records what changed after that design was archived, and why, before the same PR merged.

## Background

The original design canonicalized a `pytest-subprocess`-style `.register(cmdline, stdout=...,
occurrences=...)` API on `RunMock`, with longest-leading-prefix matching and an `occurrences` limit
that falls through to the next matching registration once exhausted.

Three rounds of adversarial code review against the same PR, after the design was archived and
implemented, found nearly every real finding concentrated in `.register()`'s own bookkeeping:

- A same-length-prefix tie-break question that, on investigation, turned out to already be handled
  correctly by the shipped code (first-registered-with-remaining-occurrences wins) — but the
  ambiguity itself, and how easy it was for a plausible-sounding "fix" to silently break that
  semantics, was a signal the feature had more moving parts than its value justified.
- `RunMock.args` reflecting the registered prefix instead of the actual invoked command line.
- The capture_output guard's interaction with the registration-vs-escape-hatch resolution paths.
- The `side_effect`-mixing guard existing only because `.register()` needed to detect a conflicting
  configuration path.

None of these findings touched the two things `RunMock` actually adds over a bare `unittest.mock.Mock`:
forcing `stdout`/`stderr` to `None` when `capture_output` wasn't requested (matching the real
runner's contract, which a bare `Mock` won't do on its own), and a fixed, explicit assertion surface
that raises `AttributeError` on a typo instead of `MagicMock`'s auto-vivification. Both survive
`.register()`'s removal untouched.

## Decision

Remove `.register(...)` and `_Registration` from `RunMock` entirely. Callers configure `RunMock`
exactly like any other `Mock`: set `.mock.return_value`/`.mock.side_effect` to a `CommandResult`
built via `make_command_result(...)`, using a `side_effect` callable for per-command dispatch — the
standard `unittest.mock` idiom, not a bespoke API to learn.

`make_command_result(...)` is unchanged in its public signature, but its implementation now
requires `stdout`/`stderr` to resolve to the same underlying type (both `str` or both `bytes`),
coercing a `str` default when the other side is passed as `bytes` — `CommandResult[T]` ties both
fields to the same `T`, so independently-typed `stdout`/`stderr` was never a valid `CommandResult`
for any single `T`, and mypy in CI (though not reproducibly in local runs against the same mypy
version) correctly rejected the previous implementation on exactly this point.

## Non-goals

- No change to the `Context._run_impl`/`_chdir_impl`/`_prompt_stream` seams, `ContextForTesting`, or
  `make_context`'s `run=`/`chdir=`/`prompt_input=` parameters — all of that stands as designed and
  implemented in the superseded documents.
- No change to `RunMock`'s composition-over-`MagicMock` design, or the fixed forwarding list
  principle — only the specific set of forwarded methods grew (`assert_not_called`, `called`),
  found missing by the same adversarial review pass that motivated this document.

## Migration considerations

Nothing has been released yet — this lands in the same, still-open PR (#437) as the original design,
before any `toolr-py` version ships either shape. No `UNRELEASED.md` change needed beyond what
already describes `RunMock`/`make_command_result` (that entry never mentioned `.register()`
specifically).

## Approval

Approved directly by the user in conversation on 2026-08-16, after they asked what value `RunMock`
still had once `.register()` was dropped, confirmed the capture_output guard and fixed-forwarding
answer, and said to proceed.
