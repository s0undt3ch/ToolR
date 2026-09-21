# Context-local helpers via `contextvars`

Status: draft
Date: 2026-09-21

## Problem

Command-authoring helpers today must take `ctx: Context` as an explicit
parameter and every call site must thread it through, even for helpers many
call levels deep from the `@command` function. Developers who don't want
that threading have no alternative.

## Goal

Add an ambient accessor, `toolr.current_context() -> Context`, that a helper
function can call to get the `Context` of the toolr command currently
executing, without it being passed as a parameter.

## Non-goals

- Not a replacement for the `ctx` parameter. `@command`-decorated functions
  keep receiving `ctx` as their required first argument, unchanged. Helpers
  that already take `ctx` explicitly are untouched and keep working exactly
  as today.
- Not a concurrency primitive. `toolr` runs one command per process,
  single-threaded, and this feature does not change that. It exists purely
  to remove parameter threading, not to enable safe cross-task context
  sharing.
- Not solving arbitrary background-thread or asyncio-task access to context.
  See "Limitations" below — that's out of scope, and papering over it with
  `threading.local` would silently give wrong answers under asyncio, so it's
  explicitly rejected rather than deferred.

## Design

### Storage

A module-private `contextvars.ContextVar[Context]` in `toolr/_context.py`:

```python
_current_ctx: ContextVar[Context] = ContextVar("toolr_current_context")
```

### Public accessor

```python
def current_context() -> Context:
    """Return the `Context` of the toolr command currently executing.

    Raises `RuntimeError` if called outside of a running command, at
    import time, or from a thread/task that wasn't given the context
    explicitly (see the `current_context` docs for why).
    """
```

Implementation calls `_current_ctx.get()` and catches `LookupError`,
re-raising as `RuntimeError` with a message naming all three ways this can
happen (see "Error message" below). No `current_context_or_none()` variant —
one function, it raises.

### Where the var gets set

**Production path** (`toolr/_runner.py`): set at the call site right before
the target function is invoked (around `_runner.py:554`'s
`ctx = _build_context(spec)`), not inside `_build_context()` itself.
`_build_context()` stays side-effect-free — it's also used by
`toolr.testing.make_context()`'s lineage of "build a Context" logic
conceptually, and construction should never have the side effect of
mutating global state.

```python
ctx = _build_context(spec)
_current_ctx.set(ctx)
# ... call target function with ctx ...
```

No reset needed on this path: the process exits after the one command runs.

**Test path** (`toolr/testing/`): `make_context()` does **not** auto-set the
contextvar. Building a `Context` object stays a pure, side-effect-free
factory call — consistent with the production path's `_build_context()`, and
consistent with tests today that build several `Context`/`ContextForTesting`
instances in a single test (e.g. to compare two configurations) without any
of them becoming ambiently "the" current one.

Instead, add an explicit opt-in context manager:

```python
# toolr/testing/_current_context.py
@contextmanager
def set_current_context(ctx: Context) -> Iterator[Context]:
    """Make `ctx` the value `toolr.current_context()` returns for this block.

    Use when a command (or a helper it calls) reads `toolr.current_context()`
    instead of taking `ctx` as a parameter, and a test needs that call to
    resolve to a specific `Context` under test:

        ctx = make_context(repo_root)
        with toolr.testing.set_current_context(ctx):
            my_command(ctx, ...)
    """
```

A test that calls a command through `current_context()`-using helpers
without this wrapper gets the `RuntimeError` from `current_context()` —
same failure mode as forgetting to pass `ctx` to a helper that requires it
explicitly, not a new one.

### Error message

`current_context()`'s `RuntimeError` names all three ways it fires, since
this message is the entire UX of the feature when it goes wrong:

```text
current_context() has no context to return. This happens when:
  - called outside of a running toolr command (e.g. interactively, or from
    a script that isn't a toolr command/runner invocation)
  - called at import time (import happens before the command's Context
    is built and set)
  - called from a thread or async task that wasn't given the context
    explicitly — see toolr.current_context's docs

If testing a helper that calls this, wrap the call in
toolr.testing.set_current_context(ctx).
```

### Limitations (documented, not solved)

`ContextVar` propagates to code running in the same OS thread / same
`asyncio` task tree as whoever called `.set()`. It does **not** propagate
into:

- a `threading.Thread` started manually inside a command,
- a `concurrent.futures.ThreadPoolExecutor` worker,
- (it *does* propagate into `asyncio.create_task` children — no action
  needed there).

A command that spawns real OS threads and wants `current_context()` to work
inside them must pass `ctx` into the thread explicitly (e.g. via the
thread's target closure) — the accessor doesn't cross that boundary and
nothing in this design attempts to make it. `toolr-command-authoring`'s
docs call this out explicitly so it isn't discovered by surprise.

## Public API surface changes

- `toolr.current_context` — new, added to `toolr/__init__.py`'s `__all__`.
- `toolr.testing.set_current_context` — new, added to
  `toolr/testing/__init__.py`'s `__all__`.

Both are walked by the existing `build-skill-refs` generators (per
CLAUDE.md, `toolr.__all__` and `toolr.testing.__all__` are already
registered) — run `cargo xtask build-skill-refs` after adding the exports,
no new generator needed.

`toolr-command-authoring`'s prose (not just its generated reference table)
needs a new section: when to use the `ctx` parameter vs. `current_context()`,
and the thread/asyncio limitation above. The generator won't catch a missed
prose update — this is a manual check per CLAUDE.md's guidance on adding
names to an existing `__all__`.

## Testing

- `current_context()` raises when nothing is set.
- `current_context()` returns the value set by `_runner.py` when invoked
  through a real (or `assert_cmd`-driven integration) command run.
- `toolr.testing.set_current_context(ctx)` makes `current_context()` return
  `ctx` for the duration of the `with` block, and raises again after it
  exits (even if the block raised).
- Nesting `set_current_context` (inner block temporarily overrides outer)
  restores the outer value on exit — standard `ContextVar.set()`/`.reset()`
  token behavior, verified with a test rather than assumed.
- A helper called via `current_context()` from inside an `@command` function
  invoked through `make_context()` + `set_current_context()` sees the same
  `Context` instance the command itself received.

## Release notes

`UNRELEASED.md` entry, `feat(...)` — new public API, minor version
territory per this repo's version-bump convention.

## Out of scope / explicitly rejected

- Auto-setting the contextvar inside `make_context()`. Rejected: it would
  make `Context` construction have a global side effect, breaking the
  "building a Context is pure" invariant the production path also relies
  on, and would silently make whichever `Context` was built *last* in a
  test module become ambient for unrelated tests unless every test
  remembered to reset it — worse ergonomics than one explicit `with`.
- `threading.local` instead of `contextvars.ContextVar`. Rejected: gives a
  wrong (stale or missing) answer under `asyncio`, where `ContextVar` gives
  the correct per-task answer for free.
- Making `current_context()` fall back to some default/empty `Context`
  instead of raising. Rejected: a silently-wrong `Context` (empty repo_root,
  no console) is worse than a loud, specific `RuntimeError`.
