# Argv and dispatch consolidation

## Why

The binary reads its own argv in several hand-written places before and after clap, each with its own idea
of "root flag", "value option" and "first command", and it looks the project root up more than once per
run.

- **R-05 / C-04 / U-01 (scan part)** (argv pre-parsers mirror `cli.rs`): `bootstrap::first_positional`
  with its `ROOT_VALUE_OPTIONS` and `BUILTINS` lists (kept honest by two mirror tests),
  `main.rs`'s `want_help` / `want_short`, `dispatch::help_mode_from_argv`, the argv walk in
  `dispatch::dispatch_help_from_argv`, `dispatch::raw_complete_tokens` (which calls
  `std::env::args()` again) and `cli::command_path` (added by WP6) each re-derive the same thing.
- **X-12** (root resolved repeatedly): `ensure_manifest`, `load_or_empty`, `no_project_hint` and
  `dispatch` each call `discover_project_root`.
- **R-06** (`Manifest` literals): 35 full `Manifest { … }` literals and six private `empty_manifest` /
  `manifest_with` helpers; no `Manifest::empty()`.
- **R-02** (`dispatch.rs` god file): 1,205 lines holding help interception, the user-command run, every
  `self` built-in, the `__complete` glue and `OutputOptions` extraction.
- **R-11** (`verbosity` string): `OutputOptions.verbosity` is a `String` compared to literals.

## Already gone on main

WP6 (#731) and WP10 (#728) removed part of the audit's list; this PR leaves those alone.

- `argv_requests_quiet`: the cache hint reads `quiet` / `help` from `matches`.
- `dispatch_help_from_argv`'s unused `_manifest` parameter.
- `"init"` in `BUILTINS` (R-01), pinned by `fires_for_a_user_group_named_init`.
- `persist_manifest` and the two bootstrap steps: one `manifest_build::refresh` call, so
  `should_skip_auto_rebuild` runs three times per run, not four, and the root is looked up two to four
  times, not four to seven.

## Bugs the scanners cause (fixed here)

Both reproduced with the dev binary on main.

- **`--version` anywhere skips the manifest build.** `should_skip_auto_rebuild` looks for `--version` /
  `-V` in all of argv, but the root flag is not global. A user command with its own `--version` option
  (`toolr rel bump --version 1.2`) skips the build: on a fresh clone it fails with
  `unrecognized subcommand 'rel'`, and on a stale manifest it runs with the old command set. Only the
  leading run of root flags counts now.
- **Help after a root flag shows root help.** `toolr -q self --help` and
  `toolr --timeout-secs 5 project venv --help` print the root help: the fallback walk stops at the first
  `-` word. It now skips the leading root flags as clap does.

One harmless difference: `__install-uv-now` was missing from `BUILTINS`, so it built the manifest and
counted as a user command. With the built-in set taken from the clap tree it skips the build, as the
other built-ins do.

## Design

- **`RootArgv` (`invocation.rs`).** Built once in `main::run` from `std::env::args()`; the same `Vec`
  goes to `try_get_matches_from` (the `args_os()` call goes; `args()` already panics on non-UTF-8, so
  nothing changes). It finds the first word after the leading run of root flags, skipping a value-taking
  root option's detached value. The value options come from `cli::root_command()`, and the built-in
  names from the new `cli::builtin_subcommands()`, which `build_command` also uses. Accessors replace the
  scanners: `skips_manifest_build`, `names_user_command`, `is_tab_completion`, `help_mode` (`--help`
  over `-h`, before any `--`), `complete_tokens` (the words after `__complete <cwd>`) and `command_path`
  (the subcommands clap matches from the first word, with the matched `clap::Command`). The
  `ROOT_VALUE_OPTIONS` / `BUILTINS` lists and their mirror tests go.
- **`scan_root_flags` stays.** It reads the user's half-typed line at Tab, with dead ends clap cannot
  express (C-04), so it is a different job. It now reads `cli::root_command()` instead of building the
  whole tree on each Tab.
- **Root resolved once (X-12).** `Invocation { argv, cwd, root }` holds the lookup in a `OnceCell`, done
  on first use, so Tab and `--version` stay walk-free. `root()` lends the `Result` (`load_or_empty`
  needs `MissingPyproject`, `no_project_hint` needs `NotFound`); `into_root()` hands the owned error to
  the user-command path, so its `?` message and source chain are unchanged. Out of scope: `project`
  handlers (they take `--path`, and core's `ensure_venv_ready` starts from the cwd), and the
  canonicalisation inside core trust / venv-key code.
- **`Manifest::empty()` (R-06).** In `toolr-core`'s model, with `schema_version: SCHEMA_VERSION`. Not
  `Default`, which would make the version 0. Helpers and test literals become
  `Manifest { groups, ..Manifest::empty() }`; a literal that pins an old `schema_version` on purpose
  keeps it.
- **`Verbosity` (R-11).** `enum Verbosity { Quiet, Normal, Verbose }` in `OutputOptions`, with `as_str()`
  where the spec is built. `ContextSpec.verbosity` stays a string; the wire format is unchanged, so no
  schema bump.
- **Split `dispatch.rs` (R-02).** A pure move into `dispatch/mod.rs` (the router and help interception),
  `dispatch/run.rs` (the user command), `dispatch/self_cmd.rs` (`self …` and `__install-uv-now`) and
  `dispatch/complete.rs` (`__complete`). `output_options_from_matches` moves next to `OutputOptions`.

## Non-goals

- C-04's larger idea (classifying completion lines with clap's own errors).
- Threading the root into `project` handlers or core.
- Any change to help text or the runner spec.
