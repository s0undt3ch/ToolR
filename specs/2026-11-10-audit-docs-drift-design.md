# Docs drift and release guard

Findings U-07, P-06, P-03 (guard and recovery doc only) and U-11 from the 2026-11 audit.

## Why

Docs and config named things that no longer exist or contradicted the code. One item was a wrong
instruction (plugins told to depend on `toolr`, the binary wheel). `release.yml` could be dispatched
from any branch, and the half-published recovery lived only in a shell error string. Option help
hid the value placeholder, so `--name` looked like a flag.

## Changes

U-07, twelve items. Dropped as already fine on main or invalid:

- Item 11: `internals/index.md` no longer says "bash, zsh, and fish"; only `concepts.md` did.
- Item 8: `install.sh` has no `--repo` flag (only the `TOOLR_REPO` env var), so only `--triple`,
  `--dry-run` and `--no-verify` are documented.

The rest, by page:

- `cli.md`: `project init` usage and flags gain `-y/--yes` and `-q`.
- `third-party.md`: depend on `toolr-py`; no top-level `init`; `--check` exits `2` with a diff.
- `internals/manifest.md`, `internals/cache.md`: parser path; `schema_version` in the `meta.json` example.
- `installation/index.md`: the extra `install.sh` flags.
- `concepts.md`, `project init` next steps: PowerShell joins the shell list.
- `writing-commands/context.md`: `ctx.warn`, `ctx.which`.
- Help renderer (`help.rs`): multi-value positionals render `<PACKAGE>...`.

U-11: the help renderer falls back to the argument id when no value name is set, so options render
`--name <name>`, matching the parse error. Snippets are regenerated.

P-06: CONTRIBUTING says four crates and where `pin-github-actions` comes from; stale
`regen-doc-snippets.py` names replaced; dead `tests/cli` ruff and mypy entries removed; the five
top-level `tests/test_*.py` files move into `tests/<area>/` and `check-tests-layout` rejects new ones.

P-03: `release.yml`'s first job fails on any ref but `refs/heads/main`. CONTRIBUTING gains a
"Recovering a half-published release" section.

## Non-goals

Splitting the publish job, `continue-on-error` on Test PyPI (both accepted in triage), checking the
GitHub environment's branch policy (not readable from the repo), the `-V/--version` flag.
