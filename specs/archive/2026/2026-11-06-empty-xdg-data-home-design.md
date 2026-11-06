# Empty XDG variables count as unset (FU-30)

Audit follow-up FU-30, from the maintainability review of PR #684.

## Problem

The XDG Base Directory spec says an empty or unset `XDG_*_HOME` means "use the default". Two code paths
disagree about that:

- `toolr self completion install` (`run_completion_install` in `crates/toolr/src/dispatch.rs`) reads
  `XDG_DATA_HOME` and `XDG_CONFIG_HOME` through the private `non_empty_env_path`, so an empty value falls
  back to `$HOME/.local/share`.
- `toolr_core::uv::{toolr_data_dir, toolr_state_dir, toolr_cache_dir}` read the same variables with
  `std::env::var_os(..).map(|v| PathBuf::from(v).join("toolr"))`. An empty value becomes the relative path
  `toolr`, resolved against the cwd.

The review named only `toolr_data_dir`. The state and cache twins have the identical bug, so this design
fixes all three.

## Call sites

Every environment read in `crates/*/src` and `crates/toolr-py/python/toolr` was checked. The Python package
reads only `os.environ` wholesale (`utils/command.py`), `CI` presence (`utils/_logs.py`) and the runner spec
variable (`_runner.py`); none is a path variable.

| Site | Variable | Empty today | Action |
| --- | --- | --- | --- |
| `toolr-core/src/uv/mod.rs` `toolr_data_dir` | `XDG_DATA_HOME` | relative `toolr` (bug) | switch |
| `toolr-core/src/uv/mod.rs` `toolr_state_dir` | `XDG_STATE_HOME` | relative `toolr` (bug) | switch |
| `toolr-core/src/uv/mod.rs` `toolr_cache_dir` | `XDG_CACHE_HOME` | relative `toolr` (bug) | switch |
| `toolr/src/dispatch.rs` `run_completion_install` | `XDG_DATA_HOME`, `XDG_CONFIG_HOME` | unset (correct) | switch to shared helper |
| `toolr/src/dispatch.rs` `print_bash_source_hint` | `TOOLR_BASH_COMPLETION_SYSROOT`, `HOMEBREW_PREFIX` | unset (correct) | switch to shared helper |
| `toolr/src/dispatch.rs` `dirs_home` | `HOME` | `PathBuf::new()`, so the install path is relative | switch; empty reports the existing "$HOME is not set or empty" error |
| `toolr/src/dispatch.rs` `print_bash_source_hint` | `XDG_DATA_DIRS` | passed raw; `bash_completion_installed` skips empty entries (tested) | leave |
| `toolr-core/src/venv/resolve.rs` | `TOOLR_VENV_LOCATION` | unset (correct, inline `!raw.is_empty()`) | leave |
| `toolr-core/src/uv/install.rs` | `TOOLR_UV_LIBC` | empty string reaches `detect_linux_libc_from` | leave, see open questions |
| `toolr-core/src/trust.rs`, `uv/install.rs` | `TOOLR_TRUST_ALL`, `TOOLR_AUTO_INSTALL_UV` | only `== "1"` counts | leave |
| `toolr/src/help.rs` | `NO_COLOR` | empty counts as set | leave, see open questions |
| `toolr/src/main.rs` | `TOOLR_NO_CACHE_HINT` | empty counts as set | leave (a presence flag by design) |
| `toolr/src/help.rs` | `COLUMNS` | unparsable value falls through | leave |
| `PATH`, `SHELL`, `PATHEXT` reads | n/a | not directory-resolving | leave |

## Reproduction

On the dev binary at `main` (`f65b07a2`), with `HOME` pointing at an empty temporary directory:

- `XDG_DATA_HOME= XDG_CONFIG_HOME= toolr self completion install bash` writes
  `$HOME/.local/share/bash-completion/completions/toolr`. Empty counts as unset.
- From an empty cwd, `XDG_CACHE_HOME= toolr self cache list` prints
  `no cached virtualenvs found under toolr`. The cache root is the relative path `toolr`.
- A throwaway test (not kept) set all three variables to `""` and printed:
  `data=Some("toolr") state=Some("toolr") cache=Some("toolr") managed_uv=Some("toolr/bin/uv")`.

So with an empty `XDG_DATA_HOME`, a managed uv would be probed for and installed under `./toolr/bin/uv`,
and venvs and the trust store land in `./toolr` of whichever directory the command ran from. The trust
store case is the worst: `trust.rs` rejects a state dir inside the repository, so the command fails
rather than silently writing, but only after the cwd happens to be inside a repo.

## Design

A new module `crates/toolr-core/src/env.rs`, `pub mod env;` behind `#[cfg(feature = "full")]` in `lib.rs`
like its neighbours. It uses only `std`, so `full` gains no dependency and toolr-py's build is unchanged.

```rust
/// The value of `name` as a path, or `None` when it is unset or empty.
///
/// An empty value reads as unset, as the XDG base directory spec requires; using it would resolve
/// relative to the cwd.
pub fn non_empty_env_path(name: &str) -> Option<PathBuf>
```

One function is enough: every call site wants a `PathBuf`. `toolr_{data,state,cache}_dir` become
`non_empty_env_path("XDG_…_HOME").map(|v| v.join("toolr")).or_else(…)`. `dispatch.rs` deletes its private
copy and imports the shared one; `dirs_home` becomes
`non_empty_env_path("HOME").ok_or_else(|| anyhow!("$HOME is not set or empty; cannot pick install path"))`.

Not changed: path layout, precedence between the XDG variable and the `dirs` fallback, and the
`toolr_state_dir` to `toolr_data_dir` fallback chain. A relative non-empty value such as
`XDG_DATA_HOME=rel` is still accepted as before; the spec says it is invalid, but `trust.rs` already
handles it deliberately and redesigning that is out of scope.

Windows: `dirs::data_dir()` and friends ignore `XDG_*` there, but toolr honours the variable first on all
platforms, and still does. The empty-value fix applies equally. `HOME` is normally unset on Windows, so
`dirs_home` already errors there; that is unchanged.

## User-visible change

An empty `XDG_DATA_HOME`, `XDG_STATE_HOME` or `XDG_CACHE_HOME` now selects the platform default
(`~/.local/share/toolr`, `~/Library/Application Support/toolr`, and so on) instead of `./toolr` under the
current directory. An empty `HOME` makes `toolr self completion install` fail with the existing
"$HOME is not set or empty" error rather than writing under the cwd. A release note is required.

## Open questions

- `NO_COLOR=` (empty) currently disables colour; no-color.org says an empty value should not. Same class,
  different spec. Left out to keep this change to paths; worth its own follow-up.
- `TOOLR_UV_LIBC=` (empty) reaches `detect_linux_libc_from` as `Some("")`. Check whether it already falls
  through to detection; if not, it joins the same follow-up.
- Should a relative non-empty `XDG_*_HOME` also be ignored, as the spec says? Not here.
