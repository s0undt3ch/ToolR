# Global flags and `--quiet`

## Why

`--quiet` had three contracts and the root flags had the wrong heading and were easy to misplace.

- **U-01** (`--quiet` has three contracts): root `-q`, `venv sync -q` (which also turned three errors into
  exit 0, found by matching error text), `trust -q`, and `venv run --quiet` (no short form) each meant
  something different. `main.rs` also scanned argv by hand for `-q` before clap ran.
- **U-02** (root flags are root-only and wrongly headed): two of the "Output Options" were `ctx.run` timeouts,
  and a root flag after a command failed with a bare clap error.
- **R-09** (substring-matched errors): `venv_sync_unattended_quiet_exit` matched three substrings of the
  error chain.
- **R-03** (repeated prologue): nine handlers repeated the venv-ready prologue.
- **R-08** (hand-rolled prompts): four `[y/N]` prompts with three acceptance rules.

## Design

toolr's flags belong to toolr: they are accepted **only at the root, before the first subcommand**, with
no copy on any command (built-ins included). A copy on a command, or `global(true)`, was tried and dropped:
clap propagates values between same-id args across levels (a Python parameter named `quiet` panicked or
flipped toolr's flag), and a command's own `-q` / `--timeout-secs` collides with the copy.

- **Root flags (U-01, U-02).** `-d`, `-q`, `--timestamps`, `--no-timestamps`, `--timeout-secs`,
  `--no-output-timeout-secs` stay root args under a "Global Options" heading; root `--help` ends with
  "toolr's global options go before the command: toolr -d <command> ...". The per-command `quiet` args
  on `init`, `venv run/sync/lock/add/remove` and `trust` go. Breaking.
- **Misplaced-flag hint.** When clap reports `UnknownArgument` and the argument is one of those flags
  (any spelling or alias, `--timeout-secs=5`, a `-qd` cluster), `main.rs` prints clap's error and then
  ``toolr: `-d` is a toolr global option; put it before the command: `toolr -d ci hello ...` ``.
  `cli::misplaced_flag_error` builds it from the command names clap matched (`command_path`), the flag
  (its value only when plain: `--x=v` or the next word, else the placeholder) and `...`, never from the
  rest of the user's argv. If root flags were already typed before the command, a literal `…` stands for
  them (`toolr … -q ci hello ...`). This covers the old hook spelling `toolr project venv sync -q
  --if-project`. A command that spells the flag itself never errors, so no hint. `venv run` takes its
  command with hyphen values allowed, so there the first command token is checked with the same function
  and fails with the same message instead of being run (`venv run -- python -q` is untouched).
- **`venv sync --if-project` (U-01).** The benign exits (no project, no readable venv location, uv absent
  and not installable unattended) move from `--quiet` to `--if-project`; `-q` is output control only.
  Breaking for the mise hook, now `toolr -q project venv sync --if-project`. A nothing-to-sync failure
  without the flag ends with `hint: pass --if-project to exit 0 when there is nothing to sync`.
- **Typed error (R-09).** `NothingToSync` is the context of the two discovery steps in
  `ensure_venv_ready`; `is_nothing_to_sync` also accepts `UvError::UserRefusedInstall`. Messages are
  unchanged; the substring table and the trust-refusal special case go.
- **Argv scan deleted (U-01).** The passive cache hint runs after clap parses and reads `quiet`, `help` and
  the `self cache` subcommand from `matches`. `argv_requests*` go.
- **`ProjectCtx` + `ready_venv` (R-03).** One helper builds the consent, `EnsureOpts` and trust mode from
  the root `quiet`; the project handlers and `dispatch::resync_if_needed` use it.
- **`yes_no` (R-08).** `toolr_core::prompt::yes_no(input, out, question, default)`: `y`/`yes` accept,
  `n`/`no` decline, Enter takes the default, anything else or EOF declines (EOF ends the line). Used by the
  trust, sync, overwrite and cache-wipe prompts. The uv `[I/M]` choice is a different question and stays.

## Non-goals

- A single `RootArgv` and the remaining argv scans (`first_positional`, `help_mode_from_argv`,
  `is_tab_completion`): WP8.
- Dropping `--no-timestamps` or showing the aliases: it conflicts with `--timestamps`, so passing both is
  already an error; the flag stays as the documented default.
- A full error-type pass (R-09 beyond the one substring match).
