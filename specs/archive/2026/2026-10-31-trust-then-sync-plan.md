# Trust Then Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `toolr project trust` trusts the repository and then syncs its tools venv, and a command run that shows the
trust prompt syncs a missing venv once the user agrees, so a fresh clone needs one step (F-32).

**Architecture:** `toolr_core::trust::decide` reports whether this run's prompt granted trust. A new
`toolr_core::project::sync_after_trust` checks freshness, asks the sync prompt (or answers it from
`TOOLR_AUTO_INSTALL_UV`, `--quiet` and the terminal check) and runs `ensure_venv_ready` with uv install consent
pre-granted. `project_trust` and dispatch call it.

**Tech Stack:** Rust (clap, anyhow), assert_cmd integration tests with a pty on stdin (`libc::openpty`, as in
`crates/toolr/tests/quiet_uv_consent.rs`) and the stub uv from `tests/common`.

**Spec:** [`specs/archive/2026/2026-10-31-trust-then-sync-design.md`](./2026-10-31-trust-then-sync-design.md)

## Global Constraints

- No real network: interactive tests use `VenvFixture`'s stub uv on `PATH`; non-interactive tests must never reach a
  real `uv sync`.
- Trust tests isolate state with `TrustEnv`-style env (no `TOOLR_TRUST_ALL`, private XDG dirs).
- pty tests are `#![cfg(unix)]` in their own file; `trust_commands.rs` runs on every platform.
- `project trust` stdout stays the trusted root only.
- British English. Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. Format touched files
  only.

## Review Focus

1. Dispatch offers the sync only after a trust prompt answered `y` in this run, for a `Missing`, `Stale` or `Broken`
   venv.
2. At most two prompts: an accepted sync prompt pre-grants uv install consent.
3. `--quiet`, no terminal and the two env vars answer prompts as in the spec's table.

---

### Task 1: Trust outcome

- [ ] A `Trusted` proof (root + `granted_at_prompt`) that only `toolr_core::trust` builds; `decide`, `require_trust` and
  `TrustStore::grant` return it. Unit tests: env and record give `granted_at_prompt() == false`, a `y` gives `true`.

### Task 2: Sync prompt and trust-then-sync step (`toolr_core::project`)

- [ ] `confirm_sync(input, out) -> bool` with the spec's text; `[y/N]`, EOF declines. Unit tests with `Cursor`.
- [ ] `EnsureOpts::trusted: Option<Trusted>` skips the inner `require_trust` for the root the proof covers.
- [ ] `sync_after_trust(&Trusted, quiet, never_prompt, prompt_io) -> SyncAfterTrust { NotNeeded, Synced(ResolvedVenv), Declined }`;
  `SYNC_LATER_HINT`. Answer order: fresh → `NotNeeded`; `TOOLR_AUTO_INSTALL_UV=1` → yes; quiet or no prompt IO →
  `Declined`; else prompt. A yes syncs with `ConsentMode { yes_flag: true, .. }`, after releasing the stdin lock.
  pty tests with no uv anywhere prove no uv prompt follows a `y`.

### Task 3: `toolr project trust`

- [ ] Integration tests first (`crates/toolr/tests/trust_then_sync.rs`, pty): `y` syncs (stub uv ran, stdout is the
  root); `n` keeps trust, prints the hint, exit 0, uv not run; `--no-sync` asks nothing; `-q` and no terminal decline
  silently or with the hint; `TOOLR_AUTO_INSTALL_UV=1` without a terminal syncs; fresh venv asks nothing.
- [ ] `--no-sync` and `-q/--quiet` in `cli.rs`; `project_trust` calls `sync_after_trust`.

### Task 4: Dispatch

- [ ] Integration tests first: trust `y` + sync `y` on a missing venv → uv synced, command ran; sync `n` → exit 2 with
  the hint, command not run, trust recorded; fresh venv → one prompt only; `TOOLR_TRUST_ALL=1` + missing venv → the
  existing missing-interpreter error, no sync.
- [ ] In `dispatch.rs`, after `require_trust`: when the proof says `granted_at_prompt`, call `sync_after_trust` (any venv that
  isn't fresh); otherwise `resync_if_needed` as before. Tests for `Stale` and `Broken` after a fresh trust.

### Task 5: Docs, release note, snippets

- [ ] README bullet and "Scaffold your repo" (below line 24), `docs/concepts.md`, `docs/cli.md`, `docs/quickstart.md`,
  `docs/installation/mise.md`, `skills/toolr-command-authoring/SKILL.md`.
- [ ] `toolr pre-commit regen-doc-snippets`; `cargo xtask build-skill-refs`.
- [ ] `toolr release-notes add --slug trust-then-sync --text "…"`, committed as `docs(release-notes): …`;
  `toolr release-notes check --base origin/main`.

### Task 6: Verify and archive

- [ ] `cargo test -p toolr-core -p toolr`, `uv run pytest`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `prek run --all-files`, `uv run --group docs mkdocs build --strict`, patch coverage ≥ 99% against `origin/main`.
- [ ] `git mv` design and plan to `specs/archive/2026/`, set the design's status to implemented, fix the plan's
  `Spec:` link. Commit `docs(specs): archive trust-then-sync design and plan`.
