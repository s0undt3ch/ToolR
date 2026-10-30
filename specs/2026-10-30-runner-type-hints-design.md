# Resolve command type hints one parameter at a time

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-03 (high).
- **Plan:** [`2026-10-30-runner-type-hints-plan.md`](./2026-10-30-runner-type-hints-plan.md)

## Problem

The runner coerces the binary's string values with the target function's type
hints. `_hints_and_signature` (`crates/toolr-py/python/toolr/_runner.py`) calls
`typing.get_type_hints(target)` once. That call is all-or-nothing: if one
annotation names something not bound at runtime, it raises, and the runner
falls back to `hints = {}`. Every parameter then gets its raw value.

The usual trigger is an import used only in annotations, moved under
`if TYPE_CHECKING:` with `from __future__ import annotations`. Ruff's TC001-TC003
do this automatically under `select = ["ALL"]`, which this repo and its
examples use. The Rust parser resolves `TYPE_CHECKING` imports
(`parser/symbols.rs`), so `--help` looks right while dispatch degrades without
a word:

```python
from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from pathlib import Path

@group.command
def f(ctx, path: Path, count: int = 1): ...
```

`toolr g f /tmp/x --count 3` calls `f` with `path='/tmp/x'` and `count='3'`. An
omitted `T | None` positional raises a bare `TypeError: missing 1 required
positional argument`, because the `None` fill depends on the hints too.

Enums are the one rescued case: the binary sends `enum_modules`, and the runner
imports those classes into `localns`. Nothing else is rescued.

`detect_dispatch_parameter` (`toolr/utils/_signature.py`) has the same
all-or-nothing call. One unresolvable annotation hides the `DispatchCommand`
parameter, and the command is not treated as a dispatcher.

`tests/runner/test_runner_coverage.py::test_coerce_args_falls_back_when_get_type_hints_raises`
locks the current behaviour in.

## Decision

A new shared helper, `toolr/utils/_annotations.py`, resolves hints per
parameter. Both `_runner._hints_and_signature` and `detect_dispatch_parameter`
use it.

### 1. Per-parameter resolution

Read the unevaluated annotations, then evaluate each one on its own against the
target's module globals and the runner's `localns`:

- Python 3.14+: `annotationlib.get_annotations(target, format=Format.STRING)`.
  The default `VALUE` format evaluates everything and raises, the same problem
  again.
- Python 3.11-3.13: `inspect.get_annotations(target)`, without `eval_str`. It
  returns strings under `from __future__ import annotations`. Without that
  import, an unbound name fails when the module is imported, so the runner
  never sees it.

Each annotation is evaluated with `get_type_hints` semantics: `None` becomes
`NoneType`, and `include_extras=False`. A failure affects that parameter only.
Use `inspect.unwrap(target)` for the globals, as `get_type_hints` does.

### 2. Rescue `TYPE_CHECKING` imports

When a parameter fails with `NameError`, the helper looks for the missing name
among the target module's `TYPE_CHECKING` imports. If it finds it, it imports
that module, adds the binding to `localns` and retries.

- Find the imports with `ast` on the module's source (`inspect.getsource(module)`).
  Look in `if TYPE_CHECKING:` and `if typing.TYPE_CHECKING:` blocks at module
  top level. Handle `from X import Y [as Z]`, `import X [as Z]` and relative
  `from .x import Y`, resolved against `module.__package__`.
- Parse the source at most once per module, and only after a `NameError`. The
  common path costs nothing.
- Import only the module that binds the missing name, and only at coercion
  time. Every module has finished importing by then. The existing
  `_localns_for_enum_modules` docstring explains why this is safe.
- If the import fails, or the name isn't there, the parameter stays
  unresolved. No new failure mode.

`enum_modules` stays. It seeds `localns` first, as it does now. Removing it
would change the runner spec and gains nothing.

### 3. Say so when a hint stays unresolved

If a parameter still can't be resolved, the runner passes its raw value, as
now. It also prints one line per parameter to stderr, unless verbosity is
`quiet`:

```text
toolr: warning: can't resolve the type of `path` on tools.x.f (name 'Path' is not defined); passing the raw value. Import it at runtime, not only under `if TYPE_CHECKING:`.
```

### 4. Errors that aren't missing names

- A `TypeError` from `_unknown_arg_keyword_error` still becomes that
  `SpecError`.
- Any other exception from evaluating one annotation becomes a `SpecError`
  that names the parameter and the function. Today these are swallowed, and
  `detect_dispatch_parameter` already lets them through. A broken annotation
  is a bug to report, not a hint to drop.
- `NameError` and `AttributeError` (an unresolvable `mod.Name`) count as
  "unresolved" (part 3).

`detect_dispatch_parameter` treats an unresolved parameter as "not
`DispatchCommand`" and checks the others. It never warns: the coercion that
follows does.

## Scope and compatibility

- No schema change. The runner spec doesn't change.
- Behaviour change: a command whose annotations raise something other than
  `NameError`/`AttributeError` now fails with a clear error instead of running
  on raw strings. Release note under "Fixed".
- Docs: `docs/writing-commands/arguments.md` (or the section covering
  annotations) gets one paragraph. `TYPE_CHECKING` imports work for argument
  types; an alias that can't be found prints the warning above.
- `UNRELEASED.md`:

  > Command arguments are now converted to their annotated types even when
  > the annotation's import sits under `if TYPE_CHECKING:` (as ruff's
  > TC001-TC003 rules arrange). One such annotation used to stop conversion
  > for every argument of that command, so `count: int` arrived as the
  > string `"3"`. When toolr can't resolve an annotation, it now names the
  > argument in a warning instead of silently passing the raw string.
