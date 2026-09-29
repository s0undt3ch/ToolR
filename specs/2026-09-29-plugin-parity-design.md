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
- `tuple[T1, T2]` (`fixed_arity`) is rejected with `UnsupportedArgumentKind`.

A third defect, found while writing this spec, is **nested plugin groups are broken**. A plugin
declaring `docker` → `image` → `build` produces this fragment (checked with the dev binary on
`main` at `efb86265`):

```json
"groups": [{"name": "docker.image", "title": "Image", "description": "Image tools."}]
```

- `docker` is dropped, because the group filter keeps only groups that hold a command directly.
- `docker.image` has no `parent`, so `merge.rs` turns it into a top-level group literally named
  `docker.image`. `cli.rs` indexes children by `parent`, so the command tree is wrong.
- `merge.rs` also dedups groups by leaf `name`, so two nested groups with the same leaf collide.

## Decisions

| Question | Decision |
| --- | --- |
| Approach | One shared builder. The fragment carries the manifest types unchanged. |
| A plugin command in a group the plugin doesn't declare | Build error, like a local command. It was never documented. |
| Fragment version counter | One counter: `manifest::SCHEMA_VERSION`. `FRAGMENT_SCHEMA_VERSION` goes. |
| Key name in the fragment | Stays `toolr_schema_version`. Its meaning changes, see *Versioning*. |
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
  3. the package filter (`module == pkg || module.starts_with("pkg.")`);
  4. the group filter, fixed: keep every group that holds a surviving command **and every
     ancestor of such a group**;
  5. the empty-package check;
  6. sort groups by `full_path()` and commands by `(group, name)` (unchanged);
  7. compute `toolr_schema_version` (see *Versioning*).

Argparse grafting and `DispatchCommand` stay local-only. They put a repo's own argparse or Django
tooling behind the toolr CLI. That's a repo concern, not something a plugin ships.

`BuildFragmentError` wraps `BuildError` for the shared checks, and keeps its own variants for
`MissingSourceDir`, `NamespacePackage` and `EmptyPackage`. The duplicated `format_*` helpers in
`build_fragment.rs` go.

Behaviour change for plugin authors: a plugin that fails positional arity, or references an
undeclared group, now fails `toolr self build-manifest`. It goes in `UNRELEASED.md`.

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

### 3. Merge

`merge_into_manifest` changes:

- Groups dedup by `full_path()`, not `name`. The rule is unchanged otherwise: a group already in
  `base` or an earlier fragment keeps its title and description.
- Each merged group and command gets `origin = Origin::ThirdParty`. Nothing else is filled in.
- Each merged command runs `Argument::validate`. A failure skips that plugin (see *Skipped
  plugins*) rather than panicking the clap builder later. It catches hand-edited fragments, such as
  `fixed_arity` without `nargs`.
- A local command with the same `(group, name)` still wins, with a debug log. Two plugins
  declaring the same command still abort with `DuplicateCommand`.

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

#### Load rule

`MIN_FRAGMENT_SCHEMA: u32 = 2` is the oldest fragment schema this reader understands. v1 is lossy
and isn't migrated. A reader whose `SCHEMA_VERSION` is `C` loads a fragment that declares `M` when
`MIN_FRAGMENT_SCHEMA <= M <= C`:

| Case | Result |
| --- | --- |
| `M < 2` (a v1 fragment) | Skip: `skipping plugin <pkg>: built with toolr schema 1, this toolr needs >= 2. Rebuild the plugin with toolr >= 0.34.0.` |
| `M > C` | Skip: `skipping plugin <pkg>: needs toolr schema <M>, this toolr supports <C>. Upgrade toolr.` |
| `2 <= M <= C` | Load. |
| Key missing or not an integer | Abort with `MissingVersion` (unchanged). |

`0.34.0` stands for the release that ships this. It is written into the message at implementation
time from the next minor version, not from `CARGO_PKG_VERSION`.

#### Compatibility this buys

- **Newer toolr, older plugin:** loads, because `M <= C`. This holds only while shared shapes don't
  break. v2 is meant to be the last breaking bump. A later breaking change to `Group`, `Command`,
  `Argument`, `ArgMetadata`, `SupportedType`, `ArgumentKind` or `Nargs` must add a migration step in
  `parse_fragment`, not only bump the counter.
- **Older toolr, newer plugin using only old features:** loads, because the builder computed a low
  `M`. New optional fields get `#[serde(default)]`. No type uses `deny_unknown_fields`, so an old
  reader ignores fields it doesn't know.
- **Older toolr, newer plugin using a new feature:** skipped with "Upgrade toolr". It never reaches
  a serde error.

#### Computing `M`

`M` is the maximum of `MIN_FRAGMENT_SCHEMA` and the `since` schema of every feature the fragment
uses. Each hook is exhaustive, so the compiler forces an answer for every addition:

- `SupportedTypeKind::since_schema(self) -> u32`: an exhaustive `match`. Every current kind returns
  2.
- `SupportedType::min_schema(&self) -> u32`: its own kind's `since_schema()`, maxed with its inner
  types for `List`, `Tuple` and `Optional`. `list[NewType]` counts.
- `ArgumentKind::since_schema()` and `Nargs::since_schema()`: exhaustive matches. An old reader
  can't deserialise a new variant of either, just like a new type.
- `min_schema(&self)` on `Group`, `Command`, `Argument`, `ArgMetadata` and `HelpSection`. Each
  destructures the struct **without `..`**, so a new field doesn't compile until someone decides.
  An ignorable field (an old reader may drop it and still behave correctly) contributes nothing.
  A field an old reader must not drop contributes `since N` when it's set. Today every field
  contributes nothing beyond the floor.

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
  mismatch or a failed `Argument::validate` skips that plugin. Every other `ThirdPartyError` still
  aborts.
- On `StaticDrift`, `bootstrap.rs::carry_forward_cached_entries` carries `skipped_plugins` over from
  the cache, the same way it carries the cached third-party commands.
- Every dispatch prints each entry to stderr as `toolr: warning: <reason>` before running the
  command. That includes dispatch from a fresh cache. Tab completion stays silent, because output on
  the completion path corrupts the shell's candidates.
- `RebuildOutcome.warnings` (always empty today) is filled from `skipped_plugins`, so
  `toolr project manifest rebuild` reports them too.

### 6. CLI and docs fallout

- `self build-manifest --schema-version` is removed from `cli.rs` and `dispatch.rs`. Regenerate
  `docs/cli-files/self-build-manifest-help.txt` and drop the line in `docs/cli.md`.
- `docs/third-party.md`: update the v1 JSON example to v2, and rewrite the "rejects any other
  version" paragraph to match the load rule. Document the new build-time checks.
- `crates/xtask/src/build_skill_refs/packaging.rs`:
    - The `SkillRefFragmentVersion` entry points at `third_party/model.rs`, which loses the constant.
  Point it at the `SkillRefSchemaVersion` region instead, or drop it. Rewrite its narrative for the
  minimum-reader meaning.
    - `SkillRefManifestFragment` narrows to the new `ManifestFragment` struct.
    - Run `cargo xtask build-skill-refs` and commit the regenerated `references/`.
- Check the prose in `skills/toolr-command-packaging/SKILL.md` by hand for mentions of the fragment
  version, `--schema-version` or the lossy fields.
- Regenerate `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json` with the dev
  binary. `--check` in CI keeps it honest.
- `UNRELEASED.md`, with a minor bump (it ships with #502):
    - Plugin commands now get type validation, `arg()` metadata, completion and `tuple` support.
    - Nested plugin groups work.
    - **Migration:** rebuild and republish plugins. v1 fragments are skipped with a warning.
  Fragments built by this toolr are rejected by toolr 0.33.0 and older, which abort the merge on
  any v2 fragment. That can't be fixed in binaries that already shipped.
    - `--schema-version` is removed.
    - New plugin build errors: positional arity, undeclared group.

## Testing

All tests live in `toolr-core` unless noted. They follow the existing `TempDir` fixture style in
`build_fragment.rs` and `third_party/tests.rs`.

- **Parity.** One command set built both ways, compared as `Command`s equal apart from `origin`
  and the module prefix (`tools.` vs `<pkg>.`). The set covers:
    - a nested `docker` → `image` → `build` group;
    - `tuple[str, int]` (`fixed_arity`);
    - an `Annotated[..., arg(aliases=..., metavar=..., env=..., help_section=...)]` argument;
    - a typed argument (`int`, `Email`, a path-state type).

  Pipeline: `build_third_party_fragment` → `serialise_fragment` → `parse_fragment` →
  `merge_into_manifest`. The round trip is part of the test.
- **Nested groups.** The fragment keeps `docker` with `parent: None` and `image` with
  `parent: Some("docker")`. After merge, two nested groups with the same leaf name both survive.
- **Plugin build checks.** A plugin command in an undeclared group fails with the "did you mean"
  hint. A plugin with a bad positional order fails positional arity.
- **Load rule.**
    - `M = 2` loads.
    - `M = SCHEMA_VERSION + 1` is skipped with the "Upgrade toolr" reason.
    - `M = 1` is skipped with the "Rebuild the plugin" reason. `crates/toolr/tests/untrusted_repo.rs`
  hard-codes a v1 fragment today. Convert it into this test at the binary level.
    - A fragment with `fixed_arity` and no `nargs` is skipped, not a panic.
- **Warning persistence** (`crates/toolr` integration, `assert_cmd`): the first run warns on
  stderr and still runs a local command. A second run on a fresh cache warns again. Tab completion
  prints no warning.
- **Computed `M`.** Every current fragment computes 2. A `#[cfg(test)]` hook raises one kind's
  `since_schema()`, and that type in a nested position (`list[T | None]`) raises `M`. No real type
  is newer than 2 yet, so the hook is test-only.
- **Lockstep.** `manifest_schema_version_lockstep.rs`.
- **Existing tests.** The `third_party/tests.rs` and `build_fragment.rs` tests that build
  `Fragment*` values move to `Group`/`Command`. The `UnsupportedArgumentKind` test is replaced by
  a `fixed_arity` round-trip test.

Verification: full `mise run test`, which includes `cargo xtask build-skill-refs --check`, then
`prek run --all-files`.

## Out of scope

- Skipping, rather than aborting on, malformed JSON or a duplicate command across two plugins.
  File a follow-up issue.
- Argparse grafting or `DispatchCommand` for plugins.
- Migrating v1 fragments. They are skipped.
