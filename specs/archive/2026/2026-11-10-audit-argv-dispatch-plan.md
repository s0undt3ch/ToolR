# Argv and dispatch consolidation: plan

1. `toolr-core`: `Manifest::empty()` + a test; use it in every helper and test literal that only fills
   defaults (`toolr-core` and `toolr`).
2. Regression tests first (failing on main): a user command with its own `--version` option on a fresh
   clone; `-q self --help` and `--timeout-secs 5 project venv --help` print the subcommand's help.
3. `invocation.rs`: `RootArgv` + `Invocation` with unit tests; `cli::builtin_subcommands()`. `main.rs`,
   `bootstrap.rs` and `dispatch.rs` switch to the accessors; delete `first_positional`,
   `ROOT_VALUE_OPTIONS`, `BUILTINS`, `help_mode_from_argv`, `dispatch_help_from_argv`,
   `raw_complete_tokens`, `cli::command_path` and their mirror tests. `scan_root_flags` reads
   `cli::root_command()`. Release note for the two fixes.
4. `Verbosity` enum in `execute_build.rs`.
5. Pure move: `dispatch.rs` into `dispatch/{mod,run,self_cmd,complete}.rs`; `output_options_from_matches`
   into `execute_build.rs`. Coverage run right after, to see moved lines that count as changed.
6. `prek run --all-files`, `cargo test --workspace --exclude toolr-py`, `uv run pytest`, patch coverage
   against `main`, archive the specs.
