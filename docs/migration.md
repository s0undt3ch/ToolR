# Migration: legacy decorators → string-path API

Toolr's original decorator API attached commands to a captured
`CommandGroup` binding. That style is **deprecated** and will be
removed in **toolr 1.0**. Every legacy call emits a
`ToolrDeprecationWarning` at runtime so the offenders are easy to
spot.

The replacement is mechanical — most projects can migrate with two
search-and-replace passes per file.

## What's changing

| Legacy (deprecated)                              | Replacement                                  |
|--------------------------------------------------|----------------------------------------------|
| `group = command_group("foo", ...)`              | `command_group("foo", ...)` (no assignment)  |
| `@group.command`                                 | `@command(group="foo")`                      |
| `@group.command("custom-name")`                  | `@command("custom-name", group="foo")`       |
| `parent.command_group("child", ...)`             | `command_group("parent.child", ...)`         |
| `command_group("child", parent=parent_var)`      | `command_group("parent.child", ...)`         |

The legacy forms continue to work through the 0.x line; the goal of
migrating now is to silence the deprecation warnings and avoid
breakage when 1.0 lands.

## Step 1 — Imports

Add `command` to your `toolr` imports (alongside `command_group`):

```python
from toolr import command
from toolr import command_group
```

## Step 2 — Group declarations

Drop the binding. The string in the first positional argument is the
only identity the new API needs.

Before:

```python
group = command_group(
    "example",
    "Example commands",
    description="…",
)
```

After:

```python
command_group(
    "example",
    "Example commands",
    description="…",
)
```

## Step 3 — Command decorators

Replace the bound method with the free-function decorator. The group
name is passed by string.

Before:

```python
@group.command
def hello(ctx, name="world"):
    ctx.print(f"hello, {name}")


@group.command("custom-name")
def some_function(ctx):
    ...
```

After:

```python
@command(group="example")
def hello(ctx, name="world"):
    ctx.print(f"hello, {name}")


@command("custom-name", group="example")
def some_function(ctx):
    ...
```

## Step 4 — Nested groups

Replace method-call subgroups with dotted paths. The child names its
parent inline.

Before:

```python
docker = command_group("docker", "Docker", "Container utilities")
docker_image = docker.command_group("image", "Image")


@docker_image.command
def build(ctx, tag: str): ...
```

After:

```python
command_group("docker", "Docker", "Container utilities")
command_group("docker.image", "Image")


@command(group="docker.image")
def build(ctx, tag: str): ...
```

The `parent="parent"` keyword is an equivalent alternative if you
prefer to keep the child's leaf name unprefixed:

```python
command_group("image", parent="docker", description="…")
```

## Step 5 — Run the deprecation warnings to zero

Invoke any toolr command from your repo and inspect stderr. Each
remaining legacy site emits a one-time warning identifying:

- The deprecated call.
- The exact line of source.
- The migration recipe.

Re-run after each batch of edits until no warnings remain.

## Why migrate

- **Files become decoupled.** No need to import a shared
  `CommandGroup` binding across files just to attach commands.
- **Order independence.** A `@command` in `tools/foo.py` can attach
  to a `command_group` declared in `tools/_common.py`, regardless of
  scan order.
- **Typo safety.** Misspelled group references fail manifest-build
  with a "did you mean" suggestion instead of a runtime
  `NameError` (or silent registration on the wrong binding).
- **Forward compatibility.** The legacy decorators won't ship in
  toolr 1.0. Migrating now means no last-minute scramble.

If you hit anything the recipe above doesn't cover — for instance,
projects that subclass `CommandGroup` or wrap the decorators — open
an issue on [GitHub](https://github.com/s0undt3ch/ToolR/issues) so we
can document the path.
