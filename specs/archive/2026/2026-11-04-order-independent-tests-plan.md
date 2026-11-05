# Order-independent Python tests and pytest-randomly: plan

> **For agentic workers:** use superpowers:subagent-driven-development or superpowers:executing-plans, one
> task at a time. Steps use checkbox (`- [ ]`) syntax. Spec:
> `specs/2026-11-04-order-independent-tests-design.md`.

**Goal:** the Python suite passes in any order, and pytest-randomly runs by default.

**Architecture:** three source-level state fixes, then the dependency, then docs, then a seed sweep.

**Tech stack:** pytest 9, pytest-randomly 5.0.0, uv.

Every commit carries the `Release-Notes: skip` trailer. Stage by path, never `git add -A`.

## Task 1: Reproduce (before any change)

- [ ] Reverse order: `uv run pytest --co -q -p no:randomly | grep :: > ids.txt`, reverse it with `tail -r`, then
  `uv run pytest -p no:cacheprovider -p no:randomly "@ids-reversed.txt"`. Expect 4 failed, 1 error.
- [ ] `uv run --with pytest-randomly pytest -p randomly --randomly-seed=8 -p no:cacheprovider` in the
  foreground (not `&`/`nohup`: it makes SIGINT ignored and fails `test_signals.py`). Expect dozens of
  `FileNotFoundError` failures.

## Task 2: Fix the leaks

**Files:** `tests/conftest.py`, `tests/utils/logs/test_setup.py`, `tests/runner/conftest.py` (new),
`tests/runner/test_path_hygiene.py`, `tests/runner/test_current_context.py`,
`tests/runner/test_runner_coverage.py`.

- [ ] `tests/conftest.py`: add autouse `_restore_cwd` after `_clean_registry`:

  ```python
  @pytest.fixture(autouse=True)
  def _restore_cwd() -> Iterator[None]:
      """Put the process back in its starting directory after every test."""
      original = os.getcwd()
      yield
      os.chdir(original)
  ```

- [ ] `tests/utils/logs/test_setup.py`: replace the five `patch("logging.StreamHandler").start()` comprehensions
  with `[MagicMock() for _ in range(N)]` (import `MagicMock`).
- [ ] `tests/runner/conftest.py`: autouse `_isolate_repo_tools(monkeypatch)` doing
  `monkeypatch.setattr(sys, "path", sys.path[:])` and `monkeypatch.delitem(sys.modules, name)` for every
  `_is_repo_tools_module(name)`.
- [ ] `test_path_hygiene.py` and `test_current_context.py`: remove the manual `saved_path`/`saved_cwd`,
  `sys.path[:] =`, `os.chdir` and `sys.modules.pop` cleanup. Keep the `copy_context()` call.
- [ ] `test_runner_coverage.py::make_tools_package`: drop the two lines the new fixture now does.
- [ ] Run the full suite in reverse order (Task 1 recipe): all green. Commit
  `test: restore cwd, logging and tools state between tests`.

## Task 3: Adopt pytest-randomly

**Files:** `pyproject.toml`, `uv.lock`, `CONTRIBUTING.md`.

- [ ] Add `"pytest-randomly>=5.0.0"` to `[dependency-groups] dev` (alphabetical, after `pytest`), run
  `uv lock`, and confirm the diff only adds `pytest-randomly` (and `pytest-randomly` is not newer than the
  cooldown). Do not touch the `pre-commit` group.
- [ ] `CONTRIBUTING.md` Tests section: add "Reproducing a failure" (seed from the `Using --randomly-seed=N`
  header; `uv run pytest -p randomly --randomly-seed=N`; `-p no:randomly` for the default order; the reverse
  recipe).
- [ ] Confirm the CI log shows the seed: `uv run pytest tests/test_types_module.py -v | head`. No workflow
  change.
- [ ] Commit `test: run the Python suite in random order with pytest-randomly`.

## Task 4: Distribution suite

- [ ] `uv run pytest -m distribution -p randomly --randomly-seed=1`. Green: done. Order-dependent: fix the test at
  the source; no `-p no:randomly` exemption (decided).

## Task 5: Verify

- [ ] Run `mise run test` once (skill-refs check, cargo, pytest with a random seed).
- [ ] 20 seeds, one foreground run each: `for s in $(seq 1 20); do uv run pytest -p no:cacheprovider
  -p randomly --randomly-seed=$s -q || echo "FAILED seed $s"; done`. No failures.
- [ ] Reverse order once more. All green.
- [ ] Any failure is a new root cause: fix it at the source and re-run the failing seed and the sweep.
- [ ] `prek run --all-files`.

## Task 6: Archive

- [ ] As the last commit before the PR, `git mv` the design and plan into `specs/archive/2026/`.
