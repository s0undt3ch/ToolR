# Test infrastructure honesty

Audit findings C-02, C-07, P-04, P-07, P-09. A green local run should mean the tests ran.

## Per finding

- **C-02, bash probe.** `bashes()` in `crates/toolr/tests/complete_shells.rs` also probes the newest
  `conda-bash` install under `$MISE_DATA_DIR` (default `~/.local/share/mise`) and the PATH-style list
  `$TOOLR_TEST_BASH`. Newest only, because every test loops over `bashes()` and each extra install
  multiplies the run time; `TOOLR_TEST_BASH` targets an older one. The result is cached (`OnceLock`).
  With no bash 4.4+, CI fails in `find_bashes`; elsewhere the new test `a_bash_4_4_or_newer_was_found`
  fails unless `TOOLR_SKIP_BASH_COMPLETION_TESTS=1` waives it, so a vacuous pass is opt-in. An early
  stderr warning was dropped: libtest hides it for passing tests. The bash 5.1 failure first seen here
  was a harness artefact (the drivers left the parent's `PWD` set); a later commit sets `PWD` for the
  drivers and 5.1 passes.
- **C-02, `conda:bash` pin: not done.** CI runs `mise install --locked`, and `mise.lock` needs a
  `conda:bash` entry per platform, which needs network access that this change could not get. Left for
  the maintainer: add `"conda:bash" = "5.3"` to `mise.toml` and run `mise lock`. CI keeps using
  `brew install bash` and the runner's own bash, so nothing depends on the pin.
- **C-07, protocol lockstep.** New `crates/toolr/tests/complete_protocol_lockstep.rs` reads
  `COMPLETE_PATHS`, `COMPLETE_DIRECTORIES` and `COMPLETE_MATCH_VAR` from `dispatch.rs` as text and
  asserts the four scripts handle those codes and the zsh and fish scripts set that variable (bash
  must not). No shell needed. A new file, because `complete_smoke.rs` is being edited elsewhere.
- **P-04, gate tests.** `tests/tools/precommit/workflows/test_check_gate_needs.py` drives
  `check_gate_needs` on tmp workflow trees: missing and unknown `needs`, key placement before `steps`,
  wrong `if`, step-level `if`, wrong `jobs` input, detection by action not name, two gates, check
  mode, and the repo's own workflows. `ruamel.yaml` joins the `dev` and `pre-commit` groups (it only
  lived in `tools/pyproject.toml`). The mypy hook `files:` is left alone: importing the module from a
  test puts it under mypy already, and `.pre-commit-config.yaml` is being edited elsewhere.
  `docs.py` (`_find_capture_toolr`) is left out: the docs job already checks the captures.
- **P-07, stubtest.** `test_context_stub.py` is parametrised over `toolr._context` and
  `toolr.utils._rust_utils`, each with its own allowlist (stubtest fails on an unused entry). The stub's
  `run_command_impl` parameter order now matches pyo3. The allowlist covers the stub-only TypedDicts, the
  missing `__all__`, and `DocstringParser`'s `@final`/`@disjoint_base` demands.
- **P-09, host leakage.** `tests/installation/test_install_sh_attestation.py` links tools found on a
  `PATH` stripped of `mise/shims`. `tests/docs/test_llmstxt_gate.py` skips the module when mkdocs is
  missing, except on CI where it still fails.

## Non-goals

No old-shell CI, no change to the bash 4.4 floor, no restructuring of
`complete_shells.rs`, no per-test bash process pooling for the attestation tests.
