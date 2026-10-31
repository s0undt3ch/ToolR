# Pin the toolr binary CI dogfoods to the locked toolr-py

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-25 (low-medium, plausible).
- **Plan:** [`2026-10-31-dogfood-pinned-version-plan.md`](./2026-10-31-dogfood-pinned-version-plan.md)
- **Stacks on:** `fix/release-tree-binding` (F-10, `specs/2026-10-31-release-tree-binding-design.md`); merge that
  first.

## Problem

Six in-repo calls of the Setup ToolR action (`uses: ./`) pass `version: latest`:

| Workflow | Job |
|---|---|
| `.github/workflows/ci.yml` | `prepare-ci`, `pre-commit`, `packslip-check`, `docs` |
| `.github/workflows/release.yml` | `prepare-ci` |
| `.github/workflows/_prepare-release.yml` | `prepare-release` |

So CI runs the newest released binary. The tools venv it dispatches into is built with `uv sync --frozen` from
`tools/uv.lock`, which pins `toolr-py` (0.35.1 today). Local development uses the binary pinned in `mise.toml`
(`"aqua:s0undt3ch/ToolR" = "0.35.1"`). Renovate's "toolr release" group bumps `mise.toml` and `tools/uv.lock`
together after each release; nothing bumps CI's `latest`, because it moves on its own the moment a release publishes.

Between a release and the merge of that Renovate PR, CI runs binary N+1 against `toolr-py` N. If the release bumped
`RUNNER_SCHEMA_VERSION`, `toolr-py`'s `load_spec` (`crates/toolr-py/python/toolr/_runner.py`) rejects every spec:

```text
toolr-py in this tools venv speaks schema 3, but the toolr binary emitted schema 4.
The venv is out of sync with the binary.
```

`toolr ci generate-build-matrix` in `prepare-ci` is the first command to dispatch, so every PR and every push to
`main` goes red, including the Renovate PR that would fix it. The `docs` job's comment already assumes the released
binary matches "the released toolr-py in the tools venv"; with `latest` that only holds by luck.

Not proven yet: no schema bump has shipped since `latest` was adopted. Plan Task 1 reproduces the mismatch locally
before changing anything.

Just dropping `version:` doesn't help. The action's `inputs.version.default` is rewritten to the new version by the
release commit itself (`toolr version bump --include-action`), so it runs ahead of `tools/uv.lock` in the same window.

## Decision

Add one pin, `TOOLR_VERSION=0.35.1`, to `.github/env`, which every job that calls `uses: ./` already loads into
`GITHUB_ENV`. The five top-level calls pass `version: ${{ env.TOOLR_VERSION }}`.

`_prepare-release.yml` is a reusable workflow and can't see the caller's env. Mirror how `mise-version` reaches it:
both callers' `prepare-ci` jobs expose a `toolr-version` output; `_prepare-release.yml` takes a required
`toolr-version` input and passes `version: ${{ inputs.toolr-version }}`.

Keep the pin in step with the lock:

- `renovate.json5`: a regex `customManager` for `.github/env`, matching `TOOLR_VERSION=(?<currentValue>\d+\.\d+\.\d+)`,
  `depNameTemplate: "s0undt3ch/ToolR"`, `datasourceTemplate: "github-releases"`,
  `extractVersionTemplate: "^v?(?<version>.*)$"`, as the `MISE_VERSION` manager does. Add `"custom.regex"` to the
  `matchManagers` and `".github/env"` to the `matchFileNames` of the "Track our own toolr release in mise / GH Actions
  ASAP" rule, so the pin moves in the same "toolr release" PR as `mise.toml` and `tools/uv.lock`.
- `tests/test_dogfood_toolr_pin.py` fails when the three disagree, or when any `uses: ./` call passes anything other
  than the pin.

Rejected alternatives:

- **A literal `version: 0.35.1` on each call.** Renovate's github-actions manager doesn't track `with:` values of a
  local `uses: ./`, so six copies would rot.
- **Read the version out of `mise.toml` at run time.** One source of truth, but each of the six jobs needs a parsing
  step, and an empty parse silently falls back to `latest` in the action (`${INPUT_VERSION:-latest}`). The pin plus a
  consistency test is simpler and fails loudly.
- **Derive it from `tools/uv.lock`.** Same parsing cost, and `mise.toml` is the pin humans already maintain.
- **Keep `latest` and bump the lock in the release commit.** Makes the release depend on its own wheels being on PyPI
  before `uv lock` can see them. Circular.

## Scope and compatibility

- No runner or manifest schema change.
- CI and Renovate config only; no `UNRELEASED.md` entry.
- After a release, CI keeps using the previous binary until the Renovate "toolr release" PR merges. That's the point.
- Assumes F-24 and F-10 have landed. Touches `_prepare-release.yml` only in its `inputs:` block and the `Setup ToolR`
  step; touches `release.yml` only in `prepare-ci` and the `prepare-release` job's `with:`.
- The public action, its `inputs.version.default` and the `toolr-ci-setup` skill are unchanged: callers outside this
  repo aren't affected.
