# Reject symlinked modules under `tools/`: implementation plan

**Goal:** a `*.py` symlink under `tools/` (or a plugin package) fails the
manifest build, the freshness hash and `CommandsTester.discover()` with one
message listing every offender. TAB completion stays silent. Design:
`2026-11-02-symlinked-tools-files-design.md`.

## Tasks

- [ ] **Failing tests first.**
    - `hash.rs`: nested links (`tools/linked.py`, `tools/sub/other.py`) and a
      dangling one are all reported, sorted; a symlinked `tools/.venv`, a
      symlinked non-`.py` directory and a `.py` link inside a pruned directory
      don't error; control characters in a link name are escaped.
    - `parser/build.rs`: `build_commands` (the fragment path) names the package
      directory in paths and hint.
    - `complete/tests.rs`: `resolve_manifest_at_tab` returns the error with and
      without a cached manifest.
    - `crates/toolr/tests/cli_smoke.rs`: the repro tree fails `toolr --help`,
      `toolr demo hello` and `toolr self build-manifest`.
    - `crates/toolr/tests/complete_smoke.rs`: `__complete` on the repro tree
      exits 0, prints nothing on stderr, offers built-ins and no `demo`.
    - `tests/registry/test_integration.py`: `discover()` raises `ToolrError`
      with the same text, before importing anything.
    - `tests/e2e/`: the binary's stderr and the `discover()` message carry the
      same body.
- [ ] **Implement.** `list_python_files` returns
  `Result<Vec<PathBuf>, SymlinkedModules>`; `hash_tools_dir` propagates it;
  `BuildError` gets a transparent `SymlinkedModules` variant. Python: a
  pre-import walk in `toolr.testing._discovery` mirroring `list_python_files`.
  Commit as `fix(parser)!: reject symlinked modules under tools/`.
- [ ] **Docs.** `docs/concepts.md`, `docs/internals/manifest.md`,
  `docs/writing-commands/testing.md`.
- [ ] **Release note** starting `**Breaking:**`.
- [ ] **Verify.** cargo tests, pytest, clippy, prek, mkdocs `--strict`, patch
  coverage against `origin/main`.
- [ ] **Archive** both spec files under `specs/archive/2026/`.
