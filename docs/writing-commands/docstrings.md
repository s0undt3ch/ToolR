# Docstrings

Toolr requires **Google-style docstrings** on every command. The
docstring drives the `--help` output: the summary line, the long
description, and per-argument help text.

## Anatomy

```python
--8<-- "docs/writing-commands/files/docstrings-example.py"
```

The module-level docstring of `docstrings-example.py` is passed to
`command_group(..., docstring=__doc__)` and becomes the group's
description. The per-command docstrings become the subcommand
descriptions; the `Args:` block populates `--help` for each parameter.

## Rules

- The **first line** of the docstring is the command summary used in
  the parent group's `--help` listing.
- The rest of the body (until `Args:`) is the long description shown
  on `<command> --help`.
- The `Args:` block lists parameters: `<name>: <description>`. Lines
  must follow Google's indentation convention; consult
  [the napoleon docs](https://sphinxcontrib-napoleon.readthedocs.io/en/latest/example_google.html)
  if you need a refresher.
- Argument descriptions support inline Markdown — the docstring of
  `docstrings-example.py` includes a table that renders in `--help`.

## Failing without a docstring

A command function with no docstring is rejected at manifest-build
time: toolr refuses to ship undocumented commands. Add a one-line
summary and you're good.

Next: [Using `ctx` →](context.md)
