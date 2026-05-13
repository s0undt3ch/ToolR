<!-- rumdl-disable MD046 -->

# Nested groups

Subgroups organise related commands under a common parent. Express
the relationship in one of two equivalent ways.

## Dotted path inside `command_group(...)`

The shortest spelling. Everything before the final dot is the parent's
full path; the leaf becomes the child group's name.

```python
--8<-- "docs/writing-commands/files/nesting-example.py"
```

That file declares:

- A top-level `docker` group.
- Two subgroups: `docker image` and `docker container`.
- A `build` command on `docker image` and a `start` command on
  `docker container`.

…and produces the CLI hierarchy:

- `toolr docker --help` lists the two subgroups (`image`, `container`).
- `toolr docker image build my-image:latest` reaches the `build` command.
- `toolr docker container start my-container` reaches the `start` command.

No fixed depth limit — `command_group("a.b.c.d")` works just as well.

## `parent="..."` keyword

When the dotted name reads awkwardly, spell the parent out explicitly:

```python
command_group("docker", title="Docker")
command_group("image", parent="docker", description="Image subcommands")


@command(group="docker.image")
def build(ctx, tag: str) -> None: ...
```

The two styles are exchangeable; mixing them inside one project is
fine. (Combining a dotted name *and* a `parent=` kwarg is allowed but
the dotted form wins — toolr logs a warning at registration time so
the conflict is visible.)

## Cross-file parents

A subgroup doesn't have to live in the same file as its parent.
Toolr's parser collects every `command_group(...)` declaration across
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

Either way the resulting CLI is `toolr ci helm-diff backend`.

!!! note "Shell tab completion"
    Top-level groups, their subgroups, and the commands attached to
    either tab-complete out of the box. If you're using an older
    shell-completion script and notice nested subgroups don't
    complete, run `toolr self completion install <shell> --force`
    to refresh it.

!!! warning "Legacy method form deprecated"
    `parent.command_group("child", ...)` — the method call on a
    captured binding — still works but emits a
    `ToolrDeprecationWarning` and will be removed in toolr 1.0.
    Replace each call with a dotted `command_group(...)` declaration.
    See the [migration guide](../migration.md).
