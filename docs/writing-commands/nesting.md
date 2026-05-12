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

The intended CLI hierarchy is `toolr docker image build my-image:latest`
and `toolr docker container start my-container`.

!!! warning "Currently rust-front-end-limited"
    The rust binary's manifest model treats every group as top-level;
    it doesn't yet carry the parent relationship forward into the CLI
    surface. Nested groups appear as flat sibling groups instead of
    a true hierarchy. The python registry behaviour is unchanged —
    only the rust binary's `--help` and dispatch are affected.

    Tracked in [GitHub issue #193](https://github.com/s0undt3ch/ToolR/issues/193).
