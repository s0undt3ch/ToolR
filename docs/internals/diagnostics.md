# Diagnostics

When a command fails because of missing dependencies, toolr aims for
a one-line diagnostic that names the fix — not a bare Python
traceback at the user.

## Missing-dependency interception

When a command's import fails at runtime, toolr intercepts the
`ImportError` from the Python subprocess. When detected, it appends a
`toolr project venv sync` suggestion to the captured traceback before
re-emitting it:

```text
ModuleNotFoundError: No module named 'foo'

toolr: hint: the import above failed. If `foo` should be in the tools
toolr:       venv, run `toolr project venv sync`.
```

The original traceback is preserved — toolr only adds a hint
underneath.
