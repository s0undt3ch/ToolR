# Release-notes fragments implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `UNRELEASED.md` with per-note files in `release-notes/`, managed by a project-local
`toolr release-notes` command group, and gate PRs that ship a user-visible change without a note.

**Architecture:** One module, `tools/release_notes.py`, holds pure helpers (name parsing, file checks,
rendering, the gate decision) plus four thin commands (`add`, `check`, `render`, `clear`) that shell
out to git via `ctx.run`. The release workflow calls `render` and `clear --rendered`, CI calls
`check --base`, and a prek hook calls `check`. git-cliff's template is untouched.

**Tech Stack:** Python 3.11+, toolr-py (`command_group`, `Context`), pytest with `toolr.testing.make_context`,
GitHub Actions, git-cliff, prek.

**Spec:** `specs/2026-11-01-release-notes-fragments-design.md`

## Global Constraints

- Branch `feat/release-notes-fragments` is stacked on `fix/release-prek-failures` (#625) with git-spice.
  Restack (`git-spice upstack restack`) if #625 changes; submit with `git-spice branch submit --draft`.
- Note name: `release-notes/<YYYY-MM-DD>.<HH-MM-SS>.<slug>.md`, UTC creation time.
- Slug pattern: `[a-z0-9]+(-[a-z0-9]+)*`.
- `release-notes/README.md` is excluded by name from every subcommand.
- A missing `release-notes/` directory means "no notes" for `render`, `clear` and `check`; `add`
  creates it.
- Breaking marker: note content starting with `**Breaking:**`.
- Skip trailer: `Release-Notes: skip`. Existing trailer honoured: `Changelog: skip`.
- Note-only commit convention: `docs(release-notes): …`.
- Commits for `tools/release_notes.py` and its tests use `chore(tools): …`. The command is repo-internal,
  and `feat` would list it under Features in the public changelog.
- CI runs `toolr` from `version: latest` (released 0.35.1 binary) with the tools venv's locked
  `toolr-py` (`tools/uv.lock`). Use only parameter types and toolr-py APIs already used in `tools/`
  (`command_group`, `Context.run/print/error/exit`, `str | None`/`bool` parameters).
- `CommandResult.stdout` is typed `IO[str] | IO[bytes]`; reads need the
  `# type: ignore[assignment]` that `tools/version.py` uses, or a cast.
- `render` writes nothing to stdout except the notes: no `ctx.info`, and `ctx.run` calls (if any)
  with `stream_output=False`. Its stdout feeds `TOOLR_RELEASE_NOTES`.
- Python tests: factory fixtures over `tmp_path`, imports at the top of the file. Model on
  `tests/test_tools_version.py` and `tests/test_tools_ci_packslip.py` (`pytest.raises(SystemExit)` plus
  `ctx.stderr` for failures).
- Never `git add -A`; stage paths explicitly. No `Co-Authored-By` trailer.

## Review Focus

1. **A note with Windows line endings or a trailing blank line** renders identically to an LF note,
   and doesn't trip the heading or empty checks. Test in Task 1 (`test_render_normalises_crlf`).
2. **`check --base` with an unresolvable ref or an empty range** (typo, shallow clone) fails loudly
   instead of passing. Tests in Task 4 (`test_gate_unknown_base_fails`, `test_gate_empty_range_fails`).
3. **A branch named `main`, `HEAD` (detached), or `fix/` with nothing after the slash** gives a clear
   `add` error instead of writing `….main.md` or `….md`. Test in Task 2
   (`test_slug_from_branch_rejects_unusable`).
4. **The release loses notes**: notes exist but the rendered changelog lacks `### Notes`. `clear
   --rendered` refuses and deletes nothing. Test in Task 2 (`test_clear_rendered_guard`).
5. **CI's real HEAD shape**, the synthetic `refs/pull/N/merge` commit, is judged on the PR's commits
   only. Test in Task 4 (`test_gate_on_pr_merge_commit`).

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
    - `_note_paths(notes_dir: Path) -> list[Path]` — every entry except `README.md`, sorted by name;
      `[]` if the directory is missing.
    - `_read_note(path: Path) -> str` — CRLF→LF, `.strip()`.
    - `_render(notes_dir: Path) -> str`
    - `_github_env_block(name: str, value: str) -> str` — delimiter `f"{name}_{secrets.token_hex(16)}"`,
      regenerated while it occurs in `value`. Use `import secrets` (module attribute, so tests can
      monkeypatch `secrets.token_hex`).
    - command `render(ctx: Context, github_env: str | None = None) -> None`
- [ ] **Step 1: Write failing tests** with a `notes_dir` factory fixture `(files: dict[str, str]) -> Path`
  writing into `tmp_path / "release-notes"`:
    - `test_render_empty_dir` → `_render(dir) == ""`; also when only `README.md` exists.
    - `test_render_missing_dir` → `_render(tmp_path / "release-notes") == ""` with no directory.
    - `test_render_excludes_readme` → README text absent from output.
    - `test_render_breaking_first` → files `2026-01-01.00-00-00.a.md` ("A"),
      `2026-01-02.00-00-00.b.md` ("**Breaking:** B"), `2026-01-03.00-00-00.c.md` ("C") render
      `"**Breaking:** B\n\nA\n\nC"`.
    - `test_render_normalises_crlf` → `"One\r\n\r\nTwo\r\n\r\n"` renders as `"One\n\nTwo"`.
    - `test_github_env_block_format` → output equals `f"TOOLR_RELEASE_NOTES<<{d}\nvalue\n{d}\n"`, where
      `d` starts with `TOOLR_RELEASE_NOTES_`.
    - `test_github_env_block_avoids_delimiter_in_value` → monkeypatch `secrets.token_hex` to return
      `"X"` then `"Y"`; value `"TOOLR_RELEASE_NOTES_X"` yields delimiter `TOOLR_RELEASE_NOTES_Y`.
    - `test_render_command_stdout` via `make_context(repo_root=tmp_path)`: `ctx.stdout` **equals** the
      rendered text plus one trailing newline.
    - `test_render_command_github_env` → with `github_env=str(file)` the file gains the block
      (appended, existing content kept) and `ctx.stdout == ""`.
- [ ] **Step 2: Run** `uv run pytest tests/test_tools_release_notes.py -v` — expect import errors.
- [ ] **Step 3: Implement** the interfaces above. Paths resolve against `ctx.repo_root`. Task 3 adds the
  file-check call to `render`.
- [ ] **Step 4: Run** the tests — expect PASS. `uv run mypy tools/release_notes.py` — clean.
- [ ] **Step 5: Commit** `chore(tools): add release-notes render command` (stage the two files).

### Task 2: `add` and `clear`

**Files:**

- Modify: `tools/release_notes.py`
- Test: `tests/test_tools_release_notes.py`

**Interfaces:**

- Consumes: `NOTES_DIR`, `NOTE_NAME_RE`, `_note_paths` (Task 1).
- Produces:
    - `_slug_from_branch(branch: str) -> str | None` — strip one leading `<word>/`, lowercase, replace
      runs of non-`[a-z0-9]` with `-`, trim `-`; `None` if the result is empty or the branch is `HEAD`
      or `main`.
    - `_note_filename(now: datetime, slug: str) -> str` → `f"{now:%Y-%m-%d}.{now:%H-%M-%S}.{slug}.md"`.
    - `_find_by_slug(notes_dir: Path, slug: str) -> Path | None`
    - `git_repo` test fixture: factory `(branch: str = "main") -> Path` running `git init -b main`,
      setting `user.name`/`user.email`, making one commit, and checking out `branch` if different.
    - command `add(ctx, slug: str | None = None, text: str | None = None, append: bool = False) -> None`
    - command `clear(ctx, rendered: str | None = None) -> None`
- [ ] **Step 1: Write failing tests:**
    - `test_slug_from_branch` parametrized: `fix/ctx-run-input-deadlock`→`ctx-run-input-deadlock`,
      `feat/Repo_Trust`→`repo-trust`, `release-notes-fragments`→`release-notes-fragments`.
    - `test_slug_from_branch_rejects_unusable` parametrized: `HEAD`, `main`, `fix/`, `fix/___` → `None`.
    - `test_note_filename` → `datetime(2026, 11, 1, 14, 3, 27, tzinfo=UTC)`, `"x"` →
      `"2026-11-01.14-03-27.x.md"`.
    - `test_add_writes_and_stages` (no `release-notes/` yet; `add` creates it) with `slug="foo"`,
      `text="Hello"`: exactly one file matching `NOTE_NAME_RE` with slug `foo`, content `"Hello\n"`,
      listed by `git diff --cached --name-only`, path printed.
    - `test_add_without_text_creates_empty_file`.
    - `test_add_derives_slug_from_branch` → on branch `fix/some-thing`, no `slug` → slug `some-thing`.
    - `test_add_detached_head_fails` → `SystemExit` non-zero, no file written.
    - `test_add_duplicate_slug_refused`; `test_add_append` → existing `"A"` plus `text="B"` gives
      `"A\n\nB\n"`; `test_add_append_without_text_fails`; `test_add_rejects_invalid_slug`
      (`slug="Bad Slug"`).
    - `test_clear_keeps_readme`; `test_clear_empty_ok`; `test_clear_missing_dir_ok`.
    - `test_clear_rendered_guard` → one note, `rendered` file without a `### Notes` line → `SystemExit`
      non-zero and the note still exists; with a `### Notes` line → note deleted; no notes and a file
      without `### Notes` → succeeds.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement.** Branch via `git -C <repo_root> branch --show-current` (empty → detached).
  Time via `datetime.now(UTC)`. `git add` the new or appended file. `clear` only unlinks. `### Notes`
  match: a line equal to `### Notes` after `.strip()`.
- [ ] **Step 4: Run** — expect PASS; mypy clean.
- [ ] **Step 5: Commit** `chore(tools): add release-notes add and clear commands`.

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
    - `test_valid_tree_no_problems`; `test_missing_dir_no_problems`.
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
- [ ] **Step 3: Implement.** Validate with `datetime.strptime(f"{date} {time}", "%Y-%m-%d %H-%M-%S")`.
  Heading = line matching `^#{1,6}(\s|$)` outside ```` ``` ````/`~~~` fences. Call `_file_problems`
  at the start of `render`.
- [ ] **Step 4: Run** — expect PASS; mypy clean.
- [ ] **Step 5: Commit** `chore(tools): add release-notes check command`.

### Task 4: `check --base` gate

**Files:**

- Modify: `tools/release_notes.py`
- Test: `tests/test_tools_release_notes.py`

**Interfaces:**

- Consumes: `_file_problems`, `check` (Task 3); `git_repo` fixture (Task 2).
- Produces:
    - `class Commit(NamedTuple): sha: str; subject: str; body: str`
    - `SUBJECT_RE` = `^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:`
    - `BREAKING_FOOTER_RE` = `^BREAKING[ -]CHANGE:` with `re.M`.
    - `_is_breaking(commit: Commit) -> bool` — `bang` or footer match.
    - `_requires_note(commit: Commit) -> bool` — no `SUBJECT_RE` match → False; breaking → True;
      scope starting with `deps` → False; `re.search("Changelog: skip", body)` → False (mirrors
      `cliff.toml`); else type in `{"feat", "fix", "revert"}`.
    - `_has_skip_trailer(body: str) -> bool` — `re.search(r"^Release-Notes:\s*skip\s*$", body, re.M)`.
    - `class NoteChanges(NamedTuple): changed: list[str]; deleted: list[str]` (README excluded).
    - `_gate_failures(commits: list[Commit], notes: NoteChanges, breaking_note: bool) -> list[str]` —
      `[]` if any commit has the skip trailer; otherwise one message per unmet requirement: a
      note-requiring commit with no changed note (for `revert`, a deleted note also counts); a breaking
      commit with `breaking_note` False.
    - `_commits_in_range(ctx, base: str) -> list[Commit]` —
      `git log --no-merges --format=%H%x1f%s%x1f%b%x1e {base}..HEAD`.
    - `_note_changes(ctx, base: str) -> NoteChanges` —
      `git diff --name-status {base}...HEAD -- release-notes/`; `A`/`M` → changed, `D` → deleted.
    - `breaking_note` = any changed note whose content starts with `BREAKING_PREFIX`.
- [ ] **Step 1: Write failing tests.** Pure: `test_requires_note` parametrized — `feat: x`→T,
  `fix(cli): x`→T, `revert: x`→T, `refactor!: x`→T, `refactor: x` + body `BREAKING CHANGE: y`→T,
  `fix(deps): x`→F, `fix(deps)!: x`→T, `chore: x`→F, `docs(release-notes): x`→F, `fix: x` + body
  `Changelog: skip`→F, `Revert "feat: x"`→F, `Merge branch 'x'`→F. Real git (base `main` with one
  commit, feature branch on top):
    - `test_gate_feat_without_note_fails` → non-zero exit; stderr names the commit subject and the
      remedies (`toolr release-notes add`, `Release-Notes: skip`).
    - `test_gate_feat_with_note_passes`; `test_gate_appended_existing_note_passes` (note on `main`,
      branch modifies it); `test_gate_readme_only_change_fails`.
    - `test_gate_breaking_needs_breaking_note` → `feat!: x` with a plain note fails, naming
      `**Breaking:**`; with a `**Breaking:**` note passes.
    - `test_gate_revert_with_deleted_note_passes`.
    - `test_gate_chore_only_passes`.
    - `test_gate_empty_range_fails` → branch with no commits beyond `main` exits non-zero.
    - `test_gate_trailer_in_any_commit` → `feat: a`, then `chore: b` with `Release-Notes: skip` → passes.
    - `test_gate_ignores_base_side_commits` → a `feat` commit on `main` after branching doesn't count.
    - `test_gate_on_pr_merge_commit` → `feat: a` + note on the branch; then check out `main`, add an
      unrelated commit, `git merge --no-ff` the branch, and run the gate at that merge commit with the
      pre-merge `main` tip as base → passes; same without the note → fails.
    - `test_gate_unknown_base_fails` → `base="nope"` exits non-zero naming the ref.
    - `test_gate_runs_file_checks_too` → invalid note fails even with no `feat`.
- [ ] **Step 2: Run** — expect FAIL.
- [ ] **Step 3: Implement.** Resolve `base` first with `git rev-parse --verify --quiet {base}^{commit}`;
  non-zero → error and exit. Check `returncode` on every git call.
- [ ] **Step 4: Run** — expect PASS; mypy clean.
- [ ] **Step 5: Commit** `chore(tools): gate PRs on release notes with check --base`.

### Task 5: Migration and hook

**Files:**

- Create: `release-notes/README.md`, `release-notes/2026-11-01.<HH-MM-SS>.<slug>.md` (one per note)
- Delete: `UNRELEASED.md`
- Modify: `.pre-commit-config.yaml`
- [ ] **Step 1:** Split `UNRELEASED.md` (minus its HTML comment) into one note per logical note, wording
  unchanged. A logical note is every block up to the next note's first paragraph: the bool-flag note
  is **one file holding three blocks** (the `**Breaking:**` intro, the `-` list, and the
  `Annotated[bool, …]` alias paragraph). Timestamps one second apart in current file order, starting
  `2026-11-01.00-00-00`. Prefix the repository-trust note with `**Breaking:**` and a space (no other
  wording change).
- [ ] **Step 2: Verify** content is preserved. Run:

  ```sh
  tmp=$(mktemp -d)
  git show HEAD:UNRELEASED.md | python3 -c 'import re,sys; t=re.sub(r"<!--.*?-->","",sys.stdin.read(),flags=re.S).strip(); print("\n".join(sorted(t.split("\n\n"))))' > "$tmp/old.txt"
  toolr release-notes render | python3 -c 'import sys; print("\n".join(sorted(sys.stdin.read().strip().split("\n\n"))))' > "$tmp/new.txt"
  diff "$tmp/old.txt" "$tmp/new.txt"
  ```

  Expected: the only difference is the `**Breaking:**` prefix on the trust note's first block.
- [ ] **Step 3:** Write `release-notes/README.md`: what a note is, the name scheme, `toolr release-notes
  add`, the `**Breaking:**` convention, no headings, the gate and its trailer, and "after a parent PR
  merges, restack and push; don't re-run old CI".
- [ ] **Step 4:** `git rm UNRELEASED.md`. Add a hook beside the other `language: system` toolr hooks:
  `id: release-notes`, `entry: toolr release-notes check`, `pass_filenames: false`,
  `files: ^(release-notes/|UNRELEASED\.md$)`.
- [ ] **Step 5: Verify** `prek run --all-files` passes (new hook, rumdl, typos).
- [ ] **Step 6: Commit** `docs(release-notes): migrate UNRELEASED.md to release-notes/` (notes, README,
  deletion, hook config).

### Task 6: Release and CI wiring, docs

**Files:**

- Modify: `.github/workflows/_prepare-release.yml` (steps "Get the Python Binary path from Mise",
  "Load UNRELEASED.md into TOOLR_RELEASE_NOTES env var", "Reset UNRELEASED.md", and their comments)
- Modify: `.github/workflows/ci.yml` (`pre-commit` job)
- Modify: `cliff.toml` (`commit_parsers`)
- Modify: `CLAUDE.md`, `CONTRIBUTING.md`, `renovate.json5`, `crates/xtask/tests/rust_version_lockstep.rs`
- [ ] **Step 1:** Replace the loader step's body with `toolr release-notes render --github-env "$GITHUB_ENV"`,
  renamed "Load release notes into TOOLR_RELEASE_NOTES". Delete "Get the Python Binary path from Mise";
  confirm `grep -n mise-python-executable .github/workflows/_prepare-release.yml` → no hits.
- [ ] **Step 2:** Replace "Reset UNRELEASED.md" with a "Clear queued release notes" step running
  `toolr release-notes clear --rendered .release-changes.md`, in the same position (after both git-cliff
  steps). Leave the root `release-notes.md` body file the "Prepare GH Release changelog" step writes
  alone; it is unrelated to `release-notes/`.
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
    - Confirm: `git grep -n UNRELEASED -- ':!specs/archive' ':!CHANGELOG.md' ':!specs/2026-11-01-*'`
      → only `tools/release_notes.py`, its tests, `.pre-commit-config.yaml`, and the CLAUDE.md "never
      recreate" line.
- [ ] **Step 6: Verify:**
    - `prek run --all-files` passes (`jactionlint-fix`, `shellcheck`, `pin-github-actions`,
      `check-gate-needs`).
    - `TOOLR_RELEASE_NOTES="$(toolr release-notes render)" git cliff --unreleased --strip all` shows a
      `### Notes` section with the trust and bool-flag notes first, and no `*(unreleased)*`
      Documentation lines.
    - `cargo test -p xtask --test rust_version_lockstep` passes.
- [ ] **Step 7: Commit** `ci(release): build release notes from release-notes/` (workflows, cliff.toml)
  and `docs: point contributors at toolr release-notes` (CLAUDE.md, CONTRIBUTING.md, renovate.json5,
  lockstep test).

### Task 7: Full verification and archive

- [ ] **Step 1:** `mise run test` — poll output every 30–60s; expect green.
- [ ] **Step 2:** `toolr release-notes check --base fix/release-prek-failures` → passes.
- [ ] **Step 3:** `git mv specs/2026-11-01-release-notes-fragments-{design,plan}.md specs/archive/2026/`;
  commit `docs(specs): archive release notes fragments design and plan`.
- [ ] **Step 4:** `git-spice branch submit --draft`; then run the `monitor-pr` skill.

### Task 8: Rebase open PRs (after the cutover merges)

- [ ] For each of #626 `fix/tester-discover-errors`, #624 `fix/action-latest-repo`,
  #622 `fix/ancestor-tools-discovery`: rebase onto `main` (`paddle-shared:rebase` skill), resolve the
  modify/delete conflict on `UNRELEASED.md` by dropping the file and recreating the branch's entry with
  `toolr release-notes add --slug <slug> --text …` (#622 is `fix(discovery)!`, so its note starts with
  `**Breaking:**`), commit `docs(release-notes): …`, and force-push after confirming with the
  maintainer.
- [ ] Verify each with `toolr release-notes check --base origin/main`.
