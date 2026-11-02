# Trust first, then sync the tools venv (F-32)

- **Status:** Implemented (2026-11-02)
- **Origin:** whole-project review finding F-32 (low). Rewritten 2026-11-02 after the maintainer chose a
  behaviour change over a README-only fix.
- **Plan:** [`2026-10-31-trust-then-sync-plan.md`](./2026-10-31-trust-then-sync-plan.md)

## Problem

The "Why ToolR" list in `README.md` says:

> Toolr resolves a per-repo Python venv via `uv` on first invocation.

It doesn't. In `crates/toolr/src/dispatch.rs`, `resync_if_needed` returns early on `Freshness::Missing` (no
`.toolr-sync-stamp` marker or no `uv.lock`), so dispatch never creates a venv. The `python.is_file()` check that follows
then fails. In a fresh clone of a repo that already has `tools/`:

```sh
git clone <repo> && cd <repo>
toolr demo hello
# toolr: Python interpreter not found at …
# Run `toolr project venv sync` to materialise the tools venv.
```

Dispatch only re-syncs a venv that already exists and has gone stale or broken. `toolr project init` creates the venv
for a new project, and `toolr project venv sync` creates it in a clone. Since repository trust (#612) a clone also needs
`toolr project trust` (or a `y` at the trust prompt), so the first run in a clone takes two separate steps.

## Decision

Trust first, then sync, and let the trust trigger the sync.

1. **`toolr project trust` syncs.** It records trust, then syncs the tools venv in the same run when the venv needs it
   (`Freshness::Missing`, `Stale` or `Broken`). `--no-sync` keeps the old trust-only behaviour. A new `-q/--quiet`
   never prompts, prints only the trusted root and passes `--quiet` to uv; the root `toolr -q project trust` means
   the same. A sync the user agreed to that then fails keeps the trust and exits non-zero (2, like every toolr error).
2. **Dispatch syncs after a trust prompt.** When running a command shows the trust prompt and the user answers `y`,
   and the venv is `Missing`, `Stale` or `Broken`, toolr offers to sync it with the same sync prompt and then runs the
   command. The trigger is the trust granted by that prompt, not "the repository is trusted": an already-trusted
   repository, or one run under `TOOLR_TRUST_ALL=1`, keeps today's behaviour (`resync_if_needed` re-syncs a `Stale`
   or `Broken` venv and leaves a `Missing` one alone). Without trust nothing is synced.
3. **Two prompts, each its own consent.** First the existing trust prompt, then, only when a sync is needed, a sync
   prompt. The sync prompt replaces the uv install prompt for that sync: a `y` is also consent to install uv, so the
   user sees at most two prompts, never three. It is asked even when uv is already on `PATH`, because it is consent to
   run `uv sync` against the repository's `tools/pyproject.toml`, not only to download uv.
4. **Declining the sync keeps the trust.** `toolr project trust` prints the trusted root, then the hint on stderr, and
   exits 0. Dispatch prints the same hint as its error and exits 2, like every other toolr error, without running the
   command.
5. **No new environment variable.** Each prompt is answered the way its predecessor is answered today:

   | Run                                     | Trust prompt (dispatch only) | Sync prompt          |
   | --------------------------------------- | ---------------------------- | -------------------- |
   | Terminal on stdin, no `--quiet`         | asked                        | asked                |
   | `--quiet` or no terminal                | refused                      | declined (hint)      |
   | `TOOLR_TRUST_ALL=1`                     | answered `y`, never shown    | not reached¹         |
   | `TOOLR_AUTO_INSTALL_UV=1`               | unchanged                    | answered `y`, even quiet |
   | `toolr project trust --no-sync`         | n/a                          | not reached          |

   ¹ For dispatch. `toolr project trust` ignores `TOOLR_TRUST_ALL` and always records trust.

   The sync prompt absorbed the uv install prompt, so it follows that prompt's rules: `TOOLR_AUTO_INSTALL_UV=1`
   answers it, and with `--quiet` or no terminal it is never shown and counts as a refusal. That keeps a scripted
   `toolr project trust` from downloading anything unless the script opted in.

`--quiet` (or the root `-q`) suppresses the decline hint for `toolr project trust`; stdout stays the trusted root alone,
so scripts that read it keep working. The sync's own progress goes to stderr, with the venv path escaped by
`trust::shown` like the root.

### Prompt texts

```text
toolr hasn't run code from this repository before:
  /path/to/repo
Running its commands executes tools/*.py and syncs tools/pyproject.toml with uv.
Trust it? [y/N] y
The tools venv needs a sync before toolr can run this repository's commands.
Syncing may download uv and a Python, and installs the dependencies in tools/pyproject.toml.
Sync it now? [y/N]
```

The first prompt is the existing one, unchanged. The sync prompt defaults to no, like the trust prompt: declining costs
nothing, since the hint names the command to run later. Anything but `y`/`yes`, including bare Enter and EOF, declines.

Decline hint (stderr for `project trust`, the error for dispatch):

```text
toolr: the tools venv isn't synced; run `toolr project venv sync` when you're ready.
```

### Rejected alternatives

- **Fix the README only** (this spec's first version). Leaves a fresh clone needing two commands and two concepts
  before the first run.
- **Dispatch syncs any missing venv in a trusted repository.** A command run long after trust, under `--quiet` or in CI
  would start downloading without anyone having been asked. Tying the sync to a trust prompt answered moments earlier
  keeps every download behind a prompt or an explicit env var.
- **One combined prompt** ("trust and sync?"). Trust and download are separate decisions; a user may trust a repository
  but want to sync on a better connection.
- **A new `TOOLR_AUTO_SYNC` variable.** `TOOLR_AUTO_INSTALL_UV=1` already means "download what you need without
  asking"; a second switch would only have to agree with it.

## Scope and compatibility

- Code: `toolr_core::trust` (`require_trust` and `grant_trust` return a `Trusted` proof that only the trust module can
  build, and only against the default store with the real `TOOLR_TRUST_ALL`: `decide` and `TrustStore::grant` are
  private; the proof names the root and whether this run's prompt granted trust), `toolr_core::project` (sync
  prompt, the trust-then-sync step, `EnsureOpts::trusted` taking that proof so the trust check isn't repeated for the
  root it covers), `crates/toolr/src/project.rs::project_trust`, `crates/toolr/src/dispatch.rs`, `cli.rs`
  (`--no-sync`, `--quiet` on `project trust`).
- The sync prompt is read with the stdin lock released before syncing: finding uv may lock stdin again, and the lock
  isn't reentrant.
- No manifest or runner schema change.
- Behaviour change: `toolr project trust` on a terminal now asks to sync. Without a terminal it records trust and prints
  the hint on stderr unless `TOOLR_AUTO_INSTALL_UV=1`, in which case it syncs. Stdout is unchanged.
- `toolr project venv sync`, `venv run`, `venv shell` and `project init` are unchanged: they already sync, and their
  own trust prompt is not followed by a sync prompt.
- Follow-up: the existing "synced venv at …" lines of `toolr project venv sync` and `toolr project init` print the venv
  path unescaped; they should use `trust::shown` too.
- Known limitation: on the first run the command was resolved from the in-memory manifest built before trust. Commands
  from third-party plugins in the new venv appear from the next run, once `finalize_sync` has rebuilt the manifest.
- Docs: `README.md` ("Why ToolR" bullet and "Scaffold your repo"), `docs/concepts.md` (repository trust, the tools
  venv; the only page with the full consent rules), `docs/cli.md` and its regenerated help snippet,
  `docs/installation/index.md` (`TOOLR_AUTO_INSTALL_UV`), `docs/quickstart.md`, `docs/installation/mise.md`
  (first-time bootstrap), and the trust step in `skills/toolr-command-authoring/SKILL.md`. `cli.md` and
  `installation/index.md` state the effect in a line and link to `concepts.md`. Keep README lines 5–24 untouched:
  `docs/index.md` includes them.
- Release note: one note in `release-notes/` (`toolr release-notes add --slug trust-then-sync`), starting with
  `**Breaking:**`: `toolr project trust` can now run `uv sync` (accepted on a terminal, or `TOOLR_AUTO_INSTALL_UV=1`)
  and exits non-zero when that sync fails, where it used to always exit 0. `--no-sync` keeps the old behaviour. The
  feat commit is `feat(trust)!:`.
