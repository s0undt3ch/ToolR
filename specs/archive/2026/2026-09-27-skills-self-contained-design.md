# Self-contained agent skills

## Goal

Each skill under `skills/` must work when only its own directory is
installed, with no repository checkout and no network access. This is
how skillshare installs skills today, and how the mise packslip backend
will install them (see `2026-09-27-packslip-design.md`, which stacks on
this work).

An agent that uses an installed skill must never need to read `docs/`,
the docs site, or a GitHub page to do the skill's job.

## Non-goals

- No restructure of the docs into a separate fragment layer. The spike
  in "Why not a content layer" below explains why.
- No skill coverage of external command sources (`DispatchCommand`,
  argparse, Django). That is a separate gap.
- No fix for the static parser accepting unknown `arg()` keywords. That
  is a separate bug (see "Found, out of scope").

## Audit: what the skills lack today

Audit of `skills/*/{SKILL.md,references,examples}` against `docs/`,
2026-09-27.

### Explicit links out of the skill directory

- `toolr-command-authoring/SKILL.md` links
  `../../docs/writing-commands/arguments.md`, the supported-types table.
- `toolr-ci-setup/SKILL.md` links
  `docs/project-config.md#venv-location` on GitHub `main`.
- `toolr-command-packaging/SKILL.md` sends the reader to
  `examples/plugin-package/pyproject.toml`, which is outside the skill
  directory.
- 11 cross-skill links point to `github.com/s0undt3ch/toolr/tree/main/skills/…`.
  They read `main`, not the installed version.

### Knowledge the skills depend on without linking it

| Needed content | Current source | Size |
|---|---|---|
| Supported annotation types and `toolr.types` | `arguments.md` "Supported types" | ~56 lines |
| Path constraints (`must_exist`, `must_be_file`, `must_be_dir`) | `arguments.md` "Path constraints" | ~35 lines |
| How a signature maps to CLI shape: positionals, `T \| None`, defaults, bool flags, `Literal`, enums, `list[T]`, `*args`, tuples, `Count`, type aliases | `arguments.md`, remaining sections | ~110 lines of prose |
| Prek/pre-commit hook YAML for `build-manifest --check` (used by the ci-setup and packaging skills) | `third-party.md` "Pre-commit hook" | ~17 lines |
| `ctx.run` has no `check=`; a nonzero exit does not raise | `context.md` | ~5 lines |
| A command without a docstring is rejected | `docstrings.md` | ~5 lines |
| TOML form of `venv-location` | `project-config.md` | ~4 lines |
| Resolution order when project and plugin commands collide | `third-party.md` | ~12 lines |
| A plugin goes into the tools venv, not the ambient `python` | `project-config.md` | ~4 lines |

### The docs are wrong in places

- `arguments.md` "Path constraints" documents `arg(path_must_exist=True)`,
  `path_must_be_file` and `path_must_be_dir`. The real keywords are
  `must_exist`, `must_be_file` and `must_be_dir`
  (`crates/toolr-py/python/toolr/utils/_signature.py`,
  `crates/toolr-core/src/parser/types/path_constraints.rs`). Checked
  end to end: `--help` renders, then the command fails when run with
  `TypeError: arg() got an unexpected keyword argument 'path_must_exist'`.

So copying docs into skills as they are would ship docs bugs to agents.
Content with a source in code must be generated from code.

## Design

The rule: **one source of truth per fact, pulled into the skill by
`cargo xtask build-skill-refs`, and checked by
`cargo xtask build-skill-refs --check`**. That command already runs
first in `mise run test` and in CI.

The sources, in order of preference:

1. **Code**, when the fact is defined in code.
2. **A named docs section**, when the fact is prose that humans and
   agents both need.
3. **The skill itself**, for a handful of short facts that only the
   agent needs in that form.

### 1. Code-generated tables

**Supported types.** Add a catalogue in `toolr-core` next to
`SupportedType` (`parser/types/supported.rs`). It is one row per
variant: annotation spelling, what validates it, wire format, and the
Python type received.

- It is filled by an exhaustive `match` over the variants, so adding a
  variant without a row does not compile.
- A unit test checks that every name in `toolr.types.__all__` has a row,
  and that every `toolr.types.*` row appears in `__all__`. This closes
  a drift gap: today no generator walks `toolr.types.__all__`.

**Path constraints.** The same approach, from `PathConstraints`: field
name, effect, and implications (`must_be_file` implies `must_exist`).

**Output.** xtask renders both tables to:

- `skills/toolr-command-authoring/references/types.md`, for the skill;
- `docs/writing-commands/files/supported-types.md` and
  `docs/writing-commands/files/path-constraints.md`, which
  `arguments.md` includes through `--8<--` in place of the
  hand-written tables.

The docs therefore stop drifting from the code as well. Both are
generated files, so they are covered by `--check`.

The first plan task is to prove this works. The catalogue has to
express every column of the current table (for example `list[T]` "clap
per-element") without special cases that defeat the point. **If it
cannot, stop and bring the question back** rather than hard-code the
table in xtask.

### 2. Named docs sections

pymdown-extensions 11.0.2 (the version pinned in `uv.lock`) supports
named snippet sections:

```markdown
<!-- --8<-- [start:arg-shapes] -->
…prose…
<!-- --8<-- [end:arg-shapes] -->
```

Section markers go around these parts of the docs:

- `arguments.md`: everything from "Positional arguments" to "Counting
  flags" (`arg-shapes`).
- `third-party.md`: "Pre-commit hook" (`prek-hook`).

mkdocs ignores the markers when rendering the page. xtask gains a
section extractor that reads a named section and turns it into plain
Markdown an agent can read:

- expands `--8<-- "path"` and `--8<-- "path:start:end"` includes inline;
- converts `!!! kind "title"` admonitions to blockquotes
  (`> **Note — title:** …`);
- turns links into other docs pages and `#anchor` links into plain text;
- drops `{: …}` attribute lists;
- fails on any construct it does not know, rather than passing it
  through.

The extracted output goes to
`skills/toolr-command-authoring/references/arguments.md`, and to
`references/prek-hook.md` in both the ci-setup and packaging skills.
Every generated file starts with the header comment the existing
generated references use.

Only marked sections are extracted. Unmarked docs are never read by the
generator.

### 3. Short facts written into the skill

These go straight into the relevant `SKILL.md`, because generating a
five-line fact costs more than it saves:

- `ctx.run` does not raise on a nonzero exit; check `returncode`.
- Every command needs a docstring.
- `[tool.toolr] venv-location = "in-tree"`, the file form of
  `TOOLR_VENV_LOCATION`.
- Project commands win over plugin commands, a plugin can add to an
  existing group, and two plugins with the same command path fail the
  build.
- Install the plugin into the tools venv with `toolr project venv add`,
  not with the ambient `pip`.

### 4. Examples copied into the skill

xtask copies `examples/plugin-package/pyproject.toml` to
`skills/toolr-command-packaging/examples/pyproject.toml`. The skill
prose points at the local copy.

### 5. Cross-skill references by name

The 11 GitHub `main` links become references by name: "use the
`toolr-command-packaging` skill". Agents find skills by name, and a name
never points at the wrong version. The `README.md` files can keep links,
because they are for humans browsing the repo.

### 6. Fix the path-constraints docs

The table is replaced by the generated include, and the hand-written
example below it in `arguments.md` changes to `must_be_file=` and
`must_be_dir=`. That example sits inside the `arg-shapes` section, so
the skill gets the fix too.

### 7. Self-containment gate

`cargo xtask build-skill-refs --check` gains a lint over each skill's
shipped files: `SKILL.md`, `references/**` and `examples/**`. It skips
`README.md`, `REVIEW.md` and `tests/`, which are for maintainers. It
fails on:

- a Markdown link or image whose target resolves outside the skill
  directory, such as `../`;
- any URL under `github.com/s0undt3ch/toolr`, `github.com/s0undt3ch/ToolR`
  or `toolr.readthedocs.io`;
- a backticked repo path that does not exist inside the skill directory,
  such as `` `docs/…` `` or `` `examples/…` ``, where the path's first
  segment is a top-level repo directory.

Third-party URLs, such as termimad, are allowed. They are background
reading, not required knowledge.

The error names the file, the line, and the offending target.

## Why not a content layer

The alternative was a neutral `content/` directory of fragments that
both mkdocs and the skills include, so neither owns the source. The
audit found 7 needed docs sections (~210 lines), and one of them is
most of that. A two-layer docs model does not pay off at that size.

Named snippet sections are that fragment layer, only still inline. If
skills later need much more docs content, each marked region can move
to `content/` and be replaced by an include. That move is mechanical,
and nothing here would need undoing.

## Error handling

- A new `SupportedType` variant without a catalogue row fails to
  compile.
- A `toolr.types` name without a row fails a unit test.
- Docs or code edits that change generated output fail `--check` until
  someone regenerates.
- A new link out of a skill fails the gate.
- An unknown docs construct inside a marked section fails extraction.

## Testing

- Unit tests for the section extractor, one case per transform, plus the
  failure on an unknown construct.
- Unit tests for the gate: each rule has one fixture that fails and one
  that passes.
- A catalogue completeness test against `toolr.types.__all__`.
- `mkdocs build --strict` confirms the docs still render with the
  includes and markers.
- `mise run test`, the full suite, because this touches Rust.

## Found, out of scope

These are raised separately, not fixed here:

- The static parser ignores unknown `arg()` keywords
  (`path_constraints.rs`, `_ => {}`). A plugin built with
  `toolr self build-manifest` ships commands that fail on first run.
  It should reject them at manifest-build time.
- `third-party.md` uses hatchling `include`, and the packaging skill
  uses `packages`. Pick one.
- `third-party.md` says old manifest fragment schemas are migrated. The
  audit reports that `packaging.md` says otherwise. This is unverified.
- External command sources have no skill coverage.
- Idea under consideration: replace the `arg(must_exist/must_be_file/
  must_be_dir)` keywords with `toolr.types` path types (for example
  `ExistingPath`, `ExistingFile`, `ExistingDir`). A misspelled type
  fails at manifest-build time; a misspelled keyword is dropped
  silently. This needs its own design, including how it relates to
  `ResolvedPath` (already "canonicalised, must exist"). If it lands, the
  catalogue gains rows and the constraints table shrinks. Nothing here
  blocks it.
