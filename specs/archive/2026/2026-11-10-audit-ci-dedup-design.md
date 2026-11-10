# CI de-duplication

## Why

`_test.yml` runs once per Python matrix entry: 7 jobs on a PR (Linux x5, Windows x1, macOS x1) and
15 on `main`. Each one repeats work that does not depend on the matrix Python:

- `cargo install cargo-llvm-cov --locked`, compiled from source (the cargo cache covers only the
  registry and git checkouts, not installed binaries).
- The Rust suite under `cargo llvm-cov`, with `--test-threads=1`.
- The example-plugin manifest regeneration and the `cargo xtask build-skill-refs` drift gates,
  including a debug build of `xtask`.
- A Rust Codecov upload bundled with the Python one, so Codecov receives 7 to 15 near-identical
  Rust reports.

Only the Python suite and its coverage vary per interpreter.

## Change

1. `cargo-llvm-cov` is pinned in `mise.toml` as `aqua:taiki-e/cargo-llvm-cov` (prebuilt release
   binary, checksummed in `mise.lock`). `setup-virtualenv` already runs `mise install --locked`, so the
   from-source install step goes away. Contributors get the same version locally.
   The `llvm-tools-preview` rustup component stays an explicit step: the `rust` entry in `mise.toml`
   pins only clippy and rustfmt, and adding a component would grow every job's toolchain.
2. New required `rust-python` input on `_test.yml`. The one matrix entry whose Python equals it
   (the first of each OS's list) runs the Rust coverage pass and the Rust Codecov upload. The cargo and
   `target/coverage` caches are restored only there.
3. The two drift gates run only in the Linux job of that entry: one job holds both. Both are
   OS-independent.
4. The Codecov upload splits in two: the Python `coverage.xml` keeps its `<os>,<python>` flags in every
   job; the Rust `cobertura.xml` uploads once per OS with flags `<os>,rust`.

## Job graph

Before, per PR: 7 test jobs, each = Python tests + Rust coverage + `cargo install` + drift gates
(Linux) + 2 uploads. On `main`: 15 such jobs.

After, per PR: the same 7 test jobs and unchanged names and `needs`. Only the first entry of each OS
(Linux 3.11, Windows, macOS) also runs Rust coverage; only Linux 3.11 also runs the drift gates. On
`main`: 15 jobs, 3 of which run Rust. No job is added or removed, so `ci.yml`'s `codecov-notify` and
`set-pipeline-exit-status` `needs` lists are untouched and no real check leaves the merge gate.

Expected savings (estimates from the audit evidence, not measured): each skipped Rust pass saves
the from-source install (several minutes) plus the serialised Rust suite and, on Linux, an `xtask`
build. Roughly 4 skipped passes per PR and 12 on `main`, so tens of runner-minutes per PR and
around a hundred on `main`. The wall clock of a PR run drops little, since the Rust entries still
gate it; the 40-minute job timeout is left alone.

## Codecov

`.codecov.yml` defines no `flags:` and no flag-based statuses. Project and patch checks and the
per-crate components are path-based and merge every uploaded report, so one Rust report per OS
covers the same lines as 7 to 15 identical ones. Python uploads are unchanged. `manual_trigger` stays
on and `codecov-notify` still waits for every test job. `after_n_builds: 2` (one Python, one Rust)
is still met. The Rust report's flags change from `<os>,<python>` to `<os>,rust`, which only affects
the PR comment's flags table.

## Accepted trade-offs

- The `~/.rustup` cache is shared and may be saved by a non-Rust job that lacks llvm-tools, so the Rust job re-adds the component (seconds, not a failure).
- The Rust integration suite now runs against one Python per OS, and on macOS and Windows that is a different one on PRs than on `main` (the narrow lists differ); the drift gates run only on Linux.

## Non-goals

- Moving the drift gates into the `docs` job (it would compile `xtask` inside a 10-minute job).
- The `_prepare-release.yml` skill-refs checks and the prek hook: they guard other paths.
- The `pre-commit` job's `needs` and the `cargo-check` hook (another PR).
- Changing the matrix, the PowerShell pass (now a per-OS `run-powershell-tests` switch, run in the `rust-python` job) or timeouts.
