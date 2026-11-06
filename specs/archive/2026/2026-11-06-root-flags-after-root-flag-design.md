# Root flags before the first group: design

Audit follow-up FU-39 (from #691 round 2).

Written against `main` at b4085bee, which includes #691 (`fix/complete-double-dash`), #693 and #695. #691 keeps a
leading `--` as a committed token in `raw_complete_tokens` and changes the engine's `classify`; none of the three touch
the root-flag merge.

## Problem

`toolr --quiet --<TAB>` offers nothing, although clap accepts further root flags. The question that came with it,
"are groups still offered after a committed root flag?", has a worse answer: **no**.

- `toolr --quiet <TAB>` offers nothing (no groups).
- `toolr --quiet ci <TAB>` and `toolr --quiet ci he<TAB>` offer nothing (no commands).

Two causes, both in the same place: the root flags exist only in `cli::build_command`, and the engine is
manifest-driven and has never heard of them.

1. `merge_root_flags_at_top_level` (`crates/toolr/src/dispatch.rs`) returns early unless no token is committed, so it
   never offers a root flag after another one.
2. The engine's `classify` (`crates/toolr-core/src/complete/engine.rs`) walks the committed tokens looking for a group.
   A leading `--quiet` matches none, so `group_path` is empty and the slot is `Slot::None`.

## Root flags (from `cli::build_command`, verified against clap)

| Flag            | Short | Aliases (hidden) | Value         | Conflicts with    |
| --------------- | ----- | ---------------- | ------------- | ----------------- |
| `--debug`       | `-d`  |                  | no            | `--quiet`         |
| `--quiet`       | `-q`  |                  | no            | `--debug`         |
| `--timestamps`  |       | `--ts`           | no            | `--no-timestamps` |
| `--no-timestamps` |     | `--nts`          | no            | `--timestamps`    |
| `--timeout-secs` |      | `--timeout`      | f64 `SECONDS` | none              |
| `--no-output-timeout-secs` | | `--nots`    | f64 `SECONDS` | none              |
| `--help`        | `-h`  |                  | no            | none (global)     |

`conflicts_with` is declared one way on `--quiet` and `--timestamps`; clap enforces it both ways
(`--debug --quiet` and `--quiet --debug` both fail). `Command::get_arg_conflicts_with` is not symmetric on an unbuilt
command, so the specs mirror each declared conflict to the other side (guarded by a unit test).

## What clap accepts (the oracle)

Checked with `TOOLR_TRUST_ALL=1 toolr <flags> self completion print bash`; `ACCEPT` means no `error:` line.

| Line                                  | Result                                                    |
| ------------------------------------- | --------------------------------------------------------- |
| `--quiet --timestamps`                | ACCEPT                                                    |
| `--quiet --timeout-secs 5`            | ACCEPT                                                    |
| `--timeout-secs=5 --quiet`            | ACCEPT                                                    |
| `--timeout 5 --nots 6 --ts`           | ACCEPT (aliases work)                                     |
| `--ts --timeout-secs 5 --quiet`       | ACCEPT                                                    |
| `--help --quiet`                      | ACCEPT                                                    |
| `--quiet --debug`, `--debug --quiet`  | rejected: cannot be used with                             |
| `-q -d`, `-qd`, `-dq`                 | rejected: cannot be used with (short clusters are parsed) |
| `--timestamps --no-timestamps`, `--ts --nts` | rejected: cannot be used with                      |
| `--quiet --quiet`, `--debug --debug`  | rejected: cannot be used multiple times                   |
| `--timestamps --timestamps`           | rejected: cannot be used multiple times                   |
| `--timeout-secs 5 --timeout-secs 6`   | rejected: cannot be used multiple times                   |
| `--help --help`, `-h --help`          | rejected: cannot be used multiple times                   |
| `--timeout-secs --quiet`              | rejected: a value is required                             |
| `--timeout-secs=-1 --quiet`, `--nots=-1` | ACCEPT (attached, so no flag ambiguity)                |
| `--version`, `-V`, `-qV`, `--quiet --version`, `--version --version`, `-V ci` | prints the version and exits, whatever follows |
| `--timeout-secs=`, `--timeout-secs=abc`, `--timeout-secs abc`, `--timeout-secs -5` | rejected: invalid value / unexpected `-5` |
| `--quiet -- ci`, `-- ci`              | rejected: unexpected argument `ci`                        |
| `ci --quiet hello`, `--quiet ci hello --quiet` | rejected: root flags are not global            |

Consequence: a flag already given is never offered again, a conflicting flag is never offered, and a line clap rejects
completes to nothing (the rule #691 set).

## Reproduction (binary built from `main`, scratch project with one `ci hello` command)

`toolr __complete "$PWD" <tokens>`; "want" is derived from the oracle table above.

| Tokens                         | Today                         | Want                                                          |
| ------------------------------ | ----------------------------- | ------------------------------------------------------------- |
| `--`                           | all 6 root flags + `--help`   | unchanged                                                     |
| `--quiet --`                   | nothing                       | `--help --no-output-timeout-secs --no-timestamps --timeout-secs --timestamps` |
| `--quiet --debug --`           | nothing                       | nothing (clap rejects the line; `--debug` after `--quiet` is the conflict) |
| `--debug --quiet --`           | nothing                       | nothing (same)                                                |
| `--quiet --quiet --`           | nothing                       | nothing (duplicate)                                           |
| `--quiet --timestamps --`      | nothing                       | `--help --no-output-timeout-secs --timeout-secs`              |
| `--timestamps --no`            | nothing                       | `--no-output-timeout-secs` (not `--no-timestamps`)            |
| `--ts --no`                    | nothing                       | `--no-output-timeout-secs` (not `--no-timestamps`)            |
| `--nts --t`                    | nothing                       | `--timeout-secs` (not `--timestamps`)                         |
| `-q --`, `-d --`               | nothing                       | as `--quiet --` / `--debug --` minus the pair                 |
| `-q --d`                       | nothing                       | nothing (`--debug` conflicts with `-q`)                       |
| `--timeout-secs 5 --`          | nothing                       | all but `--timeout-secs`                                      |
| `--timeout 5 --t`              | nothing                       | `--timestamps` (alias counts as given)                        |
| `--timeout-secs=5 --`          | nothing                       | all but `--timeout-secs`                                      |
| `--timeout-secs --`            | nothing                       | nothing (value owed; clap rejects)                            |
| `--timeout-secs <TAB>`         | nothing                       | nothing (the in-progress word is the value; no candidates)    |
| `--timeout-secs 5 <TAB>`       | nothing                       | groups (`ci`, `project`, `self`)                              |
| `--timeout-secs abc <TAB>`     | nothing                       | nothing (clap rejects `abc`)                                  |
| `--quiet <TAB>`                | nothing                       | groups                                                        |
| `--quiet c`                    | nothing                       | `ci`                                                          |
| `--quiet ci <TAB>`             | nothing                       | `hello`                                                       |
| `--quiet ci --`                | nothing                       | `--help` (engine, group node); no root flags (not global)     |
| `--help --`                    | nothing                       | root flags minus `--help`                                     |
| `-- --`                        | nothing (#691 keeps the terminator committed) | nothing (clap rejects `toolr -- ci`) |
| `--quiet -- --`                | nothing                       | nothing                                                       |

(`--quiet --debug --` and the other conflicting pairs already print nothing today, but for the wrong reason; the new
code must keep them empty on purpose, not by accident.)

## Design

### Where the fix belongs

In the dispatcher, before the engine runs. The engine stays manifest-only (`toolr-core` has no clap and must keep
building without it); the binary already owns the root flags (`builtin_completions::root_long_flags`,
`merge_root_flags_at_top_level`). Teaching the engine about root flags would duplicate `cli::build_command`.

`run_complete` does three things instead of one:

1. **Scan** the committed tokens (`tokens[..len-1]`) for a leading run of root flags.
2. **Strip** that run and hand the engine the remainder: `toolr --quiet ci he<TAB>` reaches the engine as
   `["ci", "he"]`, exactly what it gets without the flag. This fixes groups and commands after a root flag with no
   engine change.
3. **Merge** root-flag candidates after the engine runs, filtered by what the scan saw.

### The scan

New code in `crates/toolr/src/builtin_completions.rs` (already holds `root_long_flags` and derives from the clap
`Command`):

```rust
pub struct RootScan {
    /// Tokens left for the engine: the committed tokens after the leading root flags, plus the in-progress word.
    pub rest: Vec<String>,
    /// Ids of root flags given (aliases and shorts resolved to the id).
    pub seen: Vec<String>,
    /// clap rejects the line (duplicate, conflict, bad or missing value): nothing completes.
    pub dead_end: bool,
}

pub fn scan_root_flags(tokens: &[String]) -> RootScan
```

Specs are derived once per call from `cli::build_command(&empty_manifest())` (as `root_long_flags` does): per arg its
id, long, short, aliases, whether it takes a value, and `get_arg_conflicts_with` ids. Walk the committed tokens from
the start:

- `--` : stop (terminator; it and everything after stay in `rest`).
- `--name` or an alias: resolve to a spec. Already in `seen` or conflicting with one in `seen` is a dead end. A
  value-taking flag consumes the next committed token as its value; no next committed token means the in-progress word is
  the value, which is a dead end for flag and group candidates alike. The value must `parse::<f64>()` (the same
  `FromStr` clap's `value_parser!(f64)` uses) and, when detached, may not start with `-` (it reads as a flag);
  otherwise dead end. An attached value (`--timeout-secs=-1`) is unambiguous, so only the parse applies.
  The type is read from each flag's clap value parser (`ValueKind`); a type the scan does not know never passes,
  so the line dead-ends instead of being checked as a float.
- `--name=value`: same, value in the token. An empty or non-float value is a dead end.
- `--version` / `-V`: clap prints and exits, so the line is a dead end wherever it appears in the leading run. It is
  not offered as a completion.
- `-xyz` cluster: every char must be a known short bool flag (`d`, `q`, `h`); each goes through the duplicate and
  conflict checks. An unknown char stops the scan at that token (it stays in `rest`; the engine finds no group and
  prints nothing).
- anything else (a word, a bare `-`, an unknown `--flag`): stop.

Only a *leading* run is stripped: root flags are not global, so `--quiet ci --debug` keeps `--debug` in `rest` for the
engine's normal leaf handling.

### The merge

`merge_root_flags_at_top_level(tokens, candidates)` becomes `merge_root_flags(rest, scan, candidates)`:

- Only when `rest.len() == 1` (nothing committed after the flags) and the word is `--…` or `-`: unchanged condition.
- Offer each `root_long_flags()` entry that starts with the prefix, is not in `scan.seen`, and does not conflict with
  anything in `scan.seen`.
- Drop the engine's `--help` candidate when `help` is in `scan.seen`.
- `rest` holding a committed `--` (after #691) means `rest.len() > 1`, so nothing is merged, which is the `-- --` row.

When `scan.dead_end`, `run_complete` prints nothing and returns `ExitCode::SUCCESS` before calling the engine.

### Interaction with the open PRs

- #691 (merged) keeps a leading `--` as a committed token. The scan treats that `--` as a stop, so `-- --` and
  `--quiet -- --` yield nothing.
- #691's engine change is orthogonal: the engine receives the stripped tokens and applies its `--` rules to them.
- #693 and #695 (merged) change `completion install` and the built-ins' `--quiet`, not completion.

### Not doing

- Offering short flags (`-q`, `-d`): the engine renders long flags only everywhere; unchanged.
- Offering aliases (`--ts`, `--timeout`): hidden today, stay hidden. Typed ones still count as given.
- Completing the value of `--timeout-secs`: a free-form float has no candidates.
- Validating a value beyond "parses as f64": range is the runner's concern.

## Resolved questions

1. `toolr --timeout-secs <TAB>` prints nothing: a line clap rejects completes to nothing (the rule #691 set), and a
   number is never a path.
2. A malformed value (`--timeout-secs abc`) is a dead end that prints nothing, for consistency with #691.
3. The branch is on `main`: #691 has merged.

## Review changes

- An attached negative value (`--timeout-secs=-1`) is accepted; only the detached form rejects a leading `-`.
- `--version` / `-V` are recognised (the scan builds a clone of the clap command, since the arg is added at build
  time) and end the line.
- Value validation goes through `ValueKind`, read from each flag's parser; unknown types dead-end.
- The specs are derived once per completion and carried on `RootScan`, which also yields the offerable long flags.
  `get_arg_conflicts_with` stays one-sided even after `build()`, so the conflicts are still mirrored.
