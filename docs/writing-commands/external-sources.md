# External command sources

Every toolr command is a function you write and decorate with
[`@command`](groups.md) — the ones in this guide included. *External command
sources* don't add a different kind of command. They let a single toolr command
you write (a *dispatcher*) run subcommands that **already exist** as
[argparse](https://docs.python.org/3/library/argparse.html)-style files
elsewhere in the repo, so you can drive those tools through toolr without
rewriting each subcommand as a toolr command.

The motivating case is **Django management commands** — each
`management/commands/<name>.py` file is an argparse command, and there can be
dozens. Rather than re-author each one, you point an external command source at
them: it scans those files statically and grafts the discovered commands under
your dispatcher, which runs the real upstream command when invoked. The
mechanism is generic argparse, not Django-specific; Django is simply the layout
it maps onto most cleanly (see [Command names](#command-names) for the one
convention this relies on).

## How it works

You declare one or more *argparse blocks* in `tools/pyproject.toml`. When toolr
[builds its manifest](../internals/manifest.md), each block:

1. **Scans** the files matched by its `scan_paths` globs. Each file is parsed
   statically — toolr reads the file's `add_argument(...)` calls from the AST;
   it never imports or runs the file. One file becomes one command, and its
   **name is the file's stem** (`migrate.py` → `migrate`). The module docstring
   becomes the command's summary and description.
2. **Grafts** the discovered commands as children under the dispatcher command(s)
   named in `attach`. The parent automatically becomes a command group.

At runtime, invoking a grafted command (`toolr django migrate …`) calls *your
dispatcher function* with a [`DispatchCommand`](#the-dispatcher) payload
describing which command was invoked and how its arguments parsed. Your
dispatcher decides what to do with that — typically reconstruct the argument
vector and hand it to the upstream tool.

Like everything in the static manifest, scanning runs no repository code, so
`toolr --help` and tab completion stay execution-free.

## A worked example

A Django-style project laid out like this:

```text
myproject/
├── apps/
│   └── billing/
│       └── management/
│           └── commands/
│               └── migrate.py
└── tools/
    ├── pyproject.toml
    └── dispatcher.py
```

The upstream command file is left exactly as Django expects it — toolr only
reads its `add_arguments`:

```python
# apps/billing/management/commands/migrate.py
"""Migrate the database."""


def add_arguments(self, parser):
    parser.add_argument("--check", action="store_true", help="Dry run")
    parser.add_argument("--database", default="default", help="Target DB")
```

You write one dispatcher command. Any keyword-only parameter annotated
`DispatchCommand` receives the dispatch payload:

```python
# tools/dispatcher.py
from toolr import Context, command_group
from toolr.sources import DispatchCommand

group = command_group("django", "Django", description="Run Django management commands")


@group.command
def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
    """Dispatch a Django management command."""
    # dispatched.command      -> "migrate"
    # dispatched.command_args -> {"check": True, "database": "primary"}
    # dispatched.argv         -> ["--check", "--database", "primary"]
    return ctx.run("django-admin", dispatched.command, *dispatched.argv).returncode
```

And you wire the two together in `tools/pyproject.toml`:

```toml
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]

[[tool.toolr.argparse.django.attach]]
parent = "django"
```

After a manifest rebuild, the scanned commands appear under the dispatcher:

```sh
toolr django migrate --check --database primary
```

`migrate` shows up in `toolr django --help` with its own arguments and the
docstring summary, even though you never declared it as a toolr command.

## The dispatcher

A dispatcher is an ordinary toolr command. The only requirement is a
**keyword-only parameter annotated `toolr.sources.DispatchCommand`** — toolr
detects it by its type annotation, so the parameter name is yours to choose.
The payload carries three fields:

| Field | Type | Meaning |
|-------|------|---------|
| `command` | `str` | The discovered command's name (the file stem). |
| `command_args` | `dict[str, Any]` | The parsed arguments, keyed by argument name. |
| `argv` | `list[str]` | `command_args` rebuilt into an argparse-shaped argument vector — flags, options, and positionals in the spelling the upstream tool expects. Defaults that weren't overridden are omitted. |

Use `argv` when you shell out to the upstream tool, and `command_args` when you
want the already-parsed values directly.

A dispatcher can also take its own arguments alongside the payload. They sit
*before* the grafted command on the command line:

```python
@group.command
def job(
    ctx: Context,
    *,
    cpu: str = "1000m",
    ram: str = "4Gi",
    dispatched: DispatchCommand,
) -> int:
    """Run a management command as a Jenkins job."""
    ...
```

```sh
toolr jenkins job --cpu 5000m migrate --check
#                 ^^^^^^^^^^^^ dispatcher flags   ^^^^^^^^^^^^^^^ grafted command
```

## Configuration reference

Each `[tool.toolr.argparse.<block>]` table defines one source. The table key
(`<block>`) is the source's name; it appears in build errors and need not match
any dispatcher.

| Key | Type | Meaning |
|-----|------|---------|
| `scan_paths` | list of glob strings | Files to scan, relative to the project root. Each matched file becomes one command. Files with no `add_argument` calls are skipped. |
| `common_args` | list of tables | Arguments merged into *every* command discovered by this block — handy for flags the upstream tool accepts globally (e.g. Django's `--verbosity`). Each entry takes `name`, `kind` (`positional` / `optional` / `flag` / `repeated`), and optional `help`, `default`, `choices`. |
| `attach` | list of tables | Where to graft the discovered commands. Each `[[…attach]]` entry takes a `parent` dotted path naming the dispatcher. |

```toml
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]
common_args = [
  { name = "verbosity", kind = "optional", default = "1", help = "Verbosity level" },
]

[[tool.toolr.argparse.django.attach]]
parent = "django"
```

`parent` may be a top-level group command (`"django"`) or a nested command
(`"jenkins.job"`). A single source can attach to several parents — list more
than one `[[…attach]]` entry and the same commands appear under each
independently.

If two sources graft the **same command name onto the same parent**, the
manifest rebuild fails with an error naming both sources — collisions are
surfaced at build time, never resolved silently.

## Command names

A command's name is the **stem of its source file** — `migrate.py` becomes
`migrate`. This is the one convention the feature leans on.

It fits Django's `management/commands/<name>.py` layout exactly, which is what
the feature was built for: the file you'd run as `manage.py migrate` becomes
`toolr django migrate` with no extra mapping. For other argparse-based tools the
same rule applies — the **file naming is the contract**, so name your scanned
files after the commands you want. There is currently no override (a class
attribute, an argparse `prog`, an explicit name in config); name derivation is
refined on a need basis, so open an issue if your layout doesn't map cleanly
onto the file-stem rule.

## Rebuilding the manifest

Scanning happens when the manifest is built. Rebuild explicitly with:

```sh
toolr project manifest rebuild
```

toolr also rebuilds automatically when the manifest is missing or stale, so the
first invocation after adding or editing a source picks the change up on its
own. See [Manifest layers](../internals/manifest.md) for when rebuilds trigger.
