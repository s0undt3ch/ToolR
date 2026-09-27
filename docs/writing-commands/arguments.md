# Arguments

Every parameter of a command function becomes a CLI argument. Toolr
infers the shape (positional vs optional, with-value vs flag, enum vs
free-form) from the parameter's **type hint**, **default value**, and
**syntactic position**.

The first parameter (`ctx: Context`) is always toolr's, never a CLI
argument.

## Supported types

Toolr enforces a closed set of parameter types. Anything outside this
table is rejected at manifest-build time with an error pointing at
[`toolr.types`](#richer-types-via-toolrtypes) as the extension namespace.

--8<-- "docs/writing-commands/files/supported-types.md"

Variadic `*args: T` accepts any `T` above; see the [`*args` for variadic
positionals](#args-for-variadic-positionals) section below.

Bad input fails fast at the clap parse layer — no Python spawn:

```sh
$ toolr math add A 3
error: invalid value 'A' for '<a>': invalid digit found in string
```

### Richer types via `toolr.types`

Stdlib primitives are recognised natively. Anything richer is opt-in
through the `toolr.types` namespace, which makes the supported set
**discoverable** (`dir(toolr.types)`) and stops uncoupled annotations
from quietly drifting:

```python
from toolr.types import DateTime, UUID, Email

def schedule(ctx: Context, when: DateTime, job_id: UUID, owner: Email) -> None: ...
```

Each name is a stdlib alias at runtime (`DateTime is datetime.datetime`,
`UUID is uuid.UUID`, …) — toolr-specific only at the import-path level.
If you annotate with a type toolr doesn't recognise (e.g. `datetime.datetime`
directly, or a custom dataclass), manifest-build rejects the file with
a pointer to `toolr.types` for the extension namespace.

<!-- --8<-- [start:arg-shapes] -->
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

### Zero-or-one positionals (`T | None` without a default)

A `T | None` annotation **without** a default declares a positional
that takes zero or one value:

```python
def bump(ctx: Context, new_version: str | None) -> None:
    """Bump the version.

    Args:
        new_version: Explicit version to bump to, or omit for auto.
    """
```

```sh
toolr version bump            # new_version is None
toolr version bump 0.20.0     # new_version is "0.20.0"
```

This is the type-driven replacement for the deprecated
`arg(nargs="?")` kwarg.

A few rules apply, all enforced at manifest-build time so you get a
clear error rather than confusing runtime behavior:

| Rule | Why |
|---|---|
| At most **one** `T \| None` positional per command. | clap can't disambiguate two trailing optionals — which arg fills which slot? |
| Required positionals must appear **before** the `T \| None` slot. | Once the parser accepts "no value here" it can't backtrack to fill a required slot that comes later. |
| `T \| None` and `*args: T` cannot coexist. | Both compete for the trailing slot. |
| Fixed-arity `tuple[T1, T2, …]` positionals **may** precede a `T \| None` slot. | Tuples have deterministic arity, so there's no ambiguity. |

`T | None` **with** a default (e.g. `name: str | None = None`) is a
keyword `--flag` instead — see [Optional arguments](#optional-arguments-with-a-default-value)
below. The default's presence is what flips the parameter from
positional to keyword; the annotation alone doesn't.

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
members, the resolved value is the enum instance:

```python
--8<-- "docs/writing-commands/files/docstrings-example.py:operation-enum"
```

Annotate the parameter with the enum type directly, e.g.
`operation: Operation`, the same way you would with any other
supported type.

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

## Path constraints

`pathlib.Path` (and its `toolr.types.AbsolutePath` / `ResolvedPath`
variants) accept additional opt-in filesystem checks through
`Annotated[Path, arg(...)]`:

--8<-- "docs/writing-commands/files/path-constraints.md"

```python
from pathlib import Path
from typing import Annotated
from toolr import arg

def read_config(
    ctx: Context,
    config: Annotated[Path, arg(must_be_file=True)],
    workdir: Annotated[Path, arg(must_be_dir=True)],
) -> None:
    ...
```

The constraints fire at clap-parse time — bad invocations error in
microseconds with a precise message:

```sh
$ toolr fs read /tmp/missing.txt /tmp
error: invalid value '/tmp/missing.txt' for '<config>':
path does not exist: /tmp/missing.txt
```

## Module-level type aliases

Repeating the same `Annotated[…]` blob across several command
signatures gets noisy. Define it once at module scope; toolr's
static parser follows the alias to its underlying type:

```python
from typing import Annotated, TypeAlias

from toolr import Context, arg, command_group

CommitHash: TypeAlias = Annotated[
    str | None,
    arg(help="A 40-char git SHA, or None for HEAD."),
]

git = command_group("git", title="Git helpers")


@git.command
def show(ctx: Context, sha: CommitHash = None) -> None: ...


@git.command
def diff(ctx: Context, base: CommitHash = None) -> None: ...
```

Both `show` and `diff` end up with the same `--sha` / `--base`
treatment, with the alias's `arg(...)` metadata applied to each.
Aliases compose with any supported type (see the supported-types
table), including `list[…]`, `Literal[…]`, `T | None`, and
`toolr.types.*`.

## Heterogeneous tuples

A `tuple[T1, T2, …]` parameter declares a fixed-arity positional
group; toolr enforces the count at clap-parse time and msgspec
validates each slot against its declared type:

```python
def link(ctx: Context, mapping: tuple[str, int]) -> None: ...
```

```sh
toolr graph link foo 7      # OK
toolr graph link foo bar    # error: invalid value 'bar' for slot 1: invalid digit
toolr graph link foo        # error: missing slot 1
```

The same shape works for keyword args too:

```python
def deploy(ctx: Context, port_range: tuple[int, int] = (8000, 8100)) -> None: ...
# → toolr cluster deploy --port-range 9000 9100
```

clap consumes two values per `--port-range` occurrence; msgspec coerces
each slot to `int` against the function's hint.

## Counting flags

`toolr.types.Count` turns a parameter into a "repeat the short form to
count" flag, matching the classic `-vvv` pattern:

```python
from typing import Annotated
from toolr import arg, command
from toolr.types import Count

@command(group="example")
def serve(ctx: Context, verbose: Annotated[Count, arg(aliases=["-v"])] = 0) -> None:
    ctx.print(f"verbosity level: {verbose}")
```

```sh
toolr example serve            # verbosity level: 0
toolr example serve -v         # verbosity level: 1
toolr example serve -vvv       # verbosity level: 3
```

The Python runtime value is plain `int` (`Count` is just an `int`
alias); the rust side wires `clap::ArgAction::Count` based on the
annotation.
<!-- --8<-- [end:arg-shapes] -->

Next: [Docstrings →](docstrings.md)
