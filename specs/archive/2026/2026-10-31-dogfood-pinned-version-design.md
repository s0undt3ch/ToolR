# Pin the toolr binary CI dogfoods to the locked toolr-py

- **Status:** implemented
- **Origin:** whole-project review finding F-25 (low-medium, plausible).
- **Plan:** [`2026-10-31-dogfood-pinned-version-plan.md`](./2026-10-31-dogfood-pinned-version-plan.md)
- **Stacks on:** `fix/release-tree-binding` (F-10, `specs/archive/2026/2026-10-31-release-tree-binding-design.md`); merge that
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

Add one pin, `TOOLR_VERSION=0.35.1`, to `.github/env`, and one internal composite action,
`.github/actions/setup-toolr`, that every workflow calls instead of `uses: ./`. It reads `TOOLR_VERSION` from the
checked-out `.github/env`, fails with an `::error::` when the line is missing or empty, and calls `uses: ./` (a local
action path resolves against the workspace, so this works from inside a composite action) with that version and the
caller's `cache-prefix`. The fail-on-empty matters because the action installs `latest` when `version` is empty
(`${INPUT_VERSION:-latest}`).

Reading the file in the wrapper works the same in top-level and reusable workflows, so `_prepare-release.yml` needs
no `toolr-version` input and its callers relay nothing. An earlier revision of this change passed
`version: ${{ env.TOOLR_VERSION }}` at each call, relayed the pin into `_prepare-release.yml` through a
`prepare-ci` output and a required input, and put a "Require the toolr pin" guard step before every call; the
wrapper replaces all of that with one file.

Keep the pin in step with the lock:

- `renovate.json5`: a regex `customManager` for `.github/env`, matching `TOOLR_VERSION=(?<currentValue>\d+\.\d+\.\d+)`,
  `depNameTemplate: "s0undt3ch/ToolR"`, `datasourceTemplate: "github-tags"`,
  `extractVersionTemplate: "^v?(?<version>.*)$"`. It's the same datasource as the aqua pin in `mise.toml`, so both
  move together; the pin test keeps the group PR red until PyPI (`tools/uv.lock`) catches up.
  Add `"custom.regex"` to the `matchManagers` and `".github/env"` to the `matchFileNames` of the "Track our own toolr
  release in mise / GH Actions ASAP" rule, so the pin moves in the same "toolr release" PR as `mise.toml` and
  `tools/uv.lock`.
- `tests/test_dogfood_toolr_pin.py` fails when the three pins disagree, or when a workflow
  (`.github/workflows/*.y*ml`) or composite action (`.github/actions/*/action.y*ml`) other than the wrapper calls
  `uses: ./`. It runs the wrapper's pin-reading script under bash with a set, an empty and a missing
  `TOOLR_VERSION`, and checks the wrapper passes the version it read.

Rejected alternatives:

- **A literal `version: 0.35.1` on each call.** Renovate's github-actions manager doesn't track `with:` values of a
  local `uses: ./`, so six copies would rot.
- **Read the version out of `mise.toml` at run time.** Rejected first because each of the six jobs would need its
  own parsing step, and an empty parse silently falls back to `latest`. The wrapper removes both objections: one
  parsing step, which fails loudly. It still reads `.github/env` rather than `mise.toml`: a `KEY=VALUE` line needs no
  TOML parser, Renovate already tracks that file with a regex manager, and the consistency test ties it to
  `mise.toml`.
- **Derive it from `tools/uv.lock`.** Same parsing cost, and `mise.toml` is the pin humans already maintain.
- **Keep `latest` and bump the lock in the release commit.** Makes the release depend on its own wheels being on PyPI
  before `uv lock` can see them. Circular.

## Scope and compatibility

- No runner or manifest schema change.
- CI and Renovate config only; no `UNRELEASED.md` entry.
- After a release, CI keeps using the previous binary until the Renovate "toolr release" PR merges. That's the point.
- Assumes F-24 and F-10 have landed. Touches only the `Setup ToolR` steps of `ci.yml`, `release.yml` and
  `_prepare-release.yml`.
- The public action, its `inputs.version.default` and the `toolr-ci-setup` skill are unchanged: callers outside this
  repo aren't affected.
