# 1.0 surface cleanup: plan

One commit and one release note per finding, in this order.

1. U-03/U-13. `env.rs::env_truthy`; `trust.rs`, `uv/install.rs`, `main.rs`; `arg(env=)` bool parser in
   `execute_build.rs`; messages in `uv/mod.rs`, `cli.rs`; docs and doc snippets; tests (unit in
   `env.rs`, `install.rs`; integration in `quiet_uv_consent.rs`, `uv_path_relative_entries.rs`; every
   `env_remove("TOOLR_AUTO_INSTALL_UV")` also removes `TOOLR_AUTO_SYNC`).
2. U-10. `installation/install.sh`, `install.ps1`, `action.yml`, workflows, docs, README,
   `toolr-ci-setup` skill, `build_skill_refs/ci_setup.rs`; tests in `tests/installation/`,
   `tests/action_yml/`.
3. U-09. `venv/repo_key.rs`, `cache/meta.rs`, `venv/sync.rs`, `project.rs::finalize_sync`; tests for the
   pinned digest, the serde default, stale-on-old-layout and the stamp after sync.
4. U-06 `arg()`: `_signature.py`, `arg_keywords.rs` hints, parser tests, docs, skills, tests/.
5. U-06 method form: `_decorators.py`, `parser/groups.rs`, `toolr.testing._groups/_shapes`, parity corpus
   case `err_group_method_form`, migrate in-repo users to dotted names, docs.
6. U-05: `toolr/__init__.py`, `_context.py`/`.pyi`, docs (`which`, `warn`, reference index), skill refs
   via `cargo xtask build-skill-refs`, skill prose.
7. Verify: `prek run --all-files`, cargo tests, pytest on 3.11 and newest, patch coverage against
   `perf/startup-latency`, `toolr release-notes check --base perf/startup-latency`.
8. Archive the spec.
