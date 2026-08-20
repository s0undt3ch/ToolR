# Static Symbol Resolution for Command-Signature Types — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `EnumTable`'s cross-module collision heuristics with a real import-following
resolver, so any object used in a `@command` function's parameter type/default must resolve via
same-module declaration or a traceable import in that module — and make `TYPE_CHECKING`-guarded
imports actually work at runtime, not just at manifest-build time.

**Architecture:** A new `ImportTable` (crates/toolr-core/src/parser/symbols.rs, alongside the
existing `EnumTable`/`TypeAliasTable`) walks each module's top-level imports — including relative
imports, `try/except` dual paths, and module-top-level `TYPE_CHECKING` blocks — into a
`local_name -> Vec<ImportedFrom>` map, where each `ImportedFrom` carries the resolved absolute
module path, the original (possibly aliased-away) name, and whether it came from a
`TYPE_CHECKING` branch. `EnumTable::resolve_def` consults this table before falling back to its
existing same-module/single-cross-module/identical-members logic (which becomes the tie-break for
`try/except` branch disagreement only). `SupportedType::Enum` gains a `module: String` field
carrying the resolved defining module, which flows into the manifest and is what the Python
runtime uses to do a lazy, `TYPE_CHECKING`-safe import at coercion time via `get_type_hints`'s
`localns` parameter.

**Tech Stack:** Rust (`ruff_python_ast` for parsing, already a dependency), Python 3.11+
(`toolr-py`, `msgspec`, `typing.get_type_hints`).

**Spec:** `specs/2026-08-20-command-symbol-resolution-design.md`

## Global Constraints

- Every name used in a `@command` function's parameter annotation or default must resolve via
  same-module declaration or an explicit, traceable import in that same module — never via
  another module's imports, never via Python's global/builtin scope.
- Star imports (`from foo import *`) and `import foo.bar` + attribute-chain usage
  (`foo.bar.X`) are always hard build errors when the name they'd provide is needed for a
  command's parameter type/default. Both get a specific, actionable message — not the generic
  "type is not supported" text.
- `if TYPE_CHECKING:` blocks are recognised **only** as a direct top-level statement of
  `module.body` (never nested inside a function/class), with a test expression of exactly
  `TYPE_CHECKING` or `typing.TYPE_CHECKING`.
- Any manifest schema change requires bumping `RUNNER_SCHEMA_VERSION`
  (`crates/toolr-core/src/execute/spec.rs`) and `SCHEMA_VERSION`
  (`crates/toolr-py/python/toolr/_runner.py`) together — CI fails if they disagree.
- Release notes go in `UNRELEASED.md`, never hand-edited into `CHANGELOG.md`.
- Conventional Commits (`fix(parser): …`, `fix(runner): …`).
- No `Co-Authored-By` trailer on any commit.

---

## File Structure

- `crates/toolr-core/src/parser/symbols.rs` — add `ImportTable` (new struct + impl), extend
  `EnumTable::resolve_def` to consult it. This file already holds `EnumTable`/`TypeAliasTable`/
  `ArgSectionTable`/`ConstTable`; `ImportTable` is a fifth sibling table, same file, same pattern.
- `crates/toolr-core/src/parser/types/resolve.rs` — `resolve_name` gains an `imports: &ImportTable`
  parameter (new, distinct from the existing `imports: &TypeImports` for `toolr.types` — rename
  that parameter at call sites to `type_imports` to disambiguate, since both are needed
  simultaneously).
- `crates/toolr-core/src/parser/types/supported.rs` — `SupportedType::Enum` gains `module: String`.
- `crates/toolr-core/src/parser/build.rs` — Pass 1 builds `ImportTable` per module alongside
  `EnumTable`/`TypeAliasTable`; threads it into Pass 2's `extract_commands` call.
- `crates/toolr-core/src/parser/commands.rs` — thread the new `imports: &ImportTable` parameter
  through `extract_commands`/`extract_arguments`/`resolve_arguments` call signatures.
- `crates/toolr-core/src/execute/spec.rs` — bump `RUNNER_SCHEMA_VERSION`; document the new
  `"module"` field on the manifest's enum-argument JSON shape.
- `crates/toolr-py/python/toolr/_runner.py` — bump `SCHEMA_VERSION`; `_coerce_args` builds a
  `localns` dict from the manifest's enum `module`/`name` fields and passes it to
  `get_type_hints`.
- Tests: unit tests inline in each touched Rust file (existing convention — no separate
  `tests/` files for these); `crates/toolr/tests/*` integration test for the end-to-end CLI
  behaviour; `tests/` (Python, pytest) for the `_coerce_args` `localns` behaviour.

---

## Task 1: `ImportTable` — direct absolute `from X import Y [as Z]`

**Files:**

- Modify: `crates/toolr-core/src/parser/symbols.rs`

**Interfaces:**

- Produces:

  ```rust
  pub struct ImportedFrom {
      pub module: String,       // resolved absolute dotted module path
      pub original_name: String, // name as declared in `module` (pre-aliasing)
      pub via_type_checking: bool,
  }

  #[derive(Debug, Default, Clone)]
  pub struct ImportTable {
      // local_name -> every module-level import statement that binds it,
      // across ordinary imports, TYPE_CHECKING branches, and try/except
      // branches. Multiple entries for one name mean "candidates" — see
      // Task 4 for how ambiguity between them is resolved.
      entries: HashMap<String, Vec<ImportedFrom>>,
  }

  impl ImportTable {
      pub fn from_module(module: &ModModule, module_path: &str) -> Self;
      /// All candidates for `name`. Empty if never imported.
      pub fn candidates(&self, name: &str) -> &[ImportedFrom];
  }
  ```

- [ ] **Step 1: Write the failing tests**

  Add to `crates/toolr-core/src/parser/symbols.rs`'s `#[cfg(test)] mod tests`:

  ```rust
  #[test]
  fn import_table_resolves_direct_absolute_import() {
      let src = "from tools.metrics._common import Environment\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse");
      let candidates = table.candidates("Environment");
      assert_eq!(candidates.len(), 1);
      assert_eq!(candidates[0].module, "tools.metrics._common");
      assert_eq!(candidates[0].original_name, "Environment");
      assert!(!candidates[0].via_type_checking);
  }

  #[test]
  fn import_table_resolves_aliased_import() {
      let src = "from tools.metrics._common import Environment as Env\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse");
      let candidates = table.candidates("Env");
      assert_eq!(candidates.len(), 1);
      assert_eq!(candidates[0].module, "tools.metrics._common");
      assert_eq!(candidates[0].original_name, "Environment");
  }

  #[test]
  fn import_table_ignores_unrelated_names() {
      let src = "from tools.metrics._common import Environment\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse");
      assert!(table.candidates("SomethingElse").is_empty());
  }

  #[test]
  fn import_table_handles_multiple_names_one_statement() {
      let src = "from tools.metrics._common import Environment, Region as R\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse");
      assert_eq!(table.candidates("Environment")[0].module, "tools.metrics._common");
      assert_eq!(table.candidates("R")[0].original_name, "Region");
  }
  ```

  `parse(src)` is the existing test helper already in this file's test module (used by
  `EnumTable` tests) — reuse it, don't redefine it.

- [ ] **Step 2: Run tests, verify they fail to compile**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table -- --nocapture`
  Expected: compile error, `ImportTable` doesn't exist yet.

- [ ] **Step 3: Implement `ImportTable` (absolute imports only, no relative/TYPE_CHECKING/try
  yet — those are Tasks 2–4)**

  Add near the top of `crates/toolr-core/src/parser/symbols.rs`, after the existing `EnumTable`
  block:

  ```rust
  /// One module-level import statement that bound a name into scope,
  /// resolved to the absolute module it actually names.
  #[derive(Debug, Clone)]
  pub struct ImportedFrom {
      pub module: String,
      pub original_name: String,
      pub via_type_checking: bool,
  }

  /// Every `from X import Y [as Z]` (and, once Tasks 2-4 land, its relative
  /// / `TYPE_CHECKING` / `try`-`except` variants) bound in a module's
  /// top-level scope. Deliberately does *not* track `import X` +
  /// attribute-chain usage (`X.Y`) — that shape is rejected outright for
  /// command-signature resolution; see `resolve_def`'s attribute-chain
  /// check in Task 6.
  #[derive(Debug, Default, Clone)]
  pub struct ImportTable {
      entries: HashMap<String, Vec<ImportedFrom>>,
  }

  impl ImportTable {
      pub fn from_module(module: &ModModule, module_path: &str) -> Self {
          let mut table = Self::default();
          for stmt in &module.body {
              table.collect_stmt(stmt, module_path, false);
          }
          table
      }

      fn collect_stmt(&mut self, stmt: &Stmt, module_path: &str, via_type_checking: bool) {
          if let Stmt::ImportFrom(import) = stmt {
              self.collect_import_from(import, module_path, via_type_checking);
          }
      }

      fn collect_import_from(
          &mut self,
          import: &ruff_python_ast::StmtImportFrom,
          module_path: &str,
          via_type_checking: bool,
      ) {
          if import.level != 0 {
              // Relative imports: Task 2.
              return;
          }
          let Some(target_module) = import.module.as_ref().map(|m| m.as_str()) else {
              return;
          };
          for alias in &import.names {
              if alias.name.as_str() == "*" {
                  // Star imports: tracked separately in Task 6, not as a
                  // named candidate here.
                  continue;
              }
              let original_name = alias.name.as_str().to_string();
              let local = alias
                  .asname
                  .as_ref()
                  .map(|n| n.as_str().to_string())
                  .unwrap_or_else(|| original_name.clone());
              self.entries.entry(local).or_default().push(ImportedFrom {
                  module: target_module.to_string(),
                  original_name,
                  via_type_checking,
              });
          }
          let _ = module_path; // used once relative imports land in Task 2
      }

      pub fn candidates(&self, name: &str) -> &[ImportedFrom] {
          self.entries.get(name).map(Vec::as_slice).unwrap_or_default()
      }
  }
  ```

  Note the unused `module_path` param and the early `if import.level != 0 { return; }` — this
  intentionally makes relative imports invisible for now (Task 2 fills that in); don't skip
  wiring the parameter through, later tasks need it.

- [ ] **Step 4: Run tests, verify they pass**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table -- --nocapture`
  Expected: 4 passed.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/toolr-core/src/parser/symbols.rs
  git commit -m "feat(parser): add ImportTable for absolute from-imports"
  ```

---

## Task 2: Relative imports

**Files:**

- Modify: `crates/toolr-core/src/parser/symbols.rs`

**Interfaces:**

- Consumes: `ImportTable::collect_import_from` from Task 1 (the `if import.level != 0 { return; }`
  early exit gets replaced).

- Produces: same `ImportedFrom`/`candidates` surface — no new public API, just correct `module`
  values for relative imports.

- [ ] **Step 1: Write the failing tests**

  ```rust
  #[test]
  fn import_table_resolves_relative_sibling_import() {
      // `from ._common import Environment` inside `tools.metrics.analyse`
      // resolves to `tools.metrics._common`.
      let src = "from ._common import Environment\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse");
      assert_eq!(table.candidates("Environment")[0].module, "tools.metrics._common");
  }

  #[test]
  fn import_table_resolves_relative_parent_import() {
      // `from .. import shared` inside `tools.metrics.sub.analyse`
      // resolves the *module* `shared` (not a name from it) to `tools.shared`.
      // Modelled the same way as a direct absolute import of that module
      // path — see Task 6 for why "from .. import <submodule>" specifically
      // must NOT be treated as importing a class.
      let src = "from .. import shared\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.sub.analyse");
      // `shared` binds the *submodule* `tools.shared`, not a class inside
      // it — resolve_def (Task 5) must never treat this as a class def
      // candidate. ImportTable still records it (module == "tools.shared",
      // original_name == "shared") so Task 6's diagnostic can name it
      // precisely; resolve_def rejects it by checking `EnumTable` has no
      // `ClassDef` named `shared` at that module — see Task 6.
      assert_eq!(table.candidates("shared")[0].module, "tools.shared");
  }

  #[test]
  fn import_table_resolves_relative_import_from_package_root() {
      // `from tools.metrics.analyse` importing via a single dot from a
      // package __init__ context: `from ._common import Environment`
      // where the importing module IS `tools.metrics` (an __init__.py,
      // collapsed to the package path per module_path_for_prefix).
      let src = "from ._common import Environment\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics");
      assert_eq!(table.candidates("Environment")[0].module, "tools.metrics._common");
  }
  ```

- [ ] **Step 2: Run tests, verify they fail**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table_resolves_relative -- --nocapture`
  Expected: FAIL — `module` comes back empty/wrong since level != 0 is currently skipped entirely.

- [ ] **Step 3: Implement relative-import resolution**

  Replace the `if import.level != 0 { return; }` early-exit in `collect_import_from` with real
  math. Add a private helper:

  ```rust
  /// Resolve a relative import's absolute target module.
  ///
  /// `level` is the dot-count (`from . import X` -> 1, `from .. import X`
  /// -> 2). `module` is the part after the dots (`None` for bare
  /// `from . import X`). `current_module` is the *importing* module's own
  /// dotted path, as computed by `module_path_for_prefix` — note that for
  /// an `__init__.py`, that path is already collapsed to the package
  /// itself (no trailing `.__init__`), which is exactly the "current
  /// package" a relative import inside it should resolve against.
  fn resolve_relative_module(
      level: u32,
      module: Option<&str>,
      current_module: &str,
  ) -> Option<String> {
      let mut segments: Vec<&str> = current_module.split('.').collect();
      // One dot = "this package" = drop the current module's own last
      // segment (the file itself) to get its containing package.
      // Two dots = go up one more level, etc.
      for _ in 0..level {
          segments.pop()?;
      }
      let mut out = segments.join(".");
      if let Some(m) = module {
          if !out.is_empty() {
              out.push('.');
          }
          out.push_str(m);
      }
      if out.is_empty() { None } else { Some(out) }
  }
  ```

  Then in `collect_import_from`:

  ```rust
  fn collect_import_from(
      &mut self,
      import: &ruff_python_ast::StmtImportFrom,
      module_path: &str,
      via_type_checking: bool,
  ) {
      let target_module = if import.level == 0 {
          match import.module.as_ref().map(|m| m.as_str()) {
              Some(m) => m.to_string(),
              None => return,
          }
      } else {
          let Some(m) = resolve_relative_module(
              import.level,
              import.module.as_ref().map(|m| m.as_str()),
              module_path,
          ) else {
              return;
          };
          m
      };
      for alias in &import.names {
          if alias.name.as_str() == "*" {
              continue;
          }
          let original_name = alias.name.as_str().to_string();
          let local = alias
              .asname
              .as_ref()
              .map(|n| n.as_str().to_string())
              .unwrap_or_else(|| original_name.clone());
          self.entries.entry(local).or_default().push(ImportedFrom {
              module: target_module.clone(),
              original_name,
              via_type_checking,
          });
      }
  }
  ```

  Delete the old body and the now-unused `let _ = module_path;` line from Task 1.

  **Why `segments.pop()` per level, not `level - 1`:** `current_module` is the *file's own* dotted
  path (e.g. `tools.metrics.analyse`), which already includes the file's own last segment. One dot
  (`from . import X`) means "this file's own package" — i.e. drop `analyse`, leaving
  `tools.metrics`. Two dots means drop `analyse` *and* `metrics`, leaving `tools`. That's exactly
  `level` pops, not `level - 1`. Verify this against the `tools.metrics` (an `__init__.py`) test
  case above too — that path has no trailing file segment to drop for one dot, so
  `tools.metrics.pop()` removes `metrics`, giving just `tools`.

  Wait — that contradicts the test `import_table_resolves_relative_import_from_package_root`,
  which expects `from ._common import Environment` inside `tools.metrics` (the `__init__.py`
  itself) to resolve to `tools.metrics._common`, not `tools._common`. This is the actual Python
  semantics: inside a package's `__init__.py`, `from . import X` means "import X from *this same
  package*," not "from the parent." `__init__.py`'s own file-module-path collapsing (dropping the
  `__init__` segment during `module_path_for_prefix`) means the "one file segment to drop" rule
  above is wrong specifically for the `__init__.py` case — there is no file segment to drop,
  because `module_path_for_prefix` already dropped it.

  **Fix:** the file-vs-package distinction can't be recovered from the dotted string alone once
  `__init__` has been collapsed away. `ImportTable::from_module` needs an extra `is_package: bool`
  parameter (true when the source file is an `__init__.py`), threaded from `build.rs` where the
  file path is still available. Update the signature:

  ```rust
  pub fn from_module(module: &ModModule, module_path: &str, is_package: bool) -> Self
  ```

  and `resolve_relative_module` takes `is_package`, popping `level - 1` segments when
  `is_package` is true (the module path already *is* the package), and `level` segments otherwise
  (need to drop the file's own segment first). Update the three tests above: the
  `tools.metrics.analyse` cases pass `is_package: false`; the `tools.metrics` (`__init__.py`) case
  passes `is_package: true`.

- [ ] **Step 4: Run tests, verify they pass**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table_resolves_relative -- --nocapture`
  Expected: 3 passed.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/toolr-core/src/parser/symbols.rs
  git commit -m "feat(parser): resolve relative imports in ImportTable"
  ```

---

## Task 3: `TYPE_CHECKING` and `try`/`except` branches

**Files:**

- Modify: `crates/toolr-core/src/parser/symbols.rs`

- [ ] **Step 1: Write the failing tests**

  ```rust
  #[test]
  fn import_table_tags_type_checking_import() {
      let src = r#"
  from typing import TYPE_CHECKING
  if TYPE_CHECKING:
      from tools.metrics._common import Environment
  "#;
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
      let c = table.candidates("Environment");
      assert_eq!(c.len(), 1);
      assert!(c[0].via_type_checking);
  }

  #[test]
  fn import_table_recognises_dotted_type_checking() {
      let src = r#"
  import typing
  if typing.TYPE_CHECKING:
      from tools.metrics._common import Environment
  "#;
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
      assert!(table.candidates("Environment")[0].via_type_checking);
  }

  #[test]
  fn import_table_ignores_type_checking_nested_in_function() {
      // A TYPE_CHECKING guard inside a function body is not a module-level
      // import and must not be picked up at all.
      let src = r#"
  from typing import TYPE_CHECKING

  def helper():
      if TYPE_CHECKING:
          from tools.metrics._common import Environment
  "#;
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
      assert!(table.candidates("Environment").is_empty());
  }

  #[test]
  fn import_table_collects_both_try_except_branches() {
      let src = r#"
  try:
      from tools.metrics._common import Environment
  except ImportError:
      from tools.metrics._legacy import Environment
  "#;
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
      let c = table.candidates("Environment");
      assert_eq!(c.len(), 2);
      let modules: Vec<&str> = c.iter().map(|i| i.module.as_str()).collect();
      assert!(modules.contains(&"tools.metrics._common"));
      assert!(modules.contains(&"tools.metrics._legacy"));
  }
  ```

- [ ] **Step 2: Run tests, verify they fail**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table -- --nocapture`
  Expected: compile error (signature now takes 3 args from Task 2) then, once call sites are
  fixed, FAIL on the new assertions — `TYPE_CHECKING`/`try` bodies aren't walked yet.

- [ ] **Step 3: Implement**

  Extend `collect_stmt` (called for every top-level statement, and recursively for `Try`
  branches — but explicitly NOT recursively for arbitrary `If`/function bodies):

  ```rust
  fn collect_stmt(
      &mut self,
      stmt: &Stmt,
      module_path: &str,
      is_package: bool,
      via_type_checking: bool,
  ) {
      match stmt {
          Stmt::ImportFrom(import) => {
              self.collect_import_from(import, module_path, is_package, via_type_checking);
          }
          Stmt::If(if_stmt) if is_type_checking_test(&if_stmt.test) => {
              for inner in &if_stmt.body {
                  self.collect_stmt(inner, module_path, is_package, true);
              }
              // Deliberately not walking elif_else_clauses: `else:` on a
              // TYPE_CHECKING guard is real runtime code (the opposite
              // branch), not another TYPE_CHECKING import source.
          }
          Stmt::Try(try_stmt) => {
              for inner in &try_stmt.body {
                  self.collect_stmt(inner, module_path, is_package, via_type_checking);
              }
              for handler in &try_stmt.handlers {
                  let ExceptHandler::ExceptHandler(h) = handler;
                  for inner in &h.body {
                      self.collect_stmt(inner, module_path, is_package, via_type_checking);
                  }
              }
              // Deliberately not walking `orelse`/`finalbody`: those run
              // unconditionally after a *successful* try, so an import
              // there is equivalent to a plain top-level import, but
              // supporting that shape isn't part of this design's scope
              // (no real-world pattern reported for it) — leave as a
              // documented gap, same posture as function-body imports.
          }
          _ => {}
      }
  }
  ```

  And `from_module`'s top-level loop becomes:

  ```rust
  pub fn from_module(module: &ModModule, module_path: &str, is_package: bool) -> Self {
      let mut table = Self::default();
      for stmt in &module.body {
          table.collect_stmt(stmt, module_path, is_package, false);
      }
      table
  }
  ```

  Add the `TYPE_CHECKING` test-expression matcher:

  ```rust
  /// Whether `expr` is exactly `TYPE_CHECKING` or `typing.TYPE_CHECKING`.
  /// Anything more complex (`TYPE_CHECKING and DEBUG`, `not TYPE_CHECKING`)
  /// is not recognised — those aren't the documented mypy/ruff-endorsed
  /// pattern and treating them as "definitely a TYPE_CHECKING guard" would
  /// be guessing at intent.
  fn is_type_checking_test(expr: &Expr) -> bool {
      match expr {
          Expr::Name(n) => n.id.as_str() == "TYPE_CHECKING",
          Expr::Attribute(a) => {
              a.attr.as_str() == "TYPE_CHECKING"
                  && matches!(a.value.as_ref(), Expr::Name(n) if n.id.as_str() == "typing")
          }
          _ => false,
      }
  }
  ```

  Import `ExceptHandler` at the top of the file: `use ruff_python_ast::{..., ExceptHandler};`.

- [ ] **Step 4: Run tests, verify they pass**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table -- --nocapture`
  Expected: all `import_table_*` tests pass (8 total across Tasks 1–3).

- [ ] **Step 5: Commit**

  ```bash
  git add crates/toolr-core/src/parser/symbols.rs
  git commit -m "feat(parser): collect TYPE_CHECKING and try/except imports"
  ```

---

## Task 4: Wire `ImportTable` into `EnumTable::resolve_def`

**Files:**

- Modify: `crates/toolr-core/src/parser/symbols.rs`

**Interfaces:**

- Consumes: `ImportTable::candidates` (Tasks 1–3), existing `EnumTable` internals
  (`members: HashMap<String, Vec<EnumDef>>`, `same_members` helper from commit `3fd7e35f`).

- Produces: `EnumTable::lookup`/`lookup_member` gain an `imports: &ImportTable` parameter. This
  changes their public signature — Task 5 updates every call site.

- [ ] **Step 1: Write the failing tests**

  ```rust
  #[test]
  fn resolve_def_prefers_explicit_import_over_guessing() {
      // Three modules declare a *different*-shaped `Database` class.
      // Without an import, this is the #449 ambiguous case (still errors).
      // With an explicit import naming one of them, it must resolve to
      // exactly that one, not error and not guess.
      let a = parse(
          r#"
  from enum import StrEnum
  class Database(StrEnum):
      PRIMARY = "primary"
  "#,
      );
      let b = parse(
          r#"
  from enum import StrEnum
  class Database(StrEnum):
      REPLICA = "replica"
  "#,
      );
      let mut table = EnumTable::from_module(&a, "tools.module_a");
      table.merge(EnumTable::from_module(&b, "tools.module_b"));

      let imports = ImportTable::from_module(
          &parse("from tools.module_b import Database\n"),
          "tools.module_c",
          false,
      );
      assert_eq!(
          table.lookup("Database", "tools.module_c", &imports).unwrap(),
          vec!["replica".to_string()]
      );
  }

  #[test]
  fn resolve_def_still_ambiguous_without_an_import() {
      let a = parse("from enum import StrEnum\nclass Database(StrEnum):\n    PRIMARY = \"primary\"\n");
      let b = parse("from enum import StrEnum\nclass Database(StrEnum):\n    REPLICA = \"replica\"\n");
      let mut table = EnumTable::from_module(&a, "tools.module_a");
      table.merge(EnumTable::from_module(&b, "tools.module_b"));
      let imports = ImportTable::default();
      assert!(table.lookup("Database", "tools.module_c", &imports).is_none());
  }

  #[test]
  fn resolve_def_import_pointing_nowhere_known_is_none() {
      // The import names a module we never parsed a ClassDef for under
      // that name — e.g. a typo, or a module outside the scanned tree.
      // resolve_def must not fall back to guessing; the caller (Task 6)
      // turns this into a specific error, not a silent success.
      let a = parse("from enum import StrEnum\nclass Database(StrEnum):\n    PRIMARY = \"primary\"\n");
      let table = EnumTable::from_module(&a, "tools.module_a");
      let imports = ImportTable::from_module(
          &parse("from tools.somewhere_else import Database\n"),
          "tools.module_c",
          false,
      );
      assert!(table.lookup("Database", "tools.module_c", &imports).is_none());
  }
  ```

- [ ] **Step 2: Run tests, verify they fail to compile**

  Run: `cargo test -p toolr-core --lib symbols::tests::resolve_def -- --nocapture`
  Expected: compile error, `lookup`/`resolve_def` don't take an `ImportTable` argument yet.

- [ ] **Step 3: Implement**

  Update `resolve_def` (and thread through `lookup`/`lookup_member`):

  ```rust
  fn resolve_def<'a>(
      &'a self,
      class: &str,
      current_module: &str,
      imports: &ImportTable,
  ) -> Option<&'a [EnumMember]> {
      let defs = self.members.get(class)?;
      if let Some(d) = defs.iter().find(|d| d.module == current_module) {
          return Some(&d.members);
      }
      let candidates = imports.candidates(class);
      if !candidates.is_empty() {
          // Explicit import(s) present: resolve *only* against what they
          // name. Multiple candidates only happen via try/except branches
          // (Task 3) — same identical-members tie-break as a genuine
          // cross-module collision.
          let matched: Vec<&EnumDef> = candidates
              .iter()
              .filter_map(|c| defs.iter().find(|d| d.module == c.module))
              .collect();
          return match matched.as_slice() {
              [] => None,
              [only] => Some(&only.members),
              [first, rest @ ..]
                  if rest.iter().all(|d| same_members(&d.members, &first.members)) =>
              {
                  Some(&first.members)
              }
              _ => None,
          };
      }
      // No explicit import at all: fall back to the pre-existing
      // same-name-across-modules heuristic (GH #449/#454) — this is the
      // path for code that predates real import tracking, or where the
      // parser genuinely can't see the import (see Task 6's rejected
      // shapes). Still refuses to guess between genuinely different
      // definitions.
      match defs.as_slice() {
          [only] => Some(&only.members),
          [first, rest @ ..]
              if rest.iter().all(|d| same_members(&d.members, &first.members)) =>
          {
              Some(&first.members)
          }
          _ => None,
      }
  }

  pub fn lookup(&self, class: &str, current_module: &str, imports: &ImportTable) -> Option<Vec<String>> {
      self.resolve_def(class, current_module, imports)
          .map(|m| m.iter().map(|em| em.value.clone()).collect())
  }

  pub fn lookup_member(
      &self,
      class: &str,
      member: &str,
      current_module: &str,
      imports: &ImportTable,
  ) -> Option<&str> {
      self.resolve_def(class, current_module, imports)?
          .iter()
          .find(|em| em.name == member)
          .map(|em| em.value.as_str())
  }
  ```

- [ ] **Step 4: Fix the now-broken existing `EnumTable` tests**

  Every existing `table.lookup("X", "module")` / `table.lookup_member(...)` call in this file's
  test module needs a trailing `&ImportTable::default()` argument. Grep and fix them all:

  Run: `grep -n '\.lookup(\|\.lookup_member(' crates/toolr-core/src/parser/symbols.rs`

  Add `&ImportTable::default()` as the last argument to every call found (there are roughly a
  dozen, all inside `#[cfg(test)] mod tests`).

- [ ] **Step 5: Run tests, verify everything passes**

  Run: `cargo test -p toolr-core --lib symbols:: -- --nocapture`
  Expected: all pass, including the 3 new `resolve_def_*` tests and every pre-existing `EnumTable`
  test now passing an explicit (empty, for the old tests) `ImportTable`.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/toolr-core/src/parser/symbols.rs
  git commit -m "feat(parser): resolve enum defs through explicit imports first"
  ```

---

## Task 4a: Chain-following across modules (aliasing chains, `__init__.py` re-exports)

**Why this task exists:** Task 4's `resolve_def` only matches a candidate's `module` directly
against an `EnumDef`'s declaring module — one hop. That's insufficient for the design's explicit
"chain-following, not single-hop" requirement (`__init__.py` re-exports, `from mod1 import Y as Z`
where `mod1.Y` is itself just `mod1`'s own import of something else). This task closes that gap
before any other code builds on top of the single-hop version.

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs` (Pass 1)
- Modify: `crates/toolr-core/src/parser/symbols.rs` (`EnumTable::resolve_def`)

**Interfaces:**

- Produces: `EnumTable::resolve_def`/`lookup`/`lookup_member` take an additional
  `all_imports: &HashMap<String, ImportTable>` parameter (module path -> that module's own
  `ImportTable`) instead of a single `imports: &ImportTable` for just the calling module. The
  calling module's own table is `all_imports.get(current_module)`.

- [ ] **Step 1: Move `ImportTable` construction into Pass 1**

  In `build_static_manifest_inner` (`build.rs`), alongside the existing Pass-1 loop that builds
  `enums`/`aliases`/`sections`, add:

  ```rust
  let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
  for path in &py_files {
      let module = parse_python_file(path).map_err(BuildError::Build)?;
      let module_path = module_path_for(tools_dir, path);
      let is_package = path.file_stem().map(|s| s == "__init__").unwrap_or(false);
      all_imports.insert(module_path.clone(), ImportTable::from_module(&module, &module_path, is_package));
      enums.merge(EnumTable::from_module(&module, &module_path));
      aliases.merge(TypeAliasTable::from_module(&module));
      sections.merge(ArgSectionTable::from_module(&module));
  }
  ```

  (This parses each file twice — once in this loop, once again in Pass 2, same as the existing
  `enums`/`aliases`/`sections` pattern already does. Don't try to eliminate the double-parse as
  part of this task; it's pre-existing and out of scope.)

  Mirror this in `build_fragment.rs`'s `build_third_party_fragment` Pass 1 loop.

  Then delete the Pass-2 per-file `ImportTable::from_module(...)` call added in Task 5 Step 1 (not
  yet written if you're doing tasks in order — if Task 5 hasn't happened yet, skip this; note it
  here so whoever writes Task 5 knows `ImportTable` construction has already moved to Pass 1 by
  the time they get there) and instead look the current module's table up:
  `all_imports.get(&module_path).cloned().unwrap_or_default()` (or thread a reference — check
  ownership/borrow constraints against how `enums`/`aliases` are already borrowed across the Pass-2
  loop before deciding clone vs. reference).

- [ ] **Step 2: Write the failing test**

  ```rust
  // symbols.rs test module
  #[test]
  fn resolve_def_follows_reexport_chain_through_init() {
      // tools/metrics/_common.py declares Environment.
      // tools/metrics/__init__.py does `from ._common import Environment`.
      // tools/analyse.py does `from tools.metrics import Environment`.
      let common = parse("class Environment(enum.StrEnum):\n    PRODUCTION = \"production\"\n");
      let mut enums = EnumTable::from_module(&common, "tools.metrics._common");

      let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
      all_imports.insert(
          "tools.metrics".to_string(),
          ImportTable::from_module(&parse("from ._common import Environment\n"), "tools.metrics", true),
      );
      all_imports.insert(
          "tools.analyse".to_string(),
          ImportTable::from_module(
              &parse("from tools.metrics import Environment\n"),
              "tools.analyse",
              false,
          ),
      );

      assert_eq!(
          enums.lookup("Environment", "tools.analyse", &all_imports).unwrap(),
          ("tools.metrics._common".to_string(), vec!["production".to_string()])
      );
  }

  #[test]
  fn resolve_def_follows_aliasing_chain() {
      // mod1 imports Foo from tools.real and re-aliases it as Bar; mod2
      // imports Bar from mod1.
      let real = parse("class Foo(enum.StrEnum):\n    A = \"a\"\n");
      let mut enums = EnumTable::from_module(&real, "tools.real");

      let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
      all_imports.insert(
          "tools.mod1".to_string(),
          ImportTable::from_module(&parse("from tools.real import Foo as Bar\n"), "tools.mod1", false),
      );
      all_imports.insert(
          "tools.mod2".to_string(),
          ImportTable::from_module(&parse("from tools.mod1 import Bar\n"), "tools.mod2", false),
      );

      assert_eq!(
          enums.lookup("Bar", "tools.mod2", &all_imports).unwrap(),
          ("tools.real".to_string(), vec!["a".to_string()])
      );
  }

  #[test]
  fn resolve_def_chain_cycle_guard_does_not_hang() {
      // mod_a imports X from mod_b; mod_b imports X from mod_a. Neither
      // declares X. Must terminate with None, not loop forever.
      let mut enums = EnumTable::default();
      let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
      all_imports.insert(
          "tools.mod_a".to_string(),
          ImportTable::from_module(&parse("from tools.mod_b import X\n"), "tools.mod_a", false),
      );
      all_imports.insert(
          "tools.mod_b".to_string(),
          ImportTable::from_module(&parse("from tools.mod_a import X\n"), "tools.mod_b", false),
      );
      assert!(enums.lookup("X", "tools.mod_a", &all_imports).is_none());
  }
  ```

- [ ] **Step 3: Run tests, verify they fail to compile**

  Run: `cargo test -p toolr-core --lib symbols::tests::resolve_def_follows -- --nocapture`
  Expected: compile error — `lookup`'s signature doesn't take `all_imports` yet, and it doesn't
  return `(module, values)` for the chain-following path yet either (Task 4 only added the module
  to the direct, non-chained path's return in Task 7 — check whether Task 7 has landed yet; if
  you're doing tasks strictly in order, Task 7 comes *after* this one, so `lookup` at this point
  still returns bare `Vec<String>`. **Correction**: this task's tests assume `lookup` already
  returns `(String, Vec<String>)`. Reorder: do Task 7's `resolve_def`/`lookup` return-type change
  *before* this task, not after — Task 7 as drafted doesn't depend on chain-following, but this
  task's tests depend on Task 7's return-type shape. Swap Task 4a and Task 7's ordering: implement
  Task 7 immediately after Task 4, then this task (still called 4a in the plan for cross-reference
  clarity, but executed after Task 7), then Tasks 5/6/8/9/10 as numbered.**

- [ ] **Step 4: Implement chain-following**

  Change `resolve_def`'s signature to take `all_imports: &HashMap<String, ImportTable>` in place
  of Task 4's `imports: &ImportTable`, and add a cycle-guarded recursive helper:

  ```rust
  fn resolve_def<'a>(
      &'a self,
      class: &str,
      current_module: &str,
      all_imports: &HashMap<String, ImportTable>,
  ) -> Option<(String, &'a [EnumMember])> {
      self.resolve_def_inner(class, current_module, all_imports, &mut Vec::new())
  }

  fn resolve_def_inner<'a>(
      &'a self,
      class: &str,
      current_module: &str,
      all_imports: &HashMap<String, ImportTable>,
      seen: &mut Vec<(String, String)>, // (module, class) pairs already visited
  ) -> Option<(String, &'a [EnumMember])> {
      let defs = self.members.get(class)?;
      if let Some(d) = defs.iter().find(|d| d.module == current_module) {
          return Some((d.module.clone(), &d.members));
      }
      let empty = ImportTable::default();
      let imports = all_imports.get(current_module).unwrap_or(&empty);
      let candidates = imports.candidates(class);
      if !candidates.is_empty() {
          let mut matched: Vec<(String, &[EnumMember])> = Vec::new();
          for c in candidates {
              // Direct declaration in the named module?
              if let Some(d) = defs.iter().find(|d| d.module == c.module) {
                  matched.push((d.module.clone(), &d.members));
                  continue;
              }
              // Not declared there — recurse through *that* module's own
              // imports, looking for `c.original_name` (the pre-aliasing
              // name), guarded against cycles.
              let key = (c.module.clone(), c.original_name.clone());
              if seen.contains(&key) || seen.len() > 8 {
                  continue; // cycle or depth cap — skip this candidate
              }
              seen.push(key);
              if let Some(found) =
                  self.resolve_def_inner(&c.original_name, &c.module, all_imports, seen)
              {
                  matched.push(found);
              }
          }
          return match matched.as_slice() {
              [] => None,
              [only] => Some(only.clone()),
              [first, rest @ ..]
                  if rest.iter().all(|(_, m)| same_members(m, first.1)) =>
              {
                  Some(first.clone())
              }
              _ => None,
          };
      }
      match defs.as_slice() {
          [only] => Some((only.module.clone(), &only.members)),
          [first, rest @ ..]
              if rest.iter().all(|d| same_members(&d.members, &first.members)) =>
          {
              Some((first.module.clone(), &first.members))
          }
          _ => None,
      }
  }
  ```

  Depth cap of 8 is arbitrary but generous — no legitimate re-export chain should nest that deep;
  it exists purely to bound pathological/cyclic input, same spirit as `resolve_inner`'s type-alias
  depth guard in `resolve.rs`.

  Update `lookup`/`lookup_member` to take `all_imports: &HashMap<String, ImportTable>` and return
  `Option<(String, Vec<String>)>` / unchanged respectively.

- [ ] **Step 5: Run tests, verify they pass**

  Run: `cargo test -p toolr-core --lib symbols:: -- --test-threads=4`
  Expected: all pass, including the 3 new chain-following tests and every existing test updated
  to pass `&HashMap::new()` (or a populated map) instead of `&ImportTable::default()`.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/toolr-core/src/parser/symbols.rs crates/toolr-core/src/parser/build.rs \
          crates/toolr-core/src/parser/build_fragment.rs
  git commit -m "feat(parser): follow re-export/alias chains across modules when resolving enums"
  ```

---

## Task 5: Thread `ImportTable` through `build.rs`/`commands.rs`/`resolve.rs`

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs`
- Modify: `crates/toolr-core/src/parser/commands.rs`
- Modify: `crates/toolr-core/src/parser/types/resolve.rs`
- Modify: `crates/toolr-core/src/parser/types/mod.rs` (test helpers use `resolve(...)` directly)
- Modify: `crates/toolr-core/src/build_fragment.rs` (third-party fragment build path — same Pass
  1/Pass 2 shape as `build.rs`)

**Interfaces:**

- Consumes: Task 4a already moved `ImportTable` construction into Pass 1 and produces
  `all_imports: HashMap<String, ImportTable>`; `EnumTable::lookup`/`lookup_member` already take
  `&HashMap<String, ImportTable>` (Task 4a).
- Produces: `resolve()`/`resolve_arguments()`/`extract_commands()` now take an additional
  `all_imports: &HashMap<String, ImportTable>` parameter (named to disambiguate from the
  pre-existing `type_imports: &TypeImports` for `toolr.types`), threaded from Pass 1's map through
  to wherever `enums.lookup(...)` is ultimately called.
- [ ] **Step 1: Confirm Pass 1 already builds `all_imports` (done in Task 4a)**

  If Task 4a was executed as ordered, `build_static_manifest_inner` and
  `build_third_party_fragment` already build `all_imports: HashMap<String, ImportTable>` in Pass
  1. This task's only remaining job is threading that map — by reference — into Pass 2's
  `extract_commands` call, alongside the existing `enums`/`aliases`/`sections` references:

  ```rust
  let commands = extract_commands(
      &module,
      &module_path,
      &bindings,
      &enums,
      &consts,
      &type_imports,
      &sources_imports,
      &aliases,
      &sections,
      &all_imports,
      &global_vars,
      &mut type_errors,
  );
  ```

- [ ] **Step 2: Thread the parameter through `extract_commands` (commands.rs)**

  `extract_commands`'s signature gains `symbol_imports: &ImportTable` right after the existing
  `enums: &EnumTable` parameter (grep every call site — production code in `build.rs`/
  `build_fragment.rs`, plus every test call in `commands.rs`'s own test module that currently
  passes `&EnumTable::default()`, which all need a matching `&ImportTable::default()` added).

  `extract_commands` passes it straight through to `extract_arguments`/`resolve_arguments`, which
  pass it straight through to every `resolve(...)`/`collect_allowed_values(...)`/
  `literal_default(...)` call in `signatures.rs` that currently takes `enums: &EnumTable`.

  **Note on `symbol_imports` vs `all_imports` naming below:** `resolve_name`/`resolve_inner` only
  ever need *the current module's own* `ImportTable` for the star-import/dotted-attribute-chain
  checks (Task 6) — those are inherently local ("does *this* module have a star import"), not
  chain-following. So `resolve()`'s public signature takes the single current-module
  `symbol_imports: &ImportTable` (looked up by its caller via `all_imports.get(module).unwrap_or(
  &ImportTable::default())` right before calling `resolve`), while `enums.lookup(...)` inside it
  separately takes the full `all_imports: &HashMap<String, ImportTable>` for chain-following. Both
  get passed into `resolve()`/`resolve_inner`/`resolve_name` as two distinct parameters — don't
  collapse them into one, they answer different questions (local-module facts vs. cross-module
  chain-walk).

- [ ] **Step 3: Update `resolve.rs`'s `resolve`/`resolve_inner`/`resolve_name` signatures**

  Rename the existing `imports: &TypeImports` parameter to `type_imports: &TypeImports`
  everywhere in this file (pure rename, no behaviour change) and add
  `symbol_imports: &ImportTable` alongside it. Update the one call site that actually reads
  `enums.lookup(...)` (`resolve_name`, currently at what was line 263 before this plan's edits):

  ```rust
  if let Some(values) = enums.lookup(name, module, symbol_imports) {
      ...
  }
  ```

  and the `resolve_enum_attribute_default` helper in `signatures.rs` (backs `Class.MEMBER`
  defaults) similarly gains `symbol_imports` and passes it to `enums.lookup_member(...)`.

- [ ] **Step 4: Update every test call site**

  `crates/toolr-core/src/parser/types/mod.rs`'s test module calls `resolve(&ann, &EnumTable::
  default(), &TypeImports::default(), &TypeAliasTable::default(), "tools.test")` about a dozen
  times. Add `&ImportTable::default()` as a new argument (position: immediately after the renamed
  `type_imports` argument, i.e. `resolve(&ann, &EnumTable::default(), &TypeImports::default(),
  &ImportTable::default(), &TypeAliasTable::default(), "tools.test")`). Same for
  `signatures.rs`'s own test module.

- [ ] **Step 5: Run the full parser test suite**

  Run: `cargo test -p toolr-core --lib parser:: -- --test-threads=4`
  Expected: every test compiles and passes (this task is a pure plumbing change — no new
  behaviour, so no test should newly fail; if one does, a call site was missed).

- [ ] **Step 6: Commit**

  ```bash
  git add crates/toolr-core/src/parser/build.rs crates/toolr-core/src/parser/build_fragment.rs \
          crates/toolr-core/src/parser/commands.rs crates/toolr-core/src/parser/types/resolve.rs \
          crates/toolr-core/src/parser/types/mod.rs crates/toolr-core/src/parser/signatures.rs
  git commit -m "refactor(parser): thread ImportTable through the resolver call graph"
  ```

---

## Task 6: Reject star imports and dotted-attribute-chain usage

**Files:**

- Modify: `crates/toolr-core/src/parser/symbols.rs` (`ImportTable` gains star-import tracking)

- Modify: `crates/toolr-core/src/parser/types/resolve.rs` (`resolve_name` produces the specific
  error messages)

- Modify: `crates/toolr-core/src/parser/types/supported.rs` (if a new `UnsupportedType` message
  variant/constructor is cleaner than string formatting inline — check the existing
  `UnsupportedType` shape first before adding one)

- [ ] **Step 1: Write the failing tests**

  ```rust
  // In symbols.rs's test module:
  #[test]
  fn import_table_records_star_import_presence() {
      let src = "from tools.metrics._common import *\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
      assert!(table.has_star_import());
  }

  #[test]
  fn import_table_no_star_import_by_default() {
      let src = "from tools.metrics._common import Environment\n";
      let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
      assert!(!table.has_star_import());
  }
  ```

  ```rust
  // In resolve.rs's test module (or wherever UnsupportedType message
  // assertions already live — check signatures.rs's existing pattern for
  // `unknown_dotted_name_errors_with_pointer_to_toolr_types` first):
  #[test]
  fn star_import_in_scope_gets_specific_error() {
      let src = "from tools.metrics._common import *\ndef f(x: Environment): pass\n";
      let m = module(src); // existing test helper in types/mod.rs
      let (_, ann) = first_annotation(src);
      let imports = ImportTable::from_module(&m, "tools.metrics.analyse", false);
      let err = resolve(
          &ann,
          &EnumTable::default(),
          &TypeImports::default(),
          &imports,
          &TypeAliasTable::default(),
          "tools.metrics.analyse",
      )
      .expect_err("should fail");
      let msg = err.to_string();
      assert!(msg.contains("star import"), "msg was: {msg}");
      assert!(msg.contains("Environment"), "msg was: {msg}");
  }

  #[test]
  fn dotted_attribute_chain_gets_specific_error() {
      let src = "import tools.metrics._common\ndef f(x: tools.metrics._common.Environment): pass\n";
      let m = module(src);
      let (_, ann) = first_annotation(src);
      let err = resolve(
          &ann,
          &EnumTable::default(),
          &TypeImports::default(),
          &ImportTable::default(),
          &TypeAliasTable::default(),
          "tools.metrics.analyse",
      )
      .expect_err("should fail");
      let msg = err.to_string();
      assert!(msg.contains("from tools.metrics._common import Environment"), "msg was: {msg}");
  }
  ```

- [ ] **Step 2: Run tests, verify they fail**

  Run: `cargo test -p toolr-core --lib symbols::tests::import_table_records_star -- --nocapture`
  Run: `cargo test -p toolr-core --lib resolve::tests::star_import -- --nocapture`
  Run: `cargo test -p toolr-core --lib resolve::tests::dotted_attribute_chain -- --nocapture`
  Expected: FAIL (`has_star_import` doesn't exist; messages don't mention "star import" or the
  specific `from ... import ...` suggestion — today's dotted-attribute case likely already errors
  via the generic `unknown_dotted_name_errors_with_pointer_to_toolr_types` path, just with the
  wrong message).

- [ ] **Step 3: Implement star-import tracking**

  In `ImportTable::collect_import_from`, where the existing code does
  `if alias.name.as_str() == "*" { continue; }`, instead set a flag:

  ```rust
  #[derive(Debug, Default, Clone)]
  pub struct ImportTable {
      entries: HashMap<String, Vec<ImportedFrom>>,
      star_import: bool,
  }
  ```

  ```rust
  if alias.name.as_str() == "*" {
      self.star_import = true;
      continue;
  }
  ```

  ```rust
  pub fn has_star_import(&self) -> bool {
      self.star_import
  }
  ```

- [ ] **Step 4: Implement the specific error messages in `resolve_name`**

  In `resolve.rs`'s `resolve_name` (the `_ => { ... }` arm that currently falls through to the
  generic unsupported-type error after `enums.lookup`/`aliases.lookup` both miss), add, before the
  generic fallback:

  ```rust
  if symbol_imports.has_star_import() {
      return Err(UnsupportedType::new(
          name,
          format!(
              "`{name}` isn't resolvable because this module uses a star import \
               (`from ... import *`). Please import `{name}` explicitly instead."
          ),
      ));
  }
  ```

  Check `UnsupportedType`'s actual constructor shape in `supported.rs` before writing this — it
  may take `(name, reason)` or a single formatted string; match whatever
  `unknown_dotted_name_errors_with_pointer_to_toolr_types`'s test already exercises, don't
  introduce a second, inconsistent construction style.

  For the dotted-attribute-chain case: this is the `Expr::Attribute` arm of `resolve_inner`
  (currently around what was line 225–237 before this plan's edits, the one that checks
  `imports.resolve_attribute(annotation)` for `toolr.types`/`toolr.sources` and otherwise only
  recognises `pathlib.Path`). Add a check there: if the attribute chain's *root* name is bound by
  an `import X` statement (need to track plain `Stmt::Import` bindings in `ImportTable` too — add
  a `module_bindings: HashMap<String, String>` mapping local name to the dotted module it was
  bound to, populated from `Stmt::Import`, separate from the `entries` map since these are module
  bindings, not name bindings), produce:

  ```rust
  if let Some(bound_module) = symbol_imports.resolve_module_binding(&rendered_root) {
      return Err(UnsupportedType::new(
          &full_attr_name,
          format!(
              "`{rendered}` isn't supported as a type annotation — please \
               `from {bound_module} import {attr_name}` and use `{attr_name}` directly."
          ),
      ));
  }
  ```

  where `rendered`/`rendered_root`/`attr_name` come from walking the `Expr::Attribute` chain the
  same way the existing `pathlib.Path` check already does (reuse whatever chain-flattening helper
  that check uses, don't write a second one).

- [ ] **Step 5: Run tests, verify they pass**

  Run: `cargo test -p toolr-core --lib symbols:: resolve:: -- --test-threads=4`
  Expected: all pass, including the 4 new tests from this task.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/toolr-core/src/parser/symbols.rs crates/toolr-core/src/parser/types/resolve.rs
  git commit -m "feat(parser): specific errors for star imports and dotted-attribute chains"
  ```

---

## Task 7: `SupportedType::Enum` carries its defining module

**Files:**

- Modify: `crates/toolr-core/src/parser/types/supported.rs`
- Modify: `crates/toolr-core/src/parser/types/resolve.rs` (`resolve_name`'s `Enum { name, values }`
  construction site)
- Modify: `crates/toolr-core/src/parser/commands.rs` (the `Optional`/`Enum` backfill loop added
  earlier this session — matches on `SupportedType::Enum { values, .. }`, unaffected by the new
  field since it's already using `..`)

**Interfaces:**

- Consumes: `EnumTable::resolve_def` (Task 4) already knows the resolved `EnumDef`'s `module` —
  it's just not returned today. `resolve_def`'s return type needs to change from
  `Option<&[EnumMember]>` to something that also carries the module, e.g.
  `Option<(&str, &[EnumMember])>` — check every call site (`lookup`, `lookup_member`) for the
  ripple.

- Produces: `SupportedType::Enum { name: String, module: String, values: Vec<String> }`.

- [ ] **Step 1: Write the failing test**

  ```rust
  // In resolve.rs's test module:
  #[test]
  fn resolved_enum_carries_its_defining_module() {
      let src = "from tools.metrics._common import Environment\ndef f(x: Environment): pass\n";
      let m = module(src);
      let (_, ann) = first_annotation(src);
      let mut enums = EnumTable::default();
      enums.merge(EnumTable::from_module(
          &parse_module_helper("class Environment(enum.StrEnum):\n    PRODUCTION = \"production\"\n"),
          "tools.metrics._common",
      ));
      let imports = ImportTable::from_module(&m, "tools.metrics.analyse", false);
      let resolved = resolve(
          &ann, &enums, &TypeImports::default(), &imports, &TypeAliasTable::default(),
          "tools.metrics.analyse",
      ).unwrap();
      let SupportedType::Enum { module, .. } = resolved else { panic!("expected Enum") };
      assert_eq!(module, "tools.metrics._common");
  }
  ```

  Use whichever existing helper this file already has for parsing a bare module snippet
  (`parse`/`module`, per the file's own conventions — check both `symbols.rs` and
  `types/mod.rs`'s test helpers, they may differ; reuse, don't add a third).

- [ ] **Step 2: Run test, verify it fails to compile**

  Run: `cargo test -p toolr-core --lib resolve::tests::resolved_enum_carries -- --nocapture`
  Expected: compile error — `SupportedType::Enum` has no `module` field yet.

- [ ] **Step 3: Implement**

  `supported.rs`:

  ```rust
  Enum {
      name: String,
      module: String,
      values: Vec<String>,
  },
  ```

  `symbols.rs`'s `resolve_def` return type becomes `Option<(&str, &[EnumMember])>` (module path,
  members); update its three call sites (`lookup`, `lookup_member`, and the direct call inside
  `resolve_def` itself for the recursive `[only]`/dedupe arms — those already have `d.module`/
  `first.module`/`only.module` in scope, just need to return them alongside `.members`).
  `lookup`'s signature changes to return `Option<(String, Vec<String>)>` (module, values) instead
  of bare `Option<Vec<String>>` — update its one call site in `resolve_name` accordingly:

  ```rust
  if let Some((defining_module, values)) = enums.lookup(name, module, symbol_imports) {
      return Ok(SupportedType::Enum {
          name: name.to_string(),
          module: defining_module,
          values,
      });
  }
  ```

  `lookup_member` doesn't need the module (it's only used for rendering `Class.MEMBER` default
  values to a string) — leave its return type as `Option<&str>`.

  Grep every other construction site of `SupportedType::Enum { .. }` across the crate (there
  should be exactly one production site — `resolve_name` — plus any test fixtures that construct
  it directly rather than through `resolve()`) and add the new field.

- [ ] **Step 4: Run the full parser + build test suite**

  Run: `cargo test -p toolr-core --lib parser:: build_fragment:: -- --test-threads=4`
  Expected: all pass. Fix any test that directly constructs `SupportedType::Enum { name, values }`
  without `module` (compile error will point at each one).

- [ ] **Step 5: Commit**

  ```bash
  git add crates/toolr-core/src/parser/types/supported.rs crates/toolr-core/src/parser/types/resolve.rs \
          crates/toolr-core/src/parser/symbols.rs
  git commit -m "feat(parser): carry the resolved defining module on SupportedType::Enum"
  ```

---

## Task 8: Manifest schema — serialise the enum's module, bump schema versions

**Files:**

- Modify: `crates/toolr-core/src/execute/spec.rs` (bump `RUNNER_SCHEMA_VERSION`; check its doc
  comment for the exact "changes requiring a bump" list and add this one)
- Modify: wherever `SupportedType::Enum` is serialised into the manifest's argument JSON today —
  find it first:

  Run: `grep -rn "SupportedType::Enum" crates/toolr-core/src --include='*.rs' | grep -v test`

  Likely in `crates/toolr-core/src/manifest.rs` or `crates/toolr-core/src/parser/commands.rs`
  wherever `ArgumentKind`/argument structs are assembled from a resolved `SupportedType` — this
  plan can't name the exact line without that grep result; the implementer must locate it as
  Step 1 before writing anything.
- Modify: `crates/toolr-py/python/toolr/_runner.py` (bump `SCHEMA_VERSION` to match)

**Interfaces:**

- Produces: the manifest's per-argument JSON gains an `"enum_module"` string field (only present
  when the argument's type is `Enum` or `Optional[Enum]`), alongside the existing
  `"allowed_values"` field it already carries.

- [ ] **Step 1: Locate the manifest argument struct and its serialisation**

  Run the grep above. Read the surrounding struct definition (likely something like
  `pub struct Argument { ..., pub allowed_values: Vec<String>, ... }`) to find where
  `resolved_type: Option<SupportedType>` gets turned into the argument's public/serialised fields.

- [ ] **Step 2: Write the failing test**

  Extend `imported_enum_with_identical_cross_module_defs_resolves` (added earlier this session in
  `crates/toolr-core/src/parser/build.rs`) — or add a sibling test next to it — asserting the
  built `Argument`'s new field is populated:

  ```rust
  assert_eq!(arg.enum_module.as_deref(), Some("tools.metrics._common"));
  ```

  (Exact field/accessor name depends on Step 1's findings — use whatever naming convention the
  existing `Argument` struct follows for optional metadata fields, e.g. check how `default:
  Option<String>` is named/typed and mirror it.)

- [ ] **Step 3: Run test, verify it fails**

  Run: `cargo test -p toolr-core --lib parser::build::tests::imported_enum -- --nocapture`
  Expected: compile error, field doesn't exist.

- [ ] **Step 4: Implement the field + serialisation + schema version bump**

  Add the field to the `Argument` struct, populate it at the site found in Step 1 (pull it from
  `SupportedType::Enum { module, .. }`, peeling `Optional` first the same way the earlier
  `commands.rs` backfill loop already does), and bump both:
    - `RUNNER_SCHEMA_VERSION` in `crates/toolr-core/src/execute/spec.rs`
    - `SCHEMA_VERSION` in `crates/toolr-py/python/toolr/_runner.py`

  Update each constant's doc comment to add this change to its "requires a bump" list, per
  CLAUDE.md's instruction.

- [ ] **Step 5: Run tests, check for a byte-diff regression against the committed reference
  manifest**

  Run: `cargo test -p toolr-core --lib`
  Expected: `build_fragment::tests::serialised_fragment_matches_committed_bytes` will likely FAIL
  now, since the reference `examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json`
  doesn't have the new field yet (or has it as absent/null and the byte comparison is strict).
  Regenerate that fixture — check `CLAUDE.md`/`CONTRIBUTING.md` for the exact regen command (likely
  something under `cargo xtask` or a `toolr pre-commit` subcommand; grep
  `serialised_fragment_matches_committed_bytes`'s own doc comment and neighbouring code for how
  it's normally kept in sync) and commit the regenerated fixture alongside this change.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/toolr-core/src/execute/spec.rs crates/toolr-py/python/toolr/_runner.py \
          crates/toolr-core/src/manifest.rs crates/toolr-core/src/parser/build.rs \
          examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json
  git commit -m "feat(manifest): carry an Enum argument's defining module, bump schema version"
  ```

---

## Task 9: Python runtime — `localns`-based coercion, TYPE_CHECKING-safe

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py`
- Test: wherever this file's existing pytest suite lives — grep for its test module

  Run: `grep -rln "_coerce_args\|_runner" --include='*.py' -r . | grep -i test`

**Interfaces:**

- Consumes: the manifest's new `enum_module` field (Task 8) as delivered into the Python side's
  in-memory command/argument representation — check how the Rust-built manifest's JSON reaches
  `_runner.py` (likely already deserialised into some `dict`/`msgspec.Struct` before
  `_coerce_args` runs; find that structure and add the new field there too if it's a typed
  `Struct`, not just a raw dict).

- [ ] **Step 1: Write the failing test**

  In the located test file:

  ```python
  def test_coerce_args_resolves_type_checking_only_enum(tmp_path, monkeypatch):
      """A TYPE_CHECKING-only enum import must still coerce correctly —
      the runtime must not depend on the guard having executed for real.
      """
      pkg_dir = tmp_path / "pkg"
      pkg_dir.mkdir()
      (pkg_dir / "__init__.py").write_text("")
      (pkg_dir / "_common.py").write_text(
          "import enum\n"
          "class Environment(enum.StrEnum):\n"
          "    PRODUCTION = 'production'\n"
          "    STAGING = 'staging'\n"
      )
      (pkg_dir / "user.py").write_text(
          "from typing import TYPE_CHECKING\n"
          "if TYPE_CHECKING:\n"
          "    from ._common import Environment\n"
          "\n"
          "def analyse(env: 'Environment' = None):\n"
          "    return env\n"
      )
      monkeypatch.syspath_prepend(str(tmp_path))
      import importlib
      user = importlib.import_module("pkg.user")

      positional, keyword = _coerce_args(
          user.analyse,
          {"env": "staging"},
          enum_modules={"env": "pkg._common"},
      )
      assert keyword["env"].value == "staging"
  ```

  The exact `enum_modules` parameter shape/name depends on how the manifest data actually reaches
  `_coerce_args` today (find its current call site and signature first — this test's call must
  match reality, not an assumed shape). Adjust before writing the implementation.

- [ ] **Step 2: Run test, verify it fails**

  Run: `uv run pytest <path>::test_coerce_args_resolves_type_checking_only_enum -v`
  Expected: FAIL — either a `NameError` bubbling out of `get_type_hints`, or (if the existing
  blanket `except Exception: hints = {}` swallows it) a coercion failure because `env` was never
  converted to the enum member (stays the raw string `"staging"`, so `.value` access fails with
  `AttributeError`).

- [ ] **Step 3: Implement**

  In `_coerce_args`, before the `get_type_hints` call:

  ```python
  localns: dict[str, Any] = {}
  for name, module_path in (enum_modules or {}).items():
      try:
          module = importlib.import_module(module_path)
      except ImportError:
          continue  # let get_type_hints's own NameError surface normally
      cls_name = ... # the class name, not the parameter name — check
                     # exactly what enum_modules carries per Step 1's
                     # real shape (module path keyed by *parameter name*
                     # isn't enough on its own if the annotation's class
                     # name differs from the parameter name, which it
                     # always does — this needs the class name too, e.g.
                     # `enum_types: dict[str, tuple[str, str]]` mapping
                     # parameter name -> (module, class_name))
      localns[cls_name] = getattr(module, cls_name, None)

  try:
      hints = get_type_hints(target, localns=localns or None, include_extras=False)
  except Exception:  # noqa: BLE001
      hints = {}
  ```

  Add `import importlib` at the top of the file if not already present.

  **Design correction needed here**: the parameter-name-keyed shape sketched in Step 1's test is
  insufficient — `localns` must be keyed by the **annotation's class name** (e.g. `"Environment"`),
  not the parameter name (`"env"`), since `get_type_hints` looks up names as they appear in the
  annotation source, not by parameter. Whatever new manifest field/Python-side structure Task 8
  produces must carry (parameter name -> (module, class_name)) or just a flat
  (class_name -> module) map per function, and this task's implementation must key `localns` by
  `class_name`. Revise the test in Step 1 to match once this is nailed down — don't leave the
  mismatch in place.

- [ ] **Step 4: Run test, verify it passes**

  Run: `uv run pytest <path>::test_coerce_args_resolves_type_checking_only_enum -v`
  Expected: PASS.

- [ ] **Step 5: Write and pass a second test for the "one bad annotation doesn't kill the whole
  function's hints" regression**

  ```python
  def test_coerce_args_other_params_still_coerce_when_one_annotation_unresolvable(tmp_path, monkeypatch):
      """A parameter whose annotation can't be resolved at all (no
      TYPE_CHECKING entry provided either — a genuine gap) must not
      silently disable coercion for the function's *other*, perfectly
      resolvable parameters.
      """
      ...
      positional, keyword = _coerce_args(fn, {"count": "3", "broken": "whatever"}, enum_modules={})
      assert keyword["count"] == 3  # still coerced to int despite `broken`'s
                                     # annotation being unresolvable
  ```

  This may require `get_type_hints`'s failure to be caught *per-parameter* rather than for the
  whole function — if `localns` alone doesn't fully close this gap (it won't, for a parameter
  genuinely missing rather than TYPE_CHECKING-guarded), the fallback needs to change from "hints =
  {}" to something that still returns the hints for parameters that resolved via other means.
  Check whether Python's `get_type_hints` supports partial failure at all before assuming a design
  here — if it doesn't (it raises for the whole call), the practical fix is: catch the
  `NameError`, and retry with the offending name added to `localns` as an unresolvable sentinel
  the coercion loop treats as "no type info for this one," e.g. a small
  retry-with-shrinking-localns loop, or falling back to per-parameter `eval` of each annotation
  string individually when the bulk call fails. This is real, non-trivial additional design work
  — don't hand-wave it; if it can't be resolved within this task's scope, split it into its own
  follow-up task and say so explicitly rather than shipping a half-fix.

- [ ] **Step 6: Run the full Python test suite**

  Run: `uv run pytest`
  Expected: all pass.

- [ ] **Step 7: Commit**

  ```bash
  git add crates/toolr-py/python/toolr/_runner.py <test file>
  git commit -m "fix(runner): resolve TYPE_CHECKING-only enum imports via localns at coercion time"
  ```

---

## Task 10: End-to-end regression test + docs

**Files:**

- Modify: `crates/toolr/tests/*` (find the existing integration test file that runs the built
  `toolr` binary against a fixture `tools/` tree via `assert_cmd` — grep for
  `build_static_manifest` or `manifest rebuild` usage in `crates/toolr/tests/`)

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Write an integration test reproducing the exact #454 shape end-to-end**

  Reuse the fixture shape from `imported_enum_with_identical_cross_module_defs_resolves`
  (`crates/toolr-core/src/parser/build.rs`), but this time drive it through the actual `toolr`
  binary (`assert_cmd::Command`) running `toolr project manifest rebuild` against a temp `tools/`
  directory, asserting exit code 0 and the manifest file's `enum_module`/`allowed_values` fields
  are populated correctly. Additionally cover:
    - the relative-import variant (`from ._common import Environment`, not the absolute form used
  in the existing unit test)
    - a `TYPE_CHECKING`-only variant, asserting the build succeeds (not just that resolution
  doesn't error — assert the specific manifest fields are present)
    - a star-import variant, asserting exit code 2 and the specific "star import" message text
    - a dotted-attribute-chain variant, asserting exit code 2 and the specific "please import
  directly" message text

- [ ] **Step 2: Run test, verify it fails until every prior task is done**

  Run: `cargo test -p toolr --test '*' <test name> -- --nocapture`

- [ ] **Step 3: Run test, verify it passes**

- [ ] **Step 4: Write the `UNRELEASED.md` entry**

  Replace/extend the entry added earlier this session (for the narrower dedupe fix) with the
  final, complete description covering: cross-module enum import resolution (relative imports,
  `__init__.py` re-exports, aliasing, `try/except`), the `Optional`-wrapped `allowed_values` fix,
  and `TYPE_CHECKING`-guarded imports now working end-to-end (not just building).

- [ ] **Step 5: Run the full workspace suite**

  Run: `mise run test`
  Expected: all green — this is the umbrella command (skill-refs drift gate + `cargo test
  --workspace` + `pytest`) per this repo's own `CLAUDE.md`.

- [ ] **Step 6: Archive the spec, as the final commit**

  ```bash
  git mv specs/2026-08-20-command-symbol-resolution-design.md specs/archive/2026/
  git mv specs/2026-08-20-command-symbol-resolution-plan.md specs/archive/2026/
  git add UNRELEASED.md crates/toolr/tests/
  git commit -m "test(toolr): end-to-end coverage for cross-module enum resolution"
  ```

---

## Self-Review

**Spec coverage:**

- Same-module / explicit-import-only invariant → Task 4 (`resolve_def`), Task 6 (rejecting the
  two disallowed shapes) — covered.
- Aliasing (`as`) → Task 1 — covered.
- Relative imports → Task 2 — covered.
- Chain-following / `__init__.py` re-export → **fixed in this revision**: Task 4a inserted
  between Task 4 and Task 5, moving `ImportTable` construction into Pass 1 (`all_imports:
  HashMap<String, ImportTable>`) and making `resolve_def` recurse through it with a cycle/depth
  guard. Task 4a's own step 3 additionally corrects its execution order relative to Task 7 (the
  `(module, values)` return-type change needs to land before Task 4a's tests can compile) — do
  Task 4 → Task 7 → Task 4a → Task 5 → Task 6 → Task 8 → Task 9 → Task 10, not strict numeric
  order.
- `try/except` branch disagreement tie-break → Task 4 — covered (reuses `same_members`).
- `TYPE_CHECKING` detection + runtime `localns` fix → Tasks 3, 7, 8, 9 — covered.
- Star imports / dotted-attribute-chain hard errors → Task 6 — covered.
- `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump → Task 8 — covered.
- `Optional`-wrapped `allowed_values` gap → already fixed on-branch this session, noted in the
  design doc, explicitly out of this plan's task list (would be double-counted otherwise).

**Placeholder scan:** Task 9 Step 5 intentionally documents an open design question (per-parameter
partial `get_type_hints` failure) rather than hand-waving a fake resolution — flagged as
"non-trivial, split out if needed" per the No-Placeholders rule's spirit (an honest "this needs
its own task" beats a fabricated one-liner "fix"). Everything else has concrete code.

**Type consistency:** `ImportTable::from_module`'s signature grows across Tasks 1→2→3
(`(module, path)` → `(module, path, is_package)`) — each task's steps show the full updated
signature at time of writing, not just the diff, so an implementer reading Task 3 alone sees the
3-arg form directly.

**Action required before execution:** insert Task 4a (chain-following across modules, and moving
`ImportTable` construction into Pass 1) as described above. I'm flagging it here rather than
silently rewriting the numbered tasks, since it changes Task 5's "per-module, not cross-file"
claim materially.
