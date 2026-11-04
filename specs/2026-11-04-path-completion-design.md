# Path argument completion: design

Audit finding F-36 (low, UX).

## Problem

Path-typed command arguments get no filesystem completion. `apply_value_parser`
(`crates/toolr/src/value_parsers.rs`) gives every path type a clap `ValueHint`,
but nothing reads it at TAB time. The engine (`crates/toolr-core/src/complete/engine.rs`)
only serves `allowed_values`, and the bash, zsh and fish scripts in
`crates/toolr-core/src/complete/scripts/` have no file fallback. Fish registers
with `complete -f`, which turns its own file completion off.

Reproduced on `main` at `a14ce9a9` with the dev binary and a scratch repo whose
`tools/demo.py` declares `src: pathlib.Path`, `cfg: FilePath | None`,
`out: DirectoryPath | None`, `new: NewPath | None` and `mode: Literal["a", "b"]`:

| Request (`toolr __complete "$PWD" …`) | Output              |
| ------------------------------------- | ------------------- |
| `demo hello --cfg ""`                 | nothing, exit 0     |
| `demo hello --out ""`                 | nothing, exit 0     |
| `demo hello ""` (positional `src`)    | nothing, exit 0     |
| `demo hello --cfg=""`                 | nothing, exit 0     |
| `demo hello --mode ""`                | `a`, `b`            |
| `project trust ""`                    | nothing, exit 0     |
| `self build-manifest --output ""`     | nothing, exit 0     |

The built-in path arguments miss out for two reasons. `builtin_completions::derive_argument`
sets `resolved_type: None` and ignores clap's value hint. `self build-manifest --source-dir`
and `--output` also declare no hint at all. `project trust|untrust [PATH]` use a `PathBuf`
parser, so clap already infers `ValueHint::AnyPath` for them.

### Where the type lives

The manifest already records it. `Argument.resolved_type` holds the `SupportedType`
(`{"kind": "optional", "value": {"kind": "file_path"}}` for `cfg` above), and the engine
receives the whole `Argument`. No manifest field is added. `manifest::SCHEMA_VERSION`
stays at 2, and the runner JSON spec (`RUNNER_SCHEMA_VERSION` / `SCHEMA_VERSION` in
`_runner.py`) is untouched. Third-party fragments and argparse arguments without a
`resolved_type` keep today's behaviour.

## Goals

- `toolr <cmd> --path-opt <TAB>` and a path positional complete files or directories
  natively in bash, zsh and fish, for both user commands and built-ins.
- File names are data: a file called `$(touch X)` must never run, as with candidate values.
- An installed old script keeps working against a new binary, and a new script against an
  old binary.

## Non-goals

- `--opt=value` completion. It returns nothing today for every value type (`--mode=` too),
  and bash splits on `=` (`COMP_WORDBREAKS`) differently from zsh and fish. A follow-up.
- Filtering by the type's runtime check (exec bit, writability, "must not exist").
- Auto-refreshing installed scripts on upgrade.

## Design

### Path hint in toolr-core

A new `pub enum PathCompletion { Any, File, Directory, Executable }` and an exhaustive
`SupportedType::path_completion(&self) -> Option<PathCompletion>` sit next to `is_path()`
in `parser/types/supported.rs`. `Optional(T)` and `List(T)` delegate to `T`. Every other
non-path variant returns `None`. No `_ =>` arm, so a new type has to decide, and
CONTRIBUTING's "Adding a supported type" checklist gains a compiler-enforced step.

| Type                                                 | `PathCompletion` |
| ---------------------------------------------------- | ---------------- |
| `Path`, `AbsolutePath`, `ResolvedPath`               | `Any`            |
| `FilePath`                                           | `File`           |
| `ExecutablePath`                                     | `Executable`     |
| `DirectoryPath`, `WritableDirectoryPath`, `NewPath`  | `Directory`      |

`NewPath` completes directories only, since its target must not exist yet: the user picks the
parent directory and types the new name (decision 3). `ExecutablePath` offers every file,
without exec-bit filtering (decision 2).

`value_parsers::path_hint` is rewritten to map `PathCompletion` onto `ValueHint`, so clap's
hint and TAB come from one table. The hints clap reports today stay the same, except
`NewPath`, which moves from `AnyPath` to `DirPath`. Nothing in toolr reads clap's hint other
than `value_parsers` itself, so the change only affects the one table.

### Engine

`serve_completions` keeps its signature. A new function reuses `classify`:

```rust
pub enum Completion { Values(Vec<Candidate>), Paths, Directories }
pub fn complete(manifest: &Manifest, tokens: &[String]) -> Completion
```

A `FlagValue` or `Positional` slot whose argument has empty `allowed_values` and a
`resolved_type` with a path hint returns `Paths` (`Any`, `File`, `Executable`) or
`Directories`. Anything else returns `Values(serve_completions(...))`. Non-empty
`allowed_values` always win. For user commands the CLI builder never combines a typed
argument with `allowed_values` (`cli.rs` reads `allowed_values` only when `resolved_type`
is `None`), so the mixed case only shows up in hand-written fragments. Shells complete
files with directories included, so `File` and `Executable` still let the user traverse.

`serve_completions` stays the single candidate path. The 1,389-line `complete/tests.rs`
is untouched, and the new tests cover only `complete`.

### Protocol: exit codes

`run_complete` (`crates/toolr/src/dispatch.rs`) prints nothing and exits with:

| Exit | Meaning                                              |
| ---- | ---------------------------------------------------- |
| 0    | Candidates on stdout, one `value<TAB>description` each (may be none) |
| 1    | Silent error (unchanged)                             |
| 10   | Complete file system paths                           |
| 11   | Complete directories                                 |

Why exit codes rather than a sentinel line:

- **Old script, new binary** (the default after an upgrade, since `install_script` never
  rewrites an installed script): bash does `|| return 0`, while zsh and fish get empty
  output. All three behave exactly as today. A sentinel line would be inserted as a
  candidate by all three, because bash's prefix filter and zsh/fish matching accept it
  on an empty word.
- **New script, old binary**: the old binary never exits 10 or 11, so the new script sees
  today's output.
- 2 is clap's usage-error code and 1 is taken, so 10 and 11 avoid both.
- Under `setopt PRINT_EXIT_VALUE`, zsh does not report the status of a command substitution
  in an assignment (checked), so a non-zero exit prints nothing.

### Scripts

Each script captures the exit status and, on 10 or 11, runs native completion on the
in-progress word. On any other non-zero status it returns nothing, as today.

- **bash.** It must run on macOS `/bin/bash` 3.2, which has no `compopt` and which the shell
  tests always use. Use `compgen -f -- "$raw_cur"` or `compgen -d -- "$raw_cur"` and read the
  result line by line. Quote each name with `printf %q`, as values are quoted today, and add a
  `/` to directories. `compgen` returns a typed leading `~` as written (`~/.zsh` gives
  `~/.zshrc`), so the `~` escaping applied to values must skip a leading `~` the user typed,
  or tilde expansion breaks. When `compopt` exists (bash 4+), request `compopt -o nospace`
  for directory candidates: always on exit 11, and on exit 10 when the only match is a
  directory. Bash 3.2 has no `compopt`, so it inserts a space after `dir/`; that is accepted
  (decision 1). `complete -o default` is rejected: it would fall back to files on every empty
  result, not just path slots.
- **zsh.** `_files` for 10 and `_files -/` for 11. zsh quotes the inserted names itself.
- **fish.** `__fish_complete_path "$current"` for 10 and `__fish_complete_directories "$current"`
  for 11 (both exist in fish 3.x and 4.x; checked on 4.9.3). `complete -c toolr -f` stays, so
  non-path slots still offer no files.

### Built-ins

`cli.rs` gives `--source-dir` `ValueHint::DirPath` and `--output` `ValueHint::FilePath`.
`derive_argument` maps `arg.get_value_hint()` to a `resolved_type` (`AnyPath` to `Path`,
`FilePath` to `FilePath`, `DirPath` to `DirectoryPath`, `ExecutablePath` to `ExecutablePath`).
Synthetic entries are never serialised, so the schema doesn't change. clap infers `AnyPath`
for the existing `PathBuf` parsers on `project trust|untrust`.

### Upgrade path

Users get the feature only after reinstalling the script. The release note says so, as the
completion hardening note did:

> Path-typed arguments (`pathlib.Path` and the `toolr.types` path types) now tab-complete
> files, or directories for `DirectoryPath`, `WritableDirectoryPath` and `NewPath`, in bash,
> zsh and fish. So do `toolr project trust`, `untrust` and `self build-manifest --source-dir` /
> `--output`. Re-run `toolr self completion install <shell> --force` to pick it up.

### Docs

- `docs/cli.md` `__complete`: the exit-code table.
- `docs/concepts.md` "Tab completion": path arguments complete from the file system.
- `docs/writing-commands/arguments.md`: the path types' completion behaviour in the
  `toolr.types` section. Mirror it in `skills/toolr-command-authoring/references/arguments.md`
  if that file copies the snippet (check `cargo xtask build-skill-refs --check`).

## Testing

- **Core unit tests.** `path_completion` for every path variant, plus `Optional` and `List`
  wrapping and a non-path type. `complete` returns `Paths` for a `FilePath` flag value,
  `Directories` for a `DirectoryPath` positional, `Paths` for a `list[Path]` variadic past
  the first value, `Values` when `allowed_values` is set on a path type, and `Values` for
  flag names after a path argument (`--<TAB>`).
- **Binary.** An assert_cmd test in `crates/toolr/tests/` drives `__complete` and checks
  exit 10, exit 11 and empty stdout, plus exit 0 with candidates for a `Literal`. Built-in
  cases cover `project trust ""` (10) and `self build-manifest --source-dir ""` (11).
  `builtin_completions` unit tests cover the hint mapping.
- **Shells** (`crates/toolr/tests/complete_shells.rs`). The fixture gains a path argument
  and a directory holding `with space.txt`, `sub/` and a file named `$(touch SENTINEL3)`.
  bash, on every `bashes()` entry: candidates present, quoted, inert, `sub/` has the slash,
  and `~/` keeps its tilde. Directory candidates request `nospace` where the bash has
  `compopt`, and never where it doesn't. Fish: `complete -C "toolr ci lint --cfg "` lists the files,
  inert. zsh: source inspection (zpty isn't workable here), so check that `_files` and
  `_files -/` are present and there is no `eval`.
- **Compatibility.** Run the *current* bash script (kept as a test fixture string) against
  the new binary on a path slot: no candidates, exit 0, nothing inserted.
- Patch coverage of 99% on `supported.rs`, `engine.rs`, `value_parsers.rs`, `dispatch.rs`,
  `builtin_completions.rs` and `cli.rs`. Scripts and `crates/*/tests` are excluded.

## Decisions

Recorded 2026-11-04, closing the open questions.

1. Bash 3.2 (no `compopt`) keeps the trailing space after `dir/`. On bash 4+ directory
   candidates use `compopt -o nospace`. Tests cover every bash the machine has.
2. `ExecutablePath` offers every file, with no exec-bit filtering.
3. `NewPath` completes directories only (exit 11).
4. `--opt=value` completion stays out of scope; a follow-up is logged separately.
