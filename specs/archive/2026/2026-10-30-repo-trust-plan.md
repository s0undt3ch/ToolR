# Repository Trust Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** toolr refuses to run a repository's code until the user has trusted its canonical root once. Trust replaces
the interpreter provenance gate.

**Architecture:** A new `toolr_core::trust` module owns the store (one JSON file per repo under a state dir), the
decision flow (`TOOLR_TRUST_ALL`, then record, then cache-`meta.json` migration, then TTY prompt, then refuse), and the
prompt. Three check points call `require_trust`. New subcommands live under `project` and `self`. The provenance module
is deleted.

**Tech Stack:** Rust (toolr-core, toolr binary, clap), `assert_cmd` integration tests, pytest, GitHub composite action,
mkdocs.

**Spec:** [`specs/2026-10-30-repo-trust-design.md`](./2026-10-30-repo-trust-design.md)

## Global Constraints

- Trust key = canonical repo root from `discover_project_root`. No content hashing.
- Bypass env var: `TOOLR_TRUST_ALL=1`, exactly `"1"`, mirroring `TOOLR_AUTO_INSTALL_UV`. Never infer from `CI`.
- Store: `$XDG_STATE_HOME/toolr/trust/`, else `dirs::state_dir()/toolr/trust/`, else `toolr_data_dir()/trust/`. Never
  under `toolr_cache_dir()`.
- Record file: `<blake3 hex of canonical root as OS bytes>.json`, `{"schema_version": 1, "repo_path", "trusted_at",
  "toolr_version"}`. Tempfile + rename; dir 0700, file 0600 on Unix.
- Prompt and refusal text: verbatim from the spec's "Prompt and error text" section. Prompt default is **No**; EOF is
  No.
- Migration reads only `meta.json` files under `toolr_cache_dir()`, never inside the repo.
- No new top-level CLI names. Commands: `toolr project trust [PATH]`, `toolr project untrust [PATH]`, `toolr self
  trusted [--prune]`.
- Precondition: F-01 (bash `compgen -W`) is fixed on `main` or lands in the same release. Check before opening the PR.
- Conventional Commits; the user-visible commit is `feat(security): …`. No `Co-Authored-By`. Stage by path, never `git
  add -A`. `rustfmt <file>` only on touched files.

## Review Focus

1. **Repo without `tools/pyproject.toml`** (host-Python path) must be refused when untrusted. Task 3, test
   `untrusted_repo_without_pyproject_is_refused`.
2. **`--quiet venv sync` in an untrusted repo** must exit non-zero, not be swallowed as benign. Task 3, test
   `quiet_sync_refusal_is_not_swallowed`.
3. **Symlinked checkout path** (`~/link -> /real/repo`): trusting through the link must trust the real root, and running
   from either path must pass. Task 1, test `trust_key_is_canonical_through_symlink`.
4. **`project init` in an empty dir** must succeed without `TOOLR_TRUST_ALL`, and leave the repo trusted. Task 3, test
   `project_init_records_trust_before_sync`.
5. **Store dir inside the repo** (`XDG_STATE_HOME=<repo>/state`) must refuse rather than let the repo pre-seed trust.
   Task 1, test `store_inside_repo_is_rejected`.

---

### Task 1: Trust store and decision logic in toolr-core

**Files:**

- Create: `crates/toolr-core/src/trust.rs` (+ `pub mod trust;` in `crates/toolr-core/src/lib.rs`)
- Modify: `crates/toolr-core/src/uv/mod.rs` (add `toolr_state_dir()` beside `toolr_data_dir`/`toolr_cache_dir`,
  ~170-188)

**Interfaces (produced):**

- `pub fn toolr_state_dir() -> Option<PathBuf>` in `uv/mod.rs`. Follows the Global Constraints fallback order.

- In `trust.rs`:
    - `pub struct TrustRecord { pub schema_version: u32, pub repo_path: PathBuf, pub trusted_at: DateTime<Utc>, pub
      toolr_version: String }` (serde)
    - `pub struct TrustStore { dir: PathBuf }` with:
        - `pub fn open_default() -> Result<Self>`
        - `pub fn at(dir: PathBuf) -> Self`
        - `pub fn is_trusted(&self, root: &Path) -> Result<bool>`
        - `pub fn trust(&self, root: &Path) -> Result<TrustRecord>`
        - `pub fn untrust(&self, root: &Path) -> Result<bool>` (returns whether a record existed)
        - `pub fn list(&self) -> Result<Vec<TrustRecord>>`
    - `pub struct TrustMode { pub never_prompt: bool }`
    - `#[derive(thiserror::Error)] pub enum TrustError { Untrusted(PathBuf), StoreInsideRepo(PathBuf) }`. `Untrusted`'s
      `Display` is the spec's refusal text.
    - `pub fn decide(root: &Path, store: &TrustStore, cache_root: Option<&Path>, all_env: bool, mode: TrustMode,
      prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>) -> Result<(), TrustError>`: the pure decision flow from the
      spec, steps 1-5.
    - `pub fn require_trust(root: &Path, mode: TrustMode) -> anyhow::Result<()>`: canonicalises `root`, opens the
      default store, reads `TOOLR_TRUST_ALL`, builds `prompt_io` from a stdin TTY check, and calls `decide`. Mirror
      `decide_install_auto` in `crates/toolr-core/src/uv/install.rs:115-125`.

- [ ] **Step 1: Write the failing unit tests** in `trust.rs` `#[cfg(test)] mod tests`, using `TempDir` for the store,
      repo and cache root, and `Cursor`/`Vec<u8>` for prompt IO:
    - `untrusted_without_tty_is_refused`: `decide(..., prompt_io: None)` → `Err(TrustError::Untrusted(root))`.
    - `trust_all_env_allows`: `all_env: true` → `Ok`, and no record is written.
    - `recorded_root_is_allowed`: `store.trust(root)` then `decide` → `Ok`.
    - `untrust_removes_record`: after `untrust`, `is_trusted` is `false`, and `untrust` again returns `false`.
    - `prompt_yes_records_trust`: input `"y\n"` → `Ok`, `is_trusted` true; stderr contains `Trust it? [y/N]`.
    - `prompt_default_and_eof_refuse`: inputs `"\n"` and `""` → `Err(Untrusted)`, no record.
    - `never_prompt_refuses_even_on_tty`: `TrustMode { never_prompt: true }` with input `"y\n"` → `Err(Untrusted)`.
    - `cache_meta_migrates`: write a `Meta` with `repo_path = root` into `<cache_root>/<key>/meta.json` → `Ok`, and a
      record now exists.
    - `meta_inside_repo_is_ignored`: write `<root>/tools/meta.json` with `repo_path = root`, empty cache root →
      `Err(Untrusted)`.
    - `trust_key_is_canonical_through_symlink` (unix): trust via symlink path; `is_trusted(real)` is true.
    - `store_inside_repo_is_rejected`: `TrustStore::at(root.join("state"))` → `decide` returns `Err(StoreInsideRepo)`.
    - `record_file_permissions` (unix): file mode `0o600`, dir mode `0o700`.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p toolr-core --lib trust::` → compile errors.

- [ ] **Step 3: Implement `toolr_state_dir`, `TrustStore`, `decide`, `require_trust`.** Use `crate::hash` (blake3) for
      the file name over `root.as_os_str().as_encoded_bytes()`. For migration, use
      `crate::cache::enumerate_caches(cache_root)` and compare `meta.repo_path` with the canonical root. Write the
      record via tempfile in the store dir, then `rename`.

- [ ] **Step 4: Run.** `cargo test -p toolr-core --lib trust::` → PASS.

- [ ] **Step 5: Commit.** `feat(security): add repository trust store`

### Task 2: Remove the interpreter provenance gate

**Files:**

- Delete: `crates/toolr-core/src/venv/provenance.rs` (+ its `mod`/re-exports in `crates/toolr-core/src/venv/mod.rs`)
- Modify: `crates/toolr/src/dispatch.rs` (~305-320, the `verify_interpreter` block)
- Modify: `crates/toolr-core/src/project.rs` `finalize_sync` (~101-120): stop setting `interpreter_*`; delete
  `provenance_cache_dir` if it has no other caller
- Modify: `crates/toolr-core/src/cache/meta.rs` (~23, ~44-52): mark `interpreter_path`/`interpreter_hash` deprecated in
  their doc comments; keep `#[serde(default)]`; keep schema v2
- Modify: tests that assert `interpreter_*` is recorded (`crates/toolr-core/src/project.rs:268-269, 305`)
- [ ] **Step 1:** Delete the module and the dispatch call. Remove the `interpreter_*` writes. Update the `project.rs`
      tests to assert the fields are `None` for new syncs.
- [ ] **Step 2:** `cargo test -p toolr-core && cargo test -p toolr --lib` → PASS. Ignore
      `crates/toolr/tests/untrusted_repo.rs`; Task 3 rewrites it.
- [ ] **Step 3: Commit.** `refactor(venv): drop interpreter provenance gate`

### Task 3: Check points, commands, `init`

**Files:**

- Modify: `crates/toolr-core/src/project.rs` `ensure_venv_ready` (~48-66): add a `trust: TrustMode` field to
  `EnsureOpts` and call `require_trust(&repo_root, opts.trust)` right after `discover_project_root`
- Modify: `crates/toolr/src/dispatch.rs`: call `require_trust(&repo_root, …)` before `write_spec_to_tempfile` (~243)
- Modify: `crates/toolr/src/project.rs`: `venv_run` `--no-sync` arm (~576), after `discover_project_root`;
  `project_init` (~53): `TrustStore::open_default()?.trust(&root)` after files are written, before the sync; wire
  `project trust` and `project untrust`; check `venv_sync_unattended_quiet_exit` (~437-465) cannot match the refusal
- Modify: `crates/toolr/src/cli.rs`: add `trust` and `untrust` under `Command::new("project")` (~240), and `trusted`
  with `--prune` under `Command::new("self")` (~466). Builtin completions derive from this tree.
- Modify: `crates/toolr/src/dispatch.rs` `self` match (~400): route `trusted`
- Modify: `.cargo/config.toml`: add `[env]` with `TOOLR_TRUST_ALL = "1"`
- Modify: `tests/conftest.py` (where it sets subprocess env, ~28): set `TOOLR_TRUST_ALL=1`
- Rewrite: `crates/toolr/tests/untrusted_repo.rs`
- Create: `crates/toolr/tests/repo_trust.rs`

**Interfaces:**

- Consumes: Task 1's `require_trust`, `TrustMode`, `TrustStore`.

- `TrustMode.never_prompt` = the same value the call site already passes as `ConsentMode.never_prompt` (`--quiet`).

- [ ] **Step 1: Write the integration tests.** Every test calls `.env_remove("TOOLR_TRUST_ALL")` and sets
      `XDG_STATE_HOME` and `XDG_CACHE_HOME` to tempdirs. Reuse the hostile-repo-with-sentinel helper from
      `untrusted_repo.rs`.
    - `untrusted_dispatch_is_refused`: hostile repo with `tools/pyproject.toml`; `toolr <group> <cmd>` exits non-zero;
      stderr contains `refusing to run code from an untrusted repository`; sentinel absent.
    - `untrusted_repo_without_pyproject_is_refused`: same without `tools/pyproject.toml`; sentinel absent.
    - `quiet_sync_refusal_is_not_swallowed`: `toolr project venv sync --quiet` → non-zero.
    - `venv_run_no_sync_is_refused`: `toolr project venv run --no-sync -- python -c 1` → non-zero with the refusal text.
    - `help_and_complete_need_no_trust`: `toolr --help` and `toolr __complete <root> ""` succeed; sentinel absent.
    - `trust_then_dispatch_runs`: `toolr project trust` exits 0 and prints the canonical root; then `toolr self trusted`
      lists it.
    - `untrust_then_refused`: trust, untrust, dispatch → refused.
    - `trusted_prune_drops_missing_paths`: trust a tempdir, delete it, `toolr self trusted --prune` → list is empty.
    - `trust_all_env_bypasses`: `.env("TOOLR_TRUST_ALL", "1")` → dispatch gets past the trust check (assert stderr does
      **not** contain the refusal text).
    - `project_init_records_trust_before_sync`: in an empty tempdir, `toolr project init --no-sync` exits 0, then `toolr
      self trusted` lists it. (`--no-sync` avoids needing uv; the ordering is enforced in code review against the spec.)

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p toolr --test repo_trust --test untrusted_repo`.

- [ ] **Step 3: Implement the three check points, `init` trust recording, CLI wiring, and the `self trusted` listing.**
      The listing prints one root per line, as `<root> (missing)` when the path no longer exists.

- [ ] **Step 4: Add `[env] TOOLR_TRUST_ALL = "1"` to `.cargo/config.toml` and `TOOLR_TRUST_ALL=1` to the pytest
      subprocess env.**

- [ ] **Step 5: Run.** `cargo test -p toolr` and `uv run pytest -x -q` → PASS.

- [ ] **Step 6: Commit.** `feat(security): require repository trust before running repo code`

### Task 4: GitHub Action, doc-snippet regen, docs, skills, release notes

**Files:**

- Modify: `action.yml` (~313): also `echo "TOOLR_TRUST_ALL=1" >> "$GITHUB_ENV"`; update the step comment
- Modify: `tools/precommit/docs.py` (~232 and the env dict ending ~259): add `"TOOLR_TRUST_ALL": "1"`
- Modify: `docs/concepts.md`: new "Repository trust" section (what's gated, the commands, `TOOLR_TRUST_ALL`, the "Not
  covered" list from the spec, "trust is a one-way door")
- Modify: the CI docs page that documents `TOOLR_VENV_LOCATION` (find with `git grep -l TOOLR_VENV_LOCATION docs`): add
  `TOOLR_TRUST_ALL`
- Modify: `CONTRIBUTING.md`: one line, "run `toolr project trust` once in your checkout"
- Modify: `skills/toolr-ci-setup/SKILL.md` prose: the Action sets `TOOLR_TRUST_ALL=1`; non-Action CI must set it
- Modify: `skills/toolr-command-authoring/SKILL.md` prose: first run in a repo asks for trust
- Modify: `UNRELEASED.md`: the spec's release text, verbatim
- [ ] **Step 1:** Make the edits.
- [ ] **Step 2: Verify.** `toolr pre-commit regen-doc-snippets` runs clean (no new diffs except expected help output for
      the new subcommands; commit those). Then `cargo xtask build-skill-refs --check && uv run mkdocs build --strict &&
      prek run --all-files` → all pass.
- [ ] **Step 3: Commit.** `docs(security): document repository trust`, plus `ci(action): export TOOLR_TRUST_ALL` as a
      separate commit for `action.yml`.

### Task 5: Full verification and archive

- [ ] **Step 1:** Confirm F-01 is fixed on `main` (or included in this branch). If not, stop and report.
- [ ] **Step 2:** `mise run test`, polling output every 30-60 s. Expected: green.
- [ ] **Step 3:** Manual smoke without the bypass: `env -u TOOLR_TRUST_ALL target/debug/toolr --help` in this repo
      works; `env -u TOOLR_TRUST_ALL target/debug/toolr project venv path` works; a scratch hostile repo is refused.
- [ ] **Step 4:** `git mv specs/2026-10-30-repo-trust-design.md specs/2026-10-30-repo-trust-plan.md
      specs/archive/2026/`; fix the plan's `Spec:` link. Commit `docs(specs): archive repo trust design and plan`.
