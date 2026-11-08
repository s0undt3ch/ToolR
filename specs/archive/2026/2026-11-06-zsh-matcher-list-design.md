# Let the shell match completion candidates (FU-34)

Status: proposed. Audit follow-up FU-34, found in #685 round 2 and pre-existing.

Written against open PRs, not `main` alone:

- #691 (`fix/complete-double-dash`) restructures `crates/toolr-core/src/complete/engine.rs`
  (`Slot::FlagValue.attached`, `Slot::Positional.after_terminator`, `dash_ok` in `values`). Every engine
  signature below is from that branch.
- FU-33 (`fix/zsh-fish-quoted-partials`, spec only) changes how `zsh.zsh` and `fish.fish` dequote words.
  See "Compatibility with FU-33".

## Problem

`toolr __complete <cwd> <words…>` prefix-filters every candidate against the last word before printing
(`groups`, `commands`, `flags`, `values`, `group_node_flags` in `engine.rs`, plus
`merge_root_flags_at_top_level` in `crates/toolr/src/dispatch.rs`). The shell only ever sees candidates that
already start with what was typed. zsh's `matcher-list` (case-insensitive, partial-word, `r:|=*` substring)
and fish's fuzzy matcher never get to run.

## Reproduction

Binary from `main`, fixture `docs/.fixtures/sample-repo` (group `example`). zsh 5.9, fish 4.9.3.

| Typed | `toolr __complete` | Expected from the shell |
| --- | --- | --- |
| `ex` | `example` | `example` |
| `EX` | nothing | `example` under `m:{a-z}={A-Z}` (zsh), by fish's case-insensitive match |
| `xam` | nothing | `example` under `r:|=*` (zsh), by fish's substring match |

fish: `complete -C "toolr EX"` and `complete -C "toolr xam"` print nothing. As a control, fish with a static
`complete -c foo -f -a "alpha beta"` answers `AL` and `lph` with `alpha`, so the loss is the engine's
pre-filter and not fish. zsh was checked at the script level only (the `complete_shells.rs` stub driver: with
`EX` the engine returns no lines, so `compadd` is never reached, and no matcher could apply). A live `zpty`
Tab is not reliable in a non-interactive harness (FU-33 hit the same wall); the plan keeps the stub driver and
records one manual check in the PR.

bash is different: `bash.sh` builds `COMPREPLY` itself and readline does not filter it afterwards.
`completion-ignore-case` only affects readline's own listing and common-prefix step. Handing bash unfiltered
candidates would insert wrong text.

## Decision

Opt-in, shell-side matching, selected by an environment variable the script sets for the one call:
`TOOLR_COMPLETE_MATCH=shell toolr __complete …`. Without it the engine prefix-filters as today.

Applies to **zsh and fish**. **bash stays on engine prefix filtering.**

### Why this wire shape

Options considered:

1. **A flag (`__complete --unfiltered`).** Rejected. An old binary with a new script fails clap parsing on the
   unknown flag, exits 2, and the script's `*) return` turns every Tab into nothing. Also `args` is a
   trailing var-arg with `allow_hyphen_values`, so a flag must precede `cwd` and `raw_complete_tokens` would
   need to learn it.
2. **Empty prefix (script sends `""` as the last word).** Rejected. The last word also drives slot
   classification (`is_flag_prefix`, `attached_value` splitting `--name=partial`, and the `-`-leading choice
   guard in `values`). Blanking it turns `--mo<Tab>` into a command or positional slot.
3. **Engine filters for bash/fish, skips for zsh, decided by the engine.** Rejected. The engine has no
   shell identity, and adding one means a new argument anyway.
4. **Env var, read in `run_complete` (chosen).** argv and exit codes are untouched; old binaries ignore it
   (they filter, so a new script on an old binary still works, only without the matcher benefit); an old
   script never sets it (new binary filters, so nothing changes for it). There is no protocol version today
   (`COMPLETE_PATHS` 10 / `COMPLETE_DIRECTORIES` 11 are the only contract, documented in `docs/cli.md`), and
   this needs none. Unknown values of the variable mean "prefix", so a typo can't change behaviour.

### Why zsh and fish, not bash, not "all shells"

- zsh: `compadd` applies the active `matcher-list` to whatever it is given. The script already calls
  `compadd -a values` with `PREFIX` set by zsh, so no script logic changes beyond the env var.
- fish: replies are matched by fish against the whole token (the script re-adds `--opt=` for that reason).
  fish does exact, prefix, case-insensitive, substring and subsequence matching itself, and ranks. The same one-line
  change is justified and the repro above shows the same loss. Fish gets its own plan task gated on the repro
  being re-run, so a fish regression doesn't hold zsh back.
- bash: readline will not filter `COMPREPLY`. Supporting `completion-ignore-case` would mean filtering in the
  script with `compgen -W`/glob, which `bash.sh` deliberately avoids for candidate data (values execute
  `$(...)` under `compgen -W`; see the comment at `bash.sh:122`). Out of scope. Recorded as a possible
  follow-up (FU-34b), not designed here.

## Engine change

In `crates/toolr-core/src/complete/engine.rs`:

```rust
/// Who narrows the candidates to the typed word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Matching {
    /// The engine keeps only candidates starting with the typed word.
    #[default]
    Prefix,
    /// The engine returns every candidate the slot allows; the shell matches.
    Shell,
}
```

- `pub fn complete_matching(manifest, tokens, matching: Matching) -> Completion`. `complete` and
  `serve_completions` call it with `Matching::Prefix`, so every existing caller and test is unchanged.
- `candidates(manifest, slot, matching)`. Each source takes `matching` and replaces its `starts_with(prefix)`
  with `matching.keeps(candidate, prefix)`: `Prefix` is `candidate.starts_with(prefix)`, `Shell` is `true`.
  Sources: `groups`, `commands` (both the command and the child-group arms), `flags`, `group_node_flags`
  and the `--help` check in `flags`.
- **Classification never changes.** `classify`, `flag_slot`, `attached_value`, `pending_flag_value` and
  `is_flag_prefix(prefix)` still see the typed word, so a `--` word still yields flag slots and a bare word
  never yields flags. In `Shell` mode a flag slot returns every flag of the command.
- **Safety filters are not matching and stay.** `values` keeps its `dash_ok` guard (clap rejects a
  dash-leading choice outside `--name=`/after `--`; offering it would build an invalid line). The final
  `has_unsafe_name_chars` retain and sort/dedup in `candidates` stay.
- A flag-looking word is still required for flag candidates: `Slot::Flag` is reached by the same rule as
  today, so `toolr example hello <Tab>` offers flags only where it did before.

In `crates/toolr/src/dispatch.rs`:

- `fn matching_from_env() -> Matching`: `Shell` iff `TOOLR_COMPLETE_MATCH == "shell"`.
- `run_complete` calls `complete_matching(&manifest, &tokens, matching)`.
- `merge_root_flags_at_top_level(tokens, candidates, matching)` skips its `starts_with` check in `Shell`
  mode (it still only runs for a root-level flag word).

Root flags and built-in entries are merged before the engine runs, so `Shell` mode covers them with no extra
work.

## Scripts

- `zsh.zsh`: `lines=("${(@f)$(TOOLR_COMPLETE_MATCH=shell toolr __complete …)}")`. Nothing else is needed for
  correctness: the existing `compset -P 1 '*='` for `--opt=value` already moves the cursor to the value, and
  `compadd` matches it against `PREFIX`. Keep `compadd -a values` without `-Q`.
- `fish.fish`: `set -l out (env TOOLR_COMPLETE_MATCH=shell toolr __complete …)`. The `opt_prefix` re-adding stays.
  Use `env`, not `set -lx`, to keep the variable out of the user's session.
- `bash.sh`: unchanged.

Both scripts keep their paths (`rc` 10 and 11) branches: paths never come back as candidates, the shell lists
them natively (`_files`, `__fish_complete_path`). Unfiltered mode therefore **never sizes with directory
contents**.

## Performance and output size

Candidate sources are the manifest (groups, commands, flags) and `allowed_values`. All are in memory already.

- Groups, commands, flags: tens to low hundreds of lines. Negligible.
- `allowed_values` (`Literal[...]`, enum-like types): the only unbounded source. Today a narrow prefix prints
  a handful of lines; `Shell` mode prints the whole set on every Tab. A 10 000-value `Literal` is ~100 KB per
  Tab, a pipe write well under a millisecond, and the engine does no more work (it already iterates the full
  list to filter).
- zsh cost is the script's per-line loop that splits `value<TAB>description` and builds two arrays. At 10 000 lines
  that is the real cost. Mitigation, in the same change: build `descriptions` only when `${#values} <= 20`
  (the only case `compadd -d` runs), and split with `${(@)lines%%$'\t'*}` instead of the loop for the
  value array. The plan measures 10 000 values before and after and records numbers in the PR; if the plain
  loop is within ~50 ms, keep the simpler code.
- No cap. A cap would hide candidates that prefix matching currently reaches. Revisit only if the
  measurement shows a problem.
- Paths: out of the picture, see above. A path-typed argument with `allowed_values` is the only overlap, and
  the engine already prefers `allowed_values` over `_files`.

## Backward compatibility

There is no script version stamp: `freshness.rs` stamps the **manifest** (`static_hash`, `third_party_hash`,
`toolr_version`), not the installed completion script, and `install.rs` just copies the script. So the
pairing is not checked anywhere and each combination has to be safe on its own.

| Script | Binary | Result |
| --- | --- | --- |
| old | new | Variable never set, engine prefix-filters, byte-for-byte today's behaviour. |
| new | old | Variable ignored, engine prefix-filters. zsh/fish work as today, without the matcher benefit. No error. |
| new | new | Shell matching. |

Because the variable is read only in `run_complete`, nothing else (`raw_complete_tokens`, exit codes,
the 10/11 contract) moves. No `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump: that pair is the runner JSON spec,
not completion.

Installed scripts are refreshed by `toolr self completion install`; the release note says to rerun it.

## Compatibility with FU-33

FU-33 dequotes the current word and earlier words in `zsh.zsh`/`fish.fish` before they reach the engine. The
two designs touch different lines and compose:

- The earlier words must still be dequoted (committed tokens are matched exactly in `classify`). Unchanged.
- The current word still goes to the engine dequoted, because it picks the slot (`--mode=` attached,
  flag word, positional). After FU-34 it no longer picks *matches*, so an imperfectly dequoted partial can
  only change the slot, never hide a choice. That removes most of the stakes of FU-33's open questions 2 and
  the unterminated-quote handling for **zsh choice values**: `dry\ r`, `'dry r` and `"dry r` match through
  zsh's own `PREFIX` handling. FU-33 remains needed for earlier words and for `--opt=` detection.
- Both edit the line that calls `toolr __complete` in each script. Whichever lands second rebases; the
  edit here is only the `TOOLR_COMPLETE_MATCH=shell` prefix on that call.
- Tests: both add to `complete_shells.rs`; expect a textual conflict only at the end of the file.
- Not folded in: FU-33's dequoting. This spec must not be implemented as "pass an empty/raw word".

## Out of scope

- bash matching (`completion-ignore-case`, FU-34b).
- Ranking, `_approximate`/`_match` completer setup (users configure those; they now have candidates to work on).
- Changing `Candidate`, the wire format (`value<TAB>description`), or exit codes.
- #691's engine restructuring beyond adding the `matching` parameter.

## Open questions

1. Name: `TOOLR_COMPLETE_MATCH=shell` vs `TOOLR_COMPLETE_FILTER=none`. Recommendation: `MATCH=shell` (positive,
   extensible to other modes). Needs a decision.
2. Should fish ship in the same PR? Recommendation yes (one-line, same repro), separate commit. Drop it if its
   repro regresses under the fish 3.x versions CI has.
3. Does `compadd -d descriptions` stay correct when `values` is now larger than before for the same typed
   word and crosses the 20 threshold? It chooses the compact grid, as for large sets today. Accepted, but it
   means `AL<Tab>` can show descriptions less often than `al<Tab>`.
4. A live zsh `zpty` check is not automatable here; accept the stub driver in CI plus a manual check
   recorded in the PR (same call as FU-33's open question 1).

## Implementation notes

- #691 and FU-33 had both merged by implementation time, so this was built on `main`.
- A bare `compadd` does not apply `matcher-list`: the style reaches `compadd` only as the `-M` that
  `_description` puts into `expl`. The zsh script calls `_description values expl value` and passes
  `"${expl[@]}"` to `compadd`. A live pty test (`zsh_tab_applies_the_matcher_list`) covers it, so the manual
  check in open question 4 is not needed.
- fish sets the variable with `set -lx` inside `__toolr_complete`, not `env`: `env` runs the binary directly
  and would skip a `toolr` function, and the local dies with the function.
- "A bare word never yields flags" is not quite right: a command with no positional left falls back to its flag
  names for any word, as before. In `Shell` mode that fallback returns every flag.
- zsh at 10 000 values: the split loop took about 96 ms and the `${(@)lines%%$'\t'*}` expansion about 3 ms,
  so the script uses the expansion.
