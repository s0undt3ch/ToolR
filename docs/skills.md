# Agent skills

Toolr ships two in-tree **agent skills** for LLM coding assistants
(Claude Code, Copilot CLI, Gemini, etc.). Skills bundle a trigger,
hand-written conceptual prose, and a set of generated references
into a single package that downstream agents load on demand.

The skills live under `skills/` in the toolr repo and are distributed
via [`skillshare`](https://github.com/skillsharehub/skillshare) from
this repository. Once installed, an agent working in your codebase
picks them up automatically when its prompt mentions toolr-shaped
intent.

## Skills

| Skill | When it fires | What it teaches |
| --- | --- | --- |
| **`toolr-command-authoring`** | Adding, editing, refactoring a toolr command in a project's own `tools/*.py` files. | The `@command` / `@command_group` decorator surface, the `Context` object, docstring-driven `--help`, the local feedback loop. |
| **`toolr-command-packaging`** | Shipping an already-written set of toolr commands as a distributable Python plugin. | Generating `toolr-manifest.json` via `toolr self build-manifest`, including it in the wheel, wiring `--check` as a CI gate. |
| **`toolr-ci-setup`** | Wiring `s0undt3ch/ToolR` into a caller repo's GitHub Actions workflow. | The action's inputs and outputs, recommended pin form, two canonical recipes (run a command; gate `--check`), common failure modes. |

The two triggers are scoped so authoring requests never fire the
packaging skill and vice versa. If you're unsure which one you need,
the rule of thumb is: **authoring is about extending toolr in your
own repo; packaging is about shipping commands so other repos can
install them.**

## Installation

Skillshare lets you install from the parent path and pick what
you want, so the install command does not grow as the skill set
evolves:

```sh
# Pick which skills to install (interactive)
skillshare install s0undt3ch/toolr/skills

# Or install everything non-interactively
skillshare install s0undt3ch/toolr/skills --all

# Or pick by name (e.g. just CI setup)
skillshare install s0undt3ch/toolr/skills -s toolr-ci-setup
```

Substitute your platform's skill-install command if you're not on
`skillshare`; the layout (`SKILL.md` + sibling `references/`) is
Claude Code-compatible and the references files are plain Markdown
that any platform can ingest.

## Managing installed skills

Listing, updating, pinning, and removing installed skills is
`skillshare`'s job — see the
[`skillshare` documentation](https://github.com/skillsharehub/skillshare)
for the full command surface. Toolr ships the skills; how you
manage them on your machine is owned upstream.

## How the references stay correct

Each skill ships a `references/` directory with files generated from
toolr's own source by `cargo xtask build-skill-refs`:

- `toolr-command-authoring/references/commands.md` is rebuilt by
  walking `toolr.__all__` and rendering every public name's
  signature and docstring.
- `toolr-command-authoring/references/docstrings.md` is rebuilt
  from `KNOWN_SECTION_HEADERS` in
  `crates/toolr-core/src/docstrings.rs`.
- `toolr-command-packaging/references/packaging.md` is rebuilt
  by extracting marker-delimited Rust regions from
  `crates/toolr-core/src/manifest/model.rs` and
  `crates/toolr-core/src/third_party/model.rs`.
- `toolr-ci-setup/references/action.md` is rebuilt from the
  repository-root `action.yml`, so the inputs/outputs surface the
  skill points agents at cannot drift from what the action
  actually accepts.

A `cargo xtask build-skill-refs --check` gate runs in CI on every
PR; a public-surface change that forgets to regenerate the
references cannot land. End users never run the regenerator — they
consume what `skillshare` distributes.

## See also

- The skill source layout lives under
  [`skills/`](https://github.com/s0undt3ch/toolr/tree/main/skills) in
  the repository — open `SKILL.md` in either directory to see the
  raw skill an agent loads.
- The design specs (now archived) describe the three-layer drift
  defense and the reasoning behind each decision; see
  `specs/archive/2026/2026-05-21-toolr-command-authoring-skill-design.md`
  and the packaging counterpart.
