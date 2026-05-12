<!-- rumdl-disable MD046 -->

# Nested groups

Beyond top-level groups, the Python registry supports **subgroups**
for organising related commands under a common parent. A
`CommandGroup` exposes a `.command_group(...)` method that returns
a child group; you keep registering commands on the child.

## Example

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

!!! note "Shell tab completion"
    Top-level groups and their direct commands tab-complete out of the
    box. Completion descends into nested groups when the corresponding
    completion script is updated; if you're using an older shell-completion
    script and notice subgroups don't complete, run
    `toolr self completion install <shell> --force` to refresh it.
