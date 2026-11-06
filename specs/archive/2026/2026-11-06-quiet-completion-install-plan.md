# Plan: `--quiet` for `toolr self completion install`

Design: [`2026-11-06-quiet-completion-install-design.md`](2026-11-06-quiet-completion-install-design.md).

1. Tests first, in `crates/toolr/tests/completion_install_hint.rs`. Each runs
   `toolr --quiet self completion install <shell>` and asserts the command succeeds with empty stdout and stderr:
    - first install for bash, zsh and fish (the `wrote` line, the zsh reminder);
    - a repeat install (`already up to date`);
    - `--force` over identical content (`rewrote`);
    - `--force` over differing content (`replaced`).
2. Also assert that `--quiet` leaves the refusal on stderr and exit code 1.
3. Watch them fail.
4. Gate the success prints in `run_completion_install` on `!quiet`.
5. Queue a release note with `toolr release-notes add`.
6. Run the verification gate from `CLAUDE.md`, then archive both specs as the last commit.
