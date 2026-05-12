# Diagnostics

When a command fails because of missing dependencies, toolr aims for
a one-line diagnostic that names the missing package and the fix —
not a 50-line Python traceback at the user.

## Pre-flight check

Before spawning the Python subprocess that runs your command, the
binary inspects the target command's recorded top-level imports
(captured by the static parser) and probes the resolved tools venv
for each:

```text
<venv>/lib/python*/site-packages/<module>/__init__.py
<venv>/lib/python*/site-packages/<module>.py
```

If any required import is missing, toolr aborts before invoking
Python and prints:

```text
toolr: pre-flight check failed: `requests` is not installed in the tools venv.
toolr:   command: my-group fetch
toolr:   venv:    /Users/you/.cache/toolr/<repo-key>/venv
toolr:   fix:     toolr project deps sync
toolr: (exit 78)
```

Exit code **78** distinguishes pre-flight failures from runtime
errors (per the `sysexits.h` `EX_CONFIG` convention). CI can branch
on this code to surface configuration vs application failures.

## Post-mortem interception

For imports that the static parser couldn't see (deferred imports
inside function bodies, conditional imports, etc.), toolr also
intercepts `ImportError` from the Python subprocess. When detected,
it appends the same `toolr project deps sync` suggestion to the
captured traceback before re-emitting it:

```text
ModuleNotFoundError: No module named 'foo'

toolr: hint: the import above failed. If `foo` should be in the tools
toolr:       venv, run `toolr project deps sync`.
```

The original traceback is preserved — toolr only adds a hint
underneath.

## Disabling pre-flight

The pre-flight check can produce false positives in edge cases —
PEP 420 namespace packages without `__init__.py`, packages that
ship a single `*.pth` file, etc. Set:

```sh
export TOOLR_NO_PREFLIGHT_DEPS=1
```

…to skip pre-flight entirely. Post-mortem interception still
fires, so missing imports are still reported with a fix hint.

Accepted truthy values: any non-empty string except `0`.

## Why not just rely on Python tracebacks?

Two reasons:

1. **Speed.** Pre-flight is a few `stat()` calls; spawning Python
   to discover a missing module costs hundreds of ms.
2. **Signal-to-noise.** A Python traceback for "module not found"
   buries the actual fix in 20 lines of stack frames. Toolr's
   diagnostic puts the fix on line two.
