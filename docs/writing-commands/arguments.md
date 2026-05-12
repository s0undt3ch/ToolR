# Arguments

Every parameter of a command function becomes a CLI argument. Toolr
infers the shape (positional vs optional, with-value vs flag, enum vs
free-form) from the parameter's **type hint**, **default value**, and
**syntactic position**.

The first parameter (`ctx: Context`) is always toolr's, never a CLI
argument.

## Positional arguments

Parameters without a default value become **required positional** CLI
arguments. The annotation is reported in `--help` and used by toolr
for shell completion.

```python
--8<-- "docs/writing-commands/files/calculator.py"
```

```sh
toolr math add --help
```

```text
--8<-- "docs/writing-commands/files/calculator-add-help.txt"
```

## Optional arguments (with a default value)

Parameters with a default value become `--name VALUE` flags. The type
hint dictates the value type:

```python
--8<-- "docs/writing-commands/files/hello.py"
```

```sh
toolr greeting hello --help
```

```text
--8<-- "docs/writing-commands/files/hello-help.txt"
```

## Boolean flags

A `bool` annotation with a default of `False` is declared as a flag:

```python
--8<-- "docs/writing-commands/files/flags-example.py"
```

## `Literal[...]` for choice-restricted values

A `Literal["a", "b", "c"]` annotation produces a `--name {a,b,c}`
flag that validates against the allowed values and shows them in
`--help`.

```python
--8<-- "docs/writing-commands/files/literal-choices.py"
```

```sh
toolr logs set-level --help
```

```text
--8<-- "docs/writing-commands/files/literal-choices-help.txt"
```

## Enums

A parameter annotated with an `enum.Enum` (or `StrEnum`) subclass
behaves the same as `Literal[...]` — the choices are the enum
members, the resolved value is the enum instance. See the
`Operation` enum on `docs/writing-commands/files/docstrings-example.py`
for a full example.

## `list[T]` for repeated values

Annotate a parameter as `list[T]` to accept `--name VALUE` repeated
multiple times (each invocation appends).

```python
--8<-- "docs/writing-commands/files/files-list.py"
```

## `*args` for variadic positionals

Capture an arbitrary number of positional arguments with `*args`. The
annotation on the parameter is the element type.

```python
--8<-- "docs/writing-commands/files/files-star-args.py"
```

!!! note "Current rust-front-end coverage"
    The Python registry resolves every form described here, but the
    rust binary's runner is still catching up with a few of them — see
    [Known limitations](limitations.md) for the exact list and the
    issues that track them.

Next: [Docstrings →](docstrings.md)
