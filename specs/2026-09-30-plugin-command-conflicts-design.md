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

`merge_into_manifest` becomes two passes over commands. Groups merge as today, in the first
pass.

1. **Collect.** For each plugin command whose `(group, name)` isn't a local key, append
   `(package, path, Command)` to that key's entry. Keys live in a `Vec` in first-seen order,
   with a `HashMap` from key to index. A bare `HashMap` iterates in random order, and a
   `BTreeMap` sorts, so neither preserves discovery order. Discovery is already deterministic:
   `glob_manifests` sorts paths and fragments keep their command order. A plugin command that
   hits a local key gets its `Shadowed` warning here, in the same place as today.
2. **Emit.** Walk the keys in order. A key with one definition is merged as today:
   `Origin::ThirdParty`, `dispatched_from = None`, `is_dispatcher = false`. A key with two or more
   gets one `Conflict` warning and is not merged.

The first-come `owner` map goes. Warning order: `Skipped` (from parsing, unchanged), then all
`Shadowed` warnings in discovery order, then all `Conflict` warnings in key first-seen order.
Tests assert that order.

Groups aren't pruned. A plugin group whose only command was disabled stays, like a plugin group
declared with no commands, which #520 already allows.

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

- `Conflict`, two plugins: `<cmd> is defined by both toolr_a and toolr_b, so it is disabled.
  Uninstall one of them. Choosing a winner in config is tracked in
  https://github.com/s0undt3ch/ToolR/issues/522`
- `Conflict`, three or more: `... is defined by toolr_a, toolr_b and toolr_c, so ...`, with the
  same tail.
- `Shadowed`: `tools/ci.py defines ci lint, hiding the one from demo. Choosing a winner in
  config is tracked in https://github.com/s0undt3ch/ToolR/issues/522`

The URL is one `const` in `merge.rs`, used by both messages.

`main.rs::run` already prints every warning on each run, and `--quiet` silences them. No change
there.

### 3.3 Removals

- `ThirdPartyError::DuplicateCommand` (`third_party/parse.rs`) and its `Display` test
  (`third_party/tests.rs::third_party_error_duplicate_command_renders_both_packages`).
- The `DuplicateCommand` lines in the doc comments of `merge_into_manifest` and
  `discover_and_merge`. Those comments now describe the `Conflict` behaviour.

### 3.4 Cache and schema

No `SCHEMA_VERSION` bump. The manifest `SCHEMA_VERSION` is also the upper bound of the plugin
fragment load rule (`third_party/parse.rs`), so a bump would change which plugins load.
`Conflict` is a new variant of a snake_case enum. An older toolr that reads a newer cache fails
to deserialize it (`manifest/io.rs::load_manifest`), treats the cache as missing and rebuilds.
No golden test covers `PluginWarning`.

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

- Two plugins, same key, no local: not merged; one `Conflict` warning with the exact message,
  `package` and `path`; the plugins' other commands still merge.
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
    - `toolr` exits 0. The local command and the third plugin's command run.
    - `toolr <group> --help` doesn't list the clashing command.
    - Completing the word after `toolr <group>` doesn't offer it.
    - Stderr has the `Conflict` warning once per run.
- Uninstall one of the clashing plugins: the next run merges the survivor's command, and the
  warning is gone.
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
