# Misc maintainability: a dead test file and a misleading "legacy" label

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-43 (low).
- **Plan:** [`2026-10-31-misc-maintainability-plan.md`](./2026-10-31-misc-maintainability-plan.md)

## Problem

F-43 bundles four small items. PR #614 (edition 2024) fixed none of them. Each was checked against `main`:

| # | Item | Status | Action |
|---|---|---|---|
| 1 | `crates/toolr/tests/uv_install_offline.rs` | open | delete the file |
| 2 | duplicate crates in `cargo tree -d` | upstream only | none |
| 3 | `CommandDecorator::LegacyVar` in the parser | open, but the audit's framing is wrong | rename, don't remove |
| 4 | `MANIFEST_SCHEMA_VERSION` in `toolr.__all__` | deliberate public API | keep |

**1. Dead test file.** `uv_install_offline.rs` holds two tests. `host_asset_present_on_supported_targets` is
`let _ = host_asset();` and asserts nothing. `asset_url_points_at_astral_releases` checks the URL prefix and suffix.
Both are covered better in `crates/toolr-core/src/uv/install.rs` `mod tests`:
`host_asset_returns_a_known_triple_or_none` asserts the triple on supported hosts, and
`asset_url_points_at_latest_download` checks the URL. The file adds a test binary to link and nothing else.

**2. Duplicate crates.** `cargo tree -d --depth 0 -e normal` lists `signal-hook` 0.3 + 0.4, `syn` 2 + 3,
`unicode-width` 0.1 + 0.2, and `memchr` and `smallvec` once each.

- `signal-hook` 0.3 comes from `crossterm` 0.29, through `termimad`. toolr-core already uses 0.4.
- `unicode-width` 0.1 comes from `termimad`. 0.2 comes from `pep440_rs` and `annotate-snippets`.
- `syn` 3 comes from `clap_derive` and a few other proc-macro crates; the rest still use `syn` 2. Build time only.
- `memchr` and `smallvec` have a single version. They show up because the host (proc-macro) and target builds are
  separate units, not because two versions exist.

None of these is toolr's to fix. Downgrading toolr-core to `signal-hook` 0.3 would trade a newer dependency for an older
one. Renovate will pick up the upstream releases. F-15 (`chore/toolr-py-deps`) removes most of this tree from the
`toolr-py` build anyway.

**3. `LegacyVar`.** `crates/toolr-core/src/parser/commands.rs` names the `@<group>.command` decorator form
`CommandDecorator::LegacyVar`, parses it in `parse_legacy_command`, and calls it "legacy" in doc comments
(`extract_commands`, the enum variant, `parse_legacy_command`) and in five `legacy_*` test names.
`crates/toolr-core/src/parser/groups.rs` says "legacy binding style" in two comments. The audit took the label at face
value and suggested scheduling removal for 1.0.

The label is wrong. `CommandGroup.command`'s docstring in `crates/toolr-py/python/toolr/_decorators.py` calls it "the
canonical single-file form". The quickstart, the docs fixture repo and the authoring skill use it throughout (about 76
uses under `docs/`, `skills/`, `README.md` and `examples/`, against 22 for `@command(group=...)`). Only
`CommandGroup.command_group(...)` is deprecated. Removing `@group.command` would break most toolr projects. Keeping the
"legacy" label invites someone to try.

**4. `MANIFEST_SCHEMA_VERSION`.** It's in `toolr.__all__` with no Python consumer inside toolr. But the CHANGELOG
migration note for the argparse removal promised it stays public. The authoring skill's reference lists it
(`skills/toolr-command-authoring/references/commands.md`). `manifest_schema_version_lockstep.rs` keeps it in step with
Rust. Removing it would be a breaking change for plugin authors who read it, and it would need a skill-ref regen, for no
gain.

## Decision

- **Item 1:** delete `crates/toolr/tests/uv_install_offline.rs`. Nothing else references it.
- **Item 3:** rename, no behaviour change.
    - `CommandDecorator::LegacyVar` → `CommandDecorator::Binding`.
    - `parse_legacy_command` → `parse_binding_command`.
    - Tests: `legacy_decorator_rejects_positional_and_name_keyword` →
      `binding_decorator_rejects_positional_and_name_keyword`, `legacy_var_decorator_still_works` →
      `binding_decorator_works`, and `legacy_decorator_*` → `binding_decorator_*` for the other three. Panic messages
      `"expected LegacyVar"` → `"expected Binding"`.
    - Doc comments: "legacy binding style" → "group-binding style", and "Legacy:" on the variant → "Group binding:".
      The `Direct` variant's "Modern:" becomes "String path:", so neither form reads as the old one.
- **Items 2 and 4:** no change. This spec records why.

Rejected alternatives:

- **Deprecate `@group.command` for 1.0** (the audit's suggestion). It's the documented canonical form. Deprecating it
  would be a product change with wide breakage, and nothing argues for it.
- **Remove `MANIFEST_SCHEMA_VERSION` from `__all__`.** Breaks a promised public name for no benefit.
- **Pin `signal-hook` 0.3 in toolr-core** to drop one duplicate. Moves toolr-core backwards to suit a transitive
  dependency.

## Scope and compatibility

- No schema change, no public API change, no behaviour change.
- Docs: none.
- `UNRELEASED.md`: none.
