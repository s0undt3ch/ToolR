# CI de-duplication: plan

1. `mise.toml`: add `"aqua:taiki-e/cargo-llvm-cov" = "0.9.1"`; run `mise lock` and commit `mise.lock`
   (additions only).
2. `.github/workflows/_test.yml`: add the `rust-python` input; guard the cargo caches, drift gates,
   llvm-tools step, Rust coverage step and Rust Codecov upload; drop `cargo install`; split the
   Codecov upload.
3. `.github/workflows/ci.yml`: pass `rust-python` from `test-linux`, `test-windows` and `test-macos`.
   Touch no other hunk (the `pre-commit` job's `needs` is edited by another PR).
4. Verify: `prek run --all-files` (actionlint, pipeline-gate needs sync, python-versions check).
   The workflow cannot run locally, so the gating conditions are checked by reading.
5. No release note: CI-only, so commits carry `Release-Notes: skip`.
6. Archive both specs as the last commit.
