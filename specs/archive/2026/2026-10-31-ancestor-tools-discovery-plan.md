# Ancestor Tools Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An unrelated `tools/` directory below the repo root no longer hides the repo's commands (F-12).

**Architecture:** `discover_project_root` returns the nearest ancestor whose `tools/pyproject.toml` is a file. A
`tools/` without one is not a toolr project; finding none is an error that names the bare `tools/`.

> **Revised during review.** The tasks below describe the first implementation, which preferred a `tools/` marked by
> `pyproject.toml` or `.toolr-manifest.json`, later bounded by `.git`. Review rejected that and two `__init__.py`-based
> rules in turn. The shipped rule requires `tools/pyproject.toml`, a breaking change that also removes the
> no-pyproject interpreter fallback. See the design's "Decision" and "Rejected alternatives".

**Tech Stack:** Rust, `tempfile`, `assert_cmd`.

**Spec:** [`specs/archive/2026/2026-10-31-ancestor-tools-discovery-design.md`](./2026-10-31-ancestor-tools-discovery-design.md)

## Global Constraints

- Markers: `tools/pyproject.toml` or `tools/.toolr-manifest.json`, checked with `is_file()`. `*.py` files are never a
  marker.
- `discover_project_root(start: &Path) -> Result<PathBuf, DiscoveryError>` keeps its signature, its error variants and
  their messages.
- Only `crates/toolr-core/src/discovery.rs` changes in production code.
- No schema change.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Repos without `tools/pyproject.toml` (#588).** A bare `tools/` with no marked ancestor must still resolve, or
   `project manifest rebuild` can't build the first manifest. Task 1, `falls_back_to_nearest_bare_tools_dir`.
2. **Nested toolr repos.** The nearest marked `tools/` wins, not the outermost. Task 1,
   `nearest_marked_ancestor_wins`.
3. **A marker that is a directory.** `tools/pyproject.toml/` is not a marker. Task 1,
   `directory_named_like_a_marker_is_not_a_marker`.
4. **End to end through the binary.** Unit tests don't prove the bootstrap and dispatch use the new root. Task 1,
   `user_group_found_below_unrelated_tools_dir`.
5. **Start directory is itself an unmarked `tools/` parent.** The repro shape: start at the directory that holds the
   unrelated `tools/`. Covered by `skips_unmarked_tools_dir_for_marked_ancestor`, which starts at `src/pkg`.

---

### Task 1: Prefer marked `tools/` directories

**Files:**

- Modify: `crates/toolr-core/src/discovery.rs` (`discover_project_root`, module doc, `mod tests`)
- Create: `crates/toolr/tests/project_discovery.rs`

**Interfaces:**

- Produces: `fn is_marked_tools_dir(tools: &Path) -> bool` (private). `discover_project_root` signature unchanged.

- [ ] **Step 1: Write the unit tests** in `discovery.rs` `mod tests`:
    - `skips_unmarked_tools_dir_for_marked_ancestor`: `tmp/tools/pyproject.toml`, `tmp/src/pkg/tools/__init__.py`;
      `discover_project_root(tmp/src/pkg)` → `tmp` (canonicalised).
    - `manifest_file_marks_a_tools_dir`: `tmp/tools/.toolr-manifest.json` (no pyproject), `tmp/a/tools/` empty; start
      `tmp/a` → `tmp`.
    - `falls_back_to_nearest_bare_tools_dir`: `tmp/tools/demo.py`, `tmp/a/tools/x.py`; start `tmp/a/b` → `tmp/a`.
      Guard it with the existing `any_ancestor_has_tools` skip, since a marked `/tools` on the host would win.
    - `nearest_marked_ancestor_wins`: `tmp/tools/pyproject.toml` and `tmp/sub/tools/pyproject.toml`; start
      `tmp/sub/x` → `tmp/sub`.
    - `directory_named_like_a_marker_is_not_a_marker`: `tmp/tools/pyproject.toml` as a file,
      `tmp/a/tools/pyproject.toml/` as a directory; start `tmp/a` → `tmp`.
    - Keep `finds_tools_in_current_dir`, `finds_tools_in_ancestor` and `returns_not_found_when_no_tools_dir_exists`
      unchanged. They pass through the fallback.

- [ ] **Step 2: Write the integration test** `crates/toolr/tests/project_discovery.rs` (`mod common;`):
    - `user_group_found_below_unrelated_tools_dir`: the spec's repro. `tools/pyproject.toml` (`[project]` name/version),
      `tools/demo.py` with `group = command_group("demo", "Demo commands")` and a documented `hello` command,
      `src/pkg/tools/__init__.py` empty. `common::toolr().current_dir(src/pkg).arg("--help")` → success, stdout
      contains `demo`.
    - `manifest_rebuild_below_unrelated_tools_dir_writes_root_manifest`: same tree,
      `common::toolr().current_dir(src/pkg).args(["project", "manifest", "rebuild"])` → success; stdout contains the
      root's `tools/.toolr-manifest.json` path; `src/pkg/tools/.toolr-manifest.json` does not exist.

- [ ] **Step 3: Run them and confirm the new ones fail.**
      `cargo test -p toolr-core --lib discovery::` → `skips_unmarked_tools_dir_for_marked_ancestor`,
      `manifest_file_marks_a_tools_dir`, `nearest_marked_ancestor_wins` (if it resolves the inner bare dir) and
      `directory_named_like_a_marker_is_not_a_marker` fail. `cargo test -p toolr --test project_discovery` → both fail.

- [ ] **Step 4: Implement.** Add `is_marked_tools_dir(tools: &Path) -> bool`: `tools.join("pyproject.toml").is_file()
      || tools.join(".toolr-manifest.json").is_file()`. In the walk, when `current.join("tools").is_dir()`, return
      `current` if the dir is marked; otherwise store it in `first_bare: Option<PathBuf>` if that's still `None`. At the
      filesystem root, return `first_bare` or `NotFound`. Update the doc comment and the module doc to describe the
      marker rule and the fallback.

- [ ] **Step 5: Run** the Step 3 commands → PASS. Then `cargo test -p toolr-core` and
      `cargo test -p toolr --test project_venv_sync --test repo_trust --test trust_commands` → PASS.

- [ ] **Step 6: Commit.** `fix(discovery): skip unrelated tools/ directories when finding the project root`

### Task 2: Docs, release notes, verification, archive

**Files:**

- Modify: `docs/concepts.md` (section "The `tools/` directory")
- Modify: `UNRELEASED.md`
- [ ] **Step 1:** In `docs/concepts.md`, after the first paragraph of "The `tools/` directory", add: toolr finds the
      directory by walking up from the current directory. It uses the nearest `tools/` that contains `pyproject.toml`
      or `.toolr-manifest.json`, so an unrelated `tools/` package deeper in the tree is ignored. If none is marked, it
      uses the nearest `tools/`.
- [ ] **Step 2:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 3:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green.
- [ ] **Step 4:** Commit `docs(discovery): document how toolr finds tools/`.
- [ ] **Step 5:**
      `git mv specs/2026-10-31-ancestor-tools-discovery-design.md specs/2026-10-31-ancestor-tools-discovery-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive ancestor tools discovery design and plan`.
