# Resolve relative path defaults from the repo root

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-11 (medium-high).
- **Plan:** [`2026-10-31-path-default-repo-root-plan.md`](./2026-10-31-path-default-repo-root-plan.md)

## Problem

`build_user_command` (`crates/toolr/src/cli.rs`) hands a command's literal default to clap with
`Arg::default_value`. clap runs the arg's value parser on a default whenever the flag is absent. For the path types
that parser is `value_parsers::path_parser`, which resolves a relative path against the process cwd
(`absolutise`, `typed.try_exists()`, `dunce::canonicalize`) and then runs `check_path`.

The author wrote that default in a file under `tools/`, and the runner chdirs to the repo root before calling the
function. The default means "relative to the repo root". toolr checks it against wherever the user happens to be.

```python
@group.command
def hello(ctx: Context, *, cfg: FilePath = "tools/pyproject.toml") -> None:  # type: ignore[assignment]
    ...
```

From the repo root both `toolr demo hello` and `toolr demo hello --help` work. From `sub/dir` both fail:

```text
error: invalid value 'tools/pyproject.toml' for '--cfg <cfg>': path does not exist: tools/pyproject.toml
```

`--help` fails because `main::run` calls `try_get_matches_from` first, and the help fallback only catches
`MissingSubcommand` and `MissingRequiredArgument`, not `ValueValidation`.

The checking types fail loudly. `AbsolutePath` and `NewPath` don't fail, which is worse: a default of `build/out`
becomes `<cwd>/build/out`, a different directory for each place the user runs from. `DirectoryPath = "."` means
"wherever the user is", not the repo root.

`docs/writing-commands/arguments.md` lists this under **Limits** ("A relative string default resolves against the
working directory, not the repo root"). A bare `Path` default already means the repo root: its parser keeps it as
typed and Python resolves it after the chdir. `execute_build.rs::relative_path_warning` exists because the two bases
differ.

## Decision

A relative path default resolves against the repo root. A value typed on the command line still resolves against
the cwd, as it does today.

clap's value parser can't tell a typed value from a default, and `help.rs` reads `[default: …]` from the clap `Arg`
(`get_default_values`), so the literal must stay a clap default. Resolution moves after parsing, where
`ArgMatches::value_source` says which one it is and `build_spec` already has `repo_root`:

- `value_parsers.rs` gains `defers_path_resolution(arg: &Argument) -> bool`. It is true when `arg.kind` is
  `Optional`, the unwrapped `resolved_type` is a path type whose form is `Absolute` or `Canonical` (every path type
  but bare `Path`), and `arg.default` is a non-empty, non-`<expr>`, relative path.
- `value_parsers.rs` gains `resolve_path(ty: &SupportedType, raw: &Path, base: &Path) -> Result<PathBuf, String>`.
  It is the body of today's `path_parser` closure with `base` in place of the process cwd. `path_parser` calls it
  with the cwd, so typed values on every other arg behave exactly as now.
- `build_user_command` keeps `default_value(def)` for such an arg. It swaps the parser for an unchecked
  `value_parser!(PathBuf)` (the value hint stays), so neither parsing nor `--help` touches the filesystem.
- `execute_build.rs::extract_scalar` resolves a deferred arg's raw `PathBuf`. The base is the cwd when
  `value_source` is `CommandLine` and `repo_root` otherwise. Errors use clap's wording, and a default's error says
  where it resolved from:

  ```text
  invalid value 'x.toml' for '--cfg': path does not exist: x.toml
  default 'tools/missing.toml' for '--cfg' (resolved from the repo root <root>): path does not exist: tools/missing.toml
  ```

  `extract_value`, `pack_child_args`, `build_spec` and `build_dispatch_spec` thread a `PathBases { cwd, repo_root }`
  through and return `Result`. Errors exit 2, like clap's.
- `relative_path_warning` skips deferred args. Their typed values are relative `PathBuf`s in the matches now, but
  they resolve against the cwd, so the "resolve from the repo root" note would be wrong.

Rejected alternatives:

- **Pass the repo root to `build_command` and set `default_value(repo_root.join(def))`.** `--help` would print an
  absolute, machine-specific path, which breaks the help snippets captured from `docs/.fixtures/sample-repo/`.
- **A parser that uses the repo root when the input equals the default literal.** A user who types that same string
  from a subdirectory would get the repo-root file, not the one next to them.
- **Drop the clap default for path args and let Python's default run.** Python would get the raw `str` (no
  `Path`, no check), and `--help` would lose the `[default: …]` line.
- **Skip validation of defaults entirely.** The function would get a relative path that the runner resolves from
  the repo root unchecked. A `FilePath` would no longer promise a file.

## Scope and compatibility

- No runner or manifest schema change. The wire still carries an absolute string.
- Behaviour change: from a subdirectory, a relative `AbsolutePath`, `NewPath` or `DirectoryPath = "."` default now
  names a path under the repo root instead of under the cwd. From the repo root nothing changes.
- Plugin commands are untouched: their path types aren't checked yet (#520).
- Docs: drop the last sentence of the string-default bullet in `docs/writing-commands/arguments.md` **Limits** and
  say the default resolves from the repo root. Change the `AbsolutePath` row's `validated_by` in
  `crates/toolr-core/src/parser/types/supported.rs` to `clap (absolutise vs cwd; defaults vs repo root)`. Regenerate
  `docs/writing-commands/files/supported-types.md` and `skills/toolr-command-authoring/references/` with
  `cargo xtask build-skill-refs`.
- `UNRELEASED.md`:

  > A relative default on a path-typed argument, such as `cfg: FilePath = "tools/pyproject.toml"`, now resolves from
  > the repo root, the same place the command runs. It used to be checked against the directory you ran `toolr`
  > from, so from a subdirectory both the command and its `--help` failed with "path does not exist". An
  > `AbsolutePath` or `NewPath` default, or `DirectoryPath = "."`, now also names a path under the repo root wherever
  > you run from. A path you type on the command line still resolves from your current directory.
