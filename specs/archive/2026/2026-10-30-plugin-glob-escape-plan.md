# Plugin Glob Escape Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `[`, `]`, `*` or `?` in a venv or project path no longer hides plugins, argparse commands or
`build-manifest` packages (F-07).

**Architecture:** Each of the three glob call sites escapes its literal path prefix and
appends the wildcard tail unescaped.

**Tech Stack:** Rust, `glob` 0.3, `assert_cmd`.

> **Superseded detail:** escaping the whole prefix with `glob::Pattern::escape` broke Windows `\\?\` paths. The
> shipped code escapes per component with `toolr_core::glob_path::escape_path`; see the design's Windows note.

**Spec:** [`specs/archive/2026/2026-10-30-plugin-glob-escape-design.md`](./2026-10-30-plugin-glob-escape-design.md)

## Global Constraints

- Escape only the literal part. `scan_paths` entries stay globs.
- No schema change.
- Test directory names: `[wip]` on every platform. `*` and `?` are illegal in Windows file names, so tests using them
  are `#[cfg(unix)]`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Hash and glob agree.** `compute_third_party_hash` must cover the fragments under a bracketed venv, or the manifest
   stays "fresh" forever. Task 1, `third_party_hash_sees_fragments_under_bracketed_venv`.
2. **`scan_paths` wildcards still work under a bracketed root.** Escaping the whole joined string would break them. Task
   1, `scan_paths_glob_under_bracketed_root`.
3. **End to end, through the binary.** Unit tests on `glob_manifests` don't prove the freshness path picks it up. Task
   1, `plugin_found_under_bracketed_repo_path`.
4. **A bracket that matches by accident.** `[ab]` as a directory name, with a sibling `a`, must not match the sibling.
   Task 1, `glob_ignores_sibling_matched_by_bracket_class` (unix-only setup is fine).
5. **Package name with a metachar** isn't a real package name, so no test. Escape it anyway.

---

### Task 1: Escape the three call sites

**Files:**

- Modify: `crates/toolr-core/src/third_party/glob.rs` (`glob_manifests`)
- Modify: `crates/toolr-core/src/argparse/scan.rs` (`scan_block_paths`, ~551-575)
- Modify: `crates/toolr/src/build_manifest_resolve.rs` (`find_in_venv`, ~73-95)
- Test: `crates/toolr-core/src/third_party/tests.rs`, `crates/toolr-core/src/manifest_build/hash.rs` tests (or
  `tests.rs` if hash has none), `scan.rs` `mod tests`, `build_manifest_resolve.rs` `mod tests`,
  `crates/toolr/tests/plugin_warnings.rs`

**Interfaces:** none produced. Signatures don't change.

- [ ] **Step 1: Write the tests:**
    - `third_party/tests.rs`: add `setup_fake_venv_at(root: &Path, packages)`, which builds
      `root/lib/python3.13/site-packages/<pkg>/toolr-manifest.json`. Make `setup_fake_venv` call it.
        - `glob_finds_fragments_under_bracketed_venv`: venv at `tmp/[wip]/venv` with packages `a_pkg` and `b_pkg` →
          `glob_manifests` returns both.
        - `glob_ignores_sibling_matched_by_bracket_class` (`#[cfg(unix)]`): venvs at `tmp/[ab]/venv` (one package) and
          `tmp/a/venv` (a different package) → globbing `tmp/[ab]/venv` returns only its own fragment.
        - `glob_finds_fragments_under_star_and_question_venv` (`#[cfg(unix)]`): venv at `tmp/x*y?/venv` → finds its
          fragment.
    - Hash: `third_party_hash_sees_fragments_under_bracketed_venv`: a hash of a bracketed venv with one fragment differs
      from the hash of an empty bracketed venv.
    - `scan.rs`: `scan_paths_glob_under_bracketed_root`: the existing `scan_paths_expands_globs_and_skips_unparsable`
      fixture, rooted at `tmp/[wip]/project`, with the same `apps/*/management/commands/*.py` pattern → `migrate` and
      `runserver` found.
    - `build_manifest_resolve.rs`: `find_in_venv_under_bracketed_venv`:
      `tmp/[wip]/venv/lib/python3.13/site-packages/pkg` → found.
    - `plugin_warnings.rs`: add `Project::under(subdir: &str)`, which works like `new()` but puts the project at
      `tmp/<subdir>`. Keep the `TempDir` in the struct. Store the root separately; `tools()` and `current_dir` use it.
        - `plugin_found_under_bracketed_repo_path`: `Project::under("[wip]/repo")`,
          `add_plugin("good_plugin", &fragment(2, "good_plugin", "good", None, "run"))` → `--help` stdout contains
          `good`, and the manifest contains `good_plugin.commands`.

- [ ] **Step 2: Run them and confirm they fail.**
      `cargo test -p toolr-core --lib third_party:: manifest_build:: argparse::scan` and
      `cargo test -p toolr --lib build_manifest_resolve` and
      `cargo test -p toolr --test plugin_warnings plugin_found_under_bracketed_repo_path` → the new tests fail (empty
      results), and the old ones pass.

- [ ] **Step 3: Implement.** In each site, convert the literal prefix to `&str` (keeping the existing non-UTF-8 error),
      pass it through `glob::Pattern::escape`, then append the tail. `glob_manifests` builds the tail from
      `candidate_layouts()` as it does now. `scan_block_paths` appends `/` and the user pattern. `find_in_venv` escapes
      `package` too. Update the `glob.rs` module doc to say the venv prefix is escaped.

- [ ] **Step 4: Run** the Step 2 commands → PASS. Then `git grep -n 'glob(' crates/` → every hit escapes its literal
      prefix.

- [ ] **Step 5: Commit.** `fix(plugins): escape venv and project paths before globbing`

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 3:**
      `git mv specs/2026-10-30-plugin-glob-escape-design.md specs/2026-10-30-plugin-glob-escape-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive plugin glob escape design and plan`.
