# Root `--quiet` in the other built-ins

Audit follow-up FU-41. Sibling of the `self completion install` fix (FU-29, #693).

## Problem

The root `toolr --quiet` / `-q` is documented as "Suppress non-error output". Several built-ins ignore it and
print their success lines anyway. Reproduced with the dev binary in temp dirs, all with `-q` before the
subcommand:

- `self build-manifest --source-dir <pkg> --package <name>`: `wrote N group(s) / M command(s) to <path>` on stderr.
- `self trusted --prune`: `pruned <path>` on stderr.
- `project manifest rebuild` and the hidden `__build-static-manifest`: `wrote N groups / M commands to <path>` on stdout.
- `project init`: `scaffolded tools/ …`, one `wrote <file>` line per file, the `--no-sync` notes, and the
  `synced venv …` plus next-steps block, all on stdout.
- `project venv sync|lock|add|remove`: the `synced venv`, `refreshed`, `added` and `removed` lines.

`project init` and the `venv` subcommands read only their own `--quiet`. The root flag reaches them as
`root_quiet` and only feeds uv-install consent (`never_prompt`). `project trust` already combines both
(`matches.get_flag("quiet") || root_quiet`).

One more gap in the same family: `project init` never forwards `--quiet` to the uv sync it runs, so uv's own
progress output shows even under the subcommand's `--quiet`. `venv sync|add|remove` forward only the
subcommand flag as well.

## Design

Rule: `--quiet` suppresses success lines and advice, never errors, refusals or warnings. A command's primary
output, the thing a user pipes, is not "non-error output".

Reuse the existing plumbing, no new type:

- `run_self` already receives the root `quiet` bool. Pass it into `run_self_build_manifest` and
  `run_self_trusted`, and into `run_build_static_manifest` from `dispatch`.
- `dispatch_project` already receives `root_quiet`. Pass it to `manifest_rebuild`. In `project_init` and
  `venv_sync|lock|add|remove` compute `let quiet = matches.get_flag("quiet") || root_quiet;` (the
  `project_trust` pattern) and use it for the success prints and for the uv `--quiet` forwarding.

### Per-command decisions

| Command | Under root `-q` | Why |
| --- | --- | --- |
| `self build-manifest` | `wrote …` line dropped | Success line. Warnings and the `--check` out-of-date diff stay (warning, error). |
| `self trusted` | Listing stays | It is the command's result, the thing piped to other tools. |
| `self trusted --prune` | `pruned <path>` dropped | Success line about a side effect. The listing of what remains stays. |
| `project manifest rebuild` | `wrote …` dropped | Success line. Per-manifest warnings stay. |
| `__build-static-manifest` (hidden) | `wrote …` dropped | Same line as `manifest rebuild`; consistent. Warnings stay. |
| `project init` | All `toolr:` progress, `--no-sync` notes and next steps dropped | Success lines and advice. Conflict error (exit 2), trust-store warnings and sync errors stay. |
| `project init`, uv sync | uv gets `--quiet` | uv's progress is non-error output; uv still prints its errors. |
| `project venv sync` | `synced venv …` and the run-a-command hint dropped; uv gets `--quiet` | Success line and advice. |
| `project venv lock` | `refreshed …` dropped; uv gets `--quiet` | Success line. |
| `project venv add` / `remove` | `added` / `removed` dropped; uv gets `--quiet` | Success line. |
| `project trust` | Unchanged | Already honours both flags. Stdout root is its result. |
| `project untrust` | Unchanged | Stdout root is its result. `<path> was not trusted` is a notice that nothing happened, not success chatter. |
| `project venv path` | Unchanged | Its stdout is the result. |
| `project venv shell` / `run` | Unchanged | The child owns stdout and stderr. Root `-q` already stops uv prompts. |
| `self completion print` | Unchanged | Its stdout is the script. |
| `self cache list` | Unchanged | The table is the result. |

### Deliberately not changed

`venv_sync_unattended_quiet_exit` turns "not a toolr repo", "no tools/" and "uv declined" errors into a silent
exit 0 under the subcommand's `--quiet`. That is the mise enter-hook contract. It stays keyed on the
subcommand's own flag: the root `-q` must not convert failures into success, so under root `-q` those errors
still print and exit non-zero.

## Other built-ins audited, left for follow-ups

- `self cache prune`: its summary lines (`freed …`, `nothing to prune`, `aborted, nothing removed`, lock sweep
  counts) are success chatter, but the dry-run lines are the command's result and the writer is threaded through
  several helpers with their own unit tests. Splitting the two is a separate change.
- `__install-uv-now` (hidden): prints `uv X.Y.Z ready at …` as its only result. Its caller is a user explicitly
  asking for the install, so the line is the answer; left alone.

No genuine product call came up: every case follows the rule above.

## Tests

Integration tests in `crates/toolr/tests/` assert empty stdout and stderr under root `-q` and `--quiet` for each
success path, and that errors still print. uv-backed cases use the `VenvFixture` stub uv and assert it
received `--quiet`.
