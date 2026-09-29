# Plugin command parity: one builder, lossless fragments (#520)

## Goal

A command gets identical CLI behaviour whether it lives in a repo's `tools/` or ships in a plugin
wheel. That covers parsing, validation, errors, help and completion.

## Problem

Two causes, both in `toolr-core`.

1. **Two drifted builders.** `build_static_manifest_inner` (`parser/build.rs`) and
   `build_third_party_fragment` (`build_fragment.rs`) duplicate the two AST passes. The plugin copy
   skips `validate_positional_arity` and the unknown-group check.
2. **A lossy fragment.** The plugin builder produces full `Command`s, then maps each argument into
   `FragmentArgument`, which keeps only `name`, `kind`, `help`, `default`, `type_annotation` and
   `allowed_values`. `merge.rs` then invents `resolved_type: None`, `ArgMetadata::default()` and
   `long_flag: None`.

Effect on plugin commands today:

- No clap value parser, so bad values fail late inside Python, and `Email` is never validated.
- No Rust-only checks, such as the path-state types from #502.
- No type-derived completion hints.
- Every `arg()` field is lost: `aliases`, `metavar`, `env`, `hide`, `display_order`,
  `help_section`, `conflicts_with`, `requires` and `nargs`.
- `tuple[T1, T2]` arrives as `Repeated` with no `resolved_type`, so clap can't enforce the element
  count or types. (`fixed_arity` comes only from the argparse scanner, which is local-only, so a
  plugin build never emits it.)

A third defect, found while writing this spec, is **nested plugin groups are broken**. A plugin
declaring `docker` → `image` → `build` produces this fragment (checked with the dev binary on
`main` at `efb86265`):

```json
"groups": [{"name": "docker.image", "title": "Image", "description": "Image tools."}]
```

- `docker` is dropped, because the group filter keeps only groups that hold a command directly
  (`build_fragment.rs:175-177`).
- `docker.image` has no `parent`, so `merge.rs` turns it into a top-level group literally named
  `docker.image`. `cli.rs` indexes children by `parent`, so the command tree is wrong.
- `merge.rs` seeds its dedup set with host *leaf* names but inserts fragment *full paths*, so the
  two key spaces never line up.

Once the fragment carries real `Group`s, every place that dedups groups by leaf `name` becomes a
collision bug: a plugin's `docker.image` and a local `ci.image` share the leaf `image`. There are
three such places: `merge.rs:35-39`, `bootstrap.rs:194` (`carry_forward_cached_entries`) and
`complete/freshness.rs:81` (`preserve_non_static_entries`).

## Decisions

| Question | Decision |
| --- | --- |
| Approach | One shared builder. The fragment carries the manifest types unchanged. |
| A plugin command in a group the plugin doesn't declare | Build error, like a local command. To add to a host group, the plugin declares that group too. See §1. |
| Fragment version counter | One counter: `manifest::SCHEMA_VERSION`. `FRAGMENT_SCHEMA_VERSION` goes. |
| Key name in the fragment | Stays `toolr_schema_version`. Its meaning changes, see §4. |
| `self build-manifest --schema-version N` | Removed. The version is computed. |
| A skipped plugin | Recorded in the manifest and warned about on every run. |
| Malformed JSON, duplicate command across two plugins | Still abort. Out of scope (follow-up issue). |

Rejected alternatives:

- **Keep two builders and add fields to the `Fragment*` types.** The next field drifts again, and
  the missing validations stay missing.
- **No fragment: parse the plugin's installed source at merge time.** It needs `.py` source in
  site-packages and an AST walk of every plugin on each rebuild. It also loses `--check` as a CI
  gate.

## Design

### 1. One builder

Add a shared function in `parser/build.rs`:

```rust
pub(crate) fn build_commands(
    source_root: &Path,
    module_prefix: &str,
) -> Result<(Vec<Group>, Vec<Command>), BuildError>;
```

It runs pass 1 (enum, alias, arg-section and import tables) and pass 2 (groups and commands). Then
it runs every check, in the current local order:

1. name conflicts;
2. unsupported types;
3. missing docstrings;
4. positional arity;
5. unknown group references, with the "did you mean" hint.

Callers:

- **Local:** `build_static_manifest_inner` = `build_commands(tools_dir, "tools")`, then
  `static_hash`, then argparse grafting and `is_dispatcher` flagging. Unchanged behaviour.
- **Plugin:** `build_third_party_fragment` =
    1. missing-dir and namespace-package checks (unchanged);
    2. `build_commands(source_dir, package_name)`;
    3. the empty-package check (no groups and no commands);
    4. sort groups by `full_path()` and commands by `(group, name)` (unchanged);
    5. compute `toolr_schema_version` (see §4).

Two current plugin steps go:

- **The package filter** (`build_fragment.rs:171-172`). `module_path_for_prefix` prefixes every
  walked file with the package name, so the filter never drops anything.
- **The group filter.** The local build keeps every declared group, including one with no
  commands. The fragment does the same, which also keeps `docker` in the nested case.

Argparse grafting and `DispatchCommand` stay local-only. They put a repo's own argparse or Django
tooling behind the toolr CLI. That's a repo concern, not something a plugin ships.

`BuildFragmentError` wraps `BuildError` for the shared checks, and keeps its own variants for
`MissingSourceDir`, `NamespacePackage` and `EmptyPackage`. The duplicated `format_*` helpers in
`build_fragment.rs` go.

Behaviour changes for plugin authors, all listed in `UNRELEASED.md`:

- A plugin that fails positional arity now fails `toolr self build-manifest`.
- A plugin command in a group the plugin doesn't declare now fails the build, with the "did you
  mean" hint. `docs/third-party.md` ("Command resolution") documents *group augmentation*: a plugin
  adds commands to a host group such as `ci`. That still works. The plugin declares
  `command_group("ci", ...)` itself, and at merge the host's title and description win (§3). The
  docs and the migration note say this.

### 2. Fragment v2 shape

```rust
pub struct ManifestFragment {
    /// Lowest toolr schema a reader needs to load this fragment. Not the schema of the toolr that
    /// built it. See "Versioning".
    pub toolr_schema_version: u32,
    pub package: String,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub commands: Vec<Command>,
}
```

- `Group` and `Command` are `crate::manifest` types, unchanged.
- `Group.origin` and `Command.origin` are required serde fields. The builder writes
  `"origin": "third_party"`, and `merge.rs` overwrites both anyway. We don't add
  `#[serde(default)]` to `origin`: a local manifest without it is corrupt.
- `FragmentGroup`, `FragmentCommand`, `FragmentArgument` and
  `ThirdPartyError::UnsupportedArgumentKind` are removed.
- `serialise_fragment` is unchanged: sorted keys, 2-space indent, trailing newline. Fields that
  skip when empty (`metadata`, `long_flag`, `dispatched_from`, `is_dispatcher`) keep fragments
  diffable.

### 3. Merge and group dedup

**Invariant:** a group's identity is its `full_path()`, everywhere. Groups with different full
paths, such as `docker.image` and `ci.image`, are always separate, and so are their commands. Only
groups with the same full path merge. No code may key groups by leaf `name`.

`merge_into_manifest` changes:

- Groups dedup by `full_path()`, not `name`. The rule is unchanged otherwise: a group already in
  `base` or an earlier fragment keeps its title and description.
- Each merged group and command gets `origin = Origin::ThirdParty`.
- Each merged command gets `dispatched_from = None` and `is_dispatcher = false`. The builder never
  sets them, so this only matters for a hand-edited fragment. Without the reset, such a fragment
  would reach the argparse dispatch path in `dispatch.rs`, which is local-only.
- Nothing else is filled in or changed.
- Each merged command runs `Argument::validate`. A failure skips that plugin (§5) rather than
  panicking the clap builder later. A hand-edited `fixed_arity` without `nargs` is the case it
  catches.
- A local command with the same `(group, name)` still wins, with a debug log. Two plugins
  declaring the same command still abort with `DuplicateCommand`.

The same `full_path()` dedup applies to the two cache carry-forward helpers:

- `bootstrap.rs::carry_forward_cached_entries`, on `StaticDrift`;
- `complete/freshness.rs::preserve_non_static_entries`, on the completion path.

Without this, editing a local file that declares `ci.image` drops a cached plugin's
`docker.image` until the next `ThirdPartyDrift`.

### 4. Versioning

#### One counter, two meanings

`manifest::SCHEMA_VERSION` (currently 2, bumped by #502) is the only schema counter.
`third_party::FRAGMENT_SCHEMA_VERSION` is removed. The field means different things in the two
files:

- `Manifest.schema_version`: the schema the manifest was **written** with. Any mismatch rebuilds the
  local cache (existing behaviour in `freshness::compare`).
- `ManifestFragment.toolr_schema_version`: the **lowest** schema a reader needs to load the
  fragment.

A bump for a local-only change (for example a new `Manifest` field) never raises any fragment's
minimum, so it never invalidates a published plugin.

The key keeps its name. Every toolr already shipped reads `toolr_schema_version`. With the key kept,
an old binary meeting a v2 fragment says "toolr_schema_version 2 is newer than this toolr binary
supports (max 1). Upgrade toolr." With the key renamed, it would say "not a valid toolr manifest
fragment" instead.

#### Two floors

Two constants in `third_party/model.rs`, both 2 today:

- `FRAGMENT_SHAPE_SCHEMA`: the **writer** floor. It is the schema at which the fragment's JSON
  shape last changed in a way an older reader can't parse. Bump it (and `SCHEMA_VERSION`) on any
  non-additive change to `Group`, `Command`, `Argument`, `ArgMetadata`, `HelpSection`,
  `SupportedType`, `ArgumentKind` or `Nargs`. Examples: a renamed field, a changed field type, a
  new required field.
- `MIN_READABLE_FRAGMENT_SCHEMA`: the **reader** floor, the oldest fragment this reader can parse.
  It stays below `FRAGMENT_SHAPE_SCHEMA` only when `parse_fragment` has a migration from the older
  shape. A migration helps newer readers only. Older readers still need the writer floor to turn
  them away.

The invariant `MIN_READABLE_FRAGMENT_SCHEMA <= FRAGMENT_SHAPE_SCHEMA <= SCHEMA_VERSION` is a
`const` assertion.

#### Load rule

A reader whose `SCHEMA_VERSION` is `C` loads a fragment that declares `M` when
`MIN_READABLE_FRAGMENT_SCHEMA <= M <= C`:

| Case | Result |
| --- | --- |
| `M < MIN_READABLE_FRAGMENT_SCHEMA` | Skip: `skipping plugin <pkg>: built with toolr schema <M>, this toolr needs >= <floor>. Rebuild the plugin with toolr >= 0.34.0.` |
| `M > C` | Skip: `skipping plugin <pkg>: needs toolr schema <M>, this toolr supports <C>. Upgrade toolr.` |
| In range | Load. |
| Key missing, not an integer, or 0 | Abort with `MissingVersion` (unchanged). |

`0.34.0` stands for the release that ships this, taken from `Cargo.toml` at implementation time.

#### Compatibility this buys

- **Newer toolr, older plugin:** loads when `M` is in range. A shape change raises the reader floor
  unless it comes with a migration.
- **Older toolr, newer plugin using only old features:** loads, because the builder computed a low
  `M`. New optional fields get `#[serde(default)]`. No type uses `deny_unknown_fields`, so an old
  reader ignores fields it doesn't know.
- **Older toolr, newer plugin using a new feature or a newer shape:** skipped with "Upgrade toolr".
- **The limit:** this protects honest fragments only. A hand-edited fragment that declares `M = 2`
  while using a newer type still fails to deserialise and aborts with `Json`.

#### Computing `M`

`M` is the maximum of `FRAGMENT_SHAPE_SCHEMA` and the `since` schema of every feature the fragment
uses:

- `SupportedTypeKind::since_schema(self) -> u32`: an exhaustive `match`. Every current kind returns
  2.
- `SupportedType::min_schema(&self) -> u32`: its own kind's `since_schema()`, maxed with its inner
  types for `List`, `Tuple` and `Optional`. `list[NewType]` counts.
- `ArgumentKind::since_schema()` and `Nargs::since_schema()`: exhaustive matches. An old reader
  can't deserialise a new variant of either, just as with a new type (`SupportedType` is
  `tag = "kind"`, and an unknown variant is a hard serde error).
- `min_schema(&self)` on `Group`, `Command`, `Argument`, `ArgMetadata` and `HelpSection`. Each
  destructures the struct **without `..`**, so a new field doesn't compile until someone decides:
    - **Ignorable:** an old reader may drop it and still behave correctly. It contributes nothing.
    - **Not ignorable:** it contributes `since N` when set.

  `#[serde(default)]` does not make a field ignorable. `SupportedType::Enum.module` is the
  precedent: it is serde-additive, but an old reader that drops it can't import the enum at run
  time. Today every field contributes nothing beyond the floor.

The fold takes the kind lookup as a parameter:

```rust
fn min_schema_with(&self, since: impl Fn(SupportedTypeKind) -> u32) -> u32;
pub fn min_schema(&self) -> u32 { self.min_schema_with(SupportedTypeKind::since_schema) }
```

Tests pass their own `since` to test the fold without a real type newer than 2.

The compiler forces *an* answer for each new variant, not the right one. Two guards cover the gap:

- A golden test pins the full `(SupportedTypeKind, since_schema)` table, plus the `ArgumentKind` and
  `Nargs` tables. A new variant fails the test until its row is added on purpose.
- A test asserts every `since_schema()` is `<= SCHEMA_VERSION`, and that
  `FRAGMENT_SHAPE_SCHEMA <= SCHEMA_VERSION`.
- "Adding a supported type" in `CONTRIBUTING.md` gains a step: "bump `SCHEMA_VERSION` and set the
  new kind's `since_schema()` to the new value".

`ManifestFragment::min_schema()` folds these over every group and command. The builder writes the
result as `toolr_schema_version`. `parse_fragment` reads the declared value and doesn't recompute
it.

#### Python mirror

`toolr.MANIFEST_SCHEMA_VERSION` (`_decorators.py`, in `toolr.__all__`) goes from 1 to 2. Its
docstring changes to "mirrors `SCHEMA_VERSION` in `crates/toolr-core/src/manifest/model.rs`". A new
test, `crates/toolr-core/tests/manifest_schema_version_lockstep.rs`, fails CI when the two disagree.
It follows `schema_version_lockstep.rs`. This counter is separate from the runner pair
(`RUNNER_SCHEMA_VERSION` / `_runner.py::SCHEMA_VERSION`), which is untouched.

### 5. Skipped plugins

One unloadable plugin must not break the CLI, including local commands. Silently missing commands
are just as bad, so a skip is recorded and warned about on every run.

- `Manifest` gains a field:

  ```rust
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub skipped_plugins: Vec<SkippedPlugin>,

  pub struct SkippedPlugin {
      pub package: String,
      pub path: PathBuf,
      pub reason: String,
  }
  ```

  It's additive and optional, so there's no schema bump. An older binary drops it on read, then
  rebuilds anyway because `toolr_version` differs.
- `discover_and_merge` returns the merged manifest with `skipped_plugins` filled in. A version
  outside the load rule or a failed `Argument::validate` skips that plugin. Every other
  `ThirdPartyError` still aborts.
- Freshness verdicts:
    - `ThirdPartyDrift` and a first build: `skipped_plugins` comes from the fresh merge.
      `third_party_hash` covers skipped fragments' bytes too, so fixing or removing a plugin
      triggers `ThirdPartyDrift` and clears its entry.
    - `StaticDrift`: `carry_forward_cached_entries` copies `skipped_plugins` from the cache, just as
      it copies cached third-party commands.
    - `ThirdPartyDrift` with no venv: plugin commands and skips are both dropped (existing
      behaviour for commands).
- **Where it prints:** `main.rs::run`, right after `load_or_empty` and before clap parses argv.
  So `--help`, `dispatch_help_from_argv` and the `self`/`project` builtins all get it. The format
  is `toolr: warning: <reason>` on stderr.
- **When it doesn't print:**
    - on the tab-completion path, because output there corrupts the shell's candidates;
    - when `argv_requests_quiet(&argv)` is true, matching the cache hint's `--quiet` handling
      (`main.rs:76-84`).
- `RebuildOutcome.warnings` (always empty today) is filled from `skipped_plugins`, so
  `toolr project manifest rebuild` reports them too.

### 6. CLI and docs fallout

- `self build-manifest --schema-version` is removed from `cli.rs` and `dispatch.rs`. Regenerate
  `docs/cli-files/self-build-manifest-help.txt` and drop the line in `docs/cli.md`.
- `docs/third-party.md`:
    - Replace the v1 JSON example with v2. The current example also uses `"kind": "keyword"`, which
      isn't a real `ArgumentKind`.
    - Rewrite the "rejects any other version" paragraph to match the load rule.
    - Document the new build-time checks, and how group augmentation works now (§1).
- `crates/xtask/src/build_skill_refs/packaging.rs`:
    - The `SkillRefFragmentVersion` entry points at `third_party/model.rs`, which loses
      `FRAGMENT_SCHEMA_VERSION`. Move the region to the two floor constants, and rewrite the
      narrative for the minimum-reader meaning.
    - `SkillRefManifestFragment` narrows to the new `ManifestFragment` struct.
    - Run `cargo xtask build-skill-refs` and commit the regenerated `references/`.
- Check the prose in `skills/toolr-command-packaging/SKILL.md` by hand for mentions of the fragment
  version, `--schema-version`, the lossy fields or group augmentation.
- Regenerate `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json` with the dev
  binary. CI's `--check` builds with the PR's own binary, so this is safe.
- `UNRELEASED.md`, with a minor bump (it ships with #502):
    - Plugin commands now get type validation, `arg()` metadata, completion, and typed `tuple`
      arguments.
    - Nested plugin groups work.
    - **Migration:** rebuild and republish plugins. v1 fragments are skipped with a warning.
      toolr 0.33.0 and older abort the whole merge on any fragment built by this toolr. That
      can't be fixed in binaries that already shipped.
    - To add commands to a host group, a plugin must now declare that group.
    - `--schema-version` is removed.
    - New plugin build errors: positional arity, undeclared group.
    - The unreleased #506 entry says toolr "accepts only the current `toolr_schema_version`". Amend
      it to match the load rule rather than adding a contradicting line.

## Testing

All tests live in `toolr-core` unless noted. They follow the existing `TempDir` fixture style in
`build_fragment.rs` and `third_party/tests.rs`.

- **Parity.** One command set built both ways. Both sides are serialised to JSON, and `"tools.` /
  `"tools"` are rewritten to `"<pkg>.` / `"<pkg>"`. This catches the prefix inside
  `SupportedType::Enum.module` as well as `Command.module`. `origin` is normalised as well. The
  results must be equal. The set covers:
    - a nested `docker` → `image` → `build` group, and a declared group with no commands;
    - `tuple[str, int]`, which arrives as `Repeated` with `resolved_type` `Tuple([Str, Int])`;
    - an `Annotated[..., arg(aliases=..., metavar=..., env=..., help_section=...)]` argument;
    - typed arguments: `int`, `Email`, a path-state type, and an `Enum` from another module.

  Pipeline: `build_third_party_fragment` → `serialise_fragment` → `parse_fragment` →
  `merge_into_manifest`. The round trip is part of the test.
- **Nested groups.** The fragment keeps `docker` with `parent: None` and `image` with
  `parent: Some("docker")`. After merge, a plugin `docker.image` and a local `ci.image` both
  survive.
- **Carry-forward** (`crates/toolr` integration, `assert_cmd`): a cached plugin `docker.image`
  survives a `StaticDrift` caused by editing a local file that declares `ci.image`. The completion
  path's `preserve_non_static_entries` gets the same case as a unit test.
- **Plugin build checks.** A plugin command in an undeclared group fails with the "did you mean"
  hint. A plugin with a bad positional order fails positional arity. A plugin that declares a host
  group's name merges under it, and the host title wins.
- **Load rule.**
    - `M = 2` loads.
    - `M = SCHEMA_VERSION + 1` is skipped with the "Upgrade toolr" reason.
    - `M = 1` is skipped with the "Rebuild the plugin" reason. `crates/toolr/tests/untrusted_repo.rs`
      hard-codes a v1 fragment today. Convert it into this test at the binary level.
    - A hand-built fragment with `fixed_arity` and no `nargs` is skipped, not a panic. Only a
      hand-edited fragment can have this shape.
    - A hand-built fragment with `is_dispatcher: true` merges with it reset to `false`.
- **Warnings** (`crates/toolr` integration, `assert_cmd`):
    - The first run warns on stderr and still runs a local command.
    - A second run on a fresh cache warns again.
    - `toolr --help` warns.
    - `--quiet` and tab completion print nothing.
- **Computed `M`.**
    - Every current fragment computes 2.
    - `min_schema_with` with a test `since` that returns 3 for one kind raises `M` when that kind
      is nested (`list[T | None]`).
    - The golden `since_schema` tables, and the `<= SCHEMA_VERSION` assertion.
- **Lockstep.** `manifest_schema_version_lockstep.rs`.
- **Existing tests.** The `third_party/tests.rs` and `build_fragment.rs` tests that build
  `Fragment*` values move to `Group`/`Command`. The `UnsupportedArgumentKind` test goes. The
  hand-built `fixed_arity` skip test replaces it.

Verification: full `mise run test`, which includes `cargo xtask build-skill-refs --check`, then
`prek run --all-files`.

## Out of scope

- Skipping, rather than aborting on, malformed JSON or a duplicate command across two plugins.
  File a follow-up issue.
- Argparse grafting or `DispatchCommand` for plugins.
- Migrating v1 fragments. They are skipped.
- Dishonest fragments: a declared `M` lower than the features actually used.
