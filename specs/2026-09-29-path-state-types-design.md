# Path-state types in `toolr.types`

Issue: [#502](https://github.com/s0undt3ch/ToolR/issues/502).

## Goal

A command author states what a path argument must be by picking a type, not by adding `arg()`
keywords. The type name says what the binary checked. A type checker can tell the types apart. The
command function always receives a `pathlib.Path` instance.

```python
from toolr.types import ExistingFile, NewPath

def convert(ctx, source: ExistingFile, output: NewPath) -> None: ...
```

## Non-goals

- No `Command` type that looks a name up on `PATH`. `ExecutableFile` is a path.
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

```python
AbsolutePath   = NewType("AbsolutePath", Path)
NewPath        = NewType("NewPath", AbsolutePath)
ResolvedPath   = NewType("ResolvedPath", Path)
ExistingFile   = NewType("ExistingFile", ResolvedPath)
ExistingDir    = NewType("ExistingDir", ResolvedPath)
ExecutableFile = NewType("ExecutableFile", ExistingFile)
WritableDir    = NewType("WritableDir", ExistingDir)
```

| Type | The binary rejects the value unless | Value handed to Python | Completion hint |
|---|---|---|---|
| `pathlib.Path` | (no check) | as typed | `AnyPath` |
| `AbsolutePath` | (no check) | joined to cwd if relative | `AnyPath` |
| `NewPath` | the path does not exist, and its parent is an existing directory | absolute | `AnyPath` |
| `ResolvedPath` | the path exists | canonical | `AnyPath` |
| `ExistingFile` | the path is a regular file | canonical | `FilePath` |
| `ExistingDir` | the path is a directory | canonical | `DirPath` |
| `ExecutableFile` | the path is a regular file the process can execute | canonical | `ExecutablePath` |
| `WritableDir` | the path is a directory the process can write to | canonical | `DirPath` |

"Canonical" means `std::fs::canonicalize`: absolute, with symlinks and `..` resolved. The kind checks
run on the canonical path. So a symlink to a directory counts as an `ExistingDir`.

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
  where an `ExistingFile` is expected is a type error. `p / "x"` returns a plain `Path`, which is
  correct: a child of an `ExistingDir` is not known to exist.
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

## Removing `arg(must_*)`

- Remove `must_exist`, `must_be_file` and `must_be_dir` from `arg()` in
  `crates/toolr-py/python/toolr/utils/_signature.py`.
- The parser drops them from the known-keyword set. An author who still writes one gets the #500
  unknown-keyword build error, with a type hint in place of "did you mean":
  `` unknown `arg()` keyword `must_be_file`: use `toolr.types.ExistingFile` ``. `must_exist` points
  at `ResolvedPath`, and `must_be_dir` at `ExistingDir`.
- `PathConstraints` is no longer written to local manifests or third-party fragments.

### Compatibility with plugins built by older toolr

Plugin wheels ship a `toolr-manifest.json` fragment built at wheel-build time. An older toolr
writes `path_constraints` into it. If the new binary simply ignores that field (there is no
`deny_unknown_fields`), those plugin commands lose their checks without any warning.

So loading keeps a read-only fallback. When a fragment or manifest argument carries
`path_constraints`, the loader turns it into the matching type:

| Stored as | Loaded as |
|---|---|
| any path type + `must_be_dir` | `ExistingDir` |
| any path type + `must_be_file` | `ExistingFile` |
| any path type + `must_exist` | `ResolvedPath` |

This changes behaviour in one way: a legacy constrained `Path` or `AbsolutePath` now reaches Python
canonicalised rather than as typed. The check is kept, so that is the safer direction. The fallback
goes at the 1.0 release together with the other "removed in 1.0" items. There is no
`SCHEMA_VERSION` or `FRAGMENT_SCHEMA_VERSION` bump. The old field still reads, and new manifests
simply leave it out.

An older binary reading a fragment that uses a new type fails on the unknown `SupportedType`
variant. Any new type has always had this effect, and it is not new here.

## Known limit: non-literal defaults

`config: ExistingFile = "pyproject.toml"` is a literal default. clap runs it through the value
parser, so it is checked. `config: ExistingFile = Path("pyproject.toml")` is stored as the `<expr>`
sentinel, and the CLI then applies no default (`crates/toolr/src/cli.rs`, `ArgumentKind::Optional`).
Python's own default then applies, unchecked. The docs say this. Fixing it is out of scope.

## Changes

Rust:

- `crates/toolr-core/src/parser/types/supported.rs`: variants `NewPath`, `ExistingFile`,
  `ExistingDir`, `ExecutableFile` and `WritableDir`, each with its `doc()` row and kind. Follow the
  "Adding a supported type" checklist in `CONTRIBUTING.md`.
- `crates/toolr-core/src/parser/types/resolve.rs`: map the five names.
- `crates/toolr/src/value_parsers.rs`: replace the `PathConstraints` knob on `path_parser` with a
  per-type check enum. Set completion hints from the type.
- `crates/toolr-core/src/parser/types/path_constraints.rs` and `arg_keywords.rs`: remove the
  keyword extraction. Keep a deserialise-only `PathConstraints` and the load-time fold into a type.
- `crates/toolr-core/src/manifest/model.rs`: `path_constraints` becomes read-only, skipped on write.

Python:

- `crates/toolr-py/python/toolr/types/__init__.py`: the seven `NewType`s, `__all__`, and the module
  docstring.
- `crates/toolr-py/python/toolr/_runner.py::_dec_hook`: no change expected, because msgspec passes
  the unwrapped `Path` class. A test confirms this.
- `tests/test_types_module.py`: `EXPECTED_TOOLR_TYPES_NAMES`. The alias-identity test becomes a
  `__supertype__` chain test.

Docs, skills and notes:

- `docs/writing-commands/files/path-constraints.md` becomes the path types page.
  `annotations.md` and `known-bugs.md` drop the keyword text.
- `cargo xtask build-skill-refs`: regenerate the type tables. Check the prose in
  `skills/toolr-command-authoring/` by hand for `must_*` mentions.
- `UNRELEASED.md`: the new types; the removal of `arg(must_*)` with a migration table; and the
  typing-level break, where a bare `Path` no longer type-checks where `AbsolutePath` or
  `ResolvedPath` is expected. This makes the release a minor one.

## Testing

- Rust unit tests for each type in `value_parsers.rs`: one value accepted, and every rejection
  message. Build fixtures with `tempfile`. The executable and writable tests use `chmod` on Unix and
  `#[cfg]`-gated variants on Windows.
- Parser tests that resolve each name bare, as `list[T]`, as `T | None` and as `*args: T`, shaped
  like real annotations (see the regression-test rule in `CLAUDE.md`).
- The legacy fold: a fragment carrying each `path_constraints` shape loads as the right type.
- Unknown-keyword error: each of the three removed keywords produces its type hint.
- `crates/toolr/tests/`: an `assert_cmd` end-to-end run per type, for one accept and one reject.
- pytest: the command body receives a `pathlib.Path` for every type, including in `list[T]`.
- A static typing check (mypy on a snippet in the test suite) shows that `ExistingFile` is
  accepted where `Path` is expected, and a bare `Path` is rejected where `ExistingFile` is expected.
- `mise run test`.
