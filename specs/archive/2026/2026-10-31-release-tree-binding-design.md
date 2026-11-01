# Push the release commit and tag atomically

- **Status:** implemented
- **Origin:** whole-project review finding F-10 (medium, plausible).
- **Plan:** [`2026-10-31-release-tree-binding-plan.md`](./2026-10-31-release-tree-binding-plan.md)
- **Stacked on:** `fix/release-prek-failures` (F-24, [`2026-10-31-release-prek-failures-design.md`](./2026-10-31-release-prek-failures-design.md)),
  now merged.

## Problem

`release.yml`'s `publish-release` job checks out the repo, applies the release patch built by `prepare-release`
(`.github/actions/apply-release-patch`, `git am`), tags the result, then runs:

```sh
git push origin "v${RELEASE_VERSION}"
git push
```

The finding has two halves. One is real; the other doesn't hold.

**Tag pushed, branch rejected (real).** The run takes about an hour between `prepare-release` and `publish-release`.
If a PR merges into `main` in that time, the second `git push` is rejected as non-fast-forward. The tag push already
succeeded. The result:

- `vX.Y.Z` exists on GitHub and points at a commit that isn't on any branch.
- The step fails, so no GitHub release and no PyPI upload.
- `publish-packslip` still runs (it runs on a failed `publish-release`), checks out the dangling tag, and fails on the
  missing release.
- Re-running fails at `prepare-release`: `toolr version bump --check-existing-tag` sees the tag. Someone has to delete
  it by hand first.

Reproduced in a scratch bare repo: clone at commit A, commit the release, advance the remote `main` to B, run the two
pushes. `git ls-remote --tags` shows `v9.9.9`; `main` is still B.

**Artefacts built from a different tree than the tag (doesn't hold).** The audit assumed `publish-release` checks out
the current tip of `main`. It doesn't. `actions/checkout` with no `ref:` fetches and checks out `github.sha`, the
commit the run was dispatched on, in every job of the run. `prepare-release` builds the sdists from `github.sha` + the
patch; `publish-release` tags `github.sha` + the same patch. The trees match. Nothing checks this, though: a future
edit that adds `ref: main` to either checkout would break it silently.

## Decision

1. Push branch and tag in one atomic push from the `publish-release` step (give it `id: push-release`):

    ```sh
    git push --atomic origin "HEAD:refs/heads/${GITHUB_REF_NAME}" "refs/tags/v${RELEASE_VERSION}"
    ```

   Either both refs move or neither does. If `main` moved, the push fails, nothing is published, no tag is left, and
   the fix is to dispatch the release again from the new `main`. Use `GITHUB_REF_NAME` rather than a literal `main`,
   which is what the bare `git push` does today. Before pushing, fail with a clear error unless `GITHUB_REF` starts
   with `refs/heads/`: a dispatch on a tag has no branch to push.

2. Assert the base in `.github/actions/apply-release-patch/action.yml` (step `id: apply`). After `git am`, require
   `git rev-parse HEAD^` to equal `$GITHUB_SHA`, or fail with both SHAs in the message. That pins the "same tree"
   property that today rests on `actions/checkout`'s default. Both callers (`release.yml` `publish-release`, `ci.yml`
   `docs`) check out `github.sha`, so the assertion holds on PRs and on `main`.

Rejected alternatives:

- **Push the branch first, then the tag.** Leaves a window where the bump commit is on `main` with no tag and no
  release if the tag push fails. Atomic push has no window.
- **Rebase the patch onto the new `main` and push.** The tag would then point at a tree nobody built or tested: this
  would create the mismatch the finding feared.
- **Export the prepare-time SHA as a `prepare-release` output and compare it.** It equals `github.sha` in every job,
  so the output adds a hop for nothing. It would also touch `_prepare-release.yml`, which F-24 and F-25 edit.

## Scope and compatibility

- No runner or manifest schema change.
- CI only; no `UNRELEASED.md` entry.
- Assumes F-24 has landed: `tests/workflows/conftest.py` with the `workflow_step_script` and `git_repo` fixtures. This
  spec doesn't touch `_prepare-release.yml`.
- A release whose `main` moved now fails cleanly instead of half-publishing. The release runbook step is the same as
  for any failed `prepare-release`: dispatch again.
