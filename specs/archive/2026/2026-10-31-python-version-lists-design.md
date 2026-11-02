# Tie the hand-kept Python version lists to `requires-python`

- **Status:** implemented
- **Origin:** whole-project review finding F-38 (low).
- **Plan:** [`2026-10-31-python-version-lists-plan.md`](./2026-10-31-python-version-lists-plan.md)

## Problem

The supported CPython range is written down by hand in many places. `toolr-py`'s `requires-python` is the source of
truth:

| Where | What | Kind |
|---|---|---|
| `crates/toolr-py/pyproject.toml` `project.requires-python` | `>=3.11,<3.16` | source of truth |
| `crates/toolr-py/pyproject.toml` `project.classifiers` | `Programming Language :: Python :: 3.11` … `3.15` | full range |
| `tools/ci.py` `ALL_CPYTHONS` | `cp311` … `cp315`; drives the CI test matrix | full range |
| `mise.toml` `[tools] python` | `3.15.x` … `3.11.x`; CI's `mise install --locked` needs every matrix version | full range |
| `crates/toolr-core/src/execute/python.rs` `MIN_PYTHON_MINOR` | `11` | floor |
| root `Cargo.toml` pyo3 feature | `abi3-py311` | floor |
| `tools/ci.py` `ABI3_WHEEL_PYTHONS` | `["cp311"]` | floor |
| `crates/toolr-py/pyproject.toml`, `crates/toolr/pyproject.toml` `[tool.cibuildwheel] build` | `cp311-*` | floor |
| `crates/toolr/pyproject.toml` `project.requires-python` | `>=3.11` | floor |
| root `pyproject.toml` ruff `target-version`, mypy `python_version` | `py311`, `3.11` | floor |

Only one link is enforced: `crates/toolr-core/tests/python_floor_lockstep.rs` ties `MIN_PYTHON_MINOR` to the
`requires-python` floor (added by #602). Nothing checks the rest. The comment above `requires-python` names three of
them, and the one above `ALL_CPYTHONS` names one.

Concrete drift: add `3.16` to `ALL_CPYTHONS` and forget `mise.toml`, and every 3.16 CI job fails at `mise install
--locked`. Raise the floor to 3.12 and forget the classifiers, and PyPI keeps advertising 3.11.

## Decision

A pytest, `tests/test_python_versions.py`, derives the supported minors from `requires-python` and checks every list
against them.

```python
def supported_minors(requires_python: str) -> list[int]
```

It returns `[m for m in range(40) if f"3.{m}" in SpecifierSet(requires_python)]`, so `>=3.11,<3.16` gives
`[11, 12, 13, 14, 15]`. A module-scoped fixture reads and parses the TOML files once with `tomllib`. `tools.ci` is
imported the same way `tests/test_tools_ci_packslip.py` already does.

Full-range checks compare sets: `ALL_CPYTHONS` as dotted minors, the `3.N` classifiers, and the major.minor of each
`mise.toml` python entry. Floor checks compare against `min(supported)`: pyo3's `abi3-py3N` feature,
`ABI3_WHEEL_PYTHONS`, both `cibuildwheel` `build` values, `crates/toolr`'s `requires-python` floor, ruff's
`target-version` and mypy's `python_version`. `MIN_PYTHON_MINOR` stays with its Rust test; this test doesn't read Rust
source.

Each assertion message names the file and the key to change. The three "keep aligned" comments (`requires-python`,
`ALL_CPYTHONS`, the `mise.toml` python comment) shrink to a pointer at `tests/test_python_versions.py`.

Rejected alternatives:

- **Derive `ALL_CPYTHONS` from `requires-python` at CI time**, the audit's second option. It fixes one list. `mise.toml`
  and the classifiers are static TOML and can't be derived, so a test is needed anyway, and then the derivation is a
  second mechanism for the same thing.
- **Extend the Rust lockstep test.** It would need TOML and Python-source parsing for files the Rust side never reads.
  The Python test suite already parses `tools/ci.py` by importing it.

## Scope and compatibility

- No schema change, no behaviour change. All lists agree today, so the test passes on arrival.
- Docs: none.
- Release notes: none. Internal guard only.
