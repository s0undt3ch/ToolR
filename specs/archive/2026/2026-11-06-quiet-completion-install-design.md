# `--quiet` for `toolr self completion install`

Audit follow-up FU-29.

## Problem

`toolr --quiet self completion install <shell>` still prints the `wrote …`, `rewrote …`, `replaced …` and
`already up to date` lines, plus the zsh `fpath` reminder. The root flag is documented as "Suppress non-error
output". `run_completion_install` already receives the root `quiet` value, but only `print_bash_source_hint`
reads it (#684).

Repro, in a temp `HOME` with `XDG_*` unset:

```sh
toolr --quiet self completion install zsh   # prints "wrote zsh completion script …" and the fpath reminder
toolr --quiet self completion install zsh   # prints "zsh completion already up to date …"
toolr --quiet self completion install bash  # prints "wrote bash completion script …"
```

## Design

Reuse the `quiet: bool` that `dispatch` already takes from `matches.get_flag("quiet")` and passes through
`run_self`. No new verbosity type. Gate each success `println!` in `run_completion_install` on `!quiet`:

- `wrote` / `rewrote` / `replaced` line.
- zsh `fpath` / `compinit` reminder.
- `already up to date` line.

Unchanged under `--quiet`:

- The `refusing to overwrite … (use --force)` message on stderr and its exit code 1 (an error).
- Errors from `install_script`, `$HOME` resolution and shell parsing.
- `self completion print`, whose stdout is the script itself and is not "output" in the quiet sense.

### Hint decision

The bash source hint and the zsh reminder are both advice, not warnings, so `--quiet` suppresses them. #684
already settled this for bash (`quiet_suppresses_the_hint`), and a user who passes `--quiet` is scripting the
install (mise hook, dotfiles bootstrap) and accepts owning the follow-up. A failure to install stays loud.

## Other built-ins that ignore the root `--quiet`

Out of scope here; follow-ups.

- `self build-manifest`: `wrote N group(s) / M command(s)` on stderr.
- `self trusted --prune`: `pruned <path>` on stderr, and the listing on stdout (the listing is the command's
  result, so it should stay).
- `project init`, `project venv sync|lock|add|remove`: their success lines honour only the subcommand's own
  `--quiet`. The root flag only stops uv prompts (`root_quiet` in `project.rs`).
- `project manifest rebuild`: `wrote N groups / M commands` prints unconditionally.
- `project trust`: honours both flags (already correct).
- `self cache *`: not audited for success chatter beyond the cache hint, which `cache_hint_quiet.rs` covers.

## Tests

Integration tests in `crates/toolr/tests/completion_install_hint.rs` using its `Host` helper, asserting empty
stdout and stderr under `--quiet`, for each output kind.
