# `toolr-ci-setup` skill

Agent skill that teaches LLM coding assistants how to wire the
`s0undt3ch/ToolR` GitHub Action into a repository's CI. The skill
is loaded from `SKILL.md` and the sibling `references/action.md`;
this README is for humans browsing the repo.

## Audience

Authors wiring toolr into a GitHub Actions workflow. The skill
assumes the repository already (or will shortly) have `tools/`
scaffolded by `toolr project init`. It is *not* the right skill for:

- **Authoring** the `tools/*.py` commands the workflow runs — that
  is the separate `toolr-command-authoring` skill (sibling
  directory under `skills/`).
- **Packaging** toolr commands as a distributable plugin wheel —
  that is `toolr-command-packaging`. This skill only owns the CI
  gate (`toolr self build-manifest --check`), not manifest
  generation itself.
- **Operating** toolr at runtime, outside CI — out of scope.

## How drift is prevented

The skill follows the same three-layer drift defense as the
existing two skills:

1. **Prose teaches shape.** `SKILL.md` is hand-written and explains
   the action conceptually (what it installs, what it verifies,
   what it caches) and offers two complete recipe workflows. It
   points at `references/action.md` for the input/output surface
   so the prose itself stays small and stable.
2. **`references/action.md` is regenerated from `action.yml`.**
   `cargo xtask build-skill-refs` reads the repo-root `action.yml`
   and renders its name, description, inputs, and outputs as a
   markdown table. The `--check` variant runs in CI on every PR —
   a change to the action's surface that forgets to regenerate
   the reference cannot land.
3. **The action itself is the canonical worked example.** The
   action is already maintained as load-bearing code in this
   repository, exercised by the release workflow on every release.
   The skill points consumers at `s0undt3ch/ToolR@<sha>` rather
   than reproducing the action's logic.

## Regenerating the references

```sh
cargo xtask build-skill-refs            # write
cargo xtask build-skill-refs --check    # fail on drift
```

The check runs in CI and as a `prek` hook. If it fails locally,
run without `--check` and review the diff.

## Files

```text
.
├── SKILL.md                  # frontmatter + body (loaded)
├── README.md                 # this file — human-facing
├── REVIEW.md                 # checklist for hand-written surfaces
├── references/
│   └── action.md             # generated; do not edit
└── tests/
    └── triggers.yaml         # should-fire / shouldn't-fire fixtures
```

## Installation

The skill is distributed via `skillshare` from the toolr repo.
See the toolr docs (`docs/skills.md`) for the user-facing flow.
