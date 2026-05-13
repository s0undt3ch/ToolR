<!-- rumdl-disable MD046 -->

# Annotations

When you need more than the defaults — alternate flag spellings, env-var
defaults, mutually-exclusive groups, grouped `--help` sections, filesystem
constraints — wrap the parameter's type with `typing.Annotated[...]` and
a call to [`arg(...)`][toolr.arg].

```python
from typing import Annotated
from toolr import arg, command, command_group

command_group("git", title="Git helpers")


@command(group="git")
def show(
    ctx,
    sha: Annotated[str, arg(aliases=["-s"], metavar="SHA", env="GIT_SHA")] = "HEAD",
) -> None: ...
```

## Available kwargs

| Kwarg | What it does |
|---|---|
| `aliases=["-n", "--also"]` | Extra short / long flag spellings. Single-char entries become clap shorts; longer entries become long aliases. |
| `metavar="NAME"` | Custom placeholder shown in `--help` (`<NAME>` instead of `<sha>`). |
| `env="VAR"` | Read the default from this env var when the flag isn't passed. |
| `hide=True` | Omit from `--help` output. Still parseable on the CLI. |
| `help_section=…` | Group related args under a named `--help` heading. See below. |
| `display_order=N` | Lower values render first in `--help`. |
| `conflicts_with=[...]` | Mutex relationships: at most one of these flags per invocation. |
| `requires=[...]` | If this flag is set, every name listed must also be set. |
| `path_must_exist=True` | Reject paths that don't exist (Path types only). |
| `path_must_be_file=True` | Reject non-files; implies `path_must_exist`. |
| `path_must_be_dir=True` | Reject non-dirs; implies `path_must_exist`. |

## Aliases (short flags + alternate long flags)

```python
@command(group="git")
def diff(
    ctx,
    base: Annotated[str, arg(aliases=["-b", "--from"])] = "HEAD~1",
) -> None: ...
```

`--base`, `-b`, and `--from` all reach the same parameter. Hyphens at the
start of each alias are stripped; one-character names become shorts.

## Env-var defaults

```python
@command(group="cloud")
def deploy(
    ctx,
    api_token: Annotated[str, arg(env="DEPLOY_API_TOKEN")],
) -> None: ...
```

When the user doesn't pass `--api-token`, clap reads `$DEPLOY_API_TOKEN`
instead. If both the env var and a `default=` are absent and the flag
isn't passed, the parameter remains required.

## Mutually exclusive arguments — `conflicts_with`

```python
@command(group="example")
def hello(
    ctx,
    name: str,
    verbose: Annotated[bool, arg(conflicts_with=["quiet"])] = False,
    quiet: Annotated[bool, arg(conflicts_with=["verbose"])] = False,
) -> None: ...
```

Passing `--verbose --quiet` together fails CLI validation:

```text
error: the argument '--verbose' cannot be used with '--quiet'
```

`requires=[...]` is the inverse: if you pass this flag, every name in
the list must also be set. Useful for paired options like
`--ssl-cert` + `--ssl-key`.

## Display grouping with `help_section`

Big functions accumulate flags; bare `--help` listings get hard to scan.
Group related flags under a named heading by declaring an
[`arg_section`][toolr.arg_section] at module scope, then pointing each
member's annotation at it:

```python
from toolr import arg, arg_section, command, command_group

LOGGING = arg_section(
    "Logging Options",
    description="Control verbosity and output format.",
)

command_group("deploy", title="Deploy")


@command(group="deploy")
def push(
    ctx,
    target: str,
    verbose: Annotated[bool, arg(help_section=LOGGING)] = False,
    quiet: Annotated[bool, arg(help_section=LOGGING)] = False,
    log_file: Annotated[str | None, arg(help_section=LOGGING, env="DEPLOY_LOG")] = None,
) -> None: ...
```

The `LOGGING` object is reusable — share it across commands in the
same file. Two `arg_section("X")` calls don't make the same section;
define the object once and pass it by reference.

### Inline form

If you don't want a module-level constant for a one-off heading, pass
the call directly:

```python
verbose: Annotated[bool, arg(help_section=arg_section("Logging Options"))] = False
```

Or, for the simplest case where you just want a title and no
description, a bare string is accepted:

```python
verbose: Annotated[bool, arg(help_section="Logging Options")] = False
```

## Deprecated kwargs (removed in 1.0)

These still parse but emit a `ToolrDeprecationWarning`:

- `required=` — use `T | None` or `*args: T`.
- `choices=` — use `Literal["a","b"]` or an `Enum`.
- `nargs=` — use `T | None` / `*args: T` / `tuple[T1, T2]`.
- `action=` — `bool` infers flag, `list[T]` infers append, `Count` infers count.
- `group=` — use `conflicts_with=[…]` for mutex, `help_section=` for display grouping.
- `must_exist=` / `must_be_file=` / `must_be_dir=` — rename to `path_must_exist` / `path_must_be_file` / `path_must_be_dir`.

Next: [Nested groups →](nesting.md)
