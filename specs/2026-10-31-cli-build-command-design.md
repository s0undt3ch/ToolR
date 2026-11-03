# Split `cli::build_command` into one builder per built-in subtree

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-31 (medium).
- **Plan:** [`2026-10-31-cli-build-command-plan.md`](./2026-10-31-cli-build-command-plan.md)

## Problem

`crates/toolr/src/cli.rs::build_command` is about 550 lines (lines 133-684 on main at `3a860f41`). It builds,
inline and in this order:

1. the root `toolr` command, its "Output Options" flags and the combined `-h`/`--help` arg;
2. the user group subtrees from the manifest (already delegated to `build_group_subtree`);
3. the `project` subtree (`init`, hidden `deps`, `venv {path,shell,run,sync,lock,add,remove}`, `manifest rebuild`,
   `trust`, `untrust`), about 255 lines;
4. the `self` subtree (`build-manifest`, `cache {list,prune}`, `trusted`, `completion {print,install}`), about 145
   lines;
5. the hidden `__build-static-manifest`, `__complete` and `__install-uv-now` commands.

Every new built-in flag means editing a function too long to read at once, at indentation depths of 20-36 columns.

## Decision

Pure refactor: same clap tree, same order, same help, same completions. Only `build_command`'s body changes. Its
signature and every caller (`main.rs`, `dispatch.rs` tests, `builtin_completions.rs`) stay as they are.

New private functions in `cli.rs`, each returning the `Command` it used to build inline:

- `build_root_command() -> Command`: item 1. `OUTPUT_HEADING` moves to a module-level `const`.
- `build_project_subcommand() -> Command`: item 3, which delegates to `build_project_init_subcommand()` and
  `build_project_venv_subcommand()`, the two parts long enough to need it.
- `build_self_subcommand() -> Command`: item 4, which delegates to `build_self_build_manifest_subcommand()`,
  `build_self_cache_subcommand()` and `build_self_completion_subcommand()`.
- `build_hidden_subcommands() -> [Command; 3]`: item 5, in today's order.

`build_command` becomes: `build_root_command()`, then the user-group loop, then `.subcommand(...)` for `project`,
`self` and each hidden command. Comments move with the code they explain.

Equality is checked three ways: a `{:#?}` dump of `build_command(&empty_manifest())` taken before and after (clap's
`Command` derives `Debug`); `--help` from the old and new binaries for every built-in path; and the existing gates
(`builtin_completions` tests, `toolr pre-commit regen-doc-snippets --check`, `cargo xtask build-skill-refs --check`).
`builtin_completions.rs` walks the built tree, so an identical tree means identical completions.

Rejected alternatives:

- **Move the builders to a new `cli/` module directory.** It would turn `cli.rs` into `cli/mod.rs` and conflict
  with every open branch that touches `cli.rs`, F-11 included. The file shrinks enough without it; a move can come
  later.
- **Declarative table of built-in subcommands.** It would need a mini-DSL for clap's builder options (`hide`,
  `conflicts_with`, `num_args`, headings) and wouldn't be a pure refactor.
- **Also split `dispatch.rs::dispatch`, `argparse/scan.rs::build_argument` and `parser/build.rs::build_commands`.**
  The finding lists them as "also large". Each needs its own PR so this one stays a diff a reviewer can check
  hunk by hunk.

## Scope and compatibility

- No schema change, no user-visible change, no docs change.
- Release note: none (`Release-Notes: skip`).
