# Scaling command groups across files

The bound-decorator form shown in [Groups & commands](groups.md) is
the canonical shape for tools that live in a single file. When your
tools spread across multiple files, importing the `CommandGroup`
binding around your project gets noisy. Toolr's *string-keyed*
decorator was built for that case.

## The form

Declare the group in one file:

```python
# tools/_common.py
from toolr import command_group

command_group("ci", title="CI", description="Continuous-integration helpers")
```

Attach commands from any other file — no shared import:

```python
# tools/helm.py
from toolr import Context, command


@command(group="ci")
def helm_diff(ctx: Context) -> None:
    """Diff the cluster against rendered manifests."""
    ...


@command(group="ci")
def deploy(ctx: Context, env: str) -> None:
    """Deploy to `env`."""
    ...
```

Both files contribute to the same `ci` group. `toolr ci --help`
lists `helm-diff` and `deploy` regardless of which file the static
parser visited first.

## Why use it

**Files become decoupled.** Each file is self-contained. No need to
import (or otherwise share) a `CommandGroup` binding across modules
just to attach a command to its group.

**Order independence.** Toolr does a two-pass static parse: every
`command_group(...)` declaration is collected first, then every
`@command(group=...)` reference is resolved against the registry.
The order files are scanned in doesn't matter — `tools/helm.py` can
attach to a `ci` group declared in `tools/_common.py` regardless of
which file the parser visits first.

**Typo safety.** Misspelled group references fail at manifest-build
time with the nearest match suggested:

```text
error: unknown group references (1):
  - tools.helm::deploy: references group `c1` which has no
    `command_group(...)` declaration. Did you mean `ci`?
```

The bound-decorator form catches the same kind of mistake at import
time (as a `NameError` on the binding), so both forms fail fast —
they just fail in different places. The string-keyed form fails
during *manifest build*, which is the same step that surfaces every
other static error and reads them all at once.

**Overriding the CLI name.** Pass the name as the first positional
argument:

```python
@command("snippet-checker", group="ci")
def check_snippets(ctx: Context) -> None: ...
# → toolr ci snippet-checker
```

## When to choose which form

| Situation | Form |
|---|---|
| All commands for a group live in the same file | `group = command_group(...)` + `@group.command` |
| Commands span multiple files, or you want one file to attach to a group declared elsewhere | `command_group("name", ...)` in one file, `@command(group="name")` in the others |
| Nested groups (subgroups) | `command_group("parent.child", ...)` — see [Nested groups](nesting.md) |

Both forms produce identical CLIs. Picking the wrong one for your
project size doesn't produce different behaviour, just different
ergonomics — the rule of thumb is "use the bound form until imports
start to chafe, then switch."

## Mixing styles

A single project can use both — for example, a flat in-file group in
one tools module and a string-keyed group spanning several modules
in another. Toolr resolves both through the same registry, so a
group declared with the bound form is reachable from a string key
in a different file, and vice versa.

## Sharing plain helper code

The decorator forms above are for wiring *commands* to groups. When
you just want to reuse ordinary functions between files — a shared
constant, a `helpers.py`, a `version.py` — import them directly.

`tools/` is a [PEP 420](https://peps.python.org/pep-0420/) implicit
namespace package: it needs **no `__init__.py`** (don't add one).
toolr puts the repo root on `sys.path` and imports your command
modules *under* the `tools` package, so both import styles resolve
from any command file — absolute or relative:

```python
# tools/ci.py
import tools.version                 # absolute module import
from tools import version            # absolute, name-bound
from tools.helpers import render     # absolute, import a name

from . import version                # relative module import
from .helpers import render          # relative, import a name

def build(ctx):
    ctx.info(f"Building {version.CURRENT}")
```

Use whichever reads best; both work under `toolr` at runtime and in
tests (toolr's testing harness imports every `tools.*` module the same
way). The only thing to avoid is manual `sys.path` manipulation —
toolr has already set it up for you.

Next: [Nested groups →](nesting.md)
