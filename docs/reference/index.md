# API reference

The user-facing Python API. For internal-only modules see the source
under [`crates/toolr-py/python/toolr/`](https://github.com/s0undt3ch/ToolR/tree/main/crates/toolr-py/python/toolr).

- [`Context`](context.md) — passed to every command function as `ctx`.
- [`command_group`](command_group.md) — declared at module scope in
  `tools/*.py` files.
- [`command`](command.md) — string-path decorator for attaching
  commands to a group without holding its binding.
- [`arg`](arg.md) — annotation for advanced argument options.
- [`testing`](testing.md) — helpers for testing your commands.
- [`build`](build.md) — emit a `toolr-manifest.json` for a
  third-party package.
