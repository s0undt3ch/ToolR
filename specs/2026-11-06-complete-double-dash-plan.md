# Completion after `--` and dash-leading choices: plan

Design: [`2026-11-06-complete-double-dash-design.md`](2026-11-06-complete-double-dash-design.md).
Work in the `complete-double-dash` worktree. Run everything in the foreground.

## Files

- `crates/toolr-core/src/complete/engine.rs`: `ArgScan`, `scan_args`, `classify_leaf_args`,
  `classify_dispatcher`, `Slot`, `candidates`, `values`.
- `crates/toolr-core/src/complete/tests.rs`: unit tests.
- `crates/toolr/tests/complete_smoke.rs`: one binary-level test.
- `release-notes/`: one note.

## Task 1: failing tests first

Add `dash_fixture()` to `tests.rs`, built with the existing `typed_arg` and `leaf` helpers, group `ci`:

- `lint`: `--mode` Optional Literal `fast, full, --dash, -x`; `--verbose` Flag; positional `target`
  Str with choices `alpha, beta, --dir`.
- `cat`: VarPositional `files` Str with choices `one, two`.
- `open`: positional `path` `SupportedType::FilePath`.
- `job`: dispatcher with `--level` Optional Str choices `debug, info`, child `run` (group `ci.job`) with
  positional `target` Str choices `x`.

Tests (`values(serve_completions(&m, &tokens(&[…])))` unless noted):

| Test | Tokens | Expected |
| --- | --- | --- |
| `double_dash_stops_flag_name_completion` | `ci lint -- --` | `["--dir"]` |
| | `ci lint -- --m` | `[]` |
| `double_dash_makes_flag_looking_words_positionals` | `ci lint -- --mode=` | `[]` |
| | `ci lint -- --mode ""` | `[]` |
| | `ci lint -- --mode alpha ""` | `[]` |
| `double_dash_is_not_a_positional` | `ci lint -- ""` | `["--dir","alpha","beta"]` |
| | `ci lint -- alpha ""` | `[]` |
| | `ci lint alpha -- ""` | `[]` |
| `flags_before_double_dash_are_still_skipped` | `ci lint --mode fast -- ""` | `["--dir","alpha","beta"]` |
| `variadic_keeps_completing_after_double_dash` | `ci cat -- one ""` | `["one","two"]` |
| `path_positional_after_double_dash_is_paths` | `complete(&m, …)` for `ci open -- ""` and `ci open -- --x` | `Completion::Paths` both |
| `double_dash_inside_a_dispatcher` | `ci job -- ""` | `[]` |
| | `ci job run -- ""` | `["x"]` |
| | `ci job run x -- ""` | `[]` |
| `separate_flag_value_drops_dash_leading_choices` | `ci lint --mode ""` | `["fast","full"]` |
| | `ci lint --mode -` | `[]` |
| `attached_flag_value_keeps_dash_leading_choices` | `ci lint --mode=` | `["--dash","-x","fast","full"]` |
| | `ci lint --mode=-` | `["--dash","-x"]` |
| | `ci lint --mode=--d` | `["--dash"]` |
| `positional_dash_leading_choice_only_after_double_dash` | `ci lint ""` | `["alpha","beta"]` |
| | `ci lint -- ""` | contains `--dir` (covered above) |

Rows already in the existing `attached_*` tests must keep passing unchanged.

In `complete_smoke.rs`, add `double_dash_and_dash_choices_through_the_binary` with its own small project
(not the shared `fixture()`, whose command list other tests assert on): `ci pick(ctx, opt: Literal["--dash",
"-x", "plain"] = "plain")` and `ci deploy(ctx, env: Literal["staging","production"])`. Via `complete_raw`:

- `ci deploy -- ""` prints `production` and `staging`; `ci deploy -- --` prints nothing.
- `ci pick --opt ""` prints exactly `plain`; `ci pick --opt=""` prints `--dash`, `-x`, `plain`.

Run `cargo test -p toolr-core complete::` and confirm the new tests fail for the stated reasons.

## Task 2: implement (engine.rs only)

Follow the design: `scan_args` plus `positional_indices` wrapper; early `scan_args` call in
`classify_leaf_args` with the three skips; `Slot` fields `attached` and `after_terminator`; `values(…,
dash_ok)`; dispatcher `terminator < child_index` returns `Slot::None`. Update the doc comments of
`classify_leaf_args` and `values` to say why (clap reads words after `--` as positionals; clap rejects a
flag-looking separate value). Format with `rustfmt --edition 2024 crates/toolr-core/src/complete/engine.rs
crates/toolr-core/src/complete/tests.rs crates/toolr/tests/complete_smoke.rs`.

Probe open question 1 first: `toolr <dispatcher> -- <child>` with the dev binary and `TOOLR_TRUST_ALL=1`.
Adjust the dispatcher row of the table if clap accepts it.

## Task 3: release note

```sh
toolr release-notes add --text "Tab completion no longer offers flags, flag values or option names after a \
bare \`--\`: everything after it is a positional, as the CLI reads it. Choices that start with \`-\` are \
offered only where the CLI accepts them: after \`--opt=\`, or after \`--\` for a positional."
```

Commit type `fix(complete)`. Check with `toolr release-notes check --base main`.

## Verification

```sh
cargo test -p toolr-core complete::
cargo test -p toolr --test complete_smoke --test complete_paths
cargo test -p toolr --test complete_shells -- --test-threads=1   # regression: scripts untouched
cargo llvm-cov --workspace --exclude toolr-py --tests --lcov \
  --output-path target/coverage/lcov.info -- --test-threads=1
cargo xtask patch-coverage --base main     # must report >= 99%
mise run test                              # engine and tests are Rust; full umbrella per CLAUDE.md
prek run --all-files
```

Manually re-run the scratch-project table from the design on the built binary and confirm every "Wanted" cell.
No `--no-verify`. Stage by explicit path.

## Finish

Archive both specs with `git mv` into `specs/archive/2026/` as the last commit before the PR. Commit
messages: `fix(complete): treat everything after -- as positional` and
`fix(complete): drop dash-leading choices where the CLI rejects them`, or one commit if the diff stays small.
