# Path-state types in `toolr.types`

Issue: [#502](https://github.com/s0undt3ch/ToolR/issues/502).

## Goal

A command author states what a path argument must be by picking a type, not by adding `arg()`
keywords. The type name says what the binary checked. A type checker can tell the types apart. The
command function always receives a `pathlib.Path` instance.

```python
from toolr.types import FilePath, NewPath

def convert(ctx, source: FilePath, output: NewPath) -> None: ...
```

## Non-goals

- No `Command` type that looks a name up on `PATH`. `ExecutablePath` is a path.
- No readable-file type, no symlink, fifo or socket kinds, no `-` for stdin or stdout.
- No fix for Windows `canonicalize()` returning `\\?\C:\…` paths. `ResolvedPath` already has this
  problem, and the new canonical types inherit it. File it on its own.

## Today

- `toolr.types.AbsolutePath` and `toolr.types.ResolvedPath` are plain aliases (`= pathlib.Path`).
  A type checker sees `Path` for both.
- `arg(must_exist=, must_be_file=, must_be_dir=)` adds checks to any of the three path types. The
  parser stores them as `PathConstraints` on the manifest argument
  (`crates/toolr-core/src/parser/types/path_constraints.rs`). `path_parser` in
  `crates/toolr/src/value_parsers.rs` applies them.
- Since #500, a misspelled `arg()` keyword fails the manifest build. So typo safety is no longer a
  reason for this change. The reasons are one spelling per meaning, discoverability through
  `dir(toolr.types)`, and distinct static types.

## The catalogue

Each type is a `typing.NewType`. The chain gives the subtype relation: a value of any type can go
where its parent is expected.

Every name ends in `Path`, so a reader knows it is a path type before reading the rest. `FilePath`,
`DirectoryPath` and `NewPath` use pydantic's names, and pydantic's meanings for them are the same
as ours. The names in #502 (`ExistingFile`, `ExistingDir`) are not used.

```python
AbsolutePath          = NewType("AbsolutePath", Path)
NewPath               = NewType("NewPath", AbsolutePath)
ResolvedPath          = NewType("ResolvedPath", Path)
FilePath              = NewType("FilePath", ResolvedPath)
DirectoryPath         = NewType("DirectoryPath", ResolvedPath)
ExecutablePath        = NewType("ExecutablePath", FilePath)
WritableDirectoryPath = NewType("WritableDirectoryPath", DirectoryPath)
```

| Type | The binary rejects the value unless | Value handed to Python | clap `ValueHint` |
|---|---|---|---|
| `pathlib.Path` | (no check) | as typed | `AnyPath` |
| `AbsolutePath` | (no check) | joined to cwd if relative | `AnyPath` |
| `NewPath` | the path does not exist, and its parent is an existing directory | absolute | `AnyPath` |
| `ResolvedPath` | the path exists | canonical | `AnyPath` |
| `FilePath` | the path is a regular file | canonical | `FilePath` |
| `DirectoryPath` | the path is a directory | canonical | `DirPath` |
| `ExecutablePath` | the path is a regular file the process can execute | canonical | `ExecutablePath` |
| `WritableDirectoryPath` | the path is a directory the process can write to | canonical | `DirPath` |

"Canonical" means `std::fs::canonicalize`: absolute, with symlinks and `..` resolved. The kind checks
run on the canonical path. So a symlink to a directory counts as a `DirectoryPath`.

`NewPath` is absolute, not canonical, because the path itself does not exist yet. Its parent is
checked with `is_dir()`, which follows symlinks.

### Check details

- **Executable, Unix:** `libc::access(path, X_OK) == 0`. This honours the real uid, gid and mode
  bits, which a bare `mode & 0o111` test does not.
- **Executable, Windows:** the extension is in `%PATHEXT%`, compared without regard to case.
  `PATHEXT` falls back to `.COM;.EXE;.BAT;.CMD` if it is unset.
- **Writable, Unix:** `libc::access(path, W_OK) == 0`.
- **Writable, Windows:** create and drop a file with `tempfile::tempfile_in(path)`. The read-only
  attribute means nothing for directories on Windows, and ACLs are the real control. This probe is
  the only test that works on Windows. Its cost is a short-lived file in the directory.
- Executable and writable checks are advisory. The state can change between the check and the use.
  The docs say this.

### Error messages

One message per failure, with the path as the user typed it:

- `path does not exist: <p>`
- `path is not a regular file: <p>`
- `path is not a directory: <p>`
- `path is not executable: <p>`
- `directory is not writable: <p>`
- `path already exists: <p>`
- `parent directory does not exist: <parent>`

## Why `NewType`

- **Runtime:** `NewType` returns its argument unchanged, so the command gets a `pathlib.Path`.
  msgspec unwraps chained `NewType`s to their base type during the runner's
  `msgspec.convert(value, type=hint, dec_hook=_dec_hook)` call. Checked with msgspec 0.21.1 for a
  three-level chain, for `list[T]` and for `T | None`. All three produced a `PosixPath`.
- **Static:** pyright and mypy treat each `NewType` as a distinct subtype. Passing a bare `Path`
  where a `FilePath` is expected is a type error. `p / "x"` returns a plain `Path`, which is
  correct: a child of a `DirectoryPath` is not known to exist.
- **Parser:** the Rust parser resolves `toolr.types.<Name>` by name
  (`resolve_toolr_types_name` in `crates/toolr-core/src/parser/types/resolve.rs`). It never reads
  the definition, so switching aliases to `NewType` changes nothing on the Rust side.

Rejected:

- **Plain aliases.** This is the current state. The type checker sees `Path`, so the name is the
  only signal.
- **`Path` subclasses.** Python only supports subclassing `pathlib.Path` from 3.12, and toolr
  supports 3.11 (`requires-python = ">=3.11"`). Subclasses also pass through `p / "x"` and
  `.parent`, which would claim an existence nobody checked.
- **`Annotated[Path, marker]`.** Readable, but the type checker still sees only `Path`.
- **Composite `X[File, Writable]`.** A throwaway spike with mypy `--strict` made this half work. The
  type checker saw a class `X(pathlib.Path, Generic[*Ts])`, which only existed under
  `TYPE_CHECKING`. At runtime, `X[...]` evaluated to `Annotated[pathlib.Path, ...]`, and msgspec
  still decoded the value to a `PosixPath`. Four results rejected it:
    - Every `pathlib` method that returns `Self` leaked the parameter. `p.with_suffix(".y")`
      type-checked as `X[File]`. Fixing this needs about 20 method overrides, each with a
      `# type: ignore[override]`.
    - `X[Dir, Writable]` is not assignable to `X[Dir]`. Composition was the point of the design,
      and it gives no subtype relation. The `NewType` chain does.
    - Combinations like `X[File, Dir]` would need rules to reject them at build time.
    - Naming the class `Path` clashes with `pathlib.Path`. Another name fixes only this last
      problem. `X[File | Dir]` reads as "file or dir", so `|` cannot mean "and".

## Removing `arg(must_*)`

- Remove `must_exist`, `must_be_file` and `must_be_dir` from `arg()` in
  `crates/toolr-py/python/toolr/utils/_signature.py`.
- The parser drops them from the known-keyword set. An author who still writes one gets the #500
  unknown-keyword build error, with a type hint in place of "did you mean":
  `` unknown `arg()` keyword `must_be_file`; use `toolr.types.FilePath` instead ``. `must_exist`
  points at `ResolvedPath`, and `must_be_dir` at `DirectoryPath`. The dropped `path_must_*`
  spellings from #500 get the same hints.
- `PathConstraints` goes entirely. It is not read or written, and there is no migration from it.

There is no deprecation period, including for local code: toolr is pre-1.0, and the error names the
fix.

## Where the checks run

All checks run in the Rust binary. Each type gets its own clap value parser in
`crates/toolr/src/value_parsers.rs`, and it runs while the CLI is parsed, before the Python runner
starts. A bad path is a normal clap usage error. The binary sends the checked, canonical path as a
string. The runner turns it into a `pathlib.Path` through msgspec.

The Python types carry no checks, because `NewType` is identity at runtime. Calling a command
function directly, for example in a test built with `toolr.testing.make_context`, checks nothing.
The docs say this.

## Schema bump, enforced

`manifest::SCHEMA_VERSION` in `crates/toolr-core/src/manifest/model.rs` goes from 1 to 2. The
`path_constraints` field goes away, and the argument types gain five new variants.

Today `load_manifest` accepts any version up to the current one, and freshness doesn't look at the
schema version. A released upgrade already forces a rebuild, because `compare` in
`crates/toolr-core/src/freshness/compare.rs` treats a `toolr_version` mismatch as stale (#420).
But a dev build keeps the same version string across commits, and there a cached v1 manifest would
keep loading with its `path_constraints` silently ignored. So `compare` also treats
`schema_version != SCHEMA_VERSION` as stale, next to the `toolr_version` check. The user does
nothing. If `tools/` still uses `arg(must_*)`, the rebuild fails with the unknown-keyword error.

### Plugin fragments are unaffected

`FRAGMENT_SCHEMA_VERSION` stays at 1. Fragments never carried `path_constraints` or any type
information. `FragmentArgument` has only `name`, `kind`, `help`, `default`, `type_annotation` and
`allowed_values`. `merge.rs` sets `resolved_type: None` and `path_constraints: None` on every plugin
argument. The fragment format doesn't change, so bumping it would break every published plugin for
nothing.

As a result, the new path types' checks **do not run for plugin commands**. The value still reaches
Python as a `pathlib.Path`. That gap already exists, and this design doesn't fix it. See "Found, out of scope".

## Known limit: non-literal defaults

`config: FilePath = "pyproject.toml"` is a literal default. clap runs it through the value
parser, so it is checked. `config: FilePath = Path("pyproject.toml")` is stored as the `<expr>`
sentinel, and the CLI then applies no default (`crates/toolr/src/cli.rs`, `ArgumentKind::Optional`).
Python's own default then applies, unchecked. The docs say this. Fixing it is out of scope.

## Found, out of scope

- **Plugin arguments get no clap value parser.** Fragments record no `SupportedType`, so for plugin
  commands clap accepts any string. The runner's `msgspec.convert` still converts values to the
  annotated type. So `int`, `UUID` and `DateTime` fail late, inside Python, with a msgspec error
  instead of a clap usage error. `Email` gets no validation, and there are no completion hints.
  Checks that only Rust does, which includes every path-state check here, never run for plugin
  commands. Fixing this needs a fragment schema v2 that records the type. File it as its own issue.
- **`parse_fragment` comment is wrong.** It says only the current version is accepted. The code
  accepts every version from 1 to current. Harmless while there is only v1, but the comment
  should match the code.
- **Windows `canonicalize()` returns `\\?\` paths.** This is already in the non-goals.

## Changes

Rust:

- `crates/toolr-core/src/parser/types/supported.rs`: variants `NewPath`, `FilePath`, `DirectoryPath`,
  `ExecutablePath` and `WritableDirectoryPath`, each with its `doc()` row and kind. Follow the
  "Adding a supported type" checklist in `CONTRIBUTING.md`.
- `crates/toolr-core/src/parser/types/resolve.rs`: map the five names.
- `crates/toolr/src/value_parsers.rs`: replace the `PathConstraints` knob on `path_parser` with a
  per-type check enum. Set completion hints from the type.
- `SupportedType::is_path()`: an exhaustive `match` in `supported.rs`. `execute_build.rs` uses it
  in `extract_scalar`, `extract_many` and `arg_has_relative_cli_path` in place of the three
  hand-written `Path | AbsolutePath | ResolvedPath` patterns. Each of those has a `_` arm, so a
  missed path variant would fall to `get_one::<String>` on a `PathBuf` value, and clap panics at
  run time.
- `crates/toolr-core/src/parser/types/path_constraints.rs`: delete. `arg_keywords.rs`: drop the
  three keywords and add their type hints to the unknown-keyword error.
- `crates/toolr-core/src/manifest/model.rs`: drop `path_constraints` and bump `SCHEMA_VERSION` to 2.
- `crates/toolr-core/src/freshness/compare.rs`: treat a manifest schema mismatch as stale.
- `crates/toolr-core/src/third_party/merge.rs`: drop the `path_constraints: None` line.
- `crates/toolr/Cargo.toml`: add `libc.workspace = true`, for `access(2)` on Unix.
- `crates/xtask/src/build_skill_refs/types.rs` and `mod.rs`: remove `path_constraints_snippet` and
  its use of `PathConstraints::catalogue()`. Also remove the tests that pin the keyword table, in
  that file and in `crates/xtask/tests/coverage.rs`. The new types reach the generated tables
  through `SupportedType::doc()`. Without this change, `cargo xtask build-skill-refs --check`
  breaks.

Python:

- `crates/toolr-py/python/toolr/types/__init__.py`: the seven `NewType`s, `__all__`, and the module
  docstring.
- `crates/toolr-py/python/toolr/_runner.py::_dec_hook`: no change expected, because msgspec passes
  the unwrapped `Path` class. A test confirms this.
- `tests/test_types_module.py`: `EXPECTED_TOOLR_TYPES_NAMES`. The alias-identity test becomes a
  `__supertype__` chain test.

Docs, skills and notes:

- `docs/writing-commands/files/path-constraints.md` becomes the path types page.
  `annotations.md` and `known-bugs.md` drop the keyword text. The example in `arguments.md` moves
  to the new types. `docs/internals/manifest.md` drops `path_constraints` and documents schema
  version 2.
- `cargo xtask build-skill-refs`: regenerate the type tables. Check the prose in
  `skills/toolr-command-authoring/` by hand for `must_*` mentions.
- `UNRELEASED.md`: the new types; the removal of `arg(must_*)` with a migration table; the local
  manifest schema bump, which triggers an automatic rebuild; and the
  typing-level break, where a bare `Path` no longer type-checks where `AbsolutePath` or
  `ResolvedPath` is expected. This makes the release a minor one.

## Testing

- Rust unit tests for each type in `value_parsers.rs`: one value accepted, and every rejection
  message. Build fixtures with `tempfile`. The executable and writable tests use `chmod` on Unix and
  `#[cfg]`-gated variants on Windows.
- Parser tests that resolve each name bare, as `list[T]`, as `T | None` and as `*args: T`, shaped
  like real annotations (see the regression-test rule in `CLAUDE.md`).
- Schema enforcement: a v1 local manifest is reported stale and gets rebuilt.
- Unknown-keyword error: each of the three removed keywords produces its type hint.
- `crates/toolr/tests/`: an `assert_cmd` run per type against a pre-built manifest, for one
  rejection. The accepted value is covered by the `execute_build.rs` extraction tests and the
  runner coercion test. Running the full accept path needs a Python venv.
- pytest: the command body receives a `pathlib.Path` for every type, including in `list[T]`.
- A static typing check (mypy on a snippet in the test suite) shows that `FilePath` is
  accepted where `Path` is expected, and a bare `Path` is rejected where `FilePath` is expected.
- `mise run test`.
