# Annotations

When you need more than the defaults — short flags, aliases, mutual
exclusion, value choices — wrap the parameter's type with
`typing.Annotated[...]` and a call to [`arg(...)`][toolr.arg].

## Aliases (short flags + alternate long flags)

```python
--8<-- "docs/writing-commands/files/docstrings-example.py:77:84"
```

The `Annotated[Operation, arg(aliases=["-o", "--op"])]` makes
`--operation`, `-o`, and `--op` all reach the same parameter.

## Mutually exclusive groups

Use the `group=` keyword on `arg(...)` to mark a set of arguments as
mutually exclusive — at most one of them can be set per invocation.

### Simple case

```python
--8<-- "docs/writing-commands/files/mutually-exclusive-1.py"
```

Passing `--verbose --quiet` together fails CLI validation:

```text
toolr: error: argument --quiet: not allowed with argument --verbose
```

### Multiple groups

A function can have several independent groups. Each is named; the
name is what ties members together.

```python
--8<-- "docs/writing-commands/files/mutually-exclusive-2.py"
```

Valid invocations pick at most one member from each group:

```sh
# OK — one from each group
toolr example analyze-data input.txt --verbose --json --fast

# Fails — two from the verbosity group
toolr example analyze-data input.txt --verbose --quiet

# Fails — two from the format group
toolr example analyze-data input.txt --json --yaml
```

### Constraints

- **Positional parameters cannot be in a group.** Toolr raises
  `SignatureError` at registration time if a parameter without a
  default is annotated with `arg(group=...)`.
- Every member of a group must have a default value (otherwise the CLI
  could be in a state where none of them are passed and one is
  required, which has no clean resolution).

Next: [Nested groups →](nesting.md)
