# Derive `ALL_CPYTHONS` and check the Python version lists in a prek hook

- **Status:** proposed
- **Origin:** F-38 follow-ups FU-4 and FU-5.
- **Builds on:** [`archive/2026/2026-10-31-python-version-lists-design.md`](./archive/2026/2026-10-31-python-version-lists-design.md)
- **Plan:** [`2026-11-04-python-versions-derive-plan.md`](./2026-11-04-python-versions-derive-plan.md)

## Problem

F-38 added `tests/test_python_versions.py`, which checks every hand-kept Python version list against
`crates/toolr-py/pyproject.toml`'s `requires-python`. Two gaps remain.

- **FU-5.** `tools/ci.py` still holds a hand-written `ALL_CPYTHONS = ["cp311", …, "cp315"]`, guarded by
  `test_all_cpythons_match`. `tools/ci.py` is Python and can read the TOML itself, so the list doesn't need to exist.
- **FU-4.** The checks take under a second, but they only run inside pytest. In CI that is the `_test.yml` job, which
  waits for the wheel build. A wrong classifier or `mise.toml` entry fails twenty minutes into a run instead of at
  commit time.

## Decision

### FU-5: derive `ALL_CPYTHONS`

`python3_floor` and `supported_minors` move unchanged from the test module into `tools/ci.py`. They keep the existing
rules: a `>=3.N` floor without a patch level, and a mandatory `<3.N` or `<=3.N` ceiling, which keeps the list finite.

```python
TOOLR_PY_PYPROJECT = REPO_ROOT / "crates" / "toolr-py" / "pyproject.toml"

def supported_minors(requires_python: str) -> list[int]: ...
def _requires_python(pyproject: Path) -> str: ...

ALL_CPYTHONS = [f"cp3{minor}" for minor in supported_minors(_requires_python(TOOLR_PY_PYPROJECT))]
```

The read happens at import time. `TEST_PYTHONS`, `TEST_PYTHONS_NARROW` and `generate_build_matrix` keep consuming
the module constants unchanged. `REPO_ROOT` comes from `Path(__file__).resolve()`, so the path resolves from any
working directory. Every consumer (`toolr ci generate-build-matrix` in `ci.yml` and `release.yml`, `toolr ci
packslip-check`, the tests) runs from a full checkout; no workflow uses a sparse checkout. A malformed
`requires-python` now makes every `toolr ci …` command fail on import with the existing `ValueError` message. That
is intended: the matrix must not be built from a guess.

`test_all_cpythons_match` would compare the derivation with itself, so it goes. `ABI3_WHEEL_PYTHONS` and
`BINARY_WHEEL_PYTHONS` stay hand-written and checked; they are floors with build meaning, not ranges.

The archived design rejected deriving `ALL_CPYTHONS` because `mise.toml` and the classifiers can't be derived, so a
check is needed anyway. That still holds, which is why the checker stays. Removing one hand list on top of it costs
three lines.

### FU-4: a `toolr pre-commit check-python-versions` hook

The consistency checks move out of the test module into `tools/precommit/python_versions.py`:

```python
def python_version_mismatches(repo_root: Path) -> list[str]
```

It reads the same inputs as today (`CONFIG_TOML_FILES`, `CP_TAG_WORKFLOWS`, the `tools.ci` wheel constants)
relative to `repo_root`, derives the supported minors from that tree's toolr-py `pyproject.toml`, and returns one
message per mismatch, worded as now ("`<file> <key>` is X; requires-python … implies Y"). The command
`check_python_versions(ctx)` prints each with `ctx.error` and calls `ctx.exit(1)`, like `check_gate_needs`.

The hook is `language: system`, `pass_filenames: false`, with a `files:` regex covering only the inputs: the eight
`CONFIG_TOML_FILES`, the three `CP_TAG_WORKFLOWS`, `tools/ci.py`, `tools/precommit/python_versions.py` and
`tests/test_python_versions.py`. It does not run on other commits. CI's `pre-commit` job already runs `prek run
--all-files` with Setup ToolR, so a drift fails there, before any build.

Rejected: a `uv run --no-sync pytest tests/test_python_versions.py` hook.

- CI's `pre-commit` job syncs the root venv with `uv sync --only-group pre-commit`, which has no `toolr-py`,
  `packaging` or `pytest-subtests`. The test imports `tools.ci`, which imports `toolr`, so the hook would fail at
  collection in CI. Making it work means building the toolr-py extension in that job.
- Locally it needs a synced dev venv with a built extension. In a fresh worktree `uv run --no-sync` instead creates
  an empty `.venv` and runs whatever `pytest` is on `PATH`.
- Every other project-local hook that runs Python goes through `toolr pre-commit …` from the tools venv, which
  already has `packaging` (`tools/pyproject.toml`) and `tomllib` (stdlib). No new dependency, no lock change.

### Tests

`tests/test_python_versions.py` becomes the test module for both pieces:

- the existing `supported_minors` and `python3_floor` cases, importing from `tools.ci`;
- `python_version_mismatches(REPO_ROOT) == []`, so `mise run test` still guards the real tree;
- a factory fixture that copies the input files into `tmp_path`, then one case per breakage (a `mise.toml` entry
  dropped, a classifier missing, `abi3-py312` in `Cargo.toml`, a workflow `cp-tag` changed). Each asserts the message
  names the file and key;
- a wiring test that loads `.pre-commit-config.yaml` with `pyyaml`, finds the `check-python-versions` hook, and
  asserts its `files` regex matches every input path and not an unrelated one (`README.md`).

## Scope and compatibility

- Only `ALL_CPYTHONS` becomes derived. `mise.toml`, the classifiers and the floor settings stay hand-kept and
  checked. `MIN_PYTHON_MINOR` stays with `crates/toolr-core/tests/python_floor_lockstep.rs`.
- Not covered, out of scope: `install-smoke.yml` pins `actions/setup-python` to `'3.15'`, and the docs examples use
  `>=3.11` and `3.13`. Neither is a supported-range list.
- Coverage: `.coveragerc` measures only `toolr` and `tests`, and `.codecov.yml` ignores `tests`. Lines in `tools/`
  and `tests/` therefore don't reach the patch gate. The real tests above are what guard the change.
- Docs, schema, skill refs: none. `tools/` isn't on any `__all__` surface.
- Release notes: none. Internal tooling; the PR carries `Release-Notes: skip`.
