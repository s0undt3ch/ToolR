# Empty and relative env values: plan

1. Add `xdg_dir_env` in `crates/toolr-core/src/env.rs` with unit tests (unset, empty, relative, absolute).
2. Switch the three `toolr_*_dir` functions in `uv/mod.rs` to it; test that relative values fall back.
3. Switch the `XDG_DATA_HOME` and `XDG_CONFIG_HOME` reads in `run_completion_install` (two lines in `dispatch.rs`);
   integration test in `completion_install_hint.rs`.
4. Add `no_color_requested` in `help.rs` with a unit test.
5. Queue a release note; archive these specs last.
