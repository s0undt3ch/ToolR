# Trust nothing in an untrusted repository

Audit finding U-16 (2026-11-09). Decided 2026-11-11: keep the cache in `tools/`. In an untrusted
repository toolr reads nothing from the repository's tools and writes nothing to it. The user trusts the
repository first, then toolr shows and runs its commands.

## Why

`tools/.toolr-manifest.json` is a cache. toolr already refuses to *write* it into an untrusted
repository, but it still *reads* it there. Whoever controls the repository controls `tools/` too, so
they can commit a manifest whose freshness hash matches the tree. toolr then serves it as-is. The
result: `--help`, bare `toolr` and tab completion show whatever command list, help text and argument
choices the file claims.

Ignoring only the cache would leave toolr parsing `tools/*.py` before trust, and writing a cache that
it then ignores (`toolr project manifest rebuild`). The rule is simpler and easier to audit as one
sentence: **until a repository is trusted, toolr shows only its own commands.** No parse of
`tools/*.py`, no cache read or write, no glob of the repository's venv. Help text from an untrusted
docstring never reaches the terminal.

Moving the cache out of the tree (the finding's original fix) was rejected: effort L, and the path is
already public through `.gitignore` templates and CI recipes.

## Behaviour

"Trusted" means what `toolr_core::trust::is_trusted_now` says: `TOOLR_TRUST_ALL` is on, or the trust
store holds a record for the repository. It never prompts, writes or prints.

| Invocation | Trusted (unchanged) | Untrusted (new) |
|---|---|---|
| `toolr`, `toolr --help` | built-ins + repository commands | built-ins only, plus one stderr line saying the repository's commands are hidden until `toolr project trust` |
| Tab (`toolr __complete …`) | built-ins + repository commands | built-ins only; no stderr (completion scripts discard it) |
| `toolr <user-cmd> …` | runs | trust gate first (see below), then as trusted |
| `toolr <user-cmd> --help` | that command's help | never prompts; clap's "unrecognized subcommand" plus the same hidden-commands line |
| `toolr project manifest rebuild` | rebuilds and writes | trust gate first, then as trusted |
| `toolr self build-manifest <package>` | as today | trust gate first: it reads the repository's tools venv |
| `toolr self build-manifest --source-dir PATH` | as today | as today: the user named the path |
| Other built-ins (`project trust`, `self …`) | unchanged | unchanged |

Project discovery still reads what it needs to find the repository root and look up its trust
record. Nothing else under the repository is read before trust.

### The trust gate for a user command

Today `toolr <user-cmd>` builds the manifest, finds the command, builds the execution spec, then calls
`require_trust` (`dispatch/run.rs`). With no manifest before trust, clap would reject the command as
unknown before the prompt is ever reached. So the gate moves before the manifest step:

- `argv.names_user_command()` is true, the argv is not a help request, and the repository is
  untrusted: call `require_trust` with the same `TrustMode` dispatch uses today (`-q` never prompts).
- Granted: carry on exactly as a trusted run, so the manifest is built, cached and dispatched. The
  "sync the venv after a trust prompt" step in `dispatch/run.rs` still runs; pass the grant through
  rather than prompting twice.
- Refused or not promptable: the existing refusal error and exit code.

### Consequences to accept

- **No preview before trust.** The user can't list a repository's commands until they trust it.
  `cat tools/*.py` is the preview. `TOOLR_TRUST_ALL=1` (CI) is unaffected.
- **A typo prompts.** `toolr tset` in an untrusted repository asks for trust before toolr can know
  the command doesn't exist. That's inherent: knowing needs the read we're refusing.
- **A bad path value is reported after the prompt, not before.** Today the spec is built, and a bad
  path value rejected, before the prompt. Acceptable.
- **Speed.** Untrusted runs get faster: no parse at all. Trusted runs are unchanged apart from the
  trust check Tab now makes.
- **Behaviour change visible to users**, so the release note is `**Breaking:**`.

## An in-tree venv toolr didn't build is not trusted

Trusting a repository trusts its `tools/` source, not whatever sits in `tools/.venv`. Today, once a
repository is trusted, `venv::check_freshness` decides whether to reuse the venv from the mtime of a
marker file *inside* that venv. A committed or unpacked `tools/.venv` gets a new mtime on checkout,
so it reads as fresh: toolr skips the sync and runs the planted `bin/python`. Even a sync wouldn't
help, because `uv` runs an existing venv's interpreter to inspect it.

Rule: **toolr uses an in-tree venv only if toolr built it after the repository was trusted.**

- Before trust, nothing touches `tools/.venv` (no fragment glob, no `self build-manifest <package>`
  lookup, no freshness check). This is covered by the gating above.
- When toolr builds or syncs an in-tree venv, it writes a random token to the venv's marker and the
  same token to the repository's record in the trust store, which sits outside the repository. An
  in-tree venv is fresh only when the two match, on top of today's mtime and interpreter checks.
  The repository can't forge a token it never sees.
- A mismatch or missing token means toolr didn't build that venv. toolr treats it like a missing
  venv: one stderr line saying so, the venv directory removed (never run), then a fresh build
  through the normal sync path and its prompts. `--no-sync` paths refuse with a message that names
  `toolr project venv sync`.
- `TOOLR_TRUST_ALL=1` has no trust record, and CI restores in-tree venvs from its own cache. Under
  `TOOLR_TRUST_ALL` the token check is skipped and the venv is trusted as today.
- Cache-located venvs live under the user's cache dir, which the repository can't write to without
  running code. They aren't checked.
- `toolr project untrust` drops the record, and the token with it. A later re-trust rebuilds an
  in-tree venv once.
- Upgrade cost: every existing in-tree venv has no token, so it gets rebuilt once after upgrading.
  Say so in the release note.

Token format, its file name, and how the record stores it are the builder's call. Keep the trust
record format backwards compatible: an old record without a token reads as "no token". If this half
makes the PR too large, it may ship as a second stacked PR, in the same release as the first.

## Where the repository is read before trust today

1. `toolr::bootstrap::ensure_manifest`: computes `is_trusted_now`, then calls
   `manifest_build::rebuild::refresh` with `Persist::InMemory` when untrusted. That reads the cache,
   parses `tools/*.py` and globs the venv.
2. `toolr::main::load_or_empty`, reached when `ensure_manifest` returns `None` (including a failed
   untrusted build with no cache). It reads the cache directly.
3. `complete::freshness::resolve_manifest_at_tab`: calls `refresh` with `Persist::InMemory` and no
   trust check.
4. `toolr project manifest rebuild` (`project.rs::manifest_rebuild`): parses and writes with no trust
   check.
5. `toolr self build-manifest <package>`: resolves the package in the repository's tools venv.

All five must be closed.

## Decisions for the builder

- Untrusted paths never call `refresh`, so `Persist` keeps its two values: `Write` for a trusted
  normal run, `InMemory` for trusted Tab. Update their doc comments, which describe the untrusted
  case today.
- `ensure_manifest` returns an empty manifest for an untrusted repository, so `load_or_empty` is never
  reached there. Pick the cleanest shape; the behaviour is not negotiable.
- `resolve_manifest_at_tab` calls `is_trusted_now` on the discovered root and returns an empty
  manifest when untrusted. It creates nothing and reads one small record file. Measure Tab in a
  trusted repository before and after and state the number in the PR. If it adds more than 1 ms,
  stop and report rather than optimise.
- The hidden-commands line: one line on stderr, printed for bare `toolr`, `--help` and an unknown
  user command's help or error in an untrusted repository; dropped under `-q`. Exact wording is the
  builder's call; it must name `toolr project trust`.
- `manifest rebuild` and `self build-manifest <package>` call `require_trust` the way
  `project venv run --no-sync` does (`project.rs`).
- `drop_unsafe_names` still runs on every manifest.
- Update the `bootstrap.rs` module and `ensure_manifest` doc comments (they say an untrusted repository
  gets an in-memory manifest and that Tab never reads the trust store).

## Tests

Integration tests in `crates/toolr/tests/`, through the real binary, with an isolated trust store:

1. **Nothing shown before trust.** A repository with a real command in `tools/` plus a planted
   manifest naming a command no module declares. Untrusted: `toolr --help`, bare `toolr` and
   `toolr __complete <root> ""` list neither, and help prints the hidden-commands line. Trusted:
   the real command is listed and the planted one is not (the freshness hash won't match). This is
   the regression test for the finding.
2. **Nothing read before trust.** Untrusted, with a module that fails to parse and an unreadable
   manifest file: `--help` succeeds with no parse or "unreadable" warning (proves no read happened).
3. **Nothing written before trust.** Untrusted `--help`, Tab and refused `toolr <cmd> -q` leave the
   tree byte-for-byte unchanged (extend the existing test).
4. **The gate.** Untrusted `toolr <cmd> -q` exits with the refusal error, not clap's "unrecognized
   subcommand". Untrusted `toolr <cmd> --help` doesn't prompt. With `TOOLR_TRUST_ALL=1` the command
   runs.
5. **Rebuild needs trust.** Untrusted `toolr -q project manifest rebuild` refuses and writes nothing;
   trusted, it writes.
6. **Trusted Tab still serves a fresh cache without rebuilding** (existing behaviour).
7. **Planted in-tree venv.** An in-tree `tools/.venv` with a fresh marker and a `bin/python` that
   writes a sentinel file, in a trusted repository with no token: a user command doesn't create the
   sentinel. toolr removes and rebuilds the venv, then runs the command with the rebuilt one.
8. **toolr's own venv is reused.** After toolr syncs an in-tree venv, a second run doesn't resync.
   Under `TOOLR_TRUST_ALL=1`, a venv with no token is reused.
9. **Untrust then re-trust** rebuilds an in-tree venv once.

A PTY test of granting trust at the prompt and then running the command exists for dispatch today;
keep it passing, and extend it if the gate move changes which code prompts.

## Docs and release note

- `docs/concepts.md`, "Repository trust":
    - The "These never need trust" paragraph: `--help`, bare `toolr` and Tab need no trust but show
      only toolr's own commands until the repository is trusted. Drop `project manifest rebuild` and
      `self build-manifest <package>` from that list.
    - The paragraph starting "toolr doesn't write `tools/.toolr-manifest.json`": replace it with the
      new rule (nothing read or written before trust).
- Any other doc or skill page that shows `--help` output from an untrusted repository: check
  `docs/` and `skills/` with a grep for "trust"; the regenerated snippets run trusted, so they shouldn't
  change.
- Release note: `**Breaking:**` In an untrusted repository, `--help`, bare `toolr` and Tab now show
  only toolr's own commands, and toolr neither reads nor writes `tools/.toolr-manifest.json` there.
  `toolr <command>`, `project manifest rebuild` and `self build-manifest <package>` ask for trust
  first. Trust the repository with `toolr project trust` to see its commands. toolr now uses an
  in-tree `tools/.venv` only if it built that venv itself after trust. Any other in-tree venv,
  including every existing one on the first run after upgrading, is removed and rebuilt.

## Out of scope

- Moving the cache out of `tools/`.
- Content-hashing trust (trust stays keyed on the path, as today).
