# Plugin Command Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** plugin commands get the same CLI behaviour as `tools/` commands: one shared builder, a
lossless v2 fragment, version-tolerant loading, and persisted plugin warnings.

**Architecture:** extract `build_commands` from `parser/build.rs` and use it from both builders.
`ManifestFragment` carries `manifest::Group`/`Command` unchanged. Its `toolr_schema_version` is the
minimum reader schema, computed from the features the fragment uses. The merge records skipped and
shadowed plugins in `Manifest.plugin_warnings`, which `main.rs` prints on every run.

**Tech Stack:** Rust (serde, clap, thiserror, tempfile, assert_cmd), Python 3.11+ (toolr-py).

**Spec:** `specs/2026-09-29-plugin-parity-design.md`. Read it before starting any task. Section
numbers below (§1–§6) refer to it.

## Global Constraints

- Branch `plugin-parity` (git-spice). Commit on it. Never `git add -A` or `git add .`: stage by
  path, then check `git diff --cached --name-only`. The untracked `audit/`, `ONBOARDING.md` and
  `specs/2026-09-29-open-issues-plan.md` must never be staged.
- Conventional Commits with the `(#520)` suffix, for example `feat(manifest): … (#520)`. No
  `Co-Authored-By` trailer. No `--no-verify`. Pre-commit hooks run `cargo check` and clippy, so
  commits are slow: give them a 10-minute timeout.
- British English in prose, comments and commit messages. Comments explain *why*, one line where
  possible. Default to none.
- Never write an employer name or an absolute `/Users/...` path into the repo.
- `manifest::SCHEMA_VERSION` stays **2**. `FRAGMENT_SHAPE_SCHEMA = 2`,
  `MIN_READABLE_FRAGMENT_SCHEMA = 2`. The release that ships this is **0.34.0**.
- Group identity is `Group::full_path()` everywhere. Never compare groups by leaf `name`.
- Python tests: factory fixtures, imports at the top of the file.
- `cargo test --workspace` can stall. Run long cargo commands in the background and poll every
  30–60 s.
- Model assignment per task is in each task header. The orchestrator dispatches that model.

## Review Focus

1. **Adding a local command over a cached plugin command** must warn on that same run, because
   `StaticDrift` with a venv escalates to a re-merge. Tested in Task 6.
2. **A hand-edited fragment with one bad command** (`fixed_arity` without `nargs`, as the second
   of two commands) skips the whole plugin, with no half-merged groups. Tested in Task 5.
3. **Tab completion, `project`, `self` and `--quiet`** never print plugin warnings, and
   `project manifest rebuild` prints each warning exactly once. Tested in Task 6.
4. **An older-shape v1 fragment next to a good v2 fragment:** the v2 plugin still loads, the v1 one
   is skipped, and local commands run. Tested in Task 6.
5. **Nested plugin groups next to local groups with the same leaf** (`docker.image` and
   `ci.image`) both render in `--help`, and each runs its own command. Tested in Task 6.

---

### Task 1: Full-path group dedup everywhere (model: sonnet)

**Files:**

- Modify: `crates/toolr-core/src/third_party/merge.rs:35-42`
- Modify: `crates/toolr/src/bootstrap.rs:193-197` (`carry_forward_cached_entries`)
- Modify: `crates/toolr-core/src/complete/freshness.rs:79-85` (`preserve_non_static_entries`)
- Test: `crates/toolr-core/src/complete/tests.rs`, `crates/toolr/src/bootstrap.rs` (its
  `#[cfg(test)]` module, or create one at the bottom of the file)

**Interfaces:**

- Produces: every group dedup compares `full_path()`. Later tasks rely on this.
- [ ] **Step 1: Write failing tests.**
    - In `complete/tests.rs`: a cached manifest with a `ThirdParty` group `image` (parent
      `docker`) and a fresh manifest with a `Static` group `image` (parent `ci`). After
      `preserve_non_static_entries`, both groups are present. The function is private, so put the
      test in the module that can reach it, or expose it `pub(crate)`.
    - In `bootstrap.rs`, the same for `carry_forward_cached_entries` with
      `FreshnessVerdict::StaticDrift`.
    - In `merge.rs` tests (`third_party/tests.rs`): a base manifest with a static group
      `name: "image", parent: Some("ci")`, and a fragment whose group is `"docker.image"` (v1 flat
      form). Both survive.
- [ ] **Step 2: Run them.** `cargo test -p toolr-core complete:: third_party::` and
  `cargo test -p toolr --bin toolr bootstrap` (adjust the filter to the test names). Expected:
  FAIL.
- [ ] **Step 3: Implement.** Replace each `g.name == group.name` with
  `g.full_path() == group.full_path()`. In `merge.rs`, seed `known_groups` with
  `base.groups.iter().map(Group::full_path)`. v1 `FragmentGroup` still has only `name` (a full
  path), so compare `fg.name` against that set for now. Task 4 replaces `FragmentGroup`.
- [ ] **Step 4: Run the tests.** Expected: PASS.
- [ ] **Step 5: Commit.** `fix(manifest): dedup groups by full path, not leaf name (#520)`

---

### Task 2: One shared builder (model: sonnet)

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs:36-165`
- Modify: `crates/toolr-core/src/build_fragment.rs:1-230`
- Test: `crates/toolr-core/src/build_fragment.rs` (`mod tests`)

**Interfaces:**

- Produces:

  ```rust
  // crates/toolr-core/src/parser/build.rs
  pub(crate) fn build_commands(
      source_root: &Path,
      module_prefix: &str,
  ) -> Result<(Vec<Group>, Vec<Command>), BuildError>;
  ```

  `BuildFragmentError` gains `#[error(transparent)] Build(#[from] BuildError)`, and drops the
  `Parse`, `UnsupportedTypes`, `MissingDocstrings` and `ConflictingCommandName` variants, which now
  come through `Build`. `MissingSourceDir`, `NamespacePackage` and `EmptyPackage` stay.
- [ ] **Step 1: Write failing tests** in `build_fragment.rs` `mod tests`, using its `write()`
  helper:
    - `rejects_a_plugin_command_in_an_undeclared_group`: `commands.py` declares
      `grp = command_group("plug", "Plugin", "Plugin commands.")` and a
      `@command(group="plgu")`-style command (use the decorator form `parser/build.rs` tests use for
      unknown groups; `git grep -n "UnknownGroupRefs" crates/toolr-core/src/parser/build.rs` shows
      the fixture). Assert the error string contains `did you mean` and `plug`.
    - `rejects_a_plugin_with_bad_positional_arity`: copy the fixture of the existing local arity
      test (`git grep -n "InvalidPositionalArity" crates/toolr-core/src/parser/build.rs`). Assert
      the error mentions `invalid positional arity`.
    - `plugin_with_only_empty_groups_is_empty`: one `command_group(...)` and no commands. Expect
      `EmptyPackage`.
    - Update the existing `MissingDocstrings` test to match through
      `BuildFragmentError::Build(BuildError::MissingDocstrings(_))`.
- [ ] **Step 2: Run** `cargo test -p toolr-core build_fragment`. Expected: the first two FAIL.
- [ ] **Step 3: Implement.**
    - Move the body of `build_static_manifest_inner` from `let py_files` down to the end of the
      unknown-group check into `build_commands`. Replace `module_path_for(tools_dir, path)` with
      `module_path_for_prefix(source_root, path, module_prefix)`. Map parse errors to
      `BuildError::Build`.
    - `build_static_manifest_inner` becomes
      `let (all_groups, all_commands) = build_commands(tools_dir, "tools")?;`, then the existing
      `static_hash`, `Manifest` literal and argparse grafting, unchanged.
    - `build_third_party_fragment` keeps the missing-dir and namespace checks, then calls
      `build_commands(source_dir, package_name)?`.
    - Delete the package filter: `module_path_for_prefix` already prefixes every module. Leave the
      group filter for now, since v1 still needs it. Task 4 removes it.
    - The empty check becomes `if all_commands.is_empty()`.
    - Delete the now-unused `format_type_errors`, `format_name_conflicts` and `module_docstring`
      copies in `build_fragment.rs`.
- [ ] **Step 4: Run** `cargo test -p toolr-core`. Expected: PASS, including every existing
  `parser::build` test (local behaviour is unchanged).
- [ ] **Step 5: Commit.** `refactor(parser): share one builder between tools/ and plugins (#520)`

---

### Task 3: Schema hooks (model: opus)

**Files:**

- Modify: `crates/toolr-core/src/parser/types/supported.rs` (after `impl SupportedTypeKind`)
- Modify: `crates/toolr-core/src/manifest/model.rs`
- Create: `crates/toolr-core/src/manifest/schema.rs` (the `min_schema` impls and golden tests)
- Modify: `crates/toolr-core/src/manifest/mod.rs` (add `mod schema;`, and re-export the floors)
- Modify: `CONTRIBUTING.md` ("Adding a supported type", line 71)

**Interfaces:**

- Produces (all `pub`):

  ```rust
  // manifest/model.rs, next to SCHEMA_VERSION
  pub const FRAGMENT_SHAPE_SCHEMA: u32 = 2;
  pub const MIN_READABLE_FRAGMENT_SCHEMA: u32 = 2;
  const _: () = assert!(
      MIN_READABLE_FRAGMENT_SCHEMA <= FRAGMENT_SHAPE_SCHEMA && FRAGMENT_SHAPE_SCHEMA <= SCHEMA_VERSION
  );

  impl SupportedTypeKind { pub fn since_schema(self) -> u32 }      // exhaustive match, all 2
  impl ArgumentKind      { pub fn since_schema(self) -> u32 }      // exhaustive match, all 2
  impl Nargs             { pub fn since_schema(self) -> u32 }      // exhaustive match, all 2

  impl SupportedType { pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 }
  impl Argument      { pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 }
  impl ArgMetadata   { pub fn min_schema(&self) -> u32 }
  impl HelpSection   { pub fn min_schema(&self) -> u32 }
  impl Group         { pub fn min_schema(&self) -> u32 }
  impl Command       { pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 }
  ```

  Each struct's impl destructures `self` **without `..`**, so a new field fails to compile. Every
  field contributes `FRAGMENT_SHAPE_SCHEMA` today.
- [ ] **Step 1: Write failing tests** in `manifest/schema.rs`:

  ```rust
  #[test]
  fn since_schema_golden_tables() {
      use crate::parser::types::SupportedTypeKind as K;
      let got: Vec<(K, u32)> = K::ALL.iter().map(|k| (*k, k.since_schema())).collect();
      let want: Vec<(K, u32)> = K::ALL.iter().map(|k| (*k, 2)).collect();
      assert_eq!(got, want, "a new SupportedTypeKind needs a SCHEMA_VERSION bump and a new golden row");
      // Same for every ArgumentKind and Nargs variant, listed by hand.
  }

  #[test]
  fn every_since_is_at_most_schema_version() { /* K::ALL, ArgumentKind list, Nargs list */ }

  #[test]
  fn nested_new_kind_raises_the_minimum() {
      use crate::parser::types::{SupportedType as T, SupportedTypeKind as K};
      let since = |k: K| if k == K::Email { 3 } else { 2 };
      let ty = T::List(Box::new(T::Optional(Box::new(T::Email))));
      assert_eq!(ty.min_schema_with(&since), 3);
      assert_eq!(T::List(Box::new(T::Str)).min_schema_with(&since), 2);
  }

  #[test]
  fn command_minimum_folds_over_arguments() { /* a Command whose second Argument has
     resolved_type Some(Tuple([Str, Email])) → 3 with the same `since`; None → 2 */ }
  ```

  Keep the `ArgumentKind`/`Nargs` variant lists as explicit arrays in the test. Add a comment
  saying a new variant must be added there.
- [ ] **Step 2: Run** `cargo test -p toolr-core manifest::schema`. Expected: FAIL (does not
  compile).
- [ ] **Step 3: Implement** the hooks above. `SupportedType::min_schema_with` =
  `since(self.kind())` maxed with the inner types for `List`, `Tuple` and `Optional`.
  `Argument::min_schema_with` destructures every field of `Argument`. `resolved_type` contributes
  `min_schema_with`. `kind` contributes `kind.since_schema()`. `metadata` contributes
  `metadata.min_schema()`. The others contribute `FRAGMENT_SHAPE_SCHEMA`. `ArgMetadata::min_schema`
  destructures every field. `nargs` contributes `since_schema()`, and `help_section` contributes
  `HelpSection::min_schema`.

  Add the `CONTRIBUTING.md` step to "Adding a supported type": *"Bump `SCHEMA_VERSION` in
  `crates/toolr-core/src/manifest/model.rs` and return the new value from the new kind's
  `SupportedTypeKind::since_schema()`. `since_schema_golden_tables` is expected to fail. Its new row
  is the new `SCHEMA_VERSION`, not whatever makes the test pass."*
- [ ] **Step 4: Run** the tests. Expected: PASS.
- [ ] **Step 5: Commit.** `feat(manifest): record the schema each type and field arrived in (#520)`

---

### Task 4: Fragment v2 (model: opus)

**Files:**

- Modify: `crates/toolr-core/src/third_party/model.rs` (the whole file)
- Modify: `crates/toolr-core/src/third_party/mod.rs` (re-exports, doc comment)
- Modify: `crates/toolr-core/src/third_party/merge.rs`
- Modify: `crates/toolr-core/src/third_party/parse.rs` (drop `UnsupportedArgumentKind` and
  `FRAGMENT_SCHEMA_VERSION`; the version logic stays as-is until Task 5)
- Modify: `crates/toolr-core/src/build_fragment.rs` (drop the `schema_version` parameter, keep
  every declared group, write `origin`, compute the version)
- Modify: `crates/toolr/src/cli.rs:506-512`, `crates/toolr/src/dispatch.rs:356-368` (remove
  `--schema-version`)
- Modify: `crates/toolr-py/python/toolr/_decorators.py:83-88` (`MANIFEST_SCHEMA_VERSION: int = 2`,
  docstring *"Mirrors `SCHEMA_VERSION` in `crates/toolr-core/src/manifest/model.rs`."*)
- Create: `crates/toolr-core/tests/manifest_schema_version_lockstep.rs`
- Create: `crates/toolr-core/tests/plugin_parity.rs` (the parity test)
- Modify: `crates/toolr-core/src/third_party/tests.rs`, `crates/toolr-core/src/parser/build.rs`
  tests around line 688, and any other test that builds `Fragment*` values
  (`git grep -n "FragmentGroup\|FragmentCommand\|FragmentArgument\|FRAGMENT_SCHEMA_VERSION"`)

**Interfaces:**

- Consumes: `build_commands` (Task 2), and the `min_schema_with` hooks (Task 3).
- Produces:

  ```rust
  // third_party/model.rs
  pub struct ManifestFragment {
      /// Lowest toolr schema a reader needs to load this fragment, not the schema of the toolr
      /// that built it.
      pub toolr_schema_version: u32,
      pub package: String,
      #[serde(default)] pub groups: Vec<crate::manifest::Group>,
      #[serde(default)] pub commands: Vec<crate::manifest::Command>,
  }
  impl ManifestFragment {
      pub fn min_schema(&self) -> u32; // min_schema_with(&SupportedTypeKind::since_schema)
      pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32;
  }

  // build_fragment.rs
  pub fn build_third_party_fragment(source_dir: &Path, package_name: &str)
      -> Result<ManifestFragment, BuildFragmentError>;
  ```

  Keep the `// region: SkillRefManifestFragment` markers around `ManifestFragment`. Remove the
  `SkillRefFragmentVersion` region, and add a `// region: SkillRefFragmentFloors` region around
  the two floor constants in `manifest/model.rs`.
- [ ] **Step 1: Write the failing parity test** in `tests/plugin_parity.rs`, using
  `tempfile::TempDir`.
    - One source tree is written twice: under `tools/` and under `<tmp>/parpkg/`, each with an
      `__init__.py`.
    - It contains `enums.py`, with `class Color(enum.Enum): RED = "red"; BLUE = "blue"`.
    - It contains `commands.py`, which uses `from .enums import Color` and declares:
        - `docker = command_group("docker", "Docker", "Docker tools.")`;
        - `image = docker.command_group("image", "Image", "Image tools.")`;
        - `spare = command_group("spare", "Spare", "No commands here.")`;
        - an `@image.command` function `build` with a docstring and these parameters:

          ```python
          def build(
              ctx,
              pair: tuple[str, int] | None = None,
              colour: Color = Color.RED,
              to: Email | None = None,
              out: Annotated[
                  FilePath | None,
                  arg(aliases=["-o"], metavar="FILE", env="PAR_OUT", help_section=...),
              ] = None,
              count: int = 1,
          ) -> None:
          ```

      Take the exact decorator, `arg()` and `help_section` spelling from existing fixtures:
      `git grep -n "help_section" crates/toolr-core/src/parser/build.rs`.
    - Build locally with `toolr_core::parser::build_static_manifest(&tools)`. Build the plugin with
      `build_third_party_fragment(&pkg, "parpkg")`, then `serialise_fragment`, write it to a
      fake venv `lib/python3.13/site-packages/parpkg/toolr-manifest.json`, and merge with
      `third_party::discover_and_merge(&venv, empty_base)`.
    - Normalise both sides: keep only the `origin`-bearing groups and commands from each side,
      sort groups by `full_path()` and commands by `(group, name)`, and serialise to
      `serde_json::Value` strings. Replace `"tools.` with `"parpkg.` and `"tools"` with
      `"parpkg"`, and replace `"third_party"` with `"static"` in the plugin side.
    - Assert equal.
    - Also assert that the fragment has groups `docker` (parent `None`), `docker.image` (by
      full path) and `spare`, and that `pair` has kind `Repeated` and resolved type
      `Optional(Tuple([Str, Int]))`.

  Also write `manifest_schema_version_lockstep.rs`, copying `schema_version_lockstep.rs`. It reads
  `crates/toolr-py/python/toolr/_decorators.py`, finds `MANIFEST_SCHEMA_VERSION: int = N` and
  compares it with `toolr_core::manifest::SCHEMA_VERSION`.
- [ ] **Step 2: Run** `cargo test -p toolr-core --test plugin_parity --test manifest_schema_version_lockstep`.
  Expected: FAIL.
- [ ] **Step 3: Implement.**
    - `model.rs`: the new `ManifestFragment`. Delete `FragmentGroup`, `FragmentCommand`,
      `FragmentArgument` and `FRAGMENT_SCHEMA_VERSION`. `min_schema_with` =
      `max(FRAGMENT_SHAPE_SCHEMA, max over groups of Group::min_schema, max over commands of
      Command::min_schema_with)`.
    - `build_fragment.rs`: delete the group filter and keep every declared group. Set
      `origin = Origin::ThirdParty` on every group and command. Build the fragment with
      `toolr_schema_version: 0`, then set it to `fragment.min_schema()`.
    - `merge.rs`:
        - groups dedup by `full_path()`;
        - push the `Group` with `origin = ThirdParty`;
        - each command gets `origin = ThirdParty`, `dispatched_from = None`,
          `is_dispatcher = false`;
        - delete `group_from_fragment`, `command_from_fragment`, `argument_from_fragment` and the
          `fixed_arity` rejection.
    - `parse.rs`: delete `UnsupportedArgumentKind`, and use
      `crate::manifest::SCHEMA_VERSION` where `FRAGMENT_SCHEMA_VERSION` was.
    - CLI: remove the `schema-version` arg from `cli.rs` and its read in `dispatch.rs`.
    - Python: `MANIFEST_SCHEMA_VERSION: int = 2`.
    - Fix every test that used the removed types. Rewrite the `UnsupportedArgumentKind` test in
      `third_party/tests.rs` as a round trip of a `Repeated` + `Tuple` argument.
- [ ] **Step 4: Run** `cargo test -p toolr-core` and `cargo build -p toolr`. Expected: PASS.
- [ ] **Step 5: Commit.** `feat(manifest)!: ship plugin fragments as full manifest commands (#520)`

---

### Task 5: Load rule and plugin warnings in core (model: opus)

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs` (`Manifest.plugin_warnings`,
  `PluginWarning`, `PluginWarningKind`)
- Modify: every `Manifest { … }` literal (about 41, compile-driven: `cargo build --workspace --tests`
  lists them). Add `plugin_warnings: Vec::new(),`.
- Modify: `crates/toolr-core/src/third_party/parse.rs`, `mod.rs`, `merge.rs`
- Modify: `crates/toolr-core/src/freshness/compare.rs:73-90`
- Modify: `crates/toolr/src/bootstrap.rs` (`carry_forward_cached_entries`)
- Modify: `crates/toolr-core/src/complete/freshness.rs` (`preserve_non_static_entries`)
- Modify: `crates/toolr-core/src/manifest_build/rebuild.rs` (fill `warnings`)
- Test: `crates/toolr-core/src/third_party/tests.rs`, `crates/toolr-core/src/freshness/tests.rs`

**Interfaces:**

- Consumes: `ManifestFragment` v2 (Task 4), `MIN_READABLE_FRAGMENT_SCHEMA` (Task 3).
- Produces:

  ```rust
  // manifest/model.rs (re-export from manifest/mod.rs)
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub plugin_warnings: Vec<PluginWarning>,          // new last field on Manifest

  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct PluginWarning { pub package: String, pub path: std::path::PathBuf,
                             pub kind: PluginWarningKind, pub message: String }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum PluginWarningKind { Skipped, Shadowed }

  // third_party/parse.rs
  pub enum ParsedFragment { Loaded(ManifestFragment, PathBuf), Skipped(PluginWarning) }
  pub fn parse_fragment(path: &Path) -> Result<ParsedFragment, ThirdPartyError>;
  ```

  `ThirdPartyError::UnknownVersion` is removed. `MissingVersion`, `Json`, `Io` and
  `DuplicateCommand` still abort.
- [ ] **Step 1: Write failing tests** in `third_party/tests.rs`:
    - `fragment_at_minimum_schema_loads`: v2 JSON with one group and one command → `Loaded`.
    - `newer_fragment_is_skipped_with_upgrade_hint`: `toolr_schema_version = SCHEMA_VERSION + 1`
      → `Skipped`, and the message equals
      `format!("skipping plugin demo: needs toolr schema {}, this toolr supports {}. Upgrade toolr.",
      SCHEMA_VERSION + 1, SCHEMA_VERSION)`.
      The package name comes from the JSON `package` key, read from the raw value before the full
      deserialise. If that key is missing, fall back to the parent directory name.
    - `v1_fragment_is_skipped_with_rebuild_hint`: `toolr_schema_version = 1` → message
      `skipping plugin demo: built with toolr schema 1, this toolr needs >= 2. Rebuild the plugin with toolr >= 0.34.0.`
    - `bad_command_skips_the_whole_plugin`: a v2 fragment with two commands. The second has
      `"kind": "fixed_arity"` and no `metadata.nargs`. After `discover_and_merge`, **no** group or
      command from that package is in the manifest, and there's one `Skipped` warning.
    - `local_command_shadows_plugin_with_warning`: a base with a static `ci/lint` and a fragment
      with `ci/lint`. Local is kept, and there's one `Shadowed` warning with the message
      `tools/<file> defines ci lint, hiding the one from demo`. Take `<file>` from the local
      command's `module` (`tools.ci` → `tools/ci.py`). The group path in the message uses spaces
      (`docker.image` → `docker image`).
    - `good_plugin_loads_next_to_a_skipped_one`: one v1 and one v2 fragment in the fake venv. The
      v2 commands are present, and there's one `Skipped` warning.
    - In `freshness/tests.rs`: `static_drift_with_venv_escalates_to_third_party_drift`. Change a
      tools file with a venv present → `ThirdPartyDrift`. With `venv_dir = None` → `StaticDrift`.
      Update any existing test that asserted `StaticDrift` with a venv present, and say why in its
      comment.
- [ ] **Step 2: Run** `cargo test -p toolr-core third_party freshness`. Expected: FAIL.
- [ ] **Step 3: Implement.**
    - `parse_fragment` reads the version as today, then:
        - if `version < MIN_READABLE_FRAGMENT_SCHEMA` or `version > SCHEMA_VERSION`, return
          `Skipped` with the spec's message;
        - otherwise deserialise, then run `Argument::validate` on every argument of every command
          **before returning**. The first failure returns `Skipped`, with the message
          `skipping plugin <pkg>: <validate error>`.
    - `discover_and_merge` collects `Loaded` fragments and `Skipped` warnings, then calls
      `merge_into_manifest(base, fragments)`, which now returns
      `Result<Manifest, ThirdPartyError>` with its `Shadowed` warnings pushed onto
      `base.plugin_warnings`, after the `Skipped` ones.
    - `compare`: after the version check and the hash computations, if the static hash differs
      and `venv_dir.is_some()`, return `ThirdPartyDrift`. Update the doc comment.
    - `carry_forward_cached_entries` (runs only on `StaticDrift`, which now means no venv) and
      `preserve_non_static_entries` also copy `cached.plugin_warnings` into `fresh`.
    - `rebuild_manifest_full`: `warnings: manifest.plugin_warnings.iter().map(|w| w.message.clone()).collect()`.
- [ ] **Step 4: Run** `cargo test --workspace` in the background, polling. Expected: PASS.
- [ ] **Step 5: Commit.** `feat(manifest): skip unloadable plugins and record plugin warnings (#520)`

---

### Task 6: Print warnings, and binary-level tests (model: sonnet)

**Files:**

- Modify: `crates/toolr/src/main.rs:36-40` (print after `load_or_empty`)
- Modify: `crates/toolr/tests/untrusted_repo.rs:128-193` (bump the fixture to v2, add a v1
  sibling)
- Create: `crates/toolr/tests/plugin_warnings.rs`

**Interfaces:**

- Consumes: `Manifest.plugin_warnings` (Task 5), `bootstrap::should_skip_auto_rebuild`
  (`bootstrap.rs:76`, already `pub(crate)`), and `argv_requests_quiet` (`main.rs:112`).
- [ ] **Step 1: Write failing tests** in `crates/toolr/tests/plugin_warnings.rs`. Copy the project
  and venv fixture shape from `untrusted_repo.rs::manifest_rebuilds_when_a_venv_appears`, a
  cache-located venv with a fragment under `site-packages/<pkg>/`. Put the setup in a helper. Each
  test uses `assert_cmd::Command::cargo_bin("toolr")`:
    - a v1 `demo_plugin` fragment plus a good v2 `good_plugin` → `toolr --help` succeeds, stderr
      contains `toolr: warning: skipping plugin demo_plugin`, stdout lists `good`'s group, and a
      local command still runs;
    - running a local command twice → the warning appears on both runs (the second from a fresh
      cache);
    - `toolr --quiet --help` and `toolr __complete …` (copy the completion invocation from an
      existing completion test: `git grep -n "__complete" crates/toolr/tests | head`) → stderr has
      no `warning: skipping`;
    - `toolr project manifest rebuild` → the warning appears exactly once in stderr;
    - shadowing: a v2 plugin with `ci lint`, and no local `ci lint` → run once, plugin command
      present. Add `tools/ci.py` with a local `lint` → the next run warns `hiding the one from`,
      and the local one runs. Remove it → the next run has no warning, and the plugin `ci lint` is
      back in `--help`;
    - nested: a plugin `docker.image.build` and a local `ci.image.build` → `--help` for each group
      lists `build`. Running `toolr docker image build` and `toolr ci image build` each hits its
      own function (check it through a `ctx.print` marker, or through the manifest's `module`).
      Skip running the plugin one if the fixture venv can't import the plugin package; then assert
      on `tools/.toolr-manifest.json` instead.

  In `untrusted_repo.rs`: change the fixture at about line 163 to v2 (add `"origin"` on its group
  and command, and set `"toolr_schema_version": 2`). Add `v1_plugin_is_skipped_with_a_warning` with
  the same fixture at version 1: the group is absent, and the warning is on stderr.
- [ ] **Step 2: Run** `cargo test -p toolr --test plugin_warnings --test untrusted_repo`.
  Expected: the warning tests FAIL.
- [ ] **Step 3: Implement** in `main.rs::run`, right after `let manifest = load_or_empty(&cwd);`:

  ```rust
  if !bootstrap::should_skip_auto_rebuild(&argv) && !argv_requests_quiet(&argv) {
      for w in &manifest.plugin_warnings {
          eprintln!("toolr: warning: {}", w.message);
      }
  }
  ```

- [ ] **Step 4: Run** the same tests. Expected: PASS.
- [ ] **Step 5: Commit.** `feat(cli): warn about skipped and shadowed plugins on every run (#520)`

---

### Task 7: Docs, skills, example and release notes (model: sonnet)

**Files:**

- Modify: `docs/third-party.md` (the JSON example at line 70, "Command resolution" at about 246,
  the version paragraph at about 268)
- Modify: `docs/cli.md:315` (drop `--schema-version`)
- Regenerate: `docs/cli-files/self-build-manifest-help.txt` (`toolr pre-commit regen-doc-snippets`,
  or `cargo run -p toolr -- pre-commit regen-doc-snippets`)
- Modify: `crates/xtask/src/build_skill_refs/packaging.rs:40-90`
- Regenerate: `skills/*/references/*.md` (`cargo xtask build-skill-refs`)
- Modify by hand: `skills/toolr-command-packaging/SKILL.md` (prose about the fragment version,
  `--schema-version`, group augmentation)
- Regenerate: `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json`
  (`cargo run -p toolr -- self build-manifest --source-dir examples/plugin-package/src/toolr_example_plugin`,
  or the `--check` invocation CI uses in `.github/workflows/_test.yml:139-148`)
- Modify: `UNRELEASED.md` (new entry, and amend the #506 sentence at about line 98)
- [ ] **Step 1: `packaging.rs`.** Point the version entry at `("manifest/model.rs", "SkillRefFragmentFloors")`.
  Its narrative: *"`toolr_schema_version` in a plugin's `toolr-manifest.json` is the lowest toolr
  schema that can read the fragment. `toolr self build-manifest` computes it from the types and
  features the plugin uses. A toolr whose schema is between `MIN_READABLE_FRAGMENT_SCHEMA` and its
  own `SCHEMA_VERSION` loads the fragment. Anything else is skipped with a warning, and the rest
  of the CLI keeps working."* Update the `SkillRefManifestFragment` narrative to say groups and
  commands are the manifest's own `Group`/`Command` types.
- [ ] **Step 2: Docs.**
    - Replace the `docs/third-party.md` JSON example with the regenerated example plugin fragment.
    - Rewrite the version paragraph to match spec §4's load-rule table.
    - Under "Command resolution":
        - local wins and warns on every run;
        - to add to a host group, a plugin declares that group (same full path), and the host's
          title wins;
        - link #522 for choosing a winner.
    - Add a "Build-time checks" note: positional arity and undeclared groups now fail
      `self build-manifest`.
- [ ] **Step 3: `UNRELEASED.md`.** Add an entry following the file's existing style (read the top
  of the file first) that covers:
    - typed validation, `arg()` metadata, completion and typed `tuple` for plugin commands;
    - nested plugin groups;
    - the migration: rebuild and republish plugins; v1 fragments are skipped with a warning;
      toolr 0.33.0 and older abort the whole merge on a fragment built by this toolr;
    - host-group declaration;
    - `--schema-version` removed;
    - the new build errors;
    - shadow warnings.

  Amend the #506 sentence "it accepts only the current `toolr_schema_version`" to point at the
  new rule.
- [ ] **Step 4: Regenerate** the example fragment, doc snippets and skill refs. Then run
  `cargo xtask build-skill-refs --check`, `uv run mkdocs build --strict` and
  `prek run --all-files`. Expected: all clean.
- [ ] **Step 5: Commit.** `docs(plugins): document fragment v2, the load rule and plugin warnings (#520)`

---

### Task 8: Full verification and spec archive (model: orchestrator, no dispatch)

- [ ] **Step 1:** Run `mise run test` in the background, polling every 60 s. Expected: PASS. Fix
  and re-run on failure.
- [ ] **Step 2:** Run `git grep -n "FRAGMENT_SCHEMA_VERSION\|FragmentArgument\|FragmentGroup\|FragmentCommand\|UnsupportedArgumentKind\|schema-version\|skipped_plugins"`.
  Expected: hits only under `specs/` and `CHANGELOG.md`.
- [ ] **Step 3:** Grep the changed files for employer names and absolute home paths.
  Expected: nothing.
- [ ] **Step 4:** Adversarial review of `main..HEAD` (model: fable). Fix the confirmed findings,
  each in its own commit.
- [ ] **Step 5:** Archive:
  `git mv specs/2026-09-29-plugin-parity-design.md specs/2026-09-29-plugin-parity-plan.md specs/archive/2026/`.
  Commit: `docs(specs): archive the plugin parity design and plan (#520)`.
