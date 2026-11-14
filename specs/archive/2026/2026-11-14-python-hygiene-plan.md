# Python hygiene plan (audit WP5)

One commit per item, after the specs commit; specs are archived in the last commit.

1. P-02: extend the mypy hook, fix findings in `tools/`, add `tests/tools/precommit/docs/` tests.
   Gate: `prek run mypy --all-files`, pytest.
2. M-04: add the shape test under `tests/runner/`. Gate: pytest; temporarily adding a Rust field must fail it.
3. M-05: move fixtures into `tests/runner/conftest.py`. Gate: `uv run pytest tests/runner` on 3.11 and newest.
4. P-04/U-22/M-08: delete the comment and the two classes after `git grep`. Gate: pytest.
5. U-19: fix the generator, `cargo xtask build-skill-refs`, commit refs. Gate: `cargo xtask build-skill-refs --check`.
6. Archive both specs with `git mv` to `specs/archive/2026/`.

Final gate: `mise run test`, `prek run --all-files`.
