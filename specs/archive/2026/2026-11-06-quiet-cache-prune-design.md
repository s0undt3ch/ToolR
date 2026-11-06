# Root `--quiet` in `self cache prune`, and the warning rule

Audit follow-up FU-43, left out of #695. #695 has merged, so this builds on `main`: on #695's rule and on
`dispatch_project`'s doc comment. (It was first written against #695's branch, `fix/root-quiet-builtins`.)

## Problem

### Part 1: `self cache prune` ignores the root `-q`

The root `--quiet` is documented as "Suppress non-error output and never prompt to install uv." #695 made the other
built-ins honour it. `self cache prune` still prints everything. Repro with the dev binary, a temp `HOME` and
`XDG_CACHE_HOME` holding two orphan cache entries (`TOOLR_NO_CACHE_HINT=1`):

```text
$ toolr -q self cache prune --dry-run
DRY-RUN ORPHAN  …/toolr/aaa (174 B)
DRY-RUN ORPHAN  …/toolr/bbb (174 B)
toolr: would free 348 B across 2 entries

$ toolr -q self cache prune
ORPHAN  removing …/toolr/aaa (174 B)
ORPHAN  removing …/toolr/bbb (174 B)
toolr: freed 348 B across 2 entries

$ toolr -q self cache prune          # nothing left
toolr: nothing to prune

$ toolr -q self cache prune --all --yes   # empty cache
toolr: no cache entries to remove under …/toolr
```

All of it goes to stdout. The `aborted, nothing removed` line (declined `--all` prompt) and the lock-sweep counts
(`removed N rebuild lock(s)`) are the same family. `prune` has no `--quiet` of its own, so only the root flag
applies, as in #695.

The code: `self_cache_prune::run` locks stdout once and threads `out: &mut dyn Write` through `prune_all`,
`prune_classified`, `sweep_locks` and `remove_entry`. Each helper has unit tests that pass a `Vec<u8>`.
`dispatch::run_self` receives the root `quiet` bool but does not hand it to `self_cache::dispatch`.

### Part 2: `-q` and warnings are inconsistent

- Normal dispatch: `-q` drops the "skipping plugin …" and "hiding the one from …" warnings. `main.rs` skips the whole
  warning loop when `argv_requests_quiet(&argv)`; `plugin_warnings::quiet_suppresses_the_warning` pins it.
- Built-ins after #695: warnings stay under `-q`. The rule is "`--quiet` suppresses non-error output and advice,
  never errors, refusals or warnings" (#695 design, repeated in the `dispatch_project` doc comment, which also
  forbids root `-q` from turning an error into exit 0).

Both cannot be the stated rule. One finding sharpens the question: the two sets of warnings are not the same kind.

- The `main.rs` warnings are ambient. They describe the project's manifest, not what the user asked for, and
  repeat on every run (`warning_repeats_on_every_run`). `should_skip_auto_rebuild` already excludes `self` and
  `project`, so no built-in ever reaches that loop.
- A built-in's warnings are about the operation just requested: `self build-manifest` skipped a command it could not
  parse, `project manifest rebuild` hit a bad plugin, `prune` could not remove a directory. They are part of that
  operation's result.

So today's behaviour is a split by source, not by accident, but the rule text says otherwise and `-q`'s help says
"non-error output", which a warning literally is.

## Part 1 design

Under the root `-q`, `prune` stays silent on success and prints nothing it would not print as a pure result.

| Line | Under `-q` | Why |
| --- | --- | --- |
| `ORPHAN  removing …` (per entry, real run) | dropped | Progress. |
| `freed … across N entries` | dropped | Success summary. |
| `removed N rebuild lock(s)` | dropped | Success summary. |
| `nothing to prune`, `no cache entries to remove` | dropped, except under `--dry-run` | Under `--dry-run` it is the answer to "what would go". |
| `aborted, nothing removed` | dropped | The user typed `n`; the exit code is 0 either way. |
| `DRY-RUN …` per entry, `would free …`, `would remove N rebuild lock(s)` | kept | The dry run's result. |
| `warning: failed to remove …`, `warning: failed to sweep rebuild locks` | kept | Warnings (see Part 2; flips with option b). |
| `refusing to wipe … without --yes` | kept | Error, exit non-zero. |
| the `continue? [y/N]` prompt | kept | A prompt must be seen to be answered. |

`self cache list` is unchanged (the table is the result).

### Mechanism

Replace the bare `out: &mut dyn Write` with a small `Report<'a>` in `self_cache_prune.rs`:

```rust
struct Report<'a> { out: &'a mut dyn Write, quiet: bool, dry_run: bool }
impl Report<'_> {
    /// Success chatter: dropped under `-q`, unless this is a dry run (then it is the result).
    fn note(&mut self, args: fmt::Arguments) -> io::Result<()>;
    /// The command's result: always printed (the `DRY-RUN` lines).
    fn result(&mut self, args: fmt::Arguments) -> io::Result<()>;
    /// Warnings: always printed today. Gating this one method is the whole of option (b).
    fn warn(&mut self, args: fmt::Arguments) -> io::Result<()>;
}
```

One value threads through the same four helpers, so no helper gains a `quiet` parameter and the dry-run exception
lives in one place. The thread is `run_self(.., quiet)` -> `self_cache::dispatch(m, quiet)` ->
`self_cache_prune::run(root, m, quiet)`.

Two behaviour-preserving details that need care:

- `sweep_locks` returns the removed count and its caller prints `nothing to prune` only when that is 0. The count
  must be returned whether or not the line is printed, or `-q` would change what the next line decides.
- The `--all` confirmation moves behind a parameter (`confirm: impl FnOnce(&[CachedVenv]) -> Result<bool>`;
  `run` passes `confirm_destroy_all`). It is the only way to unit-test the `aborted` branch without a terminal, and
  the 99% patch gate needs that branch covered by a real test.

Warnings stay on stdout as today. Moving them to stderr is a reasonable fix but a separate behaviour change; not in
this PR.

## Part 2: options

Facts used below. The `-q` help is "Suppress non-error output and never prompt to install uv." (`cli.rs`). Local
`--help` of the comparables, checked here: cargo `-q`: "Do not print cargo log messages"; uv `-q`: "Use quiet output"
(`-qq` for more); git `-q`: "be more quiet" (fetch, gc), "do not print names of files removed" (clean). Observed
here: `uv lock -q` hides uv's "Failed to parse `pyproject.toml`" warning (and `-qq` too); `git clone -q` of an empty
repository still prints `warning: You appear to have cloned an empty repository`. For cargo, I recall
`Shell::warn` being a no-op in `Verbosity::Quiet` (cargo's own warnings are dropped, rustc diagnostics are not),
but I could not trigger a cargo warning offline to confirm it; treat that one as unverified. So the comparables do
not agree: uv (and, by recollection, cargo) drop warnings under `-q`, git keeps them.

### (a) Keep warnings under `-q` everywhere (change dispatch)

- Change: remove `!argv_requests_quiet(&argv)` from the `main.rs` warning gate, invert
  `quiet_suppresses_the_warning`, update its doc comments.
- Pro: one rule, as written in #695. A skipped plugin is a real fault, and `-q` no longer hides why a command is
  missing.
- Cost: a shipped, tested behaviour changes: anyone running `toolr -q <cmd>` in a script with a skipped plugin
  starts getting stderr noise on every run (the warning repeats each run). Needs a **Breaking**-style release note.
  Contradicts "Suppress non-error output" as the help reads, and uv/cargo.

### (b) Silence warnings under `-q` everywhere (change #695's rule)

- Change: gate warnings in the built-ins behind the same `quiet` (in `Report::warn`, `build-manifest`'s warning
  loop, `manifest_rebuild`, `project init`'s trust-store warnings), reword the #695 rule and the `dispatch_project`
  doc comment to "never errors or refusals".
- Pro: one rule, matches the help text literally, matches uv (and cargo by recollection), and `-q` means silence
  for mise hooks and scripts. Cheap to implement for prune (one method).
- Cost: reverses a rule the user set a day earlier and touches every built-in #695 just fixed, plus their tests.
  Real signal is lost: a plugin author running `self build-manifest -q` in CI no longer sees that a command was
  skipped, and a failed `remove_dir_all` in prune is invisible. Warnings that mean "your result is incomplete"
  become indistinguishable from success.

### (c) Keep the split and document why

- Change: no behaviour change. State the rule as: `--quiet` drops success lines, progress, advice and *ambient*
  warnings (about project state other than what you asked, repeated every run); it keeps errors, refusals and
  warnings that qualify the requested operation's own result. Reword the root `-q` help to say so, update the
  #695 rule and the `dispatch_project` doc comment, and pin both sides with tests.
- Pro: zero behavioural risk, both existing decisions survive, and the line is principled (ambient noise is
  droppable; "your result is incomplete" is not). Matches git's treatment of operation warnings and uv's treatment
  of ambient ones.
- Cost: `-q` no longer means "no warnings", which is harder to explain and the one-line help gets longer. Needs
  discipline: a future warning must be classified, and the classification lives in prose.

### Decision (user, 2026-11-06): (a)

Warnings print under `-q` everywhere. `--quiet` drops success lines, progress and advice, never errors, refusals or
warnings, on every path, built-ins and normal dispatch alike.

- Remove `!argv_requests_quiet(&argv)` from the warning gate in `main.rs`, so plugin-skip and other dispatch
  warnings print under `-q`. Invert `quiet_suppresses_the_warning` (rename it to say warnings survive `-q`) and fix
  its doc comments.
- Audit every other warning gated on quiet (grep `argv_requests_quiet`, `quiet` near `warn`/`eprintln!("toolr:
  warning`) and ungate it, with a test each.
- `Report::warn` in Part 1 always prints.
- Reword the root `-q` help to "Suppress non-error output except warnings, and never prompt to install uv." (or
  equivalent wording that says warnings stay), and the `dispatch_project` doc comment to match.
- The release note starts with `**Breaking:**`: scripts running `toolr -q` now see warnings, such as skipped
  plugins, on stderr.

## Open questions

1. Should `prune`'s warnings move from stdout to stderr? Not required here; separate change if wanted.

## Tests

Unit tests in `crates/toolr/src/self_cache_prune.rs` (existing ones switch to `Report::new(&mut buf, false, dry_run)`),
and integration tests in `crates/toolr/tests/self_cache_prune.rs`. Names and assertions are in the plan.
