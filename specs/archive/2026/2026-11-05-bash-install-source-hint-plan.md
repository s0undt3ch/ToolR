# bash install source hint: plan

Design: [`2026-11-05-bash-install-source-hint-design.md`](2026-11-05-bash-install-source-hint-design.md).

1. **Probe in toolr-core.** In `crates/toolr-core/src/complete/install.rs` add
   `bash_completion_candidates(sysroot, xdg_data_dirs, homebrew_prefix) -> Vec<PathBuf>`
   and `bash_completion_installed(...) -> bool`; re-export from `complete/mod.rs`.
   Unit tests in `complete/tests.rs` over a `TempDir`: every sysroot-relative
   candidate detected when planted; empty tree not detected; a bare
   `completions/` directory not detected; `XDG_DATA_DIRS` with empty entries
   and a planted entry; `HOMEBREW_PREFIX` both files.
2. **Hint text.** Pure `bash_source_hint(path) -> String` with path quoting,
   unit-tested in `dispatch.rs`'s test module (plain path, path with a space,
   path with a single quote).
3. **Wiring.** Thread root `--quiet` into `run_self` → `run_completion_install`.
   After `Wrote` / `AlreadyInstalled` for bash, unless quiet, read
   `TOOLR_BASH_COMPLETION_SYSROOT` (default `/`), `XDG_DATA_DIRS`,
   `HOMEBREW_PREFIX`, and print the hint when the probe finds nothing.
4. **Integration tests** in `crates/toolr/tests/dispatch_coverage.rs`: empty
   sysroot → hint with the installed path; planted
   `usr/share/bash-completion/bash_completion` → no hint; `--quiet` with an
   empty sysroot → no hint; second run (`AlreadyInstalled`) → hint; zsh/fish → no hint.
5. **Docs and notes.** Update `docs/cli.md`; run
   `toolr pre-commit regen-doc-snippets`; add a release note.
6. **Verify.** `mise run test`, `prek run --all-files`, `cargo llvm-cov` +
   `cargo xtask patch-coverage --base origin/main`,
   `toolr release-notes check --base origin/main`.
7. **Archive.** `git mv` design and plan to `specs/archive/2026/` as the last commit.
