# Trust Keyed on the Git Common Dir — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Key repository trust on the git common dir, so trusting any checkout trusts the repository and all its worktrees, without letting a crafted `.git` borrow trust.

**Architecture:** A new pure resolver (`trust/repo_key.rs`) maps a project root to a `RepoKey` (key, display path, project root, repair hint) by reading `.git` and one back-link file. Every `trust.rs` entry point goes through it; records and proofs carry the key. The `toolr` crate's commands print the display path and prune by stored key.

**Tech Stack:** Rust (toolr-core `full` feature, `libc`, `dirs`), real `git` in tests only.

**Spec:** `specs/2026-11-07-trust-git-common-dir-design.md` — read it in full; this plan does not repeat its rules.

## Global Constraints

- No subprocess and no `git` binary at runtime. Tests may run `git`.
- No new dependency in `toolr-core`. `libc` (non-optional) and `dirs` (`full`) already exist.
- `GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`, `GIT_CEILING_DIRECTORIES` never affect the key.
- Every failed rule falls back to per-path (key = canonical project root), never to an error.
- Trust is unreleased: no migration. Bump `RECORD_SCHEMA_VERSION` to `2`; records with another version are skipped as unreadable.
- 99% patch coverage (`cargo xtask patch-coverage --base main`). Close gaps with real tests.
- Conventional Commits; no `Co-Authored-By` or session trailers.
- British English in prose, comments and commit messages.

## Review Focus

1. **Project root nested inside a worktree** (`wt/sub/tools/`, project root `wt/sub`): key is still the common dir. Test in Task 1.
2. **`.git` file and back-link file with CRLF, trailing whitespace or a missing trailing newline** (Windows checkouts, hand edits): parsed the same as git's `gitdir: <path>\n`. Test in Task 1.
3. **Paths with spaces and non-ASCII characters** in the worktree path: resolved and displayed correctly. Test in Task 1.
4. **Project root that no longer exists** (`untrust` or `--prune` after deletion): resolves per-path, no panic, record still removable. Test in Task 2.
5. **Store directory inside the main checkout while running from an external worktree**: guard refuses. Test in Task 2.

---

### Task 1: Repository key resolver

**Model:** opus — security-critical path resolution; every anti-goal lives here. **Reviewer:** opus.

**Files:**
- Create: `crates/toolr-core/src/trust/repo_key.rs`
- Modify: `crates/toolr-core/src/trust.rs` (add `mod repo_key;` and `pub use repo_key::RepoKey;` only)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct RepoKey {
      pub key: PathBuf,           // canonical common dir, or canonical project root (per-path)
      pub display: PathBuf,       // repository path to show (spec "What users see")
      pub project_root: PathBuf,  // canonical project root (input, canonicalised or unchanged if unresolvable)
      pub repair_hint: bool,      // `.git` is a regular file that passed rule 1 but failed rule 2
  }
  impl RepoKey {
      pub fn resolve(project_root: &Path, home: Option<&Path>) -> RepoKey;
  }
  // private, for the ownership test:
  fn resolve_as(project_root: &Path, home: Option<&Path>, euid: Option<u32>) -> RepoKey;
  ```
  `home` is the canonical home directory. `resolve` passes `euid = Some(libc::geteuid())` on Unix, `None` on Windows (check skipped).

- [ ] **Step 1: Write the failing tests** in `repo_key.rs`'s `#[cfg(test)] mod tests`. Build layouts in `TempDir`s; use real `git` (`std::process::Command::new("git")` with `-c user.name=t -c user.email=t@t`, `commit --allow-empty`) for git-made layouts. Each test asserts on `RepoKey` fields:
  - `main_checkout_keys_on_dot_git`: key == canonical `repo/.git`, display == canonical `repo`.
  - `worktree_inside_and_outside_repo_share_key`: `.worktrees/a` and a sibling `TempDir` worktree both key == `repo/.git`, display == `repo`, `repair_hint == false`.
  - `nested_project_root_in_worktree`: `wt/sub` → key == `repo/.git`. (Review Focus 1.)
  - `key_matches_git_rev_parse`: for main checkout, linked worktree and bare-repo worktree, key == canonicalised `git rev-parse --path-format=absolute --git-common-dir`.
  - `relative_paths_worktree`: `git -c worktree.useRelativePaths=true worktree add` → key == `repo/.git`.
  - `same_basename_worktrees`: `p1/same` and `p2/same` → both key == `repo/.git`.
  - `git_worktree_move_keeps_key` and `main_moved_then_repair_restores_key` (run `git worktree repair <wt>` from the moved main).
  - `plain_mv_falls_back_with_hint`: `mv` the worktree → key == project root, `repair_hint == true`.
  - `bare_repo_display_is_the_bare_dir`: bare `foo.git` + worktree → key == display == canonical `foo.git`.
  - `nested_clone_is_its_own_repo`, `submodule_is_per_path` (`git -c protocol.file.allow=always submodule add`), `submodule_worktree_keys_on_module_dir` (key == `super/.git/modules/<name>`).
  - `separate_git_dir_main_is_per_path`.
  - `crafted_gitfile_to_trusted_dot_git`, `crafted_gitfile_to_live_worktree_gitdir`, `crafted_self_contained_worktree` (`.git` → `./g`, `g/gitdir` names the `.git`, `g/commondir` names trusted), `crafted_worktrees_dir_outside_git` (`.git` → `x/worktrees/g` with matching back-link, key must not equal trusted key), `planted_commondir_in_dot_git_dir_ignored` → all: key != trusted repo's key.
  - `symlinked_dot_git_dir_and_file_are_per_path` (`#[cfg(unix)]`).
  - `foreign_owner_is_per_path` (`#[cfg(unix)]`): `resolve_as(.., Some(geteuid() + 1))` → key == project root.
  - `home_dotgit_does_not_cover_projects_below`: fake home `h` with `git init h`; `h/x` → per-path; `h/foo` with own `git init` → key == `h/foo/.git`; `h` itself → key == `h/.git`.
  - `no_home_walks_to_root`: `home = None`, project below a repo → keyed on that repo.
  - `non_git_project_is_per_path`, `missing_project_root_is_per_path`.
  - `gitfile_crlf_and_whitespace` (Review Focus 2): `.git` content `gitdir: <path>\r\n` and back-link with trailing spaces → same key as the clean form.
  - `spaces_and_unicode_in_paths` (Review Focus 3): worktree at `odd dir/wörk tree` → key == `repo/.git`.

- [ ] **Step 2: Run to verify they fail**
  Run: `cargo test -p toolr-core trust::repo_key`
  Expected: compile failure (`RepoKey` not defined).

- [ ] **Step 3: Implement `RepoKey::resolve` / `resolve_as`** following the spec's *Resolving the key* rules in order: canonicalise root (fallback unchanged) → walk up for `.git` via `symlink_metadata`, stopping at the first hit; if the hit's dir `D == home` and `project_root != home` → per-path → symlink → per-path → ownership (`MetadataExt::uid() != euid`) → per-path → dir → key = canonical `E` → regular file → parse `gitdir:` (trim, resolve relative to `D`), canonicalise `G`, require `G.parent().file_name() == "worktrees"`, read `G/gitdir` (trim, resolve relative to `G`), compare `canonical(parent) .join(".git") == D.join(".git")` → key = canonical grandparent; rule 2 failure after rule 1 pass sets `repair_hint`. Display: key file name exactly `.git` → parent, else key.

- [ ] **Step 4: Run to verify they pass**
  Run: `cargo test -p toolr-core trust::repo_key`
  Expected: all PASS.

- [ ] **Step 5: Commit**
  `git commit -m "feat(trust): resolve a project root to its git repository key"`

### Task 2: Store, proofs and the decision flow use the key

**Model:** sonnet — mechanical wiring against a fixed interface. **Reviewer:** opus (store guard is security-relevant).

**Files:**
- Modify: `crates/toolr-core/src/trust.rs`

**Interfaces:**
- Consumes: `RepoKey::resolve(project_root, home)`.
- Produces:
  ```rust
  pub struct TrustRecord { schema_version: u32 /* = 2 */, key: PathBuf, repo_path: PathBuf /* display */, trusted_at, toolr_version }
  pub struct Trusted { key: PathBuf, root: PathBuf /* project root */, granted_at_prompt: bool }
  impl Trusted { pub fn root(&self) -> &Path /* project root */; pub fn covers(&self, root: &Path) -> bool /* compares resolved keys */; }
  pub enum TrustError { Untrusted { repo: PathBuf, repair: Option<PathBuf> /* worktree path when repair_hint */ }, StoreInsideRepo(PathBuf) }
  impl TrustStore {
      pub fn is_trusted(&self, root: &Path) -> Result<bool>;
      pub fn trust(&self, root: &Path) -> Result<TrustRecord>;
      pub fn untrust(&self, root: &Path) -> Result<(PathBuf /* display */, bool /* existed */)>;
      pub fn untrust_key(&self, key: &Path) -> Result<bool>;
      pub fn list(&self) -> Result<Vec<TrustRecord>>; // skips schema_version != 2
  }
  ```
  `is_trusted_now`, `grant_trust`, `require_trust` keep their signatures.

- [ ] **Step 1: Update existing tests and add new ones** in `trust.rs`'s test module. Fix the existing `Untrusted(p)` matches to the struct variant. New tests (use real `git` for repos):
  - `worktree_proof_covers_main_and_keeps_its_own_root`: `grant` from worktree → `proof.root()` == worktree project root; `proof.covers(main)` true; `proof.covers(unrelated)` false.
  - `trust_main_then_worktree_is_trusted` and `untrust_from_worktree_untrusts_main`.
  - `two_projects_in_one_repo_share_trust`.
  - `record_stores_key_and_display`: record `key` == `repo/.git`, `repo_path` == `repo`.
  - `untrust_key_removes_by_stored_key`.
  - `list_skips_old_schema_records`: a v1-shaped JSON file → not listed.
  - `missing_project_root_untrusts_cleanly` (Review Focus 4): trust a non-git dir, delete it, `untrust(path)` → `(path, true)`.
  - `store_in_main_checkout_refused_from_external_worktree` (Review Focus 5): store dir under `main/state`, `decide` from an external worktree → `StoreInsideRepo`.
  - `home_dotgit_project_not_refused_by_guard`: fake home with `git init`, store under it, `~/tools` project → guard passes (home exemption applies to key/display/root equal to home).
  - `untrusted_error_carries_repair_hint`: `mv`-moved worktree → `Untrusted { repair: Some(worktree) }`, and its `Display` contains `git worktree repair` and the worktree path.
  - `prompt_mentions_worktrees`: prompt text contains `worktrees`.

- [ ] **Step 2: Run to verify they fail**
  Run: `cargo test -p toolr-core trust::tests`
  Expected: compile errors / FAIL on the new tests.

- [ ] **Step 3: Implement.** One private helper resolves (`RepoKey::resolve(root, canonical_home)`); `canonical()` is no longer used for trust keys anywhere else. `record_path` hashes `key`. `is_trusted` checks `record.key == key`. `guard` takes the `RepoKey` and checks the store against `key`, `display` and `project_root`, skipping any that equal home. `decide` resolves once and threads the `RepoKey`. `confirm` shows `display` and adds a line that worktrees are covered. `Untrusted` display text: keep the existing refusal sentence, show `repo`, and when `repair` is set add a line telling the user to run `git worktree repair <path>` from the main checkout. Bump `RECORD_SCHEMA_VERSION` to `2`.

- [ ] **Step 4: Run to verify they pass**
  Run: `cargo test -p toolr-core trust`
  Expected: all PASS.

- [ ] **Step 5: Commit**
  `git commit -m "feat(trust): key records and proofs on the repository"`

### Task 3: CLI commands and end-to-end worktree tests

**Model:** sonnet — small caller changes plus integration tests in an existing style. **Reviewer:** sonnet.

**Files:**
- Modify: `crates/toolr/src/project.rs` (`project_untrust`, the `TrustError::Untrusted` construction near line 831), `crates/toolr/src/dispatch.rs` (`run_self_trusted`)
- Create: `crates/toolr/tests/trust_worktrees.rs` (uses `common::TrustEnv`, `common::write_project`)

**Interfaces:**
- Consumes: Task 2's `TrustStore::untrust` → `(display, existed)`, `untrust_key`, `TrustRecord { key, repo_path }`, `TrustError::Untrusted { repo, repair }`.

- [ ] **Step 1: Write the failing integration tests** in `trust_worktrees.rs` (real `git`; `write_project` in the main checkout before committing it):
  - `trust_main_covers_worktrees`: `project trust --no-sync` in main; then `project untrust` from a worktree under `.worktrees/` prints the **main** path (proving the record covered it), re-trust, and the same from an external worktree. The resolver never reads env vars; `git_env_does_not_change_trust` below pins that end to end.
  - `trust_from_worktree_prints_main_path`.
  - `self_trusted_shows_repo_path_and_prunes_by_key`: trust, delete main checkout but keep a worktree → `self trusted` lists it without `(missing)`; delete the worktree and `rm -rf` the common dir → `(missing)`, `--prune` removes it.
  - `bare_repo_listed_as_bare_dir`: `self trusted` prints `…/foo.git`.
  - `git_env_does_not_change_trust`: run toolr in a trusted worktree with `GIT_DIR` and `GIT_COMMON_DIR` pointing at an untrusted repo → still trusted.
  - `moved_worktree_refusal_hints_repair`: refusal stderr contains `git worktree repair`.
- [ ] **Step 2: Run to verify they fail**
  Run: `cargo test -p toolr --test trust_worktrees`
  Expected: FAIL.
- [ ] **Step 3: Implement.** `project_untrust` prints the display path returned by `untrust`. `run_self_trusted` shows `record.repo_path`, tests existence with `record.key.exists()`, prunes with `untrust_key(&record.key)`. Fix the `Untrusted` construction in `project.rs` to the struct variant. Update the existing assertions in `trust_commands.rs` / `repo_trust.rs` only where output legitimately changed (a non-git temp dir still prints its own path).
- [ ] **Step 4: Run to verify they pass**
  Run: `cargo test -p toolr --test trust_worktrees --test trust_commands --test repo_trust --test trust_then_sync --test untrusted_repo --test project_init`
  Expected: all PASS.
- [ ] **Step 5: Commit**
  `git commit -m "feat(cli): show and prune trust by repository"`

### Task 4: Docs and release note

**Model:** haiku — prose edits against exact requirements. **Reviewer:** sonnet.

**Files:**
- Modify: `docs/concepts.md` (§ Repository trust), `docs/cli.md`, `docs/quickstart.md` (only where they describe what is trusted), `release-notes/2026-11-01.00-00-12.repository-trust.md`

- [ ] **Step 1: Edit `docs/concepts.md`.** Replace "Trust is keyed on the repository's canonical root path." with the trust unit as the spec's *Docs and release note* section states: the git repository and all its worktrees (wherever they live); each separate repository (nested clone, submodule) is asked for on its own; a project outside git is trusted by its path. Add one line each: a home directory under git is not the repository for projects below it (trusting `~` never trusts `~/Downloads/foo`); a symlinked `.git` or one owned by another user is trusted only by its own path; `git init --separate-git-dir` checkouts are trusted separately from their worktrees; a worktree moved with plain `mv` asks again until `git worktree repair <path>` is run from the main checkout. Keep "Content isn't hashed…".
- [ ] **Step 2: Align `docs/cli.md` and `docs/quickstart.md`** wherever they say trust is per path.
- [ ] **Step 3: Append to the release note** one sentence: trust covers the whole git repository, including all its worktrees, and untrusting from any checkout untrusts all of them.
- [ ] **Step 4: Regenerate and verify**
  Run: `toolr pre-commit regen-doc-snippets && prek run --all-files && uv run mkdocs build --strict`
  Expected: no changes left by the regen, all hooks pass, docs build clean.
- [ ] **Step 5: Commit**
  `git commit -m "docs(trust): describe repository-wide trust and its worktrees"`

### Task 5: Full verification and archive

**Model:** none — done by the controller.

- [ ] **Step 1:** `mise run test` → all PASS (poll a long `cargo test --workspace` every 30–60 s).
- [ ] **Step 2:** `cargo llvm-cov --workspace --exclude toolr-py --tests --lcov --output-path target/coverage/lcov.info -- --test-threads=1` then `cargo xtask patch-coverage --base main` → ≥ 99 %.
- [ ] **Step 3:** Whole-branch review (fresh reviewer, **opus**).
- [ ] **Step 4:** `git mv specs/2026-11-07-trust-git-common-dir-{design,plan}.md specs/archive/2026/` and commit `docs(specs): archive the trust common-dir spec and plan` with `Release-Notes: skip`.
