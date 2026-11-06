# Empty and relative env values (FU-38)

Follow-up to #689, which made an empty `XDG_*` value read as unset.

## Findings

| Item | Verdict | Evidence |
| --- | --- | --- |
| `NO_COLOR=` disables colour | Bug, fix | `help.rs` used `var_os("NO_COLOR").is_some()`; no-color.org requires a non-empty value |
| `TOOLR_UV_LIBC=` | Not a bug | `detect_linux_libc_from` only matches `musl`/`gnu`; anything else falls through to auto-detection (test `detect_linux_libc_empty_env_override_falls_through`) |
| Relative `XDG_*_HOME` | Bug, fix | `non_empty_env_path` returns it, so paths resolve against the cwd; the XDG spec says to ignore it, and `dirs` 7 does |

### NO_COLOR in Python

Rich computes `no_color` as `NO_COLOR` set to a non-empty string, so an empty value already keeps colour
(checked against the pinned rich). toolr only passes `no_color=True` from the explicit `setup_no_colors`. No change.

## Decisions

- `toolr_core::env::xdg_dir_env(name)` returns the value only when it is non-empty and absolute.
  `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` and `XDG_CONFIG_HOME` use it (`uv/mod.rs`, and the
  two reads in `run_completion_install`).
- `HOME`, `HOMEBREW_PREFIX` and `TOOLR_BASH_COMPLETION_SYSROOT` stay on `non_empty_env_path`. No spec says to
  reject a relative `HOME`, and `dirs::home_dir` does not either; whether toolr should is a product call, left open.
- `help.rs` gains `no_color_requested`: present and non-empty.
