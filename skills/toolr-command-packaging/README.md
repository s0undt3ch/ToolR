# `toolr-command-packaging` skill

Agent skill that teaches LLM coding assistants how to ship an
existing set of toolr commands as a distributable Python plugin.
The skill is loaded from `SKILL.md` and the sibling `references/`
file; this README is for humans browsing the repo.

## Audience

Plugin authors taking already-written toolr commands and producing
a pip-installable package so other projects can `pip install` and
get the commands. The skill assumes:

- The commands themselves are already written and tested (use the
  sibling `toolr-command-authoring` skill if they aren't).
- The author has working knowledge of regular Python packaging
  (build backends, `pyproject.toml`, PyPI publishing). The skill
  documents only the toolr-specific delta.

It is **not** the right skill for:

- **Authoring** commands — sibling
  [`toolr-command-authoring`](../toolr-command-authoring/) skill.
- **Operating** an installed plugin (debugging, troubleshooting) —
  out of scope.

## How drift is prevented

Same three-layer model as the authoring skill, scoped to packaging:

1. **Prose teaches shape.** `SKILL.md` is hand-written and explains
   the three packaging rules (generate, include, gate). It points
   the agent at `references/packaging.md` for schema specifics so
   the prose itself stays small.
2. **`references/packaging.md` is generated from toolr-core.**
   `cargo xtask build-skill-refs` embeds the relevant Rust types
   verbatim using `// region:` / `// endregion:` markers in
   `crates/toolr-core/src/manifest/model.rs` and
   `crates/toolr-core/src/third_party/model.rs`. The `--check`
   variant runs in CI on every PR. A change to the host or
   fragment schema that forgets to regenerate the reference cannot
   land.
3. **`examples/plugin-package/` is the canonical example.** The
   skill does *not* ship its own examples directory — it points at
   the existing in-tree reference plugin. CI builds that wheel,
   asserts it contains `toolr-manifest.json` at the package root,
   and runs the `--check` red-path. Drift in the example fails CI.

## Regenerating the reference

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
│   └── packaging.md          # generated; do not edit
└── tests/
    └── triggers.yaml         # should-fire / shouldn't-fire fixtures
```

## Installation

The skill is distributed via `skillshare` from the toolr repo. See
the toolr docs (`docs/skills.md`) for the user-facing flow.
