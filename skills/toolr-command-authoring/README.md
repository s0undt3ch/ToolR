# `toolr-command-authoring` skill

Agent skill that teaches LLM coding assistants how to author toolr
commands in a project's own `tools/*.py` files. The skill is loaded
from `SKILL.md` and the sibling `references/` files; this README is
for humans browsing the repo.

## Audience

Authors extending an existing toolr install with project-specific
commands. The skill assumes a repo that already has (or is about to
get, via `toolr project init`) a `tools/` directory. It is *not* the
right skill for:

- **Packaging** commands as a distributable plugin — that is the
  separate `toolr-command-packaging` skill (sibling directory under
  `skills/`).
- **Operating** or debugging the toolr Rust runtime — out of scope.
- **Authoring CLIs unrelated to toolr** — the trigger is scoped to
  toolr-shaped intent.

## How drift is prevented

The skill is structured around the three-layer drift defense
described in
`specs/archive/2026/2026-05-21-toolr-command-authoring-skill-design.md`:

1. **Prose teaches shape.** `SKILL.md` is hand-written and explains
   the model in conceptual terms (what a group is, how `ctx` flows,
   how decorators bind). It points the agent at `references/` for
   specifics so the prose itself stays small and stable.
2. **`references/` is regenerated from toolr's own source.**
   `cargo xtask build-skill-refs` walks `toolr.__all__` and the
   docstring parser's `KNOWN_SECTION_HEADERS` table to produce
   `references/commands.md` and `references/docstrings.md`. The
   `--check` variant runs in CI on every PR — a change to the
   public surface that forgets to regenerate the references cannot
   land.
3. **Examples are runnable and snapshot-tested.** The
   `examples/tools/` tree under this directory is a real `tools/`
   layout that toolr can introspect; the manifest + `--help`
   snapshots ship as committed fixtures. CI rebuilds them on every
   run, diffs against the committed copies, and fails on drift.

## Regenerating the references

```sh
cargo xtask build-skill-refs            # write
cargo xtask build-skill-refs --check    # fail on drift
```

The check runs in CI and as a `prek` hook. If it fails locally, run
without `--check` and review the diff.

## Files

```text
.
├── SKILL.md                  # frontmatter + conceptual body (loaded)
├── README.md                 # this file — human-facing
├── REVIEW.md                 # checklist for hand-written surfaces
├── references/
│   ├── commands.md           # generated; do not edit
│   └── docstrings.md         # generated; do not edit
├── examples/
│   └── tools/                # runnable example commands
└── tests/
    └── triggers.yaml         # should-fire / shouldn't-fire fixtures
```

## Installation

The skill is distributed via `skillshare` from the toolr repo. See
the toolr docs (`docs/skills.md`) for the user-facing flow.
