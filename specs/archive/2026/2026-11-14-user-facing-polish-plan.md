# User-facing polish: plan

One commit per item, each with a release note.

1. `fix(cli): one no-project message for every built-in` (M-02)
    - `NothingToSync::NoProject(String)`; `ensure_venv_ready` maps discovery errors through it.
    - `Invocation::project_root()`; thread `&Invocation` through `run_self`, `dispatch_project`,
      `resolve_source_and_package`; `ProjectCtx` borrows it; `manifest_rebuild`, `venv_path`, `root_from_path_arg`,
      `project_init` use it.
    - Tests as in the design.
2. `fix(manifest): show the parse error in the stale-manifest warning` (U-20)
    - `parse_python_file` message with line/column; `warn_and_keep_cache` prints the chain.
3. `fix(runner): start a command's traceback at the user's code` (U-18)
    - `_print_exception` helper in `_runner.py`, used by the generic `except Exception` and `ImportError` branches.
4. `docs(specs): archive the user-facing polish design and plan`.

Verify: targeted `cargo test -p toolr` / `-p toolr-core`, pytest on 3.11 and newest, `mise run test`, patch coverage
against `refactor/python-hygiene`.
