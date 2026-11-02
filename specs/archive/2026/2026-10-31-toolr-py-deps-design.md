# Stop linking the CLI's dependency stack into `toolr-py`

- **Status:** implemented
- **Origin:** whole-project review finding F-15 (medium).
- **Plan:** [`2026-10-31-toolr-py-deps-plan.md`](./2026-10-31-toolr-py-deps-plan.md)

## Problem

`crates/toolr-py/src/lib.rs` uses five things from toolr-core: `run_command_internal`, `CommandConfig`, the three
command error types and `SimpleDocstringParser` (plus `ThreadSafeHandle` on Windows). They live in toolr-core's
`command` and `docstrings` modules, which need only `std` and `serde`. But `crates/toolr-py/Cargo.toml` depends on all
of toolr-core, so the pyo3 extension compiles and links everything the binary needs:

- `reqwest` with `rustls`, `hyper` and the ICU / `idna` stack (uv download);
- `zip`, `tar` and their deflate backend (uv archive extraction);
- the `ruff_python_parser` / `ruff_python_ast` git dependencies (static manifest parser);
- `tokio` with `features = ["full"]`, a **normal** dependency of toolr-core whose only user is the `tokio_tests` module
  in `crates/toolr-core/src/command/command_test.rs`.

`cargo tree -p toolr-py -e normal --prefix none | sort -u | wc -l` reports 238 crates on `main`. PR #614 (edition 2024)
bumped `tokio` and `zip` but changed neither dependency list, so both halves of the finding are still open.

`winapi` is in the same state: toolr-core lists it as a Windows normal dependency, but only `command_test.rs` uses it.

The cost is wheel build time across the CI matrix, and advisories and Renovate bumps on crates the extension never
calls. Linking strips most of the dead code, so wheel size isn't the main cost.

## Decision

Gate everything except `command` and `docstrings` behind one default-on cargo feature of toolr-core, `full`, and have
toolr-py turn default features off.

```toml
# crates/toolr-core/Cargo.toml
[features]
default = ["full"]
full = ["dep:reqwest", "dep:ruff_python_parser", "dep:ruff_python_ast", "dep:zip", "dep:tar", ...]
```

- Every dependency that `command.rs` and `docstrings.rs` don't use becomes `optional = true` and is listed in `full`.
  `serde` stays non-optional. The implementer derives the exact list with `cargo check -p toolr-core
  --no-default-features`: whatever it complains about is either gated code or a dependency that must stay.
- `crates/toolr-core/src/lib.rs`: `#[cfg(feature = "full")]` on every `pub mod` except `command` and `docstrings`, and
  on the `build_fragment` re-export. `mod test_env` becomes `#[cfg(all(test, feature = "full"))]` if nothing in
  `command` or `docstrings` uses it.
- The five files in `crates/toolr-core/tests/` all use gated modules or `toml`, so each gets `#![cfg(feature =
  "full")]` at the top.
- `tokio` moves to `[dev-dependencies]`. `winapi` moves to `[target.'cfg(windows)'.dev-dependencies]` if
  `cargo check -p toolr-core --target x86_64-pc-windows-msvc` (or the Windows CI job) shows no non-test use.
- `crates/toolr-py/Cargo.toml`: `toolr-core = { path = "../toolr-core", default-features = false }`.
- `crates/toolr/Cargo.toml` and `crates/xtask/Cargo.toml` keep the default, so nothing changes for them.

Cargo unifies features across the packages it builds together, so `cargo test --workspace` and the prek hooks
(`--all-features`) always build toolr-core with `full`. The lean build happens only when maturin builds toolr-py on its
own. Two prek hooks stop it from rotting:

- `cargo-check-toolr-core-lean`: `cargo clippy -p toolr-core --no-default-features --all-targets -- -D warnings`, on
  `types: [rust]` and the Cargo files.
- `toolr-py-deps`: fails if `cargo tree -p toolr-py -e normal --prefix none` lists any of `tokio`, `reqwest`, `hyper`,
  `rustls`, `ruff_python_parser`, `ruff_python_ast`, `zip`, `tar`. Runs on `Cargo.toml`, `Cargo.lock` and
  `crates/*/Cargo.toml` changes.

Rejected alternatives:

- **Move `command` and `docstrings` into a fifth crate.** Structurally cleaner, but it moves `command.rs`, which
  several open branches for the `ctx.run` findings are editing. It also changes the four-crate
  layout that the contributor docs and the coverage setup describe. The feature gate touches only
  `Cargo.toml` files and `lib.rs`. A split can still happen later, after those branches land.
- **Fine-grained features (`uv-install`, `parser`, ...).** One consumer needs one cut. Nobody builds toolr-core with
  half of the CLI.
- **Only move `tokio` to dev-dependencies.** It's the cheapest part, but the HTTP, archive and parser stacks are most of
  the 238 crates.

## Scope and compatibility

- No runner or manifest schema change. No public API change: `toolr` and `xtask` see the same toolr-core.
- The `toolr-py` wheel's behaviour is unchanged.
- Docs: none. The documented four-crate workspace layout still holds.
- `UNRELEASED.md`: none. Internal build change only.
