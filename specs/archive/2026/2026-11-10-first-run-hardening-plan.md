# First-run hardening: plan

1. S-03: `crates/toolr/src/project.rs` print after trust; unit test captures nothing, so add an
   integration test in `crates/toolr/tests/project_init.rs` asserting stderr.
2. S-04: `crates/toolr/src/self_cache.rs`, `crates/toolr-core/src/discovery.rs`,
   `crates/toolr/src/dispatch.rs`; test with an ESC-named directory for the cache list and the
   discovery error.
3. U-08: `crates/toolr/src/main.rs`; integration test in `crates/toolr/tests/project_discovery.rs`.
4. X-03: `crates/toolr-core/Cargo.toml`, `Cargo.lock`, `discovery.rs`; Windows-only unit test.
5. X-05: `crates/toolr-core/src/execute/spawn.rs`; tests spawn a tiny child that prints the env.
6. Release notes for S-03, U-08, X-03, X-05; `prek run --all-files`; tests; patch coverage;
   archive the specs.
