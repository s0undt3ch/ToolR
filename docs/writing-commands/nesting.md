<!-- rumdl-disable MD046 -->

# Nested groups

Subgroups organise related commands under a common parent. There are
four equivalent ways to express the parent-child relationship; pick
the one that reads best for your file layout.

## Method-call style

Each `CommandGroup` exposes a `.command_group(...)` method that
returns a child group bound to it. The most common pattern, used when
every subgroup lives alongside its parent:

```python
--8<-- "docs/writing-commands/files/nesting-example.py"
```

In the Python model this declares:

- A top-level `docker` group.
- Two subgroups: `docker image` and `docker container`.
- A `build` command on `docker image` and a `start` command on
  `docker container`.

…produces the CLI hierarchy:

- `toolr docker --help` lists the two subgroups (`image`, `container`)
  as commands.
- `toolr docker image build my-image:latest` reaches the `build` command.
- `toolr docker container start my-container` reaches the `start` command.

There's no fixed depth limit — `outer.command_group("middle").command_group("inner")`
works just as well.

## Dotted-path style

The new string-path API supports parent paths inline:

```python
--8<-- "docs/writing-commands/files/nesting-dotted-example.py"
```

`command_group("docker.image", …)` declares a child of the `docker`
group. The leaf name (`image`) and the parent path (`docker`) are
split inside the registry; nothing else has to change. Multi-level
paths (`docker.image.layer`) work identically.

## `parent=` keyword

When the dotted name reads awkwardly, you can spell the parent out:

```python
# Both forms are equivalent to command_group("docker.image", ...).
command_group("image", parent="docker")

# Or pass a CommandGroup reference (works across imports, see below).
parent_group = command_group("docker", ...)
command_group("image", parent=parent_group)
```

Mixing `parent=` with a dotted name is allowed but the dotted form
wins (toolr logs a warning at registration time so the mismatch is
visible).

## Cross-file parents

A subgroup doesn't have to live in the same file as its parent. The
toolr parser collects every `command_group(...)` declaration across
`tools/**/*.py` before resolving any parent references, so this works
in either file order:

```python
# tools/_common.py
command_group("ci", docstring=__doc__)
```

```python
# tools/helm.py
command_group("ci.helm-diff", description="Helm diff helpers")


@command(group="ci.helm-diff")
def backend(ctx, env: str) -> None: ...
```

For the binding style, importing the parent's `CommandGroup` directly
also works:

```python
# tools/helm.py
from ._common import group as ci

helm_diff = ci.command_group("helm-diff", description="…")


@helm_diff.command
def backend(ctx, env: str) -> None: ...
```

Either path produces the same CLI: `toolr ci helm-diff backend`.

!!! note "Shell tab completion"
    Top-level groups, their subgroups, and the commands attached to
    either tab-complete out of the box. If you're using an older
    shell-completion script and notice nested subgroups don't
    complete, run `toolr self completion install <shell> --force`
    to refresh it.
