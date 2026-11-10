# Plan: docs drift and release guard

1. Docs pages and CONTRIBUTING fixes (design, U-07 and P-06 lists). Verify: `mkdocs build --strict`.
2. `help.rs`: `positional_label` adds `...` for multi-value positionals; option placeholders fall
   back to the arg id. Tests in `help.rs` (`multi_value_positionals_get_an_ellipsis`,
   `value_options_show_a_placeholder_named_after_the_option`). Regenerate doc snippets.
3. Move `tests/test_{make_context,pytest_plugin}.py` to `tests/testing/`,
   `test_types_{module,static}.py` to `tests/types/`, `test_dogfood_toolr_pin.py` to
   `tests/workflows/`; update references (pyproject ignore, doc snippet includes, comments).
   Add the top-level rule to `tools/precommit/tests_layout.py` with a test.
4. `release.yml` main-only guard step and the recovery section; test in
   `tests/workflows/test_release_main_guard.py`.
5. Release note, then archive the specs.
