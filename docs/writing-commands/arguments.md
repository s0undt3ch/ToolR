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

| Annotation | Validated by | Wire format | Python receives |
|---|---|---|---|
| `int` | clap | JSON number | `int` |
| `float` | clap | JSON number | `float` |
| `bool` | clap | JSON bool | `bool` |
| `str` | none (passthrough) | JSON string | `str` |
| `pathlib.Path` | clap (custom parser) | string | `pathlib.Path` |
| `toolr.types.AbsolutePath` | clap (absolutise vs cwd) | absolute string | `pathlib.Path` |
| `toolr.types.ResolvedPath` | clap (`canonicalize()`) | resolved string | `pathlib.Path` |
| `toolr.types.DateTime` | clap (chrono RFC 3339) | string | `datetime.datetime` |
| `toolr.types.Date` | clap (chrono ISO date) | string | `datetime.date` |
| `toolr.types.Time` | clap (chrono ISO time) | string | `datetime.time` |
| `toolr.types.UUID` | clap (`uuid` crate) | string | `uuid.UUID` |
| `toolr.types.IPv4` | clap (`std::net::Ipv4Addr`) | string | `ipaddress.IPv4Address` |
| `toolr.types.IPv6` | clap (`std::net::Ipv6Addr`) | string | `ipaddress.IPv6Address` |
| `toolr.types.Email` | clap (`email_address` crate) | string | `str` (pre-validated) |
| `toolr.types.Version` | clap (`pep440_rs` crate) | string | `packaging.version.Version` |
| `Literal["a", "b"]` | clap (allowed-values) | string | `Literal` value |
| `Enum` subclass | clap (member values) | string | enum member |
| `list[T]` (T above) | clap per-element | JSON array | `list[T]` |
| `tuple[T1, T2, …]` | clap arity, msgspec per-slot | JSON array | `tuple[T1, T2]` |
| `*args: T` | clap (trailing variadic) | JSON array | splatted `T...` |
| `T \| None` | clap (`required=false`) | typed or absent | `T` or `None` |

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

## Path constraints

`pathlib.Path` (and its `toolr.types.AbsolutePath` / `ResolvedPath`
variants) accept additional opt-in filesystem checks through
`Annotated[Path, arg(...)]`:

| Constraint | Effect |
|---|---|
| `arg(must_exist=True)` | reject paths that don't exist on disk |
| `arg(must_be_file=True)` | reject anything that isn't a regular file (implies `must_exist`) |
| `arg(must_be_dir=True)` | reject anything that isn't a directory (implies `must_exist`) |

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

Next: [Docstrings →](docstrings.md)
