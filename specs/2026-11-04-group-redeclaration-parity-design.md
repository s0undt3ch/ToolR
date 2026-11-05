# FU-22: group re-declaration parity

Status: ready to implement. Stacked on `fix/registry-test-isolation` (FU-12); needs a restack onto `main`
first, because the parity corpus from #674 is not in this stack yet.

## Decisions (2026-11-04)

- The Rust build warns on a conflicting group re-declaration through a non-persisted
  `Manifest.build_warnings`, printed once per rebuild. No schema bump.
- `long_description` isn't compared on the Rust side.
- The change gets a release note.
- Cross-tree behaviour (first-party wins silently) is pinned by a test.
- Amended with #677: Python logs its re-declaration message at DEBUG, so the Rust build warning is the
  user-facing signal. The parity test only requires that both sides report the same group and fields.
- A re-declaration without a description (`command_group("ci", "CI")`) fetches the group and has no
  opinion on the description, on both sides. Corpus case `ok_group_title_only_refetch`.
- Amended with #678: an empty title or description, passed or not, has no opinion either, so a later
  declaration fills it on both sides (Python fills `long_description` too). Only two non-empty values
  that differ conflict. Corpus cases `ok_group_described_after_title_only` and
  `ok_group_titled_after_description_only`.
- Plugin fragment builds (`build_commands`) drop the warnings: they are for the local `tools/` build.
- The first run can build twice (bootstrap, then the freshness step); `main.rs` prints a warning both
  builds give once.

## Problem

FU-12 made Python's `command_group` return the first group for a re-declaration and log a warning when
title or description differ. FU-12 could not confirm what the manifest build does. It was run
against scratch repos (dev binary vs `CommandsTester`, isolated XDG dirs, `TOOLR_TRUST_ALL=1`):

| Shape (one `tools/` tree) | Rust manifest | Python `CommandsTester` |
| --- | --- | --- |
| Identical twice, one module | one group, silent | one group, silent |
| Different title/description, one module | first wins, silent | first wins, warns |
| Same, across two modules (file order = import order) | first wins, silent | first wins, warns |
| Same, but `a_m` imports `z_m` first | `a_m` wins, silent | `z_m` wins, warns |
| Only `long_description` differs | silent (field not parsed) | warns |
| Nested `kid` under `p1` and under `p2` | two groups, silent | two groups, silent |
| `"p1.kid"` vs `"kid", parent="p1"` | one group, first wins, silent | same, warns |
| First-party `tools/` vs plugin, both declare it | `tools/` wins, silent (`third_party/merge.rs`) | not applicable |

Cause: `parser/build.rs` dedups with `seen_groups.insert(full_path)` and drops later bindings with no
check. Different parents already give different full paths on both sides, so they agree.

## Goals

- The manifest build warns on a conflicting re-declaration within one tree and stays silent when
  identical, like Python.
- Corpus cases pin first-wins and the warn/no-warn split on both sides.

## Non-goals

- Cross-tree (first-party vs plugin, plugin vs plugin). First-party wins and later fragments keep the
  existing group, silently. That is correct: a plugin and a local module may both declare a home group.
  Pin it with a test; do not change it.
- The import-order versus file-order winner. The parser reads files in sorted order and cannot follow
  imports. Both sides now warn and name the modules, which is the mitigation. Document it in
  `docs/writing-commands/groups.md`; no corpus case, since the sides legitimately differ.
- `long_description`: the manifest `Group` has no such field, so Rust cannot compare it.

## Design

### Detection

In `build_commands_inner`, replace the bare `seen_groups.insert` with a `HashMap<full_path, (module, Group)>`.
A later binding with the same path is dropped as today. If its `title` or `description` differ from the
first's, push a warning string. Identical bindings stay silent. The module path comes from `module_path`.

Message, mirroring Python's wording:
`group `grp` is declared more than once; keeping the one in tools.m1 and ignoring the different title,
description in tools.m2`.

### Reporting

`plugin_warnings` is per plugin (`package`, `path`, `kind`) and is reprinted on every run, so it does not
fit first-party code. The #674 duplicate-command problem is a hard `BuildError`; a group conflict is not
an error, so it cannot use that path.

Add `build_warnings: Vec<String>` to `Manifest` with `#[serde(skip)]`. It is filled only by a fresh
build, so there is no schema bump. `main.rs` prints it as `toolr: warning: ...` beside the
`plugin_warnings` loop. The warning appears when the manifest rebuilds (after an edit, `toolr project
manifest rebuild`, `toolr self build-manifest`) and not on cached runs, like Python's once-per-import log.
Chosen over a persisted field (schema bump, repeats forever) and over printing inside `toolr-core`.

### Corpus and parity test

New cases under `tests/e2e/files/parity_corpus/` (data only, no `__init__.py`):

- `ok_group_redeclared_identically`
- `ok_warn_group_conflict_one_module`
- `ok_warn_group_conflict_two_modules`
- `ok_warn_group_conflict_dotted_vs_parent`
- `ok_group_same_leaf_two_parents`
- `ok_group_title_only_refetch`

`test_parser_and_tester_agree` also compares per-group `title` and `description`, so first-wins is
asserted. For `ok_warn_*` it asserts both sides report the same (group, differing fields) pairs:
the Rust warning on stderr, the Python message in `caplog` at any level. Other `ok_*` cases report
none on either side.

## Testing

- Unit tests in `build.rs` for each shape: identical, conflicting one module, conflicting two modules,
  title-only, description-only, different parents. Real fixtures through `build_static_manifest`.
- `third_party` test: a fragment group that disagrees with `tools/` keeps the local one, no warning.
- `crates/toolr/tests/cli_smoke.rs`: the warning shows once on the first build and not on a cached run.
- All changed lines covered (99% patch gate).

## Release note

User-visible: `toolr project manifest rebuild` and first runs after an edit now warn about conflicting group
re-declarations. `toolr release-notes add --text "..."`.
