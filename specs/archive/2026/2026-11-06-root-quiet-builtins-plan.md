# Plan: root `--quiet` in the other built-ins

Design: [`2026-11-06-root-quiet-builtins-design.md`](2026-11-06-root-quiet-builtins-design.md).

1. Tests first, in `crates/toolr/tests/root_quiet_builtins.rs`. Each runs with the root `-q` or `--quiet` and
   asserts success with empty stdout and stderr:
    - `self build-manifest`, and a case where a warning still prints;
    - `self trusted --prune` (listing of kept roots stays on stdout, `pruned` line gone);
    - `project manifest rebuild` and `__build-static-manifest`;
    - `project init` with and without `--no-sync`, and that uv received `--quiet`;
    - `project venv sync`, `lock`, `add`, `remove`, each with uv receiving `--quiet`;
    - `project venv shell` and `run` (added in review): their auto-sync uv receives `--quiet`.
2. Also assert that errors survive: `project init` conflict (exit 2), `venv remove` of an undeclared package,
   `venv sync` outside a toolr repo under root `-q` (still exit non-zero with the message).
3. Watch them fail.
4. Thread `quiet` through `run_self`, `dispatch_project`, `manifest_rebuild`, `project_init` and the four
   `venv_*` handlers as in the design, plus `venv_shell` and `venv_run`. Forward it to uv in `run_project_init`.
5. Queue a release note with `toolr release-notes add`.
6. Run the verification gate from `CLAUDE.md`, then archive both specs as the last commit.
