# User-facing polish (audit WP4)

Three small output fixes from the 2026-11-13 audit.

## M-02: one "no project found" path for the built-ins

- Evidence: `dispatch/mod.rs` hands `self_cmd::run_self` and `project::dispatch_project` only the `-q` flag, so the
  `Invocation` (cwd and cached root) is dropped. `project.rs` then finds the root four ways:
  `ProjectCtx` (`current_dir()` + `discover_project_root`), `manifest_rebuild` and `venv_path` by hand, `root_from_path_arg`.
  `build_manifest_resolve.rs` does it a fifth way for `self build-manifest --package`.
- Shape: in an empty directory `toolr project venv path` prints `no toolr project found above ...`,
  `toolr project venv sync` prints
  `locating project root for the tools venv: no toolr project found above ...` (the `NothingToSync::NoProject`
  context), and `self build-manifest --package x` prints `resolving repo root for tools-venv lookup: ...`.
- Fix: the built-ins take `&Invocation`; `Invocation::project_root()` is the one lookup and the one error.
  `NothingToSync::NoProject` carries the discovery text instead of wrapping it, so `venv sync --if-project` still
  recognises it by type and the message is the bare discovery message everywhere. `ProjectCtx` borrows the `Invocation`.
  `ensure_venv_ready` (core) uses the same typed error.
- Tests: an integration test running each built-in that needs a project in an empty directory and asserting the same
  single message; existing `locating project root` assertions updated.

## U-20: keep the parse error in the stale-manifest warning

- Evidence: `bootstrap.rs::warn_and_keep_cache` prints `err.to_string()`, which for an `anyhow` chain is only the outermost
  context (`parsing <file>`). `manifest rebuild` shows the real text, with a byte range (`parser/mod.rs:48`).
- Fix: print the whole chain (`{err:#}`) on the `cause:` line, and have `parse_python_file` report `line L, column C` instead
  of the parser's byte range.
- Tests: `freshness_dispatch.rs` fallback test asserts the cause carries the parser message and
  line/column; a unit test on `parse_python_file` for the position.

## U-18: trim toolr's runner frames from a user traceback

- Evidence: `_runner.py::run` ends with bare
  `traceback.print_exc()`, so the first frames are `_runner.py ... in run` and the `target(ctx, ...)` call.
- Fix: drop the leading frames that live in the `toolr` package when a user frame follows; keep the full traceback when
  every frame is toolr's (toolr itself at fault) and under the existing root `-d/--debug` (spec `verbosity == "verbose"`).
  No new flag.
- Tests: runner tests for the trimmed and the full (`verbose`, and toolr-only) cases, plus an end-to-end subprocess case.
