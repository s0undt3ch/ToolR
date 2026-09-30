# Plugin clash resolution — design

Issue: [#522](https://github.com/s0undt3ch/ToolR/issues/522). Builds on #520
(`specs/archive/2026/2026-09-29-plugin-parity-design.md`, §3 and §5).

## 1. Goal

The repo owner picks the winner of a command clash in `tools/pyproject.toml`. Until they do:

- local vs plugin: local wins, with a `Shadowed` warning (unchanged);
- plugin vs plugin: only that command is disabled, with a `Conflict` warning that names every
  plugin and gives the config that fixes it. The rest of the CLI keeps working.

A bad rule never breaks the CLI. It produces a warning. A malformed table is a config error.

Out of scope: a command that lists clashes, rules for groups (only commands), and wildcard rules.

## 2. Config

```toml
[[tool.toolr.plugins.resolve]]
command = "ci lint"
use = "local"

[[tool.toolr.plugins.resolve]]
command = "docker image build"
use = "toolr_docker"
```

- `command`: the CLI path, as typed after `toolr`. The last word is the command name. The words
  before it, joined by `.`, are the group's `full_path()`. `"docker image build"` →
  `("docker.image", "build")`. `"hello"` → `("", "hello")`. Leading, trailing and repeated
  whitespace is ignored.
- `use`: `"local"`, or a plugin's `ManifestFragment.package` (the Python package name, such as
  `toolr_docker`, not the distribution name). No normalisation: the match is exact.

### Parsing

New module `crates/toolr-core/src/third_party/config.rs`. It follows
`crates/toolr-core/src/argparse/config.rs`: private `Root → Tool → Toolr → Plugins` serde
structs, each `#[serde(default)]`, so a missing table means no rules.

```rust
pub struct ResolveRule {
    pub command: String,
    #[serde(rename = "use")]
    pub winner: String,
}

pub struct Rules(Vec<ResolveRule>);   // parse order kept

pub fn parse_rules(toml_text: &str) -> Result<Rules, ConfigError>;
pub fn load_rules(tools_dir: &Path) -> Result<Rules, ConfigError>;   // no pyproject → empty
```

`Rules` exposes `key(&ResolveRule) -> (String, String)` and `for_key(&(String, String)) ->
Option<&ResolveRule>`.

These are config errors. They abort, like a bad `[tool.toolr.argparse.*]` block:

- a missing `command` or `use`, or a value of the wrong type (serde);
- an empty `command` (after trimming) or an empty `use`.

A second rule with the same `command` key isn't malformed, just wrong, so it's a rule problem:
the first rule wins and each later duplicate gets an `InvalidRule` warning (§4.3).

The module has its own `third_party::config::ConfigError` (`failed to parse
tool.toolr.plugins: …`). Reusing argparse's would label plugin errors as argparse ones.
`BuildError` gets a `PluginsConfig` variant. Where it surfaces:

- dispatch with a cache: `ensure_manifest_fresh` → `warn_and_keep_cache`, the old manifest keeps
  working;
- dispatch with no cache (fresh clone): `ensure_manifest_present_or_bootstrap` propagates it,
  `toolr: <err>`, exit 2, `--help` included. Same as a bad argparse block today;
- tab completion: `dispatch.rs` swallows `resolve_manifest_at_tab` errors and completes
  built-ins only;
- `toolr project manifest rebuild`: a hard error.

No existing `[tool.toolr]` deserializer uses `deny_unknown_fields` (checked:
`argparse/config.rs`, `venv/config.rs`), so the new table can't break them.

## 3. Cache invalidation

`hash_tools_dir` (`crates/toolr-core/src/hash.rs`) hashes only `tools/**/*.py`. Editing
`tools/pyproject.toml` doesn't refresh the cache today. That already breaks argparse blocks, and
it would break resolve rules.

Change: `hash_tools_dir` also hashes `tools/pyproject.toml` when it exists, under its relative
path, the same way as a `.py` file. That's only the top-level file, not every `pyproject.toml`
under `tools/`. The `ignores_non_py_files` test becomes two: `pyproject.toml` changes the hash,
and other non-`.py` files still don't.

Every existing cache drifts once on upgrade. The `toolr_version` check forces that rebuild anyway.

`tools/uv.lock` stays out. The manifest describes the installed venv, not the lock. After
`uv sync`, a plugin that changed shows up through `third_party_hash`, which hashes every
installed `toolr-manifest.json` (`manifest_build/hash.rs`). Before the sync, the venv still has
the old plugin, so a rebuild would produce the same manifest.

No `SCHEMA_VERSION` bump. The manifest `SCHEMA_VERSION` is also the plugin fragment load-rule
upper bound (`third_party/parse.rs`), so a bump would change which plugins load. The new warning
kinds are additive enum variants, and `PluginWarning.command` is an optional field that
serde ignores when it's unknown (no `deny_unknown_fields`). An older toolr that reads a newer
cache with a new kind fails to deserialize
it, treats it as missing and rebuilds. No golden test covers `PluginWarning`.

## 4. Merge

`merge_into_manifest(base, fragments, &rules)` in `crates/toolr-core/src/third_party/merge.rs`.
`discover_and_merge(tools_venv, base, &rules)` passes the rules through.
`build_static_manifest_with_venv` loads them with `load_rules(tools_dir)`.

### 4.1 Steps

1. **Groups.** Merge as today, by `full_path()`. The first definition keeps its title.
2. **Collect.** For each `(group, name)`, record the local command (if any) and every plugin
   definition in discovery order: `(package, fragment path, Command)`. Nothing is resolved yet.
   The first-come `owner` map goes. Keys live in a `Vec` in first-seen order (local commands in
   `base.commands` order, then plugin keys in discovery order), with a `HashMap` from key to
   index. Not a bare `HashMap` (random order) and not a `BTreeMap` (sorted, not discovered).
   Discovery itself is deterministic: `glob_manifests` sorts paths and fragments keep their
   command order.
3. **Resolve.** Apply §4.2 to each key. The result says which plugin command, if any, is merged
   and whether the local command is removed. It also records warnings and whether a rule was
   used.
4. **Validate rules.** A rule that matched no key, or that §4.2 marks invalid, gets an
   `InvalidRule` warning (§4.3).
5. **Prune.** Remove each `Origin::ThirdParty` group that resolution emptied: it had at least
   one plugin command before step 3, and has no commands and no child groups after it. Repeat
   up the tree, so a parent emptied that way goes too. Such a group would otherwise show in
   `--help` and fail with "subcommand required". A plugin group declared with no commands stays,
   as #520's parity rule requires. Local groups are never pruned.

Merged plugin commands keep today's tagging: `Origin::ThirdParty`, `dispatched_from = None`,
`is_dispatcher = false`.

### 4.2 Resolution table

`L` = local definition present. `P` = number of plugin definitions.

| Definitions | No rule | `use = "local"` | `use = "<pkg>"`, and `<pkg>` defines it |
| --- | --- | --- | --- |
| `L`, `P ≥ 1` | Local wins. One `Shadowed` warning per plugin. | Local wins. No warning. | `<pkg>` wins. The local command is removed (§4.4). The other plugins are dropped without a warning. |
| no `L`, `P ≥ 2` | Nothing merged. One `Conflict` warning. | Invalid rule (no local command). Falls back to *No rule*. | `<pkg>` wins. No warning. |
| `L` only, or `P = 1` only | Nothing to resolve. | Invalid rule (matches no clash). Falls back to *No rule*. | Invalid rule (matches no clash). Falls back to *No rule*. |
| none | — | Invalid rule (no such command). | Invalid rule (no such command). |

Precedence when a cell could match more than one message: a `use = "<pkg>"` that doesn't define
the command always reports "doesn't define it", even when only one source defines it (`P = 1`,
another package named). `use = "local"` with no definitions at all reports "nothing defines
it".

A key whose local definition is an argparse grafted child (`dispatched_from.is_some()`) is a
special case. With no rule, or `use = "local"`, it behaves like any local command. With `use =
"<pkg>"` the rule is invalid ("grafted from `<source>`, resolve its dispatcher instead") and
falls back to *No rule*. Replacing one grafted child would leave its dispatcher inconsistent.

Out of scope, and unchanged: a plugin group whose full path equals a local command's dotted
name (for example a plugin `ci.django` group next to a local `ci django` dispatcher). That's a
group-vs-command clash, not a command clash, and it behaves as it does today.

`use = "<pkg>"` where `<pkg>` isn't installed, or doesn't define that command: invalid rule.
Falls back to *No rule*. This covers a winner that was later uninstalled. Its fragment leaves
the venv, `third_party_hash` changes, the next run re-merges and reports the rule.

"Falls back to *No rule*" means the clash resolves as if the rule weren't there. So a stale rule
on a plugin-vs-plugin clash gives both an `InvalidRule` and a `Conflict` warning.

### 4.3 Warnings

`PluginWarningKind` gains two variants:

```rust
/// A command defined by two or more plugins and no local command, with no valid rule. Disabled.
Conflict,
/// A `[[tool.toolr.plugins.resolve]]` rule that couldn't be applied.
InvalidRule,
```

`PluginWarning` gains one additive field, `command: Option<String>` (`#[serde(default,
skip_serializing_if = "Option::is_none")]`). It holds the space-separated command path for
`Shadowed`, `Conflict` and `InvalidRule`, and is `None` for `Skipped`. §5 uses it to drop
warnings that a rule change made stale. The `path` doc comment ("the plugin's
`toolr-manifest.json`") is widened, because `InvalidRule` points at `tools/pyproject.toml`.

| Kind | `package` | `path` |
| --- | --- | --- |
| `Conflict` | the first plugin in discovery order | that plugin's fragment |
| `InvalidRule` | the rule's `use` value | `tools/pyproject.toml` |

Messages (`<cmd>` is the space-separated path, as `shadow_message` prints it today):

- `Conflict`: `<cmd> is defined by toolr_a and toolr_b, so it is disabled. Choose one in
  tools/pyproject.toml: [[tool.toolr.plugins.resolve]] command = "<cmd>", use = "toolr_a"`.
  Three or more plugins: `toolr_a, toolr_b and toolr_c`.
- `InvalidRule`, package missing or not defining it: `tools/pyproject.toml resolves <cmd> to
  <pkg>, but <pkg> doesn't define it. Defined by: local, toolr_b`. With no definitions left,
  `Defined by:` becomes `nothing defines it`.
- `InvalidRule`, `use = "local"` with no local command: `tools/pyproject.toml resolves <cmd> to
  local, but tools/ doesn't define it`.
- `InvalidRule`, no clash: `tools/pyproject.toml resolves <cmd>, but only <source> defines it.
  Remove the rule`.
- `InvalidRule`, no definitions: `tools/pyproject.toml resolves <cmd>, but nothing defines it.
  Remove the rule`.
- `InvalidRule`, grafted child: `tools/pyproject.toml resolves <cmd> to <pkg>, but <cmd> is
  grafted from <source>. Resolve its dispatcher instead`.
- `InvalidRule`, duplicate: `tools/pyproject.toml resolves <cmd> more than once. Only the first
  rule is used`.

Order in `plugin_warnings`: `Skipped` (from parsing), then `Shadowed` and `Conflict` in key
discovery order, then `InvalidRule` in rule order. That order is deterministic, so tests can
compare stderr exactly.

`main.rs::run` already prints every `plugin_warning` on each run, and `--quiet` silences them. No
change there.

`ThirdPartyError::DuplicateCommand` is deleted, along with its mention in the
`discover_and_merge` doc comment.

### 4.4 Removing a local command

When `use = "<pkg>"` beats a local command, remove it from `base.commands`. If it's an argparse
dispatcher (`is_dispatcher`), also remove its grafted children: the commands whose
`dispatched_from.is_some()` and whose `group` equals the dispatcher's dotted name.

The dotted name isn't always `<group>.<name>`. A hoisted dispatcher (`command_group("django")` +
`def django`) has the dotted name `django`, not `django.django`. Two copies of that logic exist
today: `parser/build.rs::dotted_name` and `cli.rs::dispatcher_dotted_name`. Replace both with one
`Command::dotted_name()` method in `manifest/model.rs`, and use it in the merge and in the §5
helper. A test covers the hoisted case.

The plugin command that replaces it is a plain leaf. Plugins can't be dispatchers. The docs say
so.

## 5. Paths with no venv

Two paths rebuild the local layer and carry cached plugin entries forward without globbing the
venv:

- `complete/freshness.rs::preserve_non_static_entries` (tab completion);
- `bootstrap.rs::carry_forward_cached_entries` (dispatch on `StaticDrift`, when the venv can't
  be resolved).

Both drop a cached plugin command when the fresh local build has the same key. That would undo a
`use = "<pkg>"` rule on every static-only rebuild, so the winner would differ between dispatch
and completion.

Change: a shared helper `third_party::carry_forward_plugin_commands(fresh, cached_cmds, &rules)`.
A cached plugin command whose key collides with a fresh local command is kept, and the local
command (and its grafted children, §4.4) is removed, when the rule for that key names anything
other than `"local"`. Otherwise it's dropped, as today. The cache holds at most one plugin entry
per key, so the helper doesn't need to know the package. Both call sites load rules with
`load_rules(tools_dir)`. A `ConfigError` is propagated the same way the static build's own
errors are there (§2 lists where that surfaces).

The helper also fixes the warnings it carries forward, since `carry_forward_cached_entries`
persists the manifest. Using `PluginWarning.command` and the current rules, it drops:

- an `InvalidRule` warning whose command no longer has a rule;
- a `Shadowed` warning whose command now has `use = "local"`.

Everything else needs the venv to recompute, so it's kept until the next run that has one.

Known limit: without a venv, a rule can only keep what the cache already holds. A rule that was
just added, or changed from `toolr_a` to `toolr_b`, takes effect on the next run that can
resolve the venv. Normal dispatch always resolves it, so only tab completion (and dispatch with
a broken venv) sees the old winner, and only until the next normal run. The docs say so.

## 6. Documentation

- `docs/third-party.md`, "Command resolution": rewrite. Remove "plugins sharing a name fail the
  build" and the #522 link. Give the four outcomes (local wins, a rule picks the winner,
  plugin-vs-plugin disabled, invalid rule) and link to the config reference.
- `docs/project-config.md`: a new section `## [tool.toolr.plugins]`, next to the
  `[tool.toolr]` options. It's the reference: both keys, the command-path mapping, the rule
  table from §4.2 in user terms, the four error cases, the warnings, and the no-venv limit.
- Examples live as files under `docs/project-config-files/`. The docs include them with
  `--8<--`, and they're the only copy:
    - `resolve-plugin-wins.toml`: a plugin beats a local command.
    - `resolve-keep-local.toml`: keep local and silence `Shadowed`.
    - `resolve-plugin-conflict.toml`: choose between two plugins.
    - `resolve-stale-rule.toml`: a rule for an uninstalled plugin.
    - `resolve-*.stderr.txt`: the exact warning output for the unresolved-conflict and
      stale-rule cases.
  Only `.md` needs `exclude_docs` in `mkdocs.yml`. `.toml` and `.txt` aren't pages.
- `skills/toolr-command-packaging/SKILL.md`, "Command resolution": update the prose by hand.
- The `Manifest.plugin_warnings` doc comment says "skipped or shadowed". Update it, then run
  `cargo xtask build-skill-refs`, because `references/packaging.md` quotes it.
- `UNRELEASED.md`: the new table; plugin-vs-plugin clashes no longer abort; editing
  `tools/pyproject.toml` now refreshes the manifest (this fixes argparse blocks too). Minor
  release.

## 7. Tests

### Unit (`toolr-core`)

- `third_party/config.rs`: a valid parse; the key mapping (top level, nested, extra whitespace);
  a missing key; the wrong type; an empty `command`; an empty `use`; a duplicate `command`; no
  table; no pyproject.
- `third_party/tests.rs`: every cell of §4.2, with the exact warning kind, package, path,
  command and message. Also:
    - three plugins with no rule, and with `use = "<pkg>"`;
    - local plus two plugins, both with no rule and with `use = "<pkg>"`;
    - `use = "<pkg>"` for an uninstalled package;
    - pruning, including a nested empty plugin group, and a local group left alone;
    - removing a local dispatcher and its grafted children, hoisted (`django`/`django`) and
      not (`ci`/`django`);
    - `use = "<pkg>"` on a grafted child is invalid; `use = "local"` on one is valid;
    - a duplicate rule: the first wins and the second warns;
    - a plugin group declared with no commands survives, one emptied by a `Conflict` is pruned;
    - warning order;
    - no `DuplicateCommand` path left.
- `hash.rs`: `pyproject.toml` counts; other non-`.py` files don't.
- `freshness` and the carry-forward helper: a rule that names a plugin keeps the cached plugin
  command over a fresh local one; `use = "local"` and no rule drop it; a removed rule drops its
  cached `InvalidRule`; a new `use = "local"` drops the cached `Shadowed`.
- `Command::dotted_name`: top level, nested, hoisted.

### Integration (`crates/toolr/tests/plugin_warnings.rs` harness)

- An unresolved plugin-vs-plugin conflict: local and other plugin commands still run; the command
  is gone from `toolr <group> --help` and from completion output; the `Conflict` warning prints.
- Adding a rule by editing only `tools/pyproject.toml` takes effect on the next run.
- A rule picks `toolr_a`. Uninstall `toolr_a`: the next run prints the `InvalidRule` warning and
  falls back.
- `use = "<pkg>"` over a local command survives a static-drift run with no resolvable venv.
- A malformed table: dispatch warns and keeps the cache; with no cache it exits 2;
  `toolr project manifest rebuild` fails.

### Docs examples (`crates/toolr/tests/docs_examples.rs`)

For each `docs/project-config-files/resolve-*.toml`, build a fake project with the local
commands and fake plugins the example describes. Append the example file verbatim to
`tools/pyproject.toml`. Run `toolr`. Assert:

- the documented winner (`toolr <cmd> --help` shows the expected summary, or the command is
  absent);
- where a `resolve-*.stderr.txt` exists, stderr equals it byte for byte.

The test finds the workspace root the same way `crates/xtask/tests/idempotency.rs` does. A glob
check fails if a `resolve-*.toml` has no test case, so a new example can't go untested.

## 8. Process

- Branch with `git-spice branch create`.
- Plan: `specs/2026-09-30-plugin-clash-resolution-plan.md`.
- Verify with `mise run test`, `prek run --all-files` and `mkdocs build --strict`.
- The implementing PR ends with `git mv` of this design and its plan to `specs/archive/2026/`.
