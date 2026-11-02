# Show the traceback when a command module fails to import

- **Status:** implemented
- **Origin:** whole-project review finding F-28 (medium).
- **Plan:** [`2026-10-31-runner-import-traceback-plan.md`](./2026-10-31-runner-import-traceback-plan.md)
- **Stacks on:** `fix/runner-type-hints` (F-03,
  [`specs/archive/2026/2026-10-30-runner-type-hints-design.md`](./2026-10-30-runner-type-hints-design.md));
  merged first.

## Problem

`_import_target` (`crates/toolr-py/python/toolr/_runner.py`) imports the command module with
`importlib.import_module(spec.module)`. It catches `ImportError` and raises
`SpecError(f"failed to import {spec.module}: {exc}") from exc`. The `except SpecError` arm in `run()` prints that one
line and, because the cause is an `ImportError`, calls `_print_missing_dep_hint`. It never prints the traceback.

Every other failure keeps its traceback. A `SyntaxError` or `NameError` at module level falls through to
`except Exception` and `traceback.print_exc()`. An `ImportError` raised inside the command body goes to
`except ImportError`, which prints the traceback and then the hint.

So a module-level import failure two files away loses the file and line that caused it. With this layout:

```python
# tools/demo.py
from tools._helpers import load

# tools/_helpers.py
import yaml
```

and `yaml` not installed, `toolr demo hello` prints only:

```text
toolr runner: failed to import tools.demo: No module named 'yaml'

toolr: import `yaml` failed at runtime. A dependency may be missing - run `toolr project venv sync` and check tools/pyproject.toml.
```

Nothing names `tools/_helpers.py`. Worse, the same hint appears when the missing module is the repo's own code: a typo
such as `from tools._helprs import load`, or a circular import between two `tools/*.py` files (`cannot import name
'load' from partially initialized module 'tools._helpers'`). `toolr project venv sync` never installs the in-repo
`tools` package, so it cannot fix either, and the hint sends the user the wrong way.

`tests/runner/test_runner_coverage.py::test_run_emits_missing_dep_hint_for_transitive_importerror_via_spec_error`
covers this arm with a hand-built `ImportError` that has no traceback, so the gap never showed.

## Decision

Two changes, both in `_runner.py`. F-03 changed `_hints_and_signature`, `_coerce_args` and the new
`toolr/utils/_annotations.py`; this change touches only the `run()` exception arms and `_print_missing_dep_hint`, so the
two don't overlap.

### 1. Print the cause's traceback in the `SpecError` arm

When `exc.__cause__` is an `ImportError`, the arm prints, in this order:

1. `traceback.print_exception(exc.__cause__, file=sys.stderr)`, the import chain down to the failing line.
2. The existing `toolr runner: failed to import <module>: <message>` line.
3. The hint from part 2, when it applies.

The summary line comes after the traceback so the last thing on screen names the command module and the remedy, as
the in-body `ImportError` arm does now.

`_import_target` keeps wrapping the error in `SpecError`, and the exit code stays 2. Other `SpecError`s (an unknown
`arg()` keyword, a missing attribute) don't have an `ImportError` cause and print as now. CPython strips the
`importlib._bootstrap` frames only on the C `import`/`__import__` path; `importlib.import_module` runs the pure-Python
`_gcd_import` and keeps about eight of them. So `_import_target` calls `__import__(spec.module)` and reads the module from
`sys.modules`, which leaves one `_import_target` frame above the user's frames.

### 2. Skip the venv-sync hint for the repo's own `tools` package

`_print_missing_dep_hint` returns without printing when the `ImportError`'s `name` is `tools` or starts with `tools.`.
Both `ImportError` arms call it, so the in-body arm gets the same rule. When `name` is `None` (a hand-raised
`ImportError`), the generic hint stays, as `test_run_falls_back_to_generic_hint_when_importerror_has_no_name` pins.

The rule is literal `tools`, not "the top-level package of `spec.module`". The in-repo `tools` package is never
installed into the venv, so the hint is always wrong for it. A plugin command's missing submodule (`my_plugin._util`)
can mean a partial or stale install, where reinstalling through `venv sync` does help.

Known limitation: when `tools._helpers` defines a name only after a guarded optional third-party import, importing that
name raises `ImportError(name='tools._helpers')` and gets no hint, though the traceback still shows the spot.

### Rejected alternatives

- **Drop the `SpecError` wrap and let `except ImportError` handle both cases.** It already prints a traceback, but the
  exit code would change from 2 to 1, the `failed to import <module>` line would go, and
  `test_import_target_missing_module_raises_spec_error` would need rewriting for no gain.
- **Trim the traceback to start at the user's module.** Walking `__traceback__` to skip runner and `importlib` frames
  depends on CPython's frame layout per version. One extra frame isn't worth that.
- **Replace the hint with a "fix your import" hint for `tools.*`.** The traceback already names the file and line; a
  second message would only repeat it.

## Scope and compatibility

- No schema change. The runner spec is untouched, so no `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump.
- Behaviour change: stderr only. Exit codes stay the same.
- Docs: none. No page documents the import-failure output.
- Release note, `release-notes/2026-11-02.14-12-43.runner-import-traceback.md`:

  > When a command module fails to import, whether it's one of your `tools/*.py` files or comes from a plugin, toolr
  > now prints the traceback, so an import that fails in a helper module names that file and line. The "run `toolr
  > project venv sync`" hint no longer appears when the missing module is part of your own `tools` package (a typo or a
  > circular import), since syncing the venv can't fix those.
