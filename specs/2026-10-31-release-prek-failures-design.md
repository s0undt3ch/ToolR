# Stop the release commit from swallowing prek failures

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-24 (low).
- **Plan:** [`2026-10-31-release-prek-failures-plan.md`](./2026-10-31-release-prek-failures-plan.md)

## Problem

The `Commit changes` step of `.github/workflows/_prepare-release.yml` runs:

```sh
prek run --all-files --show-diff-on-failure || true
git commit -am "chore(release): Prepare for ${{ env.RELEASE_VERSION }} release"
```

The `|| true` exists so the auto-fixing hooks (`end-of-file-fixer`, `trailing-whitespace`, `rumdl` and friends) can
tidy the git-cliff output in `CHANGELOG.md` without failing the job. It also hides every hook that fails for a real
reason: `regen-doc-snippets --check`, `check-pipeline-gate-needs`, `ruff-check`, `mypy`, `clippy`. The release commit
is made anyway, the release publishes, and `main` goes red on the next push.

Two more gaps sit next to it:

- Any edit an auto-fixer makes lands in the release commit, including edits to files the release never touched. Nobody
  reviews that commit before it reaches `main`.
- `pin-github-actions` resolves every pinned action through `gh api`. `ci.yml`'s pre-commit job passes `GH_TOKEN` for
  that; this step doesn't, so it runs unauthenticated (60 requests an hour). With the failure no longer masked, a rate
  limit would fail the release.

The next step, `Tag the release`, creates a local annotated tag named `${RELEASE_VERSION}`, with no `v` prefix. Nothing
reads it: the patch is cut with `git format-patch HEAD^`, both wheels take their version from `Cargo.toml`
(`dynamic = ["version"]`), and `release.yml`'s `publish-release` job makes the real `v`-prefixed tag. It's a trap for
anyone who later assumes the tag is the release tag.

Repro: on a branch, add a Python file with a `ruff-check` violation (an unused import) and push. The PR's
`Prepare Release` job (`ci.yml` runs this reusable workflow on every PR) prints the ruff failure and then goes green.

## Decision

Rewrite the `Commit changes` step (give it `id: commit-release`) as four phases, in a single bash script:

1. Record `git status --porcelain --untracked-files=all` before prek runs. At this point it lists what the release
   changed (`Cargo.toml`, `Cargo.lock`, `action.yml`, `CHANGELOG.md`, `UNRELEASED.md`) and the untracked
   `release-notes.md` and `.release-changes.md`.
2. Run `prek run --all-files` once and ignore its exit status. This is the fixing pass.
3. Record the status again. Any path in the second list but not the first means a hook edited or created a file the
   release didn't touch. Fail and name the paths: that's drift on `main`, which belongs in its own PR.
4. Run `prek run --all-files --show-diff-on-failure` again, without `|| true`. A failure here is real. Then commit.

Pass `GH_TOKEN: ${{ steps.app-token.outputs.token || github.token }}` to the step, the same token the checkout uses,
so `pin-github-actions` runs authenticated.

Read the version as `"$RELEASE_VERSION"` (it is already in the job env through `GITHUB_ENV`) instead of interpolating
`${{ env.RELEASE_VERSION }}` into the script. The step script then runs unchanged under `bash` in a test.

Move `Show Changes Diff` after the commit and make it `git show --stat --patch HEAD`, so the log shows what was
committed, prek fixes included, not the pre-prek diff.

Delete the `Tag the release` step.

Rejected alternatives:

- **Run prek once, strictly.** The fixers would then fail every release over formatting that git-cliff produces. The
  fixing pass is wanted; only the masking is wrong.
- **An allow-list of files the release may change.** It duplicates knowledge held by `cargo set-version` and
  git-cliff, and goes stale when either changes. Comparing before and after prek needs no list.
- **`SKIP=pin-github-actions`.** The hook checks pins in `action.yml`, which the release rewrites. Passing a token is
  cheaper than losing the check.
- **Keep the local tag but add the `v`.** It would still be unused; `publish-release` tags after applying the patch.

## Scope and compatibility

- No runner or manifest schema change.
- Workflow and test files only. Nothing ships to users, so no `UNRELEASED.md` entry.
- Releases and every PR's `Prepare Release` job now fail on hook failures they used to hide. The `pre-commit` job in
  `ci.yml` already gates on the same hooks, so a PR that passes it passes this step too.
- `release.yml` is untouched. F-10 (`fix/release-tree-binding`) and F-25 (`ci/dogfood-pinned-version`) stack on this
  branch; this spec only changes the `Commit changes`, `Show Changes Diff` and `Tag the release` steps of
  `_prepare-release.yml`.
