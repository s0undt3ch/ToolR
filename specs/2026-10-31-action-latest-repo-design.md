# Resolve the action's `version: latest` against ToolR, not the caller

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-22 (low).
- **Plan:** [`2026-10-31-action-latest-repo-plan.md`](./2026-10-31-action-latest-repo-plan.md)

## Problem

The Setup ToolR action (`action.yml`, step `resolve-version`) turns `version: latest` into a tag with:

```sh
version=$(gh release view --repo "${GITHUB_REPOSITORY:-s0undt3ch/ToolR}" --json tagName --jq '.tagName' 2>/dev/null | sed 's/^v//')
```

and only falls back to `--repo s0undt3ch/ToolR` when that prints nothing. In a consumer's workflow
`GITHUB_REPOSITORY` is the consumer, so a consumer repo with releases of its own gets its own latest tag back. The
action then downloads `toolr-<their tag>-<triple>.*` from `s0undt3ch/ToolR` (the download step hardcodes the repo),
which either fails with "release not found", trips the `0.20.0` floor with a misleading "refusing to install toolr
1.4.0"-style message, or, if the numbers happen to collide, installs an unintended toolr release.

Repro (offline, the way `tests/action_yml/test_resolve_version.py` drives the step): export
`GITHUB_REPOSITORY=consumer/repo`, stub `gh release view` to print `v9.9.9` for `--repo consumer/repo` and `v0.20.1`
for `--repo s0undt3ch/ToolR`, and run the step with `INPUT_VERSION=latest`. It writes `version=9.9.9`.

The existing tests can't see this: their `gh` stub prints the same tag whatever `--repo` says, and the harness never
sets `GITHUB_REPOSITORY`. toolr's own CI can't see it either, because there `GITHUB_REPOSITORY` is `s0undt3ch/ToolR`.

## Decision

Query `s0undt3ch/ToolR` only, in a single call:

```sh
version=$(gh release view --repo s0undt3ch/ToolR --json tagName --jq '.tagName' | sed 's/^v//')
```

- The fallback branch goes. With one fixed repo there is nothing to fall back to.
- `2>/dev/null` goes. Under `set -euo pipefail` a failing `gh` already aborts the step; keeping its stderr makes the
  failure readable instead of a bare exit.
- The empty-result check and its "could not resolve 'latest'" error stay.
- This matches the download step, which already hardcodes `--repo s0undt3ch/ToolR` and the
  `github.com/s0undt3ch/ToolR/releases/download` URL. The two lookups now agree on where releases live.
- The `version` input's description says which repository `latest` refers to. It feeds the generated
  `skills/toolr-ci-setup/references/action.md`, so the reference is regenerated.

### Rejected alternatives

- **`github.action_repository`.** It is the action's own repo for a `uses: s0undt3ch/ToolR@…` caller, but it is empty
  or unreliable for local `uses: ./` steps, which is how toolr's own CI and `_prepare-release.yml` run the action. It
  would also make a fork of the action resolve the fork's tags while still downloading from `s0undt3ch/ToolR`.
- **Keep the fallback, swap the order** (ToolR first, caller second). The caller's releases are never the right
  answer, so a second lookup only adds a way to be wrong.
- **A new `repository` input.** Nobody has asked to install toolr from a mirror, and the download and attestation steps
  would have to honour it too. Out of proportion for a low-severity fix.

## Scope and compatibility

- No manifest or runner schema change.
- Behaviour changes only for callers that pass `version: latest` (or an empty `version`) from a repository other than
  `s0undt3ch/ToolR`. Pinned versions and the baked-in default are untouched.
- Skill: `skills/toolr-ci-setup/references/action.md` regenerates from the new input description. `SKILL.md` has no
  prose about how `latest` resolves, so its narrative needs no edit.
- `UNRELEASED.md`:

  > The Setup ToolR action's `version: latest` now always resolves to the newest ToolR release. It used to ask for the
  > newest release of the repository running the workflow first, so a repository with releases of its own got its own
  > tag back and the install failed or picked the wrong toolr version.
