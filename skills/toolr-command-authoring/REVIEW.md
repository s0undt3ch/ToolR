# Review checklist for `toolr-command-authoring`

The vast majority of this skill is generated from toolr's own source
and guarded by the `cargo xtask build-skill-refs --check` CI gate.
A small set of **hand-written load-bearing surfaces** is *not*
guarded by the generator and needs human review whenever it changes.
Use this checklist before landing edits to any of them.

## Hand-written surfaces

1. `SKILL.md` frontmatter `description:` — the trigger.
2. `SKILL.md` body — the conceptual narrative.
3. Cross-references from `SKILL.md` to `references/*.md`.
4. The closing pointer to the `toolr-command-packaging` skill.
5. `README.md` — human-facing intro (lower stakes but still
   hand-written).
6. `tests/triggers.yaml` — should-fire / shouldn't-fire fixtures.

## Checklist

When editing any of the surfaces above:

- [ ] **Trigger sanity (description).** Does the description still
      list at least two concrete phrases an agent would naturally
      use to signal authoring intent (e.g. `"add a toolr command"`,
      `"extend toolr"`, an explicit decorator name like
      `@command_group`)?
- [ ] **No false-positive overlap.** Does the description still
      explicitly disclaim packaging intent (so the packaging skill's
      trigger wins on `"ship as a plugin"` / `"include
      toolr-manifest.json in the wheel"`)?
- [ ] **No reference-content duplication.** Does the body still
      avoid restating decorator signatures, `Context` method
      lists, or section-header tables? Those belong in
      `references/` and are regenerated; duplicating them in prose
      reintroduces drift.
- [ ] **Anchors point to existing toolr UX.** When the body
      describes how to scaffold or rebuild, does it point at
      `toolr project init` / `toolr <group> <cmd> --help` /
      `toolr project manifest rebuild` rather than reproducing
      their content?
- [ ] **Examples reference is current.** Does the body still send
      readers to `examples/tools/` for a runnable layout, and does
      that directory still exist with passing snapshot tests?
- [ ] **Closing packaging pointer.** Does the closing section still
      send packaging-flavored intent to the packaging skill, and is
      the link still valid?
- [ ] **`tests/triggers.yaml`.** When adding a fixture, did you
      cover both the should-fire and shouldn't-fire sides? Are the
      shouldn't-fire entries scoped to plausible-but-out-of-scope
      requests (generic Python CLI work, packaging intent,
      operating existing commands) rather than nonsense inputs?
- [ ] **No regenerated content edited.** `references/commands.md`
      and `references/docstrings.md` are produced by
      `cargo xtask build-skill-refs`. If you find yourself editing
      them by hand, stop — the drift-defense contract is broken.

## When the user-facing surface of toolr-py changes

The references regenerate themselves. The hand-written narrative may
still need updates if:

- A new authoring concept lands (e.g. a new top-level decorator
  category beyond `command` / `command_group` / `arg` /
  `arg_section`). Mention it in `SKILL.md` and decide whether it
  needs its own section.
- A scaffold/manifest UX command renames or relocates (e.g. the
  workflow stops being `toolr project init`). Update the anchors in
  `SKILL.md`.
- The packaging skill's scope changes. Update the closing pointer.

Otherwise the narrative is independent of the surface — that's the
point of the three-layer design.
