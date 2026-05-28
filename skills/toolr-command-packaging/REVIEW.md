# Review checklist for `toolr-command-packaging`

The schema reference is generated and guarded by the
`cargo xtask build-skill-refs --check` CI gate. A small set of
**hand-written load-bearing surfaces** is not guarded by the
generator and needs human review whenever it changes. Use this
checklist before landing edits to any of them.

## Hand-written surfaces

1. `SKILL.md` frontmatter `description:` — the trigger.
2. `SKILL.md` opening pointer back to the authoring skill.
3. `SKILL.md` body — the three rules and the worked-example
   anchor.
4. `SKILL.md` migration paragraph (the `<!-- review after 1.0 -->`
   block) — dated; should be removed once 1.0 is out.
5. `SKILL.md` closing pointer to the authoring skill.
6. Cross-references from `SKILL.md` to `references/packaging.md`.
7. `README.md` — human-facing intro.
8. `tests/triggers.yaml` — should-fire / shouldn't-fire fixtures.
9. The inline pointer to the `toolr-ci-setup` skill inside rule 3.

## Checklist

When editing any of the surfaces above:

- [ ] **Trigger sanity.** Does the description still list at least
      two concrete packaging-flavored phrases (e.g.
      `"ship as a plugin"`, `"publish a toolr plugin"`, `"include
      toolr-manifest.json in the wheel"`)?
- [ ] **No false-positive overlap with authoring.** Does the
      description still explicitly disclaim authoring intent so the
      authoring skill's trigger wins on `"add a toolr command"` /
      `"extend toolr"`?
- [ ] **Anchored on `examples/plugin-package/`.** Does the body
      still send readers to the in-tree reference plugin rather
      than describing a fictional one? Does the path still exist?
- [ ] **The three rules are intact.** Does the body still cover
      the generate / include / gate trio explicitly?
- [ ] **No schema-content duplication.** Does the body still avoid
      restating field names from the manifest? Those belong in
      `references/packaging.md` and are regenerated.
- [ ] **Migration paragraph is still relevant.** If 1.0 has
      shipped, this paragraph should be removed; otherwise keep
      the `<!-- review after 1.0 -->` marker so a future cleanup
      pass knows when it can go.
- [ ] **Closing authoring pointer.** Does the closing section
      still send authoring-flavored intent back to the authoring
      skill, and is the link still valid?
- [ ] **CI-setup pointer in rule 3.** Does rule 3 still link out
      to the CI-setup skill alongside the prek-hook mention, and
      is the link still valid?
- [ ] **`tests/triggers.yaml`.** Are the shouldn't-fire entries
      scoped to plausible-but-out-of-scope requests (authoring,
      generic packaging, runtime debugging) rather than nonsense
      inputs?
- [ ] **No regenerated content edited.** `references/packaging.md`
      is produced by `cargo xtask build-skill-refs`. If you find
      yourself editing it by hand, stop — the drift-defense
      contract is broken.

## When the manifest or fragment schema changes

The reference regenerates itself. The hand-written narrative may
still need updates if:

- A new packaging-relevant concept lands (e.g. a runtime-required
  manifest field a plugin must populate). Mention it explicitly in
  `SKILL.md` body so plugin authors know to set it.
- A build backend's "include data file in wheel" behaviour
  changes. Update the body and verify
  `examples/plugin-package/pyproject.toml` still works.
- The plugin discovery glob changes (`crates/toolr-core/src/
  third_party/glob.rs`). The reference rebuilds, but if the new
  layout is meaningfully different the body should mention it too.

Otherwise the narrative is independent of the schema — that's the
point of the layered design.
