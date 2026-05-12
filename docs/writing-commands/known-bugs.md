# Known bugs

These are gaps in the rust-front-end rewrite, not design choices —
the legacy argparse front-end handled all of them correctly. Each
one is tagged `blocks-ga` on the issue tracker.

If you hit any of these, you can work around them as noted below
while the fix is in flight.

- **Positional `int` / `float` coercion is broken** — arguments
  declared with `a: int` arrive at the function as strings. The
  manifest records the type correctly; the bug is in the rust spec
  builder (`src/execute/build.rs`), which always serialises
  positional values as strings instead of the typed JSON value
  msgspec would coerce. Workaround: cast in the function body
  (`int(a)`). Tracked in
  [issue #194](https://github.com/s0undt3ch/ToolR/issues/194).
- **`bool` parameters render as value-taking flags** — a parameter
  typed `bool = False` shows up as `--verbose <verbose>` instead of
  a no-value `--verbose`. Workaround: invoke `--verbose true`.
  Tracked in
  [issue #195](https://github.com/s0undt3ch/ToolR/issues/195).
- **Underscore-named parameters don't become hyphenated flags** —
  `dry_run: bool = False` exposes `--dry_run`, not `--dry-run`.
  Function names already follow the hyphen convention; parameters
  should too. Workaround: invoke `--dry_run` with the underscore.
  Tracked in
  [issue #196](https://github.com/s0undt3ch/ToolR/issues/196).
- **Enum default rendering shows `<expr>`** — `--help` prints
  `[default: <expr>]` for enum-typed parameters instead of the
  resolved member name. The runtime value is still correct — only
  the displayed help is wrong. Tracked in
  [issue #197](https://github.com/s0undt3ch/ToolR/issues/197).
- **Nested command groups appear as flat siblings** —
  `CommandGroup.command_group(...)` works in the Python registry,
  but the rust binary's manifest has no `parent` field, so
  `docker image build` becomes three sibling top-level groups
  (`docker`, `image`, `build`) at the CLI surface. Tracked in
  [issue #193](https://github.com/s0undt3ch/ToolR/issues/193) (see also
  the [Nested groups](nesting.md) page).

If you hit something else that doesn't match the documented
behaviour, please file an issue — these are bugs to fix, not the
new normal.
