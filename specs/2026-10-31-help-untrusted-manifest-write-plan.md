# Help Untrusted Manifest Write Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In an untrusted repository, the pre-clap bootstrap builds the manifest in memory and never writes
`tools/.toolr-manifest.json` (F-27).

**Architecture:** `trust::is_trusted_now` is a silent, non-prompting trust query. Both bootstrap steps compute
`persist` from it. When it is `false`, they skip the write and the rebuild lock and hand the in-memory manifest to
`main.rs`.

**Tech Stack:** Rust, `assert_cmd`, `tempfile`.

**Spec:**
[`specs/2026-10-31-help-untrusted-manifest-write-design.md`](./2026-10-31-help-untrusted-manifest-write-design.md)

## Global Constraints

- `is_trusted_now` never prompts, writes, prints or creates a directory; any error → `false`.
- `TOOLR_TRUST_ALL=1` and trusted repos behave exactly as before.
- Explicit builds (`project manifest rebuild`, `self build-manifest`, `run_build_static_manifest`) are unchanged.
- No schema change.
- Trust tests use `common::TrustEnv` (it strips `TOOLR_TRUST_ALL` and isolates state, cache and data dirs).
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **A refused dispatch.** `toolr demo hello` in an untrusted repo, with stdin not a terminal, must exit non-zero with
   the refusal and leave no manifest. Task 1, `refused_dispatch_does_not_write_manifest`.
2. **A stale manifest in an untrusted repo.** Bytes and mtime unchanged; help still shows the current commands. Task 1,
   `stale_manifest_in_untrusted_repo_is_not_rewritten`.
3. **An unusable trust store.** No state dir, or a store inside the repo, means "don't persist" and no new stderr
   output on `--help`. Task 1, `is_trusted_now_is_false_when_store_unusable`.
4. **Trusted repos keep caching.** Task 1, `trusted_repo_help_still_writes_manifest` and
   `trust_all_env_help_writes_manifest`.
5. **Help in an untrusted repo still lists user commands** with no venv. The existing `untrusted_repo.rs` test
   `help_works_with_no_venv_and_shows_first_party_commands` runs with `TOOLR_TRUST_ALL`, so add an untrusted twin. Task
   1, `root_help_in_untrusted_repo_does_not_write_manifest` asserts the stdout too.

---

### Task 1: Gate manifest persistence on trust

**Files:**

- Modify: `crates/toolr-core/src/trust.rs` (`is_trusted_now`, `mod tests`)
- Modify: `crates/toolr/src/bootstrap.rs` (`ensure_manifest_present_or_bootstrap`, `ensure_manifest_fresh`, doc
  comments)
- Modify: `crates/toolr/tests/common/mod.rs` (`hostile_repo` seeding call)
- Test: `crates/toolr/tests/repo_trust.rs`

**Interfaces:**

- Produces: `pub fn is_trusted_now(root: &Path) -> bool` in `toolr_core::trust`.
- Bootstrap signatures are unchanged.
- [ ] **Step 1: Write the tests.**
    - `trust.rs` `mod tests` (use `TrustStore::at` plus a testable inner
      `fn is_trusted_with(root: &Path, store: Option<&TrustStore>, all_env: bool) -> bool` that `is_trusted_now`
      wraps):
        - `is_trusted_now_true_for_recorded_root`: `store.trust(root)` → `true`.
        - `is_trusted_now_false_for_unknown_root`: empty store → `false`, and the store dir was not created.
        - `is_trusted_now_is_false_when_store_unusable`: `store = None` → `false`; a store under `root` → `false`
          (guard error).
        - `is_trusted_now_honours_trust_all`: `all_env = true`, empty store → `true`.
    - `repo_trust.rs`, each with `TrustEnv::new()` and a repo holding `tools/pyproject.toml` (`[project]`
      name/version) and `tools/demo.py` (`command_group("demo", "Demo commands")` with a documented `hello`):
        - `root_help_in_untrusted_repo_does_not_write_manifest`: `env.toolr(root).arg("--help")` → success, stdout
          contains `demo`; `tools/.toolr-manifest.json` does not exist. Same for bare `env.toolr(root)`.
        - `refused_dispatch_does_not_write_manifest`: `env.toolr(root).args(["demo", "hello"])` with stdin null →
          failure, stderr contains `refusing to run code from an untrusted repository`; no manifest file.
        - `stale_manifest_in_untrusted_repo_is_not_rewritten`: write a schema-2 manifest with `static_hash: "stale"`
          and no groups; record bytes and mtime; `--help` → stdout contains `demo`; bytes and mtime unchanged.
        - `trusted_repo_help_still_writes_manifest`: `env.toolr(root).args(["project", "trust"])` then `--help` → the
          manifest file exists and contains `"demo"`.
        - `trust_all_env_help_writes_manifest`: `env.toolr(root).env("TOOLR_TRUST_ALL", "1").arg("--help")` → the
          manifest exists.
- [ ] **Step 2: Run.** `cargo test -p toolr-core --lib trust::` (doesn't compile until `is_trusted_now` exists) and
      `cargo test -p toolr --test repo_trust` → the three untrusted tests FAIL (manifest written), the trusted ones
      pass.
- [ ] **Step 3: Implement.**
    - `trust.rs`: `is_trusted_with` and `is_trusted_now` as above. Canonicalise `root` the way `require_trust` does,
      and map a failure to `false`.
    - `bootstrap.rs`: in `ensure_manifest_present_or_bootstrap`, after the pyproject check, return `Ok(())` when
      `!toolr_core::trust::is_trusted_now(&root)`. In `ensure_manifest_fresh`, compute `persist` after the skip check.
      When it is `false`, call `check()` directly. On `None` (fresh) return `Ok(fresh_cache)`. On
      `Some((verdict, cached))`, run `try_rebuild` and return `Ok(Some(fresh))`. On error, run `warn_and_keep_cache`
      and return `Ok(cached)`. Never call `write_manifest` or `rebuild_if_stale`. Update both doc comments to say
      untrusted repos are built in memory only.
    - `common/mod.rs::hostile_repo`: add `.env("TOOLR_TRUST_ALL", "1")` to the seeding `--help` command, with a comment
      that the seed must persist.
- [ ] **Step 4: Run** Step 2 → PASS. Then `cargo test -p toolr --test repo_trust --test untrusted_repo --test
      trust_commands --test freshness_dispatch --test plugin_warnings --test manifest_concurrency` → PASS.
- [ ] **Step 5: Commit.** `fix(security): don't write the manifest into an untrusted repository`

### Task 2: Docs, release notes, verification, archive

**Files:**

- Modify: `docs/concepts.md` (section "Repository trust")
- Modify: `docs/internals/manifest.md` ("Toolr regenerates the manifest when")
- Modify: `UNRELEASED.md`
- [ ] **Step 1:** Add the two doc sentences from the spec's "Scope and compatibility".
- [ ] **Step 2:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 3:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green. Commit `docs(security): note that untrusted repos get no manifest file`.
- [ ] **Step 4:** Move both specs to `specs/archive/2026/` with `git mv`, fix the plan's `Spec:` link and set the
      design's status to implemented. Commit `docs(specs): archive help untrusted manifest write design and plan`.
