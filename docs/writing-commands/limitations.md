# Known limitations

The rust front-end runs every command function the Python registry
defines, but a handful of registry-side features haven't been wired
into the new manifest / dispatch pipeline yet. They behave correctly
when the underlying argparse Python runner is used directly, just not
through the `toolr` binary.

Each item below links to the issue tracking the fix.

- **Positional `int` / `float` coercion** — arguments declared with
  `a: int` arrive at the function as strings. Workaround: coerce in
  the function body (`int(a)`). Tracked in
  [issue #194](https://github.com/s0undt3ch/ToolR/issues/194).
- **`bool` flag inference** — a parameter typed `bool = False` is
  exposed as a value-taking flag (`--verbose <verbose>`) instead of a
  no-value flag (`--verbose`). Tracked in
  [issue #195](https://github.com/s0undt3ch/ToolR/issues/195).
- **Argument name normalisation** — underscores in parameter names
  aren't converted to hyphens (`dry_run` exposes `--dry_run`, not
  `--dry-run`). Tracked in
  [issue #196](https://github.com/s0undt3ch/ToolR/issues/196).
- **Enum default rendering** — `--help` shows `[default: <expr>]` for
  enum-typed parameters instead of the resolved member name. The
  default value itself is applied correctly. Tracked in
  [issue #197](https://github.com/s0undt3ch/ToolR/issues/197).
- **Nested command groups** — `CommandGroup.command_group(...)` works
  in the Python registry but the resulting children are exposed as
  flat siblings by the rust binary. Tracked in
  [issue #193](https://github.com/s0undt3ch/ToolR/issues/193) (see also
  the [Nested groups](nesting.md) page).

If you hit anything else that doesn't match the documented behaviour,
please file an issue.
