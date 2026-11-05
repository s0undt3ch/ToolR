# FU-22 group re-declaration parity: plan

Spec: `specs/2026-11-04-group-redeclaration-parity-design.md`. Steps are test first.

## Task 0: restack

`git-spice upstack onto main` (or rebase) so #674's corpus and `test_parser_tester_parity.py` are present.
Confirm `cargo build -p toolr` and the corpus test pass before changing anything.

## Task 1: detect conflicts in the parser

- Files: `crates/toolr-core/src/parser/build.rs`, `crates/toolr-core/src/manifest/model.rs`.
- [ ] Write failing unit tests in `build.rs` for the six shapes in the spec (warning text asserted exactly).
- [ ] Add `#[serde(skip)] pub build_warnings: Vec<String>` to `Manifest`; fix every struct literal
  (`plugin_warnings: Vec::new()` sites: build.rs, dispatch.rs, bootstrap.rs, tests).
- [ ] Replace `seen_groups` with a map from full path to (module, group); collect warnings in
  `build_commands_inner`'s return and set them on the manifest in `build_static_manifest_inner`.
- [ ] `cargo test -p toolr-core` green.

## Task 2: report

- Files: `crates/toolr/src/main.rs`, `crates/toolr/src/bootstrap.rs`, `crates/toolr/tests/cli_smoke.rs`.
- [ ] Failing smoke test: a conflicting repo warns on the first run and is silent on the second.
- [ ] Print `build_warnings` next to the `plugin_warnings` loop. Check `carry_forward` and the
  `self build-manifest` path print or drop them correctly (a rebuilt manifest carries fresh warnings only).
- [ ] Pin cross-tree: `third_party` test that a disagreeing fragment group keeps the local one silently.

## Task 3: corpus and parity test

- Files: `tests/e2e/files/parity_corpus/<five cases>/`, `tests/e2e/test_parser_tester_parity.py`.
- [ ] Add the five cases from the spec; extend the test to compare group `title`/`description` and the
  warn/no-warn split. Run `uv run pytest tests/e2e/test_parser_tester_parity.py`.
- [ ] Check no `__init__.py` was added under `files/`.

## Task 4: docs, note, gates

- [ ] `docs/writing-commands/groups.md`: first declaration wins, conflicts warn, import-order caveat.
- [ ] `toolr release-notes add --text "..."`.
- [ ] `mise run test`; `cargo xtask patch-coverage --base main` at or above 99%.
- [ ] `git mv` both specs to `specs/archive/2026/` as the last commit, then `git-spice branch submit --draft`.
