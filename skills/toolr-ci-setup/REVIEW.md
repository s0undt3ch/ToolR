# Review checklist for `toolr-ci-setup`

The input/output reference is generated and guarded by the
`cargo xtask build-skill-refs --check` CI gate. A small set of
**hand-written load-bearing surfaces** is not guarded by the
generator and needs human review whenever it changes. Use this
checklist before landing edits to any of them.

## Hand-written surfaces

1. `SKILL.md` frontmatter `description:` — the trigger.
2. `SKILL.md` body — the conceptual narrative.
3. `SKILL.md` "minimum viable workflow" snippet — copy-pasteable
   YAML; verify it still works against the current action.
4. `SKILL.md` two recipe workflows — Recipe 1 (run a command) and
   Recipe 2 (`--check` gate).
5. `SKILL.md` pinning-policy guidance.
6. `SKILL.md` common-failure-modes list.
7. Cross-references from `SKILL.md` to `references/action.md`.
8. Closing cross-link footer pointing to the authoring and
   packaging skills.
9. `README.md` — human-facing intro.
10. `tests/triggers.yaml` — should-fire / shouldn't-fire fixtures.

## Checklist

When editing any of the surfaces above:

- [ ] **Trigger sanity.** Does the description still list at least
      two concrete CI-flavored phrases (e.g.
      `"set up toolr in CI"`, `"GitHub Actions for toolr"`, the
      literal `uses: s0undt3ch/ToolR@…`)?
- [ ] **No false-positive overlap.** Does the description still
      explicitly disclaim authoring intent (so
      `toolr-command-authoring` wins on `"add a toolr command"`)
      and packaging intent outside the `--check` gate (so
      `toolr-command-packaging` wins on `"ship as a plugin"`)?
- [ ] **Pinning policy intact.** Does the body still recommend
      SHA-pinned with version comment as the default and discourage
      floating-major pre-1.0?
- [ ] **Minimum version still 0.20.0.** Does the body still name
      `0.20.0` as the floor, matching the action's enforced
      minimum in `action.yml`?
- [ ] **Recipes still complete.** Do both recipe workflows still
      parse as valid YAML and reference the action by the
      placeholder SHA form so callers know to substitute?
- [ ] **No reference-content duplication.** Does the body still
      avoid restating the full inputs/outputs table? Those belong
      in `references/action.md` and are regenerated.
- [ ] **Cross-link footer.** Does the closing section still point
      to both the authoring and packaging skills, and are the
      links still valid?
- [ ] **`tests/triggers.yaml`.** Are the shouldn't-fire entries
      scoped to plausible-but-out-of-scope requests (authoring,
      packaging outside the `--check` gate, non-toolr GitHub
      Actions work) rather than nonsense inputs?
- [ ] **No regenerated content edited.** `references/action.md`
      is produced by `cargo xtask build-skill-refs`. If you find
      yourself editing it by hand, stop — the drift-defense
      contract is broken.

## When `action.yml` changes

The reference regenerates itself. The hand-written narrative may
still need updates if:

- A new input is added that materially changes the consumer
  experience (e.g. a new caching toggle, a new auth mode). Mention
  it in the relevant recipe.
- The minimum supported toolr version changes. Update the body's
  "0.20.0" floor and the action's enforcement message.
- A failure mode shifts (e.g. attestation behaviour changes).
  Update the common-failure-modes list.

Otherwise the narrative is independent of the input/output surface
— that's the point of the layered design.
