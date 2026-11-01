# Skip relative `PATH` entries when looking up uv and Python

- **Status:** implemented. Beyond this design, `toolr project init`'s `python3` probe and the uv
  installer's `ldd` probe also go through `toolr_core::path_search`, whose finder requires an exec
  bit on unix. PR #622 later deletes `resolve_python_in`.
- **Origin:** whole-project review finding F-35 (low).
- **Plan:** [`2026-10-31-uv-path-lookup-plan.md`](./2026-10-31-uv-path-lookup-plan.md)

## Problem

Toolr looks up two executables on `PATH`, and both lookups resolve relative `PATH` entries against the current
directory. Toolr runs from inside the repository it serves, so a repo that ships a file named `uv` or `python3` gets it
run when the user's `PATH` has an empty or relative entry:

| Lookup | Code | Relative entry handling |
|---|---|---|
| uv | `crates/toolr-core/src/uv/discover.rs` `which_uv_in` | `std::env::split_paths(..).map(\|e\| e.join("uv")).find(is_file)`. An empty entry joins to the bare `uv`, `.` to `./uv`; `is_file()` checks both against the cwd. `probe` then runs the hit with `--version`, and `ensure_uv` hands it to every `uv sync` / `uv lock` / `uv run`. |
| Python (no tools venv) | `crates/toolr-core/src/execute/python.rs` `resolve_python_in` | `which::which_in(candidate, path, cwd)`. `which` 8 rewrites every "ordinary relative" entry, empty included on unix, to `cwd.join(entry)` (`finder.rs` `new_paths` → `is_ordinary_relative_path`). `probe` runs the hit with `--version`, and dispatch runs commands with it. |

The audit called the Python lookup fine because it goes through `which`. It doesn't: `which` resolves relative entries
against the cwd on purpose.

An empty entry is easy to get by accident: `PATH="$PATH:$EXTRA"` with `EXTRA` unset leaves a trailing `:`. Some shells
and old profiles also put `.` on `PATH`.

Repro (unix): a project with `tools/pyproject.toml`, an executable `uv` at the project root that writes a sentinel, and
no other uv:

```sh
cd repo && PATH=":/usr/bin:/bin" toolr project venv lock --quiet
```

The planted `./uv` runs (`--version`, then `lock`). The same happens with `PATH=".:..."` or a relative entry such as
`node_modules/.bin`.

## Decision

Drop every non-absolute `PATH` entry before either lookup. One crate-private helper does it, so the two sites can't
drift:

```rust
// crates/toolr-core/src/path_search.rs
pub(crate) fn absolute_search_path(path: Option<OsString>) -> Option<OsString>
```

It splits `path` with `std::env::split_paths`, keeps the entries where `Path::is_absolute()` is true, and joins them
back with `std::env::join_paths`. It returns `None` when nothing is left. That matters: `join_paths([])` is `""`, and
`which` treats `""` as the cwd on unix, so an empty result must never reach `which_in`. A `join_paths` error can't
happen for entries that came out of `split_paths`. Map it to `None` anyway rather than panic.

- `which_uv_in(path)` becomes `split_paths(&absolute_search_path(path)?)…`, and is otherwise unchanged.
- `resolve_python_in` passes `absolute_search_path(path)` to both `which::which_in` calls (the bare `$TOOLR_PYTHON` one
  and the `python3` / `python` loop). `which_in` with `None` returns an error, which the existing code already maps to
  "not found".

A `~/bin` entry is relative by `Path::is_absolute`, so it is dropped too. For uv that changes nothing: `which_uv_in`
never expanded `~`, so it was already a cwd-relative `./~/bin/uv`. For Python, `which` expanded it. POSIX `execvp`
doesn't expand `~` in `PATH` (only bash's own lookup does), so a literal `~` entry isn't a setup toolr needs to support.
The docs say so.

Rejected alternatives:

- **`which::which("uv")`**, the audit's second option. It resolves relative entries against the cwd, so it moves the
  bug instead of fixing it.
- **Resolve relative entries against the cwd but skip the repo root.** The repo root isn't the only untrusted place: a
  relative entry like `bin` points into the repo from any cwd inside it.
- **Fix uv only.** The Python lookup is the same bug and runs the hit the same way. The helper costs one call there.

## Scope and compatibility

- No runner or manifest schema change.
- A uv or Python reachable only through a relative `PATH` entry is no longer found. With `--quiet` or a declined
  prompt, toolr then reports that uv is missing. Without one, it offers the managed install. That's the fix working.
- `docs/installation/index.md`: in "Finding uv", item 1 becomes "`uv` in an absolute `PATH` directory", followed by:
  "Toolr skips empty and relative `PATH` entries (`.`, `bin`, `~/bin`), so a `uv` inside the repository never runs by
  accident." In the "Two cases don't go through uv" bullet, "from `PATH`" becomes "from an absolute `PATH` directory".
- `UNRELEASED.md`:

  > Toolr no longer runs a `uv` or `python3` found through an empty or relative `PATH` entry such as `.` or a trailing
  > `:`. Those entries point at the current directory, which is usually the repository toolr is running in, so a file
  > in the repo could run in place of the real tool. Toolr now skips them. If you relied on one, put the absolute
  > directory on `PATH` instead.
