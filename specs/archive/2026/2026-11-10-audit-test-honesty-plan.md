# Plan: test infrastructure honesty

1. `complete_shells.rs`: `extra_bash_candidates`, `version_key`, cached `bashes()`, failing `a_bash_4_4_or_newer_was_found`. Check with
   `cargo test -p toolr --test complete_shells` on stock `PATH`.
2. `complete_protocol_lockstep.rs`: two text-reading tests.
3. `tests/tools/precommit/workflows/`: gate tests; add `ruamel.yaml` to `dev` and `pre-commit`, `uv lock`.
4. `_rust_utils.pyi` parameter order; parametrise stubtest; allowlist file.
5. Hasher lookup without mise shims; mkdocs skip off CI.
6. CONTRIBUTING note. Verify with `prek run --all-files`, pytest on 3.11 and newest, the two cargo suites.
