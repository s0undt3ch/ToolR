# Runner Type Hints Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One unresolvable annotation no longer turns off coercion for a whole command. `TYPE_CHECKING` imports resolve
at run time, and anything still unresolved is named in a warning (F-03).

**Architecture:** A new `toolr/utils/_annotations.py` reads unevaluated annotations and evaluates them one parameter at
a time. On `NameError` it rescues the name from the module's `TYPE_CHECKING` imports. The runner's coercion and
`detect_dispatch_parameter` both use it.

**Tech Stack:** Python 3.11-3.15 (`annotationlib` on 3.14+), `ast`, pytest.

**Spec:** [`specs/2026-10-30-runner-type-hints-design.md`](./2026-10-30-runner-type-hints-design.md)

## Global Constraints

- No runner spec change, so no `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump.
- `NameError`/`AttributeError` → unresolved (raw value + warning). An unknown `arg()` keyword → the existing
  `SpecError`. Anything else → `SpecError` naming the parameter and `<module>.<qualname>`.
- Warning text, exactly:

    ```text
    toolr: warning: can't resolve the type of `<param>` on <module>.<qualname> (<exception message>); passing the raw value. Import it at runtime, not only under `if TYPE_CHECKING:`.
    ```

  Goes to stderr; suppressed when `spec.context.verbosity == "quiet"`.
- Parse a module's source with `ast` at most once per runner process, and only after a `NameError`.
- Python tests: factory fixtures for `tmp_path` modules, imports at the top of the file.
- Run new tests on Python 3.11 and 3.14 at least (`uv run --python 3.11 pytest …`, `uv run --python 3.14 pytest …`).
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`.

## Review Focus

1. **The exact reported shape:** `from __future__ import annotations`, `if TYPE_CHECKING: from pathlib import Path`,
   `def f(ctx, path: Path, count: int = 1)` → `path` is a `Path` and `count == 3`. Task 1,
   `test_type_checking_path_import_is_rescued`.
2. **Lazy annotations on 3.14+ without `__future__`.** The same module without the `__future__` line imports fine there,
   and `VALUE`-format reads would raise. Task 1, `test_lazy_annotations_resolve_per_parameter` (skip below 3.14).
3. **An omitted `Path | None` positional** with a `TYPE_CHECKING` import must get `None`, not a `TypeError`. Task 1,
   `test_optional_positional_with_type_checking_import_gets_none`.
4. **A dispatcher with a `TYPE_CHECKING`-only annotation on another parameter** must still be detected as a dispatcher.
   Task 2, `test_dispatch_detected_despite_unresolvable_sibling`.
5. **A user global named like a rescued name** (`Path = MyPath` at module level) must win. The rescue only runs after a
   `NameError`. Task 1, `test_module_global_wins_over_type_checking_import`.

---

### Task 1: Per-parameter resolution with TYPE_CHECKING rescue, used by the runner

**Files:**

- Create: `crates/toolr-py/python/toolr/utils/_annotations.py`
- Modify: `crates/toolr-py/python/toolr/_runner.py` (`_hints_and_signature`, `_coerce_args` and its two call sites
  ~673/683; drop the `get_type_hints` import if unused)
- Create: `tests/utils/test_annotations.py`
- Modify: `tests/runner/test_runner_coverage.py`. Replace `test_coerce_args_falls_back_when_get_type_hints_raises` with
  the per-parameter test below. That's a rewrite of a test that pinned the bug, not a weakening.

**Interfaces (produced):**

- `@dataclass(frozen=True) class ResolvedHints: hints: dict[str, Any]; unresolved: dict[str, Exception]`

- `def resolve_hints(target: Callable[..., Any], localns: dict[str, Any] | None = None) -> ResolvedHints`. Evaluated
  hints for every annotated parameter that resolves (the `return` annotation is ignored). It raises `SpecError` for
  non-name failures and passes unknown-`arg()`-keyword `TypeError`s through `_runner._unknown_arg_keyword_error`. To
  avoid an import cycle, move `_unknown_arg_keyword_error` and its regex into `_annotations.py` and import it back into
  `_runner`.

- `_coerce_args(target, raw, enum_modules=None, *, quiet=False)`. Both call sites pass
  `quiet=spec.context.verbosity == "quiet"`.

- [ ] **Step 1: Write the tests.** Use a factory fixture `make_tools_module(tmp_path, monkeypatch)` →
  `(source: str, name: str = "tc_mod", package: str | None = None) -> ModuleType`. It writes the file (and package
  `__init__.py`), runs `monkeypatch.syspath_prepend`, imports it, and removes it from `sys.modules` afterwards.
    - `tests/utils/test_annotations.py`:
        - `test_type_checking_path_import_is_rescued` (Review Focus 1, via `_coerce_args` with
          `{"path": "/tmp/x", "count": "3"}`).
        - `test_aliased_type_checking_import_is_rescued`: `from pathlib import Path as P`, `path: P`.
        - `test_relative_type_checking_import_is_rescued`: package `pkg` with `pkg/types_.py` defining a class, and
          `pkg/cmd.py` doing `if TYPE_CHECKING: from .types_ import Thing`.
        - `test_typing_dot_type_checking_block_is_scanned`: `import typing` / `if typing.TYPE_CHECKING:`.
        - `test_unresolvable_name_resolves_the_rest`: `x: DoesNotExist, count: int` → `unresolved` has `x`, and
          `hints["count"] is int`.
        - `test_module_global_wins_over_type_checking_import` (Review Focus 5).
        - `test_source_parsed_once`: two `NameError`s in one module → `ast.parse` called once (monkeypatch a counting
          wrapper).
        - `test_non_name_error_becomes_spec_error`: an annotation such as `Annotated[int, 1 / 0]` under `__future__` →
          `SpecError` naming the parameter.
        - `test_lazy_annotations_resolve_per_parameter` (Review Focus 2;
          `pytest.mark.skipif(sys.version_info < (3, 14))`).
    - `tests/runner/test_runner_coverage.py`:
        - `test_coerce_args_resolves_other_params_when_one_hint_fails`: `_fn(ctx, x: DoesNotExist, n: int)` with
          `{"x": "raw", "n": "2"}` → `{"x": "raw", "n": 2}`. stderr (`capsys`) has the exact warning for `x`.
        - `test_coerce_args_quiet_suppresses_unresolved_warning`: same with `quiet=True` → empty stderr.
        - `test_optional_positional_with_type_checking_import_gets_none` (Review Focus 3, module fixture,
          `p: Path | None` absent from raw → `None`).
        - Existing unknown-`arg()`-keyword tests stay green unchanged.

- [ ] **Step 2: Run them and confirm they fail.**
      `uv run pytest tests/utils/test_annotations.py tests/runner/test_runner_coverage.py` → new tests fail (import
      error for the new module, raw values).

- [ ] **Step 3: Implement `_annotations.py`.** Read annotations per the spec's part 1 (`annotationlib` STRING format on
      3.14+, `inspect.get_annotations` before). Evaluate each one alone. Calling `typing.get_type_hints` on a holder
      object whose `__annotations__` is `{name: ann}`, with explicit `globalns` (from
      `inspect.unwrap(target).__globals__`) and `localns`, keeps `get_type_hints` semantics. Check it on 3.11 and 3.14.
      The rescue loop: on `NameError` with `exc.name` bound by the module's cached `TYPE_CHECKING` import table, import
      the binding into a copy of `localns`, then retry. Stop when no new binding is found. Then switch
      `_runner._hints_and_signature` to `resolve_hints`, print the warnings in `_coerce_args`, and update the
      `_localns_for_enum_modules` docstring (its "silently skipped" paragraph).

- [ ] **Step 4: Run** the Step 2 command on Python 3.11 and 3.14 → PASS. Then `uv run pytest tests/runner` → PASS.

- [ ] **Step 5: Commit.** `fix(runner): resolve command type hints one parameter at a time`

### Task 2: Dispatcher detection uses the same resolver

**Files:**

- Modify: `crates/toolr-py/python/toolr/utils/_signature.py` (`detect_dispatch_parameter`, ~320-340)
- Test: `tests/utils/signature/` (the existing dispatch-detection tests live there or in
  `tests/runner/test_dispatch.py`; add to whichever covers `detect_dispatch_parameter`)

**Interfaces:** consumes `resolve_hints` from Task 1.

- [ ] **Step 1: Write the tests:**
    - `test_dispatch_detected_despite_unresolvable_sibling` (Review Focus 4): `from __future__ import annotations`;
      `def d(ctx, other: DoesNotExist, *, cmd: DispatchCommand)` → returns `"cmd"`.
    - `test_dispatch_detected_with_type_checking_dispatchcommand_import`: `DispatchCommand` imported only under
      `TYPE_CHECKING` → returns the parameter name.
    - The existing test that a malformed `arg(...)` propagates its `TypeError`/`SpecError` stays green. Update its
      expected exception type only if Task 1's helper now wraps it in `SpecError`, and keep the message assertion.
- [ ] **Step 2: Run them and confirm they fail.**
- [ ] **Step 3: Implement.** Replace the `get_type_hints` try/except with `resolve_hints(func).hints`. Unresolved
      parameters fall back to `param.annotation`, as now.
- [ ] **Step 4: Run** `uv run pytest tests/utils tests/runner` → PASS.
- [ ] **Step 5: Commit.** `fix(dispatch): detect DispatchCommand when another annotation can't resolve`

### Task 3: Docs, release notes, verification, archive

- [ ] **Step 1:** Add a short "Imports under `TYPE_CHECKING`" subsection to `docs/writing-commands/arguments.md` after
      "Module-level type aliases". It should say that such imports work for argument types, and show the warning printed
      when a name can't be found. Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(arguments): document TYPE_CHECKING imports in command signatures`.
- [ ] **Step 2:** `cargo xtask build-skill-refs --check` → clean (no public-surface change expected). `mise run test`,
      polling every 30-60 s → green. `prek run --all-files` and `uv run mkdocs build --strict` → green. mypy runs in
      prek: add no new errors in touched files.
- [ ] **Step 3:**
      `git mv specs/2026-10-30-runner-type-hints-design.md specs/2026-10-30-runner-type-hints-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive runner type hints design and plan`.
