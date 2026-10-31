# Correct the README's first-run venv claim

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-32 (low).
- **Plan:** [`2026-10-31-readme-venv-first-run-plan.md`](./2026-10-31-readme-venv-first-run-plan.md)

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
for a new project, and `toolr project venv sync` creates it in a clone. `docs/concepts.md` ("The tools venv") and
`docs/quickstart.md` already say so; only the README is wrong.

The README's "Scaffold your repo" section shows only `toolr project init`, so it doesn't cover the clone case either.

## Decision

Fix the README; don't change dispatch.

1. Rewrite the bullet's first sentence to say what happens: `toolr project init` (or `toolr project venv sync` in an
   existing checkout) builds a per-repo venv with `uv`, and toolr re-syncs it when it falls out of date. Don't list the
   files that make it stale: F-04 (`fix/venv-sync-pyproject`) is widening that list on another branch.
2. Add one sentence after the "Scaffold your repo" code block: in a clone of a repo that already has `tools/`, run
   `toolr project venv sync` once instead of `toolr project init`.

Both edits sit below line 24. `docs/index.md` includes `README.md` with `lines=5-24`, so the docs home page doesn't
change; anything inserted above line 24 would shift that include.

### Rejected alternatives

- **Create the venv on first dispatch (`Missing` → sync).** See the open question below. It is a behaviour change
  with real costs, not a doc fix.
- **Delete the bullet's venv sentence.** The bullet's point (no system Python needed) still holds; it only needs the
  right command.

## Open question

Should dispatch create a missing venv itself, so the README's original claim becomes true? `resync_if_needed` already
syncs a `Stale` or `Broken` venv, so extending it to `Missing` is mechanically small.

Recommendation: no, not in this change. The first `toolr <cmd>` in a fresh clone would download uv and a Python and
could stop at the repo-trust prompt (#612) in the middle of a command, which is a poor place for a first-time
download and a security decision. `toolr project init` already covers new projects, and the missing-interpreter error
names the exact command. If wanted, it deserves its own design covering consent, `--quiet` and CI.

## Scope and compatibility

- No schema change. No code change.
- Docs: `README.md` only.
- `UNRELEASED.md`: none (documentation fix, no behaviour change).
