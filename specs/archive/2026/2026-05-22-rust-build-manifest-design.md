# Pure-Rust `toolr self build-manifest`

**Date:** 2026-05-22
**Status:** design
**Related:** `specs/2026-05-21-dispatch-manifest-freshness-design.md` (closed the
dispatch path; this closes the authoring path).

## Branching: stacked on top of dispatch-manifest-freshness

This work **must** be implemented on a `git-spice`-tracked branch
stacked on top of `dispatch_manifest_freshness` (PR #234) — not on
`main`. That predecessor branch:

- Introduces `Origin::ThirdParty` and the `freshness::compare`
  module that this work's `build_third_party_fragment` builds on.
- Removes the legacy entry-point plugin loader, which means the
  Python `toolr.build` module is now the ONLY caller of
  `_get_command_group_storage` outside test code — making it safe to
  remove here without orphaning consumers.
- Lands `examples/plugin-package/` as the canonical fixture this
  work's golden test uses for byte-for-byte equivalence checks.
- Adds the CI step that runs `toolr self build-manifest
  toolr_example_plugin --check` on every test job — that step will
  exercise the Rust replacement automatically once this lands.

Concretely:

```bash
gs branch checkout dispatch_manifest_freshness
gs branch create rust_build_manifest                # auto-bases on current
# ... implement, commit ...
gs branch submit --draft --fill                     # opens a stacked PR
```

Do not merge `dispatch_manifest_freshness` into `main` first and then
work from `main` — the rebase would lose the stack relationship that
git-spice tracks, and the implementing session's first task is
verifying it inherited the predecessor's commits before doing
anything else.

## Problem

`toolr self build-manifest <package>` is the tool plugin authors run to
regenerate the `toolr-manifest.json` they ship inside their wheel. The
current implementation in `crates/toolr/src/dispatch.rs::run_self_build_manifest`
shells out to `python -m toolr.build` (`crates/toolr-py/python/toolr/build.py`),
which:

1. `importlib.import_module(package_name)` — **requires the package to
   be importable on `sys.path`**, i.e. `pip install -e .` first.
2. Walks submodules via `pkgutil.walk_packages` so decorators fire and
   populate the global `command_group` registry.
3. Reads that registry and emits a JSON fragment.

This is the last Python-side authoring path in toolr. Two problems:

- **Workflow friction.** Plugin authors can't regenerate a manifest from
  a fresh checkout without installing first. CI pipelines have to
  `pip install -e .` before `toolr self build-manifest --check`. The
  earlier Rust freshness work removed the Python boot from the
  dispatch path; the authoring path is the only place Python still
  matters.
- **Architectural inconsistency.** The Rust binary already AST-walks
  `tools/*.py` via `ruff_python_parser` to build the project's own
  static manifest. The same machinery can produce a third-party
  fragment — there is no good reason to maintain a parallel Python
  implementation that interprets the same decorator semantics
  differently.

This spec proposes replacing the Python path with a pure-Rust
implementation that AST-walks the package source directory and emits
the existing third-party fragment shape unchanged.

## Goals

- `toolr self build-manifest <package>` works without Python being
  callable, without the package being installed, and without
  spawning any subprocess.
- The emitted `toolr-manifest.json` is byte-for-byte identical to what
  the current Python implementation produces for any plugin that uses
  only statically-declared commands. (Equivalent under `git diff` once
  ordering and whitespace are normalised — see "Output stability"
  below.)
- The `--check` flag retains its current contract: exit 0 if the file
  on disk matches the freshly-generated fragment, exit non-zero with a
  unified-diff on stderr if it drifts.
- A `--source-dir PATH` flag lets plugin authors point the tool at
  their source tree directly, bypassing any installed-package lookup.
- Existing CLI flags (`--output`, `--schema-version`, `--check`) keep
  their current names and semantics so users' CI scripts and
  pre-commit hooks don't break.
- The Python `toolr.build` module is removed in the same change.
  There is no deprecation period: the CLI surface is preserved, the
  underlying implementation changes.

## Non-goals

- **No dynamic-pattern support.** The current Python implementation
  captures commands registered by runtime code paths (`for x in X:
  group.command(...)`). The Rust AST parser only sees statically-
  declared commands. This is a deliberate narrowing: shipping a
  static manifest is incompatible with dynamic patterns *at dispatch
  time* anyway (the Rust dispatch path doesn't run Python). Plugins
  using dynamic registration were already producing manifests that
  worked-for-build but not-for-dispatch; codifying static-only here
  makes the contract honest. Plugin authors with dynamic patterns
  can hand-edit the fragment JSON, exactly as the docs already
  recommend for any case the parser doesn't cover.
- **No new fragment fields.** The schema stays at v1. This is a
  pure-implementation swap.
- **No change to how dispatch consumes fragments.** The third-party
  glob, parse, and merge code in `crates/toolr-core/src/third_party/`
  is untouched.

## Background: the existing fragment schema

Plugins ship a JSON file named `toolr-manifest.json` at the root of
their installed package directory. The current schema (already
implemented in `crates/toolr-core/src/third_party/model.rs` and
populated by `crates/toolr-py/python/toolr/build.py`):

```json
{
  "toolr_schema_version": 1,
  "package": "toolr_example_plugin",
  "groups": [
    {
      "name": "third-party",
      "title": "Third Party Tools",
      "description": "Tools contributed by a third-party plugin."
    }
  ],
  "commands": [
    {
      "name": "hello",
      "group": "third-party",
      "module": "toolr_example_plugin.commands",
      "function": "hello_command",
      "summary": "Say hello to someone.",
      "description": "Say hello to someone.",
      "arguments": [
        {
          "name": "name",
          "kind": "optional",
          "type_annotation": "str",
          "default": "'World'",
          "help": "Name to greet (default: World).",
          "allowed_values": []
        }
      ],
      "imports": []
    }
  ]
}
```

Notably absent (compared to the *project* manifest at
`tools/.toolr-manifest.json`): `static_hash`, `third_party_hash`,
`schema_version` (the fragment uses `toolr_schema_version`),
`origin` per entry, `parent` group references. The fragment is a
strict subset — third-party plugins don't have nested groups or
hashes.

## Design

### CLI surface

Preserve existing flags. Add one new flag for the source-path workflow.

```text
toolr self build-manifest <package>           # rebuild installed plugin's manifest
toolr self build-manifest <package> --check   # exit non-zero on drift
toolr self build-manifest --source-dir PATH \  # source-tree workflow
    --package PKG [--output PATH]

# Existing flags preserved:
#   --output PATH         where to write (default: <package_dir>/toolr-manifest.json)
#   --schema-version N    override toolr_schema_version (default: current)
#   --check               drift-check mode

# Removed:
#   --python PATH         no longer relevant (no Python is spawned)
```

`<package>` and `--source-dir` are mutually exclusive. The argv parser
should reject the combination with a clear error.

### Source-directory resolution

Two entry points:

1. **`<package>` given (positional arg).** Resolve the source
   directory by globbing the project's tools venv:
   `<tools_venv>/lib/python*/site-packages/<package>/`. If exactly one
   match is found, use it. If zero matches → error: `package not
   found in tools venv; run \`uv sync\` or pass --source-dir`. If
   multiple matches (multi-Python-version venvs are theoretically
   possible) → use the first lexicographically, with a warning.

2. **`--source-dir PATH` given.** Use the path verbatim. The
   `--package` flag must also be supplied so the fragment's
   `"package"` field is correct (otherwise infer from the leaf
   directory name).

In both cases, the resolver returns a `(source_dir: PathBuf,
package_name: String)` tuple consumed by the next stage.

The tools-venv resolution reuses `toolr_core::venv::resolve_venv_path`
(already used by `ensure_manifest_fresh`). If venv resolution fails,
the CLI falls back to error rather than silently using a stale path.

### AST walk and fragment emission

A new `toolr_core::build_fragment` module:

```rust
pub fn build_third_party_fragment(
    source_dir: &Path,
    package_name: &str,
    schema_version: u32,
) -> Result<ThirdPartyFragment, BuildError>;
```

Implementation:

1. Walk `source_dir` recursively for `*.py` files. Reuse
   `crates/toolr-core/src/parser/build.rs::list_python_files`
   (rename / make `pub(crate)` if needed).
2. For each file, parse with `ruff_python_parser` and run the same
   passes `build_static_manifest_inner` runs:
   - Cross-file enum + type-alias + arg-section table merge (pass 1).
   - Per-file group + command extraction (pass 2).
3. Map the resulting `Manifest`'s `groups` and `commands` into the
   third-party `ThirdPartyFragment` shape:
   - Drop project-only fields (`origin`, `parent`, hashes).
   - Filter out groups and commands whose module path doesn't start
     with `package_name` or `package_name.` — the same "belongs to
     this package" filter the Python implementation applies via
     `_belongs_to_package`. This catches any user code accidentally
     imported into the package's namespace but not actually shipped
     with it.
4. Compute module paths from file paths the same way the project
   parser does: `package_name + "." + relative_path_without_.py`,
   with `__init__.py` collapsing to the package name itself.
5. Sort: groups by `name`, commands by `(group, name)`. Same as the
   current Python implementation, which sorts for deterministic
   output.
6. Return the fragment. The CLI layer handles serialization and
   disk I/O.

### Output stability

The fragment must serialize identically across implementations so
`--check` doesn't false-positive on a churn diff. Three things to pin:

- **JSON formatting:** `serde_json::to_string_pretty` with 2-space
  indent, sorted keys (the Python implementation uses
  `json.dumps(indent=2, sort_keys=True)`).
- **Trailing newline:** present (Python writes `serialized + "\n"`).
- **Field ordering inside objects:** irrelevant for matching since
  `sort_keys` is on, but the model's `Serialize` derive should match
  field names exactly. Bias toward serde's default snake_case
  mapping rather than custom renames.

Add a single golden test comparing Rust output vs a checked-in
reference fragment to lock this down.

### CLI dispatch wiring

In `crates/toolr/src/dispatch.rs`:

```rust
fn run_self_build_manifest(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    let (source_dir, package_name) = resolve_source_and_package(matches)?;
    let schema_version = matches
        .get_one::<u32>("schema-version")
        .copied()
        .unwrap_or(MANIFEST_SCHEMA_VERSION);
    let output_path = resolve_output_path(matches, &source_dir);

    let fragment = build_third_party_fragment(&source_dir, &package_name, schema_version)?;
    let serialized = serialize_fragment(&fragment)?;

    if matches.get_flag("check") {
        check_against_disk(&output_path, &serialized)
    } else {
        write_atomically(&output_path, &serialized)?;
        eprintln!(
            "toolr.build: wrote {} group(s) / {} command(s) to {}",
            fragment.groups.len(),
            fragment.commands.len(),
            output_path.display(),
        );
        Ok(ExitCode::SUCCESS)
    }
}
```

`check_against_disk` reads the on-disk file, compares to `serialized`,
emits a unified diff on stderr if they differ, and returns exit 1.

### Removal of the Python implementation

- Delete `crates/toolr-py/python/toolr/build.py` outright.
- Remove the now-redundant Python invocation in
  `crates/toolr/src/dispatch.rs::run_self_build_manifest` (the
  `Command::new(python).args(["-m", "toolr.build", …])` block).
- Drop `--python` from the CLI surface (`crates/toolr/src/cli.rs`).
  Add a small migration message if users pass it: emit a warning
  ("`--python` is ignored; toolr no longer spawns Python here") for
  one minor version, then remove.
- Update `docs/third-party.md` to drop any mention of Python
  requirements for build-manifest. Add a note explaining the
  static-only contract (no dynamic patterns).
- Drop tests under `crates/toolr-py/python/toolr/build.py` and
  the harness fixtures that exercised the Python build path.
- Keep `toolr._decorators._get_command_group_storage` — it's still
  used by `toolr.testing` for in-process plugin tests.

## Differences from the current behavior

| Behavior | Today (Python) | Proposed (Rust) |
|---|---|---|
| Requires `pip install -e .` of the package | Yes | No |
| Spawns Python subprocess | Yes | No |
| Captures dynamic command registration | Yes (loops, runtime code) | No (static-only) |
| Captures cross-file enums and type aliases | Yes (via runtime introspect) | Yes (via existing AST tables) |
| `--check` exit codes | 0 = match, non-zero = drift | unchanged |
| `--source-dir` flag | Does not exist | New, alongside existing `<package>` arg |
| `--python` flag | Selects interpreter | Removed (warn-then-delete) |
| Output JSON byte-equivalence | n/a | Required (golden test) |

The dynamic-pattern regression is the only material behavior change.
Plugin authors using dynamic patterns will see their next
`build-manifest --check` fail in CI. That's a feature, not a bug:
the manifest they were producing didn't accurately describe what
dispatch could resolve.

## Migration plan

This is a single commit (or short branch):

1. Add `toolr_core::build_fragment` module + tests.
2. Rewire `crates/toolr/src/dispatch.rs::run_self_build_manifest` to
   call the new Rust path. Keep the Python invocation behind a
   `TOOLR_LEGACY_BUILD_MANIFEST=1` env var for one release if we
   want a panic button, but only if reviewer asks — default position
   is delete cleanly.
3. Regenerate `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json`
   with the new tool. Confirm it matches the existing committed file
   byte-for-byte. Add the comparison as a CI step (the existing
   `--check` workflow step in `.github/workflows/_test.yml` already
   does this).
4. Delete `crates/toolr-py/python/toolr/build.py`.
5. Update `docs/third-party.md` accordingly.

If the example's regenerated manifest differs from the committed
one, that's diagnostic: either the Python implementation had a bug
the Rust one fixes, or the Rust one has a bug. Investigate before
landing.

## Testing strategy

### Unit tests in `toolr-core`

- **Golden test:** fixture directory with a known set of `.py` files
  declaring representative commands (positional/optional/flag args,
  Literal types, default values, multi-file). Compare generated
  fragment to a checked-in `expected.json`. This is the byte-for-byte
  regression guard.
- **Filtering:** include a `.py` file in the source dir that imports
  `command_group` from `toolr` and declares a group, but the import
  is from a module whose path doesn't start with the target package.
  Confirm it's filtered out of the fragment.
- **Empty package:** a directory with `.py` files but no
  `command_group` calls returns an empty fragment. The CLI layer then
  errors with "no commands found" (mirror current behavior).
- **Bad syntax:** a `.py` file with a parse error surfaces as a
  `BuildError::Build` with the file path in the message.
- **Cross-file enums:** enum defined in module `a.py`, used as a
  Literal type in command in `b.py`. Confirm the type resolves
  correctly. This exercises the existing two-pass parser.

### Integration test in `toolr` CLI

- Build a temp directory with a small plugin source tree, run
  `target/debug/toolr self build-manifest --source-dir <path>
  --package foo`, read the output, compare to expected.
- Same but with `--check` against a known-good fragment.
- Same but with `--check` against a *drifted* fragment: expect exit 1
  and a unified diff on stderr.

### Distribution-test integration

The existing `tests/distribution/test_example_plugin_contract.py`
builds the example wheel and asserts the shipped manifest matches
runtime expectations. That test continues to pass unchanged because
the fragment shape is preserved.

### Equivalence test (one-time, deletable)

During the migration commit, run both implementations against the
example plugin and confirm byte-equivalent output. This proves the
swap is safe. The test can be deleted after the Python implementation
is removed.

## Edge cases worth pinning down before implementation

- **Namespace packages.** The Python implementation explicitly rejects
  namespace packages (no `__init__.py`). Replicate this: if the
  source dir has no `__init__.py`, error with the same message.
- **Single-file packages.** A package that's just `foo.py` (not a
  directory) doesn't fit the model — the manifest lives at the
  package root. Should we support these? Probably no — issue
  the same "namespace packages not supported" error and call it
  the same restriction. Document the limitation.
- **Subpackages.** A plugin with `mypkg/`, `mypkg/sub/`, etc. — the
  walker should find commands in both. The current Python
  implementation does. The Rust AST walker should walk recursively.
- **Mixed-content directories.** If `source_dir` contains both Python
  files and non-Python (templates, data files), the walker ignores
  the non-Python content. No change needed; `WalkDir` filtering on
  `.py` extension already handles this.
- **`schema_version` override.** Today's Python tool honors
  `--schema-version N`. Preserve in the Rust CLI; pass through to
  the fragment. Useful for backporting tests.

## Open questions

- **Default behavior for "package not in venv".** Should the Rust tool
  fall back to scanning the *project's* `tools/` directory if the
  package isn't found in the venv? My instinct: no. The two concepts
  are different — `tools/*.py` is the user's own commands; plugin
  source is a separately-versioned distribution. Conflating them in
  the resolver would hide bugs. But it's worth raising.
- **`--check` output format.** Today's Python tool exits non-zero
  but doesn't emit a diff. The Rust replacement could emit a unified
  diff via `similar::TextDiff` (already in toolr-core's deps via
  some transitive path — verify, otherwise add). Worth it? Probably,
  given that CI users will see exactly what drifted without having
  to regenerate locally.
- **Performance.** The Rust path should be 10–100× faster than
  Python for any non-trivial plugin (no interpreter boot, no
  pkgutil walk). Worth measuring on a real-world plugin once
  available. Not a blocker.

## Self-review checklist (before implementing)

- [ ] Confirmed the existing third-party fragment shape is fully
      reproducible by the AST parser (no field requires runtime
      introspection like `inspect.signature`).
- [ ] Confirmed `build_static_manifest_inner` produces argument
      schemas that match the current Python `_serialize_argument`
      output. The hardest case is `Literal[...]` default values and
      typed defaults like `None` vs `"None"`.
- [ ] Confirmed `crates/toolr-core/src/parser/build.rs::list_python_files`
      is reusable (or easy to factor) for an arbitrary directory,
      not just `tools/`.
- [ ] Confirmed `toolr_core::venv::resolve_venv_path` can be called
      from the dispatch-time CLI handler without re-implementing
      project root discovery.

If any of the above fails, the design needs an addendum — most likely
a small toolr-core PR factoring the AST argument extractor into a
reusable shape before the fragment-emitter sits on top of it.
