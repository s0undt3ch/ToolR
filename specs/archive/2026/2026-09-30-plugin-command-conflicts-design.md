# Plugin command conflicts — design

Issue: [#522](https://github.com/s0undt3ch/ToolR/issues/522). Builds on #520
(`specs/archive/2026/2026-09-29-plugin-parity-design.md`, §3 and §5).

## 1. Why this, not #522 in full

Issue #522 proposes `[[tool.toolr.plugins.resolve]]` rules so a repo can pick the winner of a command
clash. Nobody has reported a clash. The full design (commit `9500ae06`, summarised on #522) needs
a config table, a change to the `static_hash` inputs, rule-aware carry-forward in both no-venv
paths, and dispatcher handling. That's a lot of machinery for a hypothetical problem.

One part isn't hypothetical. Two plugins that define the same command make the merge return
`ThirdPartyError::DuplicateCommand`, and then:

- with a cache: `ensure_manifest_fresh` → `warn_and_keep_cache`. toolr runs on a stale manifest
  and warns on every run;
- with no cache (a fresh clone, every CI runner): `ensure_manifest_present_or_bootstrap`
  propagates the error. `toolr: <err>`, exit 2. No command runs, local ones included.

That breaks #520's rule that a bad plugin must not stop toolr working. This change fixes that
and nothing more. Choosing a winner in config stays open on #522, and the warnings link there so
people can vote.

## 2. Behaviour

| Clash | Today | After |
| --- | --- | --- |
| Local vs plugin(s) | Local wins. One `Shadowed` warning per plugin. | Unchanged. The message also links #522. |
| Plugin vs plugin(s), no local | `DuplicateCommand` aborts the merge. | The command is disabled: no plugin's copy is merged. One `Conflict` warning names every plugin. Everything else merges. |

Why disable instead of "first wins": "first" is the sorted order of fragment paths. Installing
another plugin could silently change which command runs. A disabled command with a warning is
loud. A silently different one isn't.

A disabled command is missing from the manifest, so it vanishes from `--help`, completion and
dispatch with no extra code. All three are built from the manifest.

## 3. Changes

### 3.1 `crates/toolr-core/src/third_party/merge.rs`

`merge_into_manifest` becomes two passes over commands. `DuplicateCommand` was its only error,
so it becomes infallible and returns `Manifest`, not `Result`. The `.unwrap()`s in
`third_party/tests.rs` go. `discover_and_merge` stays `Result` for its `Json`, `MissingVersion`
and I/O errors. Groups merge as today, in the first
pass.

1. **Collect.** For each plugin command whose `(group, name)` isn't a local key, append
   `(package, path, Command)` to that key's entry. A package already in the entry is skipped,
   so a hand-edited fragment that lists one command twice counts once (first copy kept).
   `toolr self build-manifest` can't produce that fragment, so it isn't worth a warning.
   Keys live in a `Vec` in first-seen order,
   with a `HashMap` from key to index. A bare `HashMap` iterates in random order, and a
   `BTreeMap` sorts, so neither preserves discovery order. Discovery is already deterministic:
   `glob_manifests` sorts paths and fragments keep their command order. A plugin command that
   hits a local key gets its `Shadowed` warning here, in the same place as today.
2. **Emit.** Walk the keys in order. A key with one definition is merged as today:
   `Origin::ThirdParty`, `dispatched_from = None`, `is_dispatcher = false`. A key with two or more
   gets one `Conflict` warning and is not merged. "Two or more" means two or more distinct
   packages.

The first-come `owner` map goes. Warning order: `Skipped` (from parsing, unchanged), then all
`Shadowed` warnings in discovery order, then all `Conflict` warnings in key first-seen order.
Tests assert that order.

Groups aren't pruned. A plugin group whose only command was disabled stays, like a plugin group
declared with no commands, which #520 already allows. The visible effect:

- `toolr --help` lists the group, with the first plugin's title;
- `toolr <group>` prints the group's empty help and exits 2;
- completion offers the group.

Sibling commands and other groups aren't affected. clap 4.6 doesn't assert on a
`subcommand_required` command with no subcommands. The `Conflict` warning printed on the same
run explains the empty group.

### 3.2 Warnings

`crates/toolr-core/src/manifest/model.rs`: `PluginWarningKind` gains

```rust
/// A command defined by two or more plugins and no local command. It is disabled.
Conflict,
```

For `Conflict`, `package` is the first plugin in discovery order and `path` is that plugin's
fragment. The struct doesn't change.

Messages. `<cmd>` is the space-separated command path, as `shadow_message` builds it today. That
formatting moves to a shared `command_path(&Command)` helper.

- `Conflict`: `<cmd> is defined by more than one plugin (toolr_a, toolr_b), so it is disabled.
  Uninstall all but one. Choosing a winner in config is tracked in
  https://github.com/s0undt3ch/ToolR/issues/522`. Packages are listed in discovery order,
  comma-separated, however many there are.
- `Shadowed`: `tools/ci.py defines ci lint, hiding the one from demo. Choosing a winner in
  config is tracked in https://github.com/s0undt3ch/ToolR/issues/522`

The URL is one `const` in `merge.rs`, used by both messages.

`main.rs::run` already prints every warning on each run, and `--quiet` silences them. No change
there.

No warning in `crates/` carries a URL today. This one does on purpose: the link is how people
find #522 to vote. Two side effects:

- The message is stored in `.toolr-manifest.json`, so if the URL ever changes, the old one
  lingers until the next rebuild.
- It prints on every CI run that has a clash, which is the point.

If the warning example goes into skill prose, keep the URL in backticks.
`crates/xtask/src/build_skill_refs/self_contained.rs` rejects bare or linked
`github.com/s0undt3ch/ToolR` URLs in skills. Backticked ones are exempt.

### 3.3 Removals

- `ThirdPartyError::DuplicateCommand` (`third_party/parse.rs`) and its `Display` test
  (`third_party/tests.rs::third_party_error_duplicate_command_renders_both_packages`).
- `third_party/tests.rs::merge_errors_on_third_party_to_third_party_collision`, which does
  `.expect_err("should collide")`. It's rewritten as the "two plugins, same key" test in §5.
- The `DuplicateCommand` lines in the doc comments of `merge_into_manifest` and
  `discover_and_merge`. Those comments now describe the `Conflict` behaviour.

### 3.4 Cache and schema

No `SCHEMA_VERSION` bump. The manifest `SCHEMA_VERSION` is also the upper bound of the plugin
fragment load rule (`third_party/parse.rs`), so a bump would change which plugins load.
`Conflict` is a new variant of a snake_case enum. An older toolr that reads a newer cache fails
to deserialize it and treats the cache as missing. All three load sites do this:
`bootstrap.rs` and `complete/freshness.rs` via `.ok()`, `main.rs::load_or_empty`. It then
rebuilds. No golden test covers `PluginWarning`.

A released older binary also sees the `toolr_version` mismatch and rebuilds cleanly. A dev
build keeps its version string across commits. So a dev binary from before this change, reading
a post-change cache in a project with a clash, rebuilds with the old merge. It hits
`DuplicateCommand` and warns with no cache to fall back on. That's no worse than today, and
only affects developers switching branches.

Freshness doesn't change. A clash appears or goes away when a fragment is installed, removed or
edited, and `third_party_hash` already tracks all three. On `StaticDrift` with no venv,
`carry_forward_cached_entries` copies the cached warnings, `Conflict` included, which is right:
the plugin set hasn't changed.

A local command added with the same key as a disabled plugin command wins, as it would over any
plugin. The next run that resolves the venv re-merges, so the `Conflict` becomes `Shadowed`
warnings. Tab completion doesn't glob the venv. Until that run, it keeps the cached `Conflict`
warning and shows the local command. That's harmless.

## 4. Documentation

- `docs/third-party.md`, "Command resolution":
    - The plugin-vs-plugin bullet: replace "produce a manifest-build error" with the `Conflict`
      behaviour and an example of the warning.
    - The local-wins bullet already links #522. Add the same link to the plugin-vs-plugin
      bullet, framed as "vote on #522 if you need to choose a winner".
- `skills/toolr-command-packaging/SKILL.md`, "Command resolution": "two plugins ... fail the
  consuming project's manifest build" becomes "... the command is disabled with a warning".
  That's prose, so `build-skill-refs --check` won't catch it. Update it by hand.
- The `Manifest.plugin_warnings` doc comment (`manifest/model.rs`) says "skipped or shadowed".
  Change it to "skipped, shadowed or conflicting", then run `cargo xtask build-skill-refs`
  (`skills/toolr-command-packaging/references/packaging.md` quotes it).
- `UNRELEASED.md`: "Two plugins that define the same command no longer stop toolr from working.
  The command is disabled, and a warning names both plugins." This changes behaviour, so it's a
  minor release.

## 5. Tests

### Unit (`crates/toolr-core/src/third_party/tests.rs`)

- Two plugins, same key, no local (rewrites `merge_errors_on_third_party_to_third_party_collision`):
  not merged; one `Conflict` warning with the exact message, `package` and `path`; the plugins'
  other commands still merge.
- One fragment listing the same command twice: merged once, no warning.
- Three plugins, same key: one `Conflict` warning, all three named in discovery order.
- A nested key (`docker image build`) spells the path with spaces in the message.
- A top-level key (group `""`) spells only the name.
- Local plus two plugins: local wins, two `Shadowed` warnings, no `Conflict`. That pins
  local-first precedence.
- Warning order: `Skipped`, then `Shadowed`, then `Conflict`, across several fragments.
- The `Shadowed` message ends with the #522 pointer (update the existing exact-match
  assertions in `local_command_shadows_plugin_with_warning` and
  `shadow_message_spells_a_nested_group_with_spaces`).
- A group whose only command was disabled is kept.

### Integration (`crates/toolr/tests/plugin_warnings.rs` harness)

- A fresh project, no cache, a local group and two plugins clashing on one command, plus a
  third plugin with its own command:
    - The run exits 0, and a local command runs. The harness can't import plugin packages, so
      plugin commands are checked through `--help` and the manifest JSON, like
      `nested_groups_with_the_same_leaf_name_coexist`: the third plugin's command is listed,
      and the clashing one isn't in the manifest.
    - `toolr <group> --help` doesn't list the clashing command.
    - Completing the word after `toolr <group>` doesn't offer it.
    - Stderr has the `Conflict` warning once per run.
- Uninstall one of the clashing plugins: the next run merges the survivor's command (it's in
  `--help` and the manifest), and the warning is gone.
- The existing `shadowed_plugin_command_warns_then_recovers` test still passes, with its
  substring assertions unchanged.

### Verification

`mise run test`, `prek run --all-files`, `mkdocs build --strict`.

## 6. Process

- Branch `plugin-clash-resolution` (already created with `git-spice branch create`). The larger
  design from the first draft is removed in the same commit that adds this file. Git history and
  the #522 comment keep it.
- Plan: `specs/2026-09-30-plugin-command-conflicts-plan.md`.
- The implementing PR ends with `git mv` of this design and its plan to `specs/archive/2026/`.
- The PR doesn't close #522. It references it (`Refs #522`), because choosing a winner in config
  stays open for votes.
