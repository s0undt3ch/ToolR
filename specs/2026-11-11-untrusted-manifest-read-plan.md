# Trust nothing in an untrusted repository: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended)
> or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** In an untrusted repository toolr shows only its own commands and reads or writes nothing
under `tools/`. Once the repository is trusted, toolr uses an in-tree venv only if toolr built it.

**Architecture:** Three gates. (1) The manifest step returns an empty manifest when the repository is
untrusted, for normal runs and for Tab. (2) A user command, `project manifest rebuild` and
`self build-manifest <package>` call `require_trust` before anything reads `tools/`. (3) In-tree venv
freshness also needs a token that matches a per-venv entry in the trust store.

**Tech Stack:** Rust (`toolr-core`, `toolr`), `assert_cmd` integration tests, the existing `TrustEnv`
and `hostile_repo` test helpers in `crates/toolr/tests/common/mod.rs`.

**Spec:** `specs/2026-11-11-untrusted-manifest-read-design.md`

## Global Constraints

- "Trusted" is `toolr_core::trust::is_trusted_now(root)`. It never prompts, writes or prints.
- Before trust, nothing under the repository is read except what project discovery needs to find the
  root and its trust record.
- Trust is unreleased: change the trust store's on-disk format freely, with no migration and no
  compatibility code for earlier pre-release records.
- `-q` never prompts (existing `TrustMode { never_prompt }` rule).
- `drop_unsafe_names` still runs on every manifest.
- Under `TOOLR_TRUST_ALL=1` the venv token is neither checked nor written.
- Removing a venv never follows a symlink.
- Tab in a trusted repository: measure before and after Task 1 (`hyperfine` on
  `toolr __complete <root> ""`, 50+ runs). If it adds more than 1 ms, stop and report.
- 99% patch coverage (`cargo xtask patch-coverage --base main`), using real tests, not mocks.
- Run checks through prek. Format touched files only, never the whole crate.
- Conventional Commits with no Co-Authored-By trailer. Stage files by path; never `git add -A`.
- `common::toolr()` sets `TOOLR_TRUST_ALL=1`. Untrusted tests must use `common::TrustEnv`.

## Review Focus

1. **A worktree of a trusted repository:** trusted through the shared record, so its commands show.
   Its own `tools/.venv` gets its own venv entry, and a sync in worktree A must not rebuild
   worktree B's venv. Test in Task 3.
2. **`toolr <cmd> --help` and `toolr <cmd> -h` before trust:** never prompt, even on a TTY.
   Test in Task 2.
3. **`-q` user command before trust:** the refusal error from `require_trust`, not clap's
   "unrecognized subcommand", and exit code as today. Test in Task 2.
4. **`tools/.venv` is a symlink to a directory outside the repository:** the target survives a
   rebuild. Test in Task 3.
5. **Trust granted at the prompt for a user command:** the manifest is then built and cached, and the
   command runs on that same invocation, with at most the existing two prompts. Covered by
   `trust_then_sync.rs` PTY tests; Task 2 keeps them green and adds one if the flow changes.

---

### Task 1: Untrusted repositories get an empty manifest

**Files:**

- Modify: `crates/toolr/src/bootstrap.rs` (`ensure_manifest`, module and fn doc comments)
- Modify: `crates/toolr/src/main.rs` (`run`: hidden-commands line; `load_or_empty` unchanged)
- Modify: `crates/toolr-core/src/complete/freshness.rs` (`resolve_manifest_at_tab`)
- Modify: `crates/toolr-core/src/manifest_build/rebuild.rs` (`Persist` doc comments only)
- Test: `crates/toolr/tests/untrusted_repo.rs`

**Interfaces:**

- Produces: `ensure_manifest(run: &Invocation) -> anyhow::Result<Option<BuiltManifest>>` keeps its
  signature. For an untrusted root it returns `Ok(Some(Manifest::empty().into()))` without calling
  `resolve_venv_path` or `refresh`.
- Produces: `pub(crate) fn untrusted_project_root(run: &Invocation) -> Option<&Path>` in
  `bootstrap.rs`. It returns the discovered root when there is one and it is untrusted. Task 2 reuses
  it.
- Produces: the hidden-commands line, as `pub(crate) const HIDDEN_COMMANDS_HINT: &str` in `main.rs`.
  Wording is the implementer's call; it must name `toolr project trust`.

**Steps:**

- [ ] **Step 1: Write the failing tests** in `untrusted_repo.rs`. Each uses `TrustEnv` (untrusted):
    - `help_hides_repository_commands_before_trust`: a repository with a real command `greet` in
      `tools/`, plus `common::write_fresh_manifest` naming a `planted` command. `toolr --help`: stdout
      contains neither `greet` nor `planted`, contains `project`, and stderr contains
      `toolr project trust`. Then `env.trust(repo)` (or the existing equivalent): stdout contains
      `greet` and not `planted`.
    - `bare_toolr_hides_repository_commands_before_trust`: the same check for bare `toolr`.
    - `tab_offers_only_builtins_before_trust`: `toolr __complete <root> ""`: neither name, and
      stderr empty.
    - `nothing_is_read_before_trust`: a module with a syntax error and a manifest file of invalid
      JSON. `--help` succeeds and stderr has no `warning` line.
    - `nothing_is_written_before_trust`: snapshot the tree (paths plus contents), run `--help`,
      bare `toolr` and `__complete`, then assert the snapshot is unchanged.
    - `quiet_hides_the_hint`: `toolr -q --help` stderr is empty.
- [ ] **Step 2: Run them and see them fail.**
  `cargo test -p toolr --test untrusted_repo` fails on the new tests.
- [ ] **Step 3: Implement.** `ensure_manifest` branches on `untrusted_project_root` before
  `ensure_manifest_at`. In `resolve_manifest_at_tab`, after discovery, an untrusted root returns
  `ResolvedManifest { manifest: Manifest::empty(), project_root }`. `run` prints the hint when the
  root is untrusted, argv is bare or a help request, and `-q` is absent. Update the existing
  `untrusted_repo.rs` tests that expect repository commands before trust to the new behaviour; their
  names change if their claim changes. Keep `help_does_not_execute_repository_code` as is. Update
  the `Persist` and `bootstrap.rs` doc comments.
- [ ] **Step 4: Run** `cargo test -p toolr --test untrusted_repo` and
  `cargo test -p toolr-core complete`. Expected: PASS.
- [ ] **Step 5: Measure Tab** (Global Constraints) and record before/after numbers in the task report.
- [ ] **Step 6: Commit** `fix(trust): show only built-ins in an untrusted repository`.

### Task 2: The trust gate runs before anything reads `tools/`

**Files:**

- Modify: `crates/toolr/src/main.rs` (`run`)
- Modify: `crates/toolr/src/dispatch/run.rs` (take an existing grant instead of prompting again)
- Modify: `crates/toolr/src/project.rs` (`manifest_rebuild`)
- Modify: the `self build-manifest` handler (positional `<package>` mode only; find it from
  `crates/toolr/src/build_manifest_resolve.rs`)
- Test: `crates/toolr/tests/repo_trust.rs`; `crates/toolr/tests/trust_then_sync.rs` stays green

**Interfaces:**

- Consumes: `untrusted_project_root`, `HIDDEN_COMMANDS_HINT` (Task 1).
- Consumes: `toolr_core::trust::require_trust(root: &Path, mode: TrustMode) -> Result<Trusted>`.
  `Trusted::granted_at_prompt()` drives the existing sync-after-trust step.
- Produces: the dispatch entry point takes `Option<Trusted>`: `Some` when `run` already granted trust
  for this invocation. Then `dispatch/run.rs` skips its own `require_trust` call and uses that grant
  for `sync_after_trust_prompt`. Exact parameter placement is the implementer's call.

**Steps:**

- [ ] **Step 1: Write the failing tests** in `repo_trust.rs`, all through `TrustEnv`:
    - `quiet_user_command_refuses_before_trust`: `toolr -q greet` exits non-zero. stderr carries
      the same refusal text `require_trust` prints today, and not `unrecognized subcommand`.
    - `user_command_help_never_prompts`: on a PTY with no input, `toolr greet --help` and
      `toolr greet -h` finish without printing the trust prompt, and stderr has the hint.
    - `user_command_runs_under_trust_all`: `TOOLR_TRUST_ALL=1 toolr greet` runs.
    - `manifest_rebuild_needs_trust`: `toolr -q project manifest rebuild` refuses and
      `tools/.toolr-manifest.json` doesn't exist. After trust it exists.
    - `build_manifest_package_needs_trust`: `toolr -q self build-manifest some_pkg` refuses before
      looking in the venv. `--source-dir <dir>` is not gated (assert it gets past the gate, for
      example by reaching its own "not a directory" error for a missing dir).
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Implement.** In `run`, before `ensure_manifest`: if `untrusted_project_root` is
  `Some(root)`, `argv.names_user_command()` and `argv.help_mode()` is `None`, call `require_trust`
  with `never_prompt` from `-q`. On grant, carry on as trusted (the manifest is built and written),
  passing the grant to dispatch. A user command with `help_mode()` set falls through to clap, whose
  `InvalidSubcommand` path prints the hint in addition to its error when the root is untrusted. Gate
  `manifest_rebuild` and positional `self build-manifest` the way `project venv run --no-sync` does
  (`project.rs`).
- [ ] **Step 4: Run** `cargo test -p toolr` with `--test` for each of `repo_trust`, `trust_then_sync`,
  `untrusted_repo` and `build_manifest_cli`. Expected: PASS.
  If the prompt flow moved, add a PTY test to `trust_then_sync.rs`: grant at the
  prompt, then the command runs and the manifest exists afterwards.
- [ ] **Step 5: Commit** `fix(trust): ask for trust before reading an untrusted repository`.

### Task 3: An in-tree venv is used only if toolr built it

**Files:**

- Modify: `crates/toolr-core/src/trust.rs` (venv entries, untrust and prune cleanup)
- Modify: `crates/toolr-core/src/venv/sync.rs` (`Freshness`, `check_freshness`, marker write,
  removal before sync)
- Modify: callers that match on `Freshness`: `crates/toolr/src/dispatch/run.rs`,
  `crates/toolr/src/project.rs`, `crates/toolr-core/src/project.rs`
- Test: unit tests in `trust.rs` and `sync.rs`; integration in
  `crates/toolr/tests/dispatch_venv_freshness.rs` and `crates/toolr/tests/trust_worktrees.rs`

**Interfaces:**

- Produces, in `toolr_core::trust`:
    - `impl TrustStore { pub fn venv_token(&self, venv_dir: &Path) -> Result<Option<String>> }`
    - `impl TrustStore { pub fn record_venv(&self, root: &Path, venv_dir: &Path) -> Result<String> }`
      mints a random token, atomically writes an entry `{ key, kind, venv_dir, token }` keyed by the
      canonical venv path, and returns the token.
    - `untrust` / `untrust_key` also remove every venv entry naming that key. The `--prune` path
      removes entries whose record is missing or whose `venv_dir` no longer exists.
    - Entries live in a subdirectory of the store dir so `list()` doesn't read them as records.
- Produces, in `toolr_core::venv`:
    - `Freshness::Foreign`: the in-tree venv exists, but its marker token doesn't match the store's
      entry (or there is no entry). Never produced for cache-located venvs or under
      `TOOLR_TRUST_ALL`.
    - `check_freshness(resolved, tools_dir)` keeps its signature. It reads the store through
      `TrustStore::open_default()`. A store it can't open counts as `Foreign` for in-tree venvs.
    - A successful sync writes the token from `record_venv` into the marker (today an empty file),
      unless `TOOLR_TRUST_ALL` is set, in which case it writes the empty marker as today.
    - Before `uv sync` on a `Foreign` venv: print one stderr line naming the venv, then remove it.
      Use `symlink_metadata`: a symlink is unlinked, never followed; a real directory gets
      `remove_dir_all`.
- Callers: dispatch and `sync_after_trust` treat `Foreign` like `Missing` (offer the sync).
  `venv run --no-sync` refuses with a message naming `toolr project venv sync`.

**Steps:**

- [ ] **Step 1: Write the failing unit tests:**
    - `trust.rs`: `record_venv_then_venv_token_round_trips`;
      `untrust_removes_its_venv_entries` (two venvs under one key both gone, a third
      under another key kept); `prune_drops_venv_entries_for_missing_venvs`;
      `list_ignores_venv_entries`.
    - `sync.rs`: `in_tree_venv_without_a_token_is_foreign`, `matching_token_is_fresh`,
      `cache_located_venv_needs_no_token`, `trust_all_skips_the_token_check`,
      `removing_a_symlinked_venv_keeps_its_target`.
- [ ] **Step 2: Write the failing integration tests:**
    - `dispatch_venv_freshness.rs::planted_in_tree_venv_is_rebuilt_not_run`. Set up a trusted
      repository (`TrustEnv`, trusted) with in-tree venv location. Its `tools/.venv` has a fresh
      `.toolr-sync-stamp`, and `bin/python` is a script that writes a sentinel then exits 1. Use the
      uv stub. Running a user command with `TOOLR_AUTO_INSTALL_UV=1`: the sentinel doesn't exist,
      stderr names the removed venv, and the uv stub ran a sync.
    - `dispatch_venv_freshness.rs::own_in_tree_venv_is_reused`: after one toolr sync, a second
      dispatch runs no sync.
    - `trust_worktrees.rs::worktrees_keep_separate_venv_tokens`: two worktrees of one trusted
      repository, each with an in-tree venv. Sync A, sync B, then dispatch in A: no sync in A.
    - `trust_commands.rs::retrust_rebuilds_the_in_tree_venv_once`.
- [ ] **Step 3: Run them and see them fail.**
- [ ] **Step 4: Implement** to the Interfaces above. Mint the token from the OS RNG; check whether
  `getrandom` or `uuid` is already in `toolr-core`'s `full` feature before adding anything, and a new
  dependency must be `optional = true` under `full` (CLAUDE.md).
- [ ] **Step 5: Run** `cargo test -p toolr-core`, then `cargo test -p toolr` with `--test` for each of
  `dispatch_venv_freshness`, `trust_worktrees`, `trust_commands`, `project_venv_run`,
  `project_venv_sync` and `trust_then_sync`. Expected: PASS.
- [ ] **Step 6: Commit** `fix(venv): use an in-tree venv only if toolr built it`.

### Task 4: Docs, release notes and full verification

**Files:**

- Modify: `docs/concepts.md` ("Repository trust" section)
- Modify: `release-notes/2026-11-01.00-00-12.repository-trust.md`
- Modify: `release-notes/2026-11-01.00-00-17.untrusted-no-manifest-write.md`
- Modify: any page in `docs/` or `skills/` that says `--help` or Tab works before trust (grep
  `trust`)

**Steps:**

- [ ] **Step 1: Edit the docs and notes** exactly as "Docs and release note" in the spec says. No new
  release-notes file.
- [ ] **Step 2: Run** `prek run --all-files`, `uv run mkdocs build --strict`,
  `toolr release-notes check --base main` and `mise run test` (poll it every 30 to 60 s). Expected:
  all pass.
- [ ] **Step 3: Coverage.** Run the CI `cargo llvm-cov` command from CLAUDE.md, then
  `cargo xtask patch-coverage --base main`. Expected: at least 99%. Close any gap with a real test.
- [ ] **Step 4: Commit** `docs(trust): untrusted repositories show only built-ins`.
- [ ] **Step 5: Archive the specs** as the final commit:
  `git mv specs/2026-11-11-untrusted-manifest-read-{design,plan}.md specs/archive/2026/`, then commit
  `docs(specs): archive the untrusted-repository specs` with a `Release-Notes: skip` trailer.
