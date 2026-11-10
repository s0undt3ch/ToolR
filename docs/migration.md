# Migration: nested-group method form → dotted strings

Toolr originally let you declare a subgroup by calling
`.command_group(...)` on a captured parent binding:

```python
docker = command_group("docker", "Docker", "Container utilities")
docker_image = docker.command_group("image", "Image", "Image subcommands")
```

That form was **removed**. The manifest build fails on
it with a pointer to the dotted form, and `CommandGroup` no longer has
a `command_group` method.

The bound `@group.command` decorator on a captured `CommandGroup` is
unaffected and continues to be supported — see
[Groups & commands](writing-commands/groups.md) for the canonical
single-file usage and
[Scaling command groups across files](writing-commands/across-files.md)
for the string-keyed form you'd use across multiple files.

## What's changing

| Removed                                          | Replacement                                  |
|--------------------------------------------------|----------------------------------------------|
| `parent.command_group("child", ...)`             | `command_group("parent.child", ...)`         |

`@group.command` and `@group.command("custom-name")` continue to
work.

## Recipe

Replace the bound subgroup-method call with a dotted
`command_group(...)` declaration. The child names its parent inline:

Before:

```python
docker = command_group("docker", "Docker", "Container utilities")
docker_image = docker.command_group("image", "Image", "Image subcommands")


@docker_image.command
def build(ctx, tag: str): ...
```

After:

```python
docker = command_group("docker", "Docker", "Container utilities")
docker_image = command_group("docker.image", "Image", "Image subcommands")


@docker_image.command
def build(ctx, tag: str): ...
```

The `parent="parent"` keyword is an equivalent alternative if you
prefer to keep the child's leaf name unprefixed:

```python
command_group("image", parent="docker", description="…")
```

## Why it was removed

The captured-binding form for *subgroups* reads awkwardly once nesting
gets deeper than one level
(`a.command_group("b").command_group("c", ...)`); dotted strings
flatten that to `command_group("a.b.c", ...)` with no chain, and the
static parser reads them without following variables.

The remaining rationale for the string-keyed `@command(group="…")`
form lives in
[Scaling command groups across files](writing-commands/across-files.md) —
that decorator is a choice, not a migration step.

If you hit anything the recipe above doesn't cover — for instance,
projects that subclass `CommandGroup` or wrap the decorators — open
an issue on [GitHub](https://github.com/s0undt3ch/ToolR/issues) so we
can document the path.
