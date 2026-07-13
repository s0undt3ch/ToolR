---
name: toolr-ci-setup
description: |
  Wire the `s0undt3ch/ToolR` GitHub Action into a caller
  repository's workflows. Use when setting up toolr in CI;
  when authoring `.github/workflows/*.yml` that runs a toolr
  command; when wiring `toolr self build-manifest --check`
  as a CI gate for a plugin repository; when picking the
  right pin form for `uses: s0undt3ch/ToolR@…`; when forcing an
  in-tree `tools/.venv` in CI via `TOOLR_VENV_LOCATION`; or when
  debugging the action's minimum-version error, attestation
  verify failures, or persistent venv cache misses. Triggers
  on phrases like "set up toolr in CI", "GitHub Actions for
  toolr", "use the toolr action", "cache toolr in CI",
  "in-tree venv in CI", "verify SLSA attestation in CI", and
  literal `uses: s0undt3ch/ToolR@` snippets. Stays inert on local
  authoring requests (covered by the `toolr-command-authoring`
  skill), on wheel-building outside a CI gate (covered by
  `toolr-command-packaging`), and on toolr's own internal
  `.github/actions/*` sub-actions.
---

# Setting up toolr in GitHub Actions

You are wiring the `s0undt3ch/ToolR` composite action into a
repository's CI. The action installs the toolr Rust binary, verifies
its SLSA build provenance, caches the binary and the per-repo
`tools/.venv`, and hands the next workflow step a `toolr` on PATH.
Your job is to produce (or modify) a `.github/workflows/*.yml` that
consumes the action correctly.

This skill teaches the **consumer side** of the action. The
authoritative input/output surface lives in
[`references/action.md`](references/action.md), regenerated from
`action.yml` on every release — read it when you need exact defaults
or argument shapes.

## What this skill covers

- Pinning `uses: s0undt3ch/ToolR@<sha>` correctly.
- A minimal one-step workflow that runs a toolr command.
- The two canonical recipes: running a toolr command in CI, and
  gating `toolr self build-manifest --check` for plugin repos.
- The three failure modes a typical caller hits first.

## What this skill does not cover

- Authoring or editing the `tools/*.py` commands the workflow runs —
  see the
  [`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring)
  skill.
- Building or shipping a toolr plugin wheel — see the
  [`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging)
  skill. This skill only covers the *CI gate* side
  (`--check`), not manifest generation itself.
- Non-action install paths in CI (manual `curl | sh` fallback,
  `mise-action`, self-hosted runner image baking). Use the action.
- Toolr's own internal `.github/actions/*` sub-actions
  (`apply-release-patch`, `configure-git`, `setup-pre-commit`,
  `setup-virtualenv`, `throttle`). Those are toolr's release
  plumbing, not a public consumer surface — do not call them from
  external repos.

## The minimum viable workflow

```yaml
name: toolr
on: [push, pull_request]
jobs:
  run:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: toolr <group> <cmd>
```

Replace `<sha>` with the full commit SHA of the release tag and
`<group> <cmd>` with the toolr command you want to run. That is
the whole surface for the common case — the action handles
attestation verification, caching, and venv setup.

## Pinning policy

Recommended default — **SHA-pinned with a version comment**:

```yaml
- uses: s0undt3ch/ToolR@a1b2c3d4e5f6...   # v0.20.0
```

This is the form GitHub's own security guidance recommends and the
form toolr itself uses for upstream actions (see `actions/cache` in
`action.yml`). The version comment lets a human reader (and
Dependabot) match the SHA to a release tag without resolving the ref.

Acceptable for prototypes — **tag-pinned**:

```yaml
- uses: s0undt3ch/ToolR@v0.20.0
```

Easier to write while iterating; trade reproducibility for readability.

**Do not use floating-major** (`@v0`) pre-1.0. Toolr's pre-1.0
contract permits breaking changes on minor bumps, so `@v0` is
effectively `latest` and may silently break a workflow.

The action enforces a minimum version of `0.20.0` (the first
binary-only release shape). Anything below that fails fast with a
clear error.

## Recipe 1 — Run a toolr command in CI

Single-OS form was shown above. For multi-OS coverage (use this when
your toolr commands shell out to platform-specific tooling or you
ship a plugin that must work cross-platform):

```yaml
name: toolr
on: [push, pull_request]
jobs:
  run:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: toolr <group> <cmd>
```

The action ships binaries for `x86_64`/`aarch64` Linux (glibc and
musl), `x86_64`/`aarch64` macOS, and `x86_64` Windows. The right
archive is selected automatically from `RUNNER_OS` + `uname -m`.

For the *authoring* side — how the `tools/<file>.py` defining the
command you're running is structured — see
[`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring).

## Recipe 2 — Gate plugin manifests with `--check`

When you ship toolr commands as a plugin wheel, the committed
`toolr-manifest.json` must match what `toolr self build-manifest`
would produce from the current source. The `--check` flag gives you
a non-zero exit on drift. Wire it as a CI gate so a stale manifest
cannot land:

```yaml
name: toolr-manifest
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: |
          toolr self build-manifest \
            --source-dir src/my_plugin \
            --package my_plugin \
            --check
```

Replace `src/my_plugin` with your plugin source directory and
`my_plugin` with the importable package name.

For the *generation* side — how to produce `toolr-manifest.json`
in the first place, what schema it follows, and how to include it
in the wheel — see
[`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging) —
this skill only owns the `--check` gate side.

## Recipe 3 — Run a command-package's tests in CI

For deterministic CI test runs, sync the venv explicitly, then run tests
against that already-synced venv rather than letting the run step re-sync:

```yaml
name: toolr
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: s0undt3ch/ToolR@<sha>   # v0.20.0
      - run: toolr project venv sync
      - run: toolr project venv run --no-sync -- pytest tools/
```

`--no-sync` makes the run step fail fast (instead of silently re-syncing) if
the venv is missing or stale — the deterministic behaviour you want in CI, as
opposed to the auto-sync-by-default `toolr project venv run` recipe used for
local iteration.

## Inputs and outputs at a glance

The full input/output surface (defaults, descriptions, what each
input controls) lives in
[`references/action.md`](references/action.md). It is regenerated
from `action.yml` on every release, so it cannot drift. Read it when
you need to override caching, point at a different release, or pass
extra `uv sync` flags.

## Where the venv lives in CI

Locally, toolr materialises the tools venv in a per-repo **cache**
directory by default (`$XDG_CACHE_HOME/toolr/<repo-key>/venv/`). That
path is volatile on CI runners, so the action exports
**`TOOLR_VENV_LOCATION=in-tree`** for you, which forces the venv to
`tools/.venv/` inside the checkout regardless of what
`[tool.toolr] venv-location` says in your `tools/pyproject.toml`. That
stable, in-checkout path is exactly what the `tools/.venv` cache
(see `cache-tools-venv`) keys on.

- **Using the action?** Do nothing — it sets the env var itself.
- **Running `toolr` in CI *without* the action** (a bare `run:` step,
  which this skill otherwise discourages)? Export it yourself so the
  venv lands somewhere cacheable:

  ```yaml
  - run: toolr <group> <cmd>
    env:
      TOOLR_VENV_LOCATION: in-tree
  ```

`TOOLR_VENV_LOCATION` accepts `in-tree` or `cache`; a typo is a hard
error rather than a silent fallback. See the [`venv-location`
reference](https://github.com/s0undt3ch/toolr/blob/main/docs/project-config.md#venv-location)
for the file-configured equivalent.

## Common failure modes

- **`refusing to install toolr <ver> — minimum supported version is
  0.20.0`** — the action enforces a `0.20.0` floor because earlier
  releases shipped as a Python package and are no longer compatible.
  Upgrade your pin to `0.20.0` or later; do not try to work around
  the check.
- **`gh attestation verify` fails on a fork** — the action verifies
  the SLSA build provenance of every downloaded archive. On runners
  without `gh` available, set `skip-attestation: true` *only* if you
  understand you are turning off the supply-chain gate. Prefer
  installing `gh` (it's already present on GitHub-hosted runners) or
  pre-baking it into self-hosted runner images.
- **`tools/.venv` cache misses every run** — the venv cache key
  hashes `tools/pyproject.toml`, `tools/uv.lock`, and `uv.lock`. If
  none of those are committed (or if your `tools/` layout is
  non-standard), the key never stabilises. Commit the lock files
  alongside `tools/pyproject.toml`. Local complement to the CI gate:
  the `--check` recipe above works equally well as a prek hook in
  your `pre-commit` config.

## Authoring and packaging are different problems

If you haven't written the toolr commands yet, this skill cannot
help you produce them. Invoke
[`toolr-command-authoring`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-authoring)
to write them, then come back here to wire the workflow. For
shipping commands as a distributable plugin, see
[`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging) —
this skill only owns the `--check` gate side.
