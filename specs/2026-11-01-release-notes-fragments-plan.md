# Release-notes fragments implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `UNRELEASED.md` with per-note files in `release-notes/`, managed by a project-local
`toolr release-notes` command group, and gate PRs that ship a user-visible change without a note.

**Architecture:** One module, `tools/release_notes.py`, holds pure helpers (name parsing, file checks,
rendering, the gate decision) plus four thin commands (`add`, `check`, `render`, `clear`) that shell
out to git via `ctx.run`. The release workflow calls `render` and `clear`, CI calls `check --base`,
and a prek hook calls `check`. git-cliff's template is untouched.

**Tech Stack:** Python 3.11+, toolr-py (`command_group`, `Context`), pytest with `toolr.testing.make_context`,
GitHub Actions, git-cliff, prek.

**Spec:** `specs/2026-11-01-release-notes-fragments-design.md`

## Global Constraints

- Branch `feat/release-notes-fragments` is stacked on `fix/release-prek-failures` (#625) with git-spice.
  Restack (`git-spice upstack restack`) if #625 changes; submit with `git-spice branch submit --draft`.
- Note name: `release-notes/<YYYY-MM-DD>.<HH-MM-SS>.<slug>.md`, UTC creation time.
- Slug pattern: `[a-z0-9]+(-[a-z0-9]+)*`.
- `release-notes/README.md` is excluded by name from every subcommand.
- Breaking marker: note content starting with `**Breaking:**`.
- Skip trailer: `Release-Notes: skip`. Existing trailer honoured: `Changelog: skip`.
- Note-only commit convention: `docs(release-notes): …`.
- CI runs `toolr` from `version: latest` (released binary) with the tools venv's locked `toolr-py`. Use
  only toolr-py APIs already used elsewhere in `tools/` (`command_group`, `Context.run/print/error/exit`).
- Python tests: factory fixtures over `tmp_path`, imports at the top of the file. Model on
  `tests/test_tools_version.py` and `tests/test_tools_ci_packslip.py`.
- Never `git add -A`; stage paths explicitly. No `Co-Authored-By` trailer.
- Format only touched files; no whole-crate `cargo fmt`.

## Review Focus

1. **A note with Windows line endings or a trailing blank line** renders identically to an LF note,
   and doesn't trip the heading or empty checks. Test in Task 1 (`test_render_normalises_crlf`).
2. **`check --base` with an unresolvable ref** (typo, shallow clone without the base fetched) fails
   loudly instead of passing on an empty range. Test in Task 4 (`test_gate_unknown_base_fails`).
3. **A branch named `main`, `HEAD` (detached), or `fix/` with nothing after the slash** gives a clear
   `add` error instead of writing `….main.md` or `….md`. Test in Task 2
   (`test_slug_from_branch_rejects_unusable`).
4. **A note body containing the env-file delimiter**, or a `#` line inside a fenced code block, renders
   correctly: the delimiter is regenerated, the fenced `#` isn't a heading. Tests in Task 1
   (`test_github_env_block_avoids_delimiter_in_value`) and Task 3 (`test_heading_inside_fence_ok`).
5. **A PR whose only `feat` commit was reverted or squashed into a `Release-Notes: skip` commit**
   still respects the trailer anywhere in range. Test in Task 4 (`test_gate_trailer_in_any_commit`).

---

### Task 1: Note model and `render`

**Files:**

- Create: `tools/release_notes.py`
- Test: `tests/test_tools_release_notes.py`

**Interfaces:**

- Produces:
    - `NOTES_DIR: Final[str] = "release-notes"`; `README_NAME: Final[str] = "README.md"`
    - `NOTE_NAME_RE: Final[re.Pattern[str]]` =
      `^(?P<date>\d{4}-\d{2}-\d{2})\.(?P<time>\d{2}-\d{2}-\d{2})\.(?P<slug>[a-z0-9]+(?:-[a-z0-9]+)*)\.md$`
    - `BREAKING_PREFIX: Final[str] = "**Breaking:**"`
    - `group = command_group("release-notes", "Manage queued release notes", docstring=__doc__)`
    - `_note_paths(notes_dir: Path) -> list[Path]` — every entry except `README.md`, sorted by name.
    - `_read_note(path: Path) -> str` — CRLF→LF, `.strip()`.
    - `_render(notes_dir: Path) -> str`
    - `_github_env_block(name: str, value: str) -> str`
    - command `render(ctx: Context, github_env: str | None = None) -> None`
- [ ] **Step 1: Write failing tests** with a `notes_dir` factory fixture `(files: dict[str, str]) -> Path`
  writing into `tmp_path / "release-notes"`:
    - `test_render_empty_dir` → `_render(dir) == ""`; also when only `README.md` exists.
    - `test_render_excludes_readme` → README text absent from output.
    - `test_render_breaking_first` → files `2026-01-01.00-00-00.a.md` ("A"),
      `2026-01-02.00-00-00.b.md` ("**Breaking:** B"), `2026-01-03.00-00-00.c.md` ("C") render
      `"**Breaking:** B\n\nA\n\nC"`.
    - `test_render_normalises_crlf` → `"One\r\n\r\nTwo\r\n\r\n"` renders as `"One\n\nTwo"`.
    - `test_github_env_block_format` → output matches `f"TOOLR_RELEASE_NOTES<<{d}\nvalue\n{d}\n"` for the
      delimiter `d` it chose; `d` is not empty.
    - `test_github_env_block_avoids_delimiter_in_value` → monkeypatch `secrets.token_hex` to return
      `"X"` then `"Y"`; value `"aXb"` yields delimiter `TOOLR_RELEASE_NOTES_Y` (delimiter is
      `f"TOOLR_RELEASE_NOTES_{token}"`).
    - `test_render_command_stdout` and `test_render_command_github_env` via
      `make_context(repo_root=tmp_path)`: stdout contains the rendered text; with `github_env=str(file)`
      the file gains the block (appended, existing content kept) and stdout is empty.
- [ ] **Step 2: Run** `uv run pytest tests/test_tools_release_notes.py -v` — expect import errors.
- [ ] **Step 3: Implement** the interfaces above. `render` first calls `_file_problems` (Task 3) — until
  Task 3 lands, have it call nothing; Task 3 adds the call. Paths resolve against `ctx.repo_root`.
- [ ] **Step 4: Run** the tests — expect PASS.
- [ ] **Step 5: Commit** `feat(tools): add release-notes render command` (stage the two files).

### Task 2: `add` and `clear`

**Files:**

- Modify: `tools/release_notes.py`
- Test: `tests/test_tools_release_notes.py`

**Interfaces:**

- Consumes: `NOTES_DIR`, `NOTE_NAME_RE`, `_note_paths` (Task 1).
- Produces:
    - `_slug_from_branch(branch: str) -> str | None` — strip one leading `<word>/`, lowercase, replace
      runs of non-`[a-z0-9]` with `-`, trim `-`; return `None` if empty or if branch is `HEAD`, `main`.
    - `_note_filename(now: datetime, slug: str) -> str` → `f"{now:%Y-%m-%d}.{now:%H-%M-%S}.{slug}.md"`.
    - `_find_by_slug(notes_dir: Path, slug: str) -> Path | None`
    - command `add(ctx, slug: str | None = None, text: str | None = None, append: bool = False) -> None`
    - command `clear(ctx) -> None`
- [ ] **Step 1: Write failing tests:**
    - `test_slug_from_branch` parametrized: `fix/ctx-run-input-deadlock`→`ctx-run-input-deadlock`,
      `feat/Repo_Trust`→`repo-trust`, `release-notes-fragments`→`release-notes-fragments`.
    - `test_slug_from_branch_rejects_unusable` parametrized: `HEAD`, `main`, `fix/`, `fix/___` → `None`.
    - `test_note_filename` → `datetime(2026, 11, 1, 14, 3, 27, tzinfo=UTC)`, `"x"` →
      `"2026-11-01.14-03-27.x.md"`.
    - `test_add_writes_and_stages` in a real git repo (`git_repo` factory fixture: `git init`, user
      config, one commit) with `slug="foo"`, `text="Hello"`: exactly one file matching `NOTE_NAME_RE`
      with slug `foo`, content `"Hello\n"`, listed by `git diff --cached --name-only`, path printed.
    - `test_add_without_text_creates_empty_file`.
    - `test_add_derives_slug_from_branch` → on branch `fix/some-thing`, no `slug` → slug `some-thing`.
    - `test_add_detached_head_fails` → `ctx.exit` non-zero (catch `SystemExit`), no file written.
    - `test_add_duplicate_slug_refused` and `test_add_append` → existing `"A"` plus `text="B"` gives
      `"A\n\nB\n"`; `test_add_append_without_text_fails`.
    - `test_add_rejects_invalid_slug` → `slug="Bad Slug"` fails.
    - `test_clear_keeps_readme` → notes deleted, `README.md` kept; `test_clear_empty_ok`.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement.** Branch via `git -C <repo_root> branch --show-current` (empty output →
  detached). Time via `datetime.now(UTC)`. `git add` the new or appended file. `clear` only unlinks.
- [ ] **Step 4: Run** — expect PASS.
- [ ] **Step 5: Commit** `feat(tools): add release-notes add and clear commands`.

### Task 3: `check` file mode

**Files:**

- Modify: `tools/release_notes.py`
- Test: `tests/test_tools_release_notes.py`

**Interfaces:**

- Consumes: Task 1 helpers.
- Produces:
    - `_file_problems(repo_root: Path) -> list[str]` — one human-readable line per problem.
    - command `check(ctx, base: str | None = None) -> None` (gate added in Task 4).
- [ ] **Step 1: Write failing tests** (each asserts the problem list content):
    - `test_valid_tree_no_problems`; `test_missing_dir_no_problems` (no `release-notes/` at all is
      fine).
    - `test_bad_name` → `foo.md`, `2026-11-01-foo.md`, `2026-11-01.14-03-27.Foo.md` each flagged.
    - `test_impossible_date` → `2026-02-30.10-00-00.x.md` and `2026-01-01.25-00-00.x.md` flagged.
    - `test_subdirectory_flagged`.
    - `test_empty_note` → `""` and `"  \n"` flagged.
    - `test_heading_flagged` → `"Text\n\n## Heading"` flagged.
    - `test_heading_inside_fence_ok` → ```` "```sh\n# comment\n```" ```` not flagged.
    - `test_unreleased_md_flagged` → `UNRELEASED.md` at repo root flagged with a message naming
      `toolr release-notes add`.
    - `test_check_command_exit` → problems make `check` exit non-zero and print each problem; clean
      tree exits 0.
    - `test_render_refuses_invalid_tree` → `render` exits non-zero on an empty note.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement.** Validate the date/time with `datetime.strptime(f"{date} {time}",
  "%Y-%m-%d %H-%M-%S")`. Heading = line matching `^#{1,6}(\s|$)` outside ```` ``` ````/`~~~` fences.
  Wire `_file_problems` into `render` (Task 1 Step 3).
- [ ] **Step 4: Run** — expect PASS.
- [ ] **Step 5: Commit** `feat(tools): add release-notes check command`.

### Task 4: `check --base` gate

**Files:**

- Modify: `tools/release_notes.py`
- Test: `tests/test_tools_release_notes.py`

**Interfaces:**

- Consumes: `_file_problems`, `check` (Task 3); `git_repo` fixture (Task 2).
- Produces:
    - `class Commit(NamedTuple): sha: str; subject: str; body: str`
    - `SUBJECT_RE` = `^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:`
    - `_requires_note(commit: Commit) -> bool` — `bang` → True; else scope starting with `deps` →
      False; else `re.search("Changelog: skip", body)` → False (mirrors `cliff.toml`); else type in
      `{"feat", "fix"}`.
    - `_has_skip_trailer(body: str) -> bool` — `re.search(r"^Release-Notes:\s*skip\s*$", body, re.M)`.
    - `_gate_failures(commits: list[Commit], changed_notes: list[str]) -> list[Commit]` — `[]` if any
      commit has the skip trailer or `changed_notes` holds a non-README path; else the commits that
      require a note.
    - `_commits_in_range(ctx, base: str) -> list[Commit]` —
      `git log --no-merges --format=%H%x1f%s%x1f%b%x1e {base}..HEAD`.
    - `_changed_notes(ctx, base: str) -> list[str]` —
      `git diff --diff-filter=AM --name-only {base}...HEAD -- release-notes/`.
- [ ] **Step 1: Write failing tests.** Pure: `test_requires_note` parametrized over subjects/bodies —
  `feat: x`→T, `fix(cli): x`→T, `refactor!: x`→T, `fix(deps): x`→F, `fix(deps)!: x`→T, `chore: x`→F,
  `docs(release-notes): x`→F, `fix: x` with body `Changelog: skip`→F. Real git (base branch `main`
  with one commit, feature branch on top):
    - `test_gate_feat_without_note_fails` → exits non-zero, output names the commit subject and both
      remedies (`toolr release-notes add`, `Release-Notes: skip`).
    - `test_gate_feat_with_note_passes`; `test_gate_appended_existing_note_passes` (note exists on
      `main`, branch modifies it).
    - `test_gate_readme_only_change_fails`.
    - `test_gate_chore_only_passes`; `test_gate_no_commits_passes`.
    - `test_gate_trailer_in_any_commit` → `feat: a`, then `chore: b` with `Release-Notes: skip` →
      passes.
    - `test_gate_ignores_base_side_commits` → a `feat` commit added to `main` after branching doesn't
      count against the branch.
    - `test_gate_unknown_base_fails` → `base="nope"` exits non-zero with a message naming the ref.
    - `test_gate_runs_file_checks_too` → invalid note fails even with no `feat`.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement.** Resolve `base` first with `git rev-parse --verify --quiet {base}^{commit}`;
  non-zero → error and exit. Check `returncode` on every git call.
- [ ] **Step 4: Run** — expect PASS. Then `uv run mypy tools/release_notes.py` (or the repo's mypy
  hook via `prek run mypy --files tools/release_notes.py tests/test_tools_release_notes.py`) clean.
- [ ] **Step 5: Commit** `feat(tools): gate PRs on release notes with check --base`.

### Task 5: Release and CI wiring

**Files:**

- Modify: `.github/workflows/_prepare-release.yml` (steps "Get the Python Binary path from Mise",
  "Load UNRELEASED.md into TOOLR_RELEASE_NOTES env var", "Reset UNRELEASED.md", and their comments)
- Modify: `.github/workflows/ci.yml` (`pre-commit` job: checkout `fetch-depth: 0`, new gate step)
- Modify: `cliff.toml` (`commit_parsers`)

**Interfaces:**

- Consumes: `render --github-env`, `clear`, `check --base` (Tasks 1–4).
- [ ] **Step 1:** Replace the loader step's body with `toolr release-notes render --github-env "$GITHUB_ENV"`,
  renaming it "Load release notes into TOOLR_RELEASE_NOTES". Delete "Get the Python Binary path from
  Mise" (its only consumer was the loader; confirm with
  `grep -n mise-python-executable .github/workflows/_prepare-release.yml` → no hits after).
- [ ] **Step 2:** Replace "Reset UNRELEASED.md" with a "Clear queued release notes" step running
  `toolr release-notes clear`, in the same position (after both git-cliff steps). Don't confuse
  `release-notes/` with the `release-notes.md` body file the "Prepare GH Release changelog" step
  writes at the repo root; it stays as is.
- [ ] **Step 3:** In `ci.yml`'s `pre-commit` job set `fetch-depth: 0` on checkout and add, after
  "Run Prek":

  ```yaml
  - name: Check release notes
    if: github.event_name == 'pull_request'
    env:
      BASE_REF: ${{ github.base_ref }}
    run: toolr release-notes check --base "origin/${BASE_REF}"
  ```

- [ ] **Step 4:** In `cliff.toml`, add before the `^doc` parser:
  `{ message = "^docs\\((release-notes|unreleased)\\)", skip = true },`
- [ ] **Step 5: Verify** `prek run --files .github/workflows/_prepare-release.yml .github/workflows/ci.yml cliff.toml`
  passes (actionlint, zizmor, pin checks, `check-gate-needs`). Verify git-cliff locally:
  `TOOLR_RELEASE_NOTES="$(toolr release-notes render)" git cliff --unreleased --strip all` shows a
  `### Notes` section and no `*(unreleased)*` Documentation lines.
- [ ] **Step 6: Commit** `ci(release): build release notes from release-notes/`.

### Task 6: Migration, hook and docs

**Files:**

- Create: `release-notes/README.md`, `release-notes/2026-11-01.<HH-MM-SS>.<slug>.md` (one per note)
- Delete: `UNRELEASED.md`
- Modify: `.pre-commit-config.yaml`, `CLAUDE.md`, `CONTRIBUTING.md`, `renovate.json5`,
  `crates/xtask/tests/rust_version_lockstep.rs`
- [ ] **Step 1:** Split `UNRELEASED.md` (minus its HTML comment) into one note per logical note,
  keeping multi-paragraph notes together and wording unchanged. Timestamps one second apart in current
  file order, starting `2026-11-01.00-00-00`. The bool-flag `**Breaking:**` paragraph is its own note;
  prefix the repository-trust note with `**Breaking:**` and a space (no other wording change).
- [ ] **Step 2: Verify** content is preserved: `toolr release-notes render` output, with
  `**Breaking:**` and a space removed from the trust note and notes reordered back, equals the old body's
  paragraphs. Simplest check: `diff <(old body paragraphs sorted) <(render output paragraphs sorted)`
  differs only by the trust-note marker.
- [ ] **Step 3:** Write `release-notes/README.md`: what a note is, the name scheme, `toolr release-notes
  add`, the `**Breaking:**` convention, no headings, the gate and its trailer.
- [ ] **Step 4:** `git rm UNRELEASED.md`. Add the hook to `.pre-commit-config.yaml` beside the other
  `language: system` toolr hooks:
  `id: release-notes`, `entry: toolr release-notes check`, `pass_filenames: false`,
  `files: ^(release-notes/|UNRELEASED\.md$)`.
- [ ] **Step 5:** Update references:
    - `CLAUDE.md`: "Where things live" → `release-notes/`; the release-bump rule reads `release-notes/`;
      replace the "Queue release notes in `UNRELEASED.md`" action with one telling agents to run
      `toolr release-notes add` for every user-visible change, use `docs(release-notes):` for note-only
      commits, add `Release-Notes: skip` when a `feat`/`fix` has nothing for users, start breaking
      notes with `**Breaking:**`, and never recreate `UNRELEASED.md`. Keep "never hand-edit
      `CHANGELOG.md`".
    - `CONTRIBUTING.md`: step 6 of "Adding a supported type", plus a short "Release notes" section.
    - `renovate.json5` `prBodyNotes`: "…run `toolr release-notes add` saying so before merging."
    - `rust_version_lockstep.rs` assert message: "…add a release note with `toolr release-notes add`."
    - Confirm nothing else: `git grep -n UNRELEASED -- ':!specs/archive' ':!CHANGELOG.md' ':!specs/2026-11-01-*'`
      → only the `check` guard and its test.
- [ ] **Step 6: Verify** `prek run --all-files` passes (includes the new hook, rumdl, typos).
- [ ] **Step 7: Commit** in two commits: `docs(release-notes): migrate UNRELEASED.md to release-notes/`
  (notes, README, deletion, hook) and `docs: point contributors at toolr release-notes`
  (CLAUDE.md, CONTRIBUTING.md, renovate.json5, lockstep test).

### Task 7: Full verification and archive

- [ ] **Step 1:** `mise run test` — monitor output every 30–60s; expect green.
- [ ] **Step 2:** `cargo test -p xtask --test rust_version_lockstep` passes.
- [ ] **Step 3:** Exercise the gate on this branch:
  `toolr release-notes check --base fix/release-prek-failures` → passes (this branch adds notes).
- [ ] **Step 4:** `git mv specs/2026-11-01-release-notes-fragments-{design,plan}.md specs/archive/2026/`;
  commit `docs(specs): archive release notes fragments design and plan`.
- [ ] **Step 5:** `git-spice branch submit --draft`; then run the `monitor-pr` skill.

### Task 8: Rebase open PRs (after the cutover merges)

- [ ] For each of #626 `fix/tester-discover-errors`, #624 `fix/action-latest-repo`,
  #622 `fix/ancestor-tools-discovery`: rebase onto `main` (`paddle-shared:rebase` skill), resolve the
  modify/delete conflict on `UNRELEASED.md` by dropping the file and recreating the branch's entry with
  `toolr release-notes add --slug <slug> --text …` (keep `**Breaking:**` for #622, whose commit is
  `fix(discovery)!`), commit `docs(release-notes): …`, and force-push after confirming with the
  maintainer.
- [ ] Verify each with `toolr release-notes check --base origin/main`.
