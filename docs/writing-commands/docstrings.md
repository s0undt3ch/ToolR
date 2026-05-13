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

## Help-output rendering

`--help` text is rendered as Markdown through
[termimad](https://github.com/Canop/termimad), so the body of a
docstring can use the usual elements:

- Headings (`#`, `##`, …) get distinct styling.
- Inline `code spans` are highlighted.
- Fenced code blocks render in a separate frame.
- Tables work — the `docstrings-example.py` group docstring above
  ships with one.
- Lists, bold, italic, and links all render through.

Note the asymmetry between `-h` and `--help`:

- `-h` on any command prints a single-line summary (the first line
  of the docstring) and the args block. Useful for quick lookup.
- `--help` prints the full docstring — summary, description, args —
  rendered with Markdown.
- On a *parent group*, both forms list child commands by their
  summary line only; the long description is reserved for leaf
  commands.

## Failing without a docstring

A command function with no docstring is rejected at manifest-build
time: toolr refuses to ship undocumented commands. Add a one-line
summary and you're good.

Next: [Using `ctx` →](context.md)
