# Start-up latency: plan

1. X-01. `crates/toolr-core/src/cache/{enumerate,hint,mod}.rs`, `crates/toolr/src/main.rs`.
   Tests: `cache/tests.rs` (hint ignores size, unsized scan), `main.rs` unit tests for the flag
   scan, `crates/toolr/tests/cache_hint_quiet.rs` (`--version` / `--help` silent).
2. X-02. `toolr/__init__.py`, `_context.py`, `_runner.py`, `types/__init__.py`.
   Tests: `tests/runner/test_lazy_imports.py` (subprocess checks `sys.modules`).
3. U-04. `_context.py`. Test in `tests/context/test_prompt.py` runs a real subprocess with
   closed and empty stdin.
4. Release notes for faster start and the prompt message. Archive these specs last.
