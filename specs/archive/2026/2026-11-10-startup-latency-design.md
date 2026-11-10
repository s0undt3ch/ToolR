# Start-up latency

## Why

Two start-up costs hit every command, and one prompt failure looks like a toolr crash.

- **X-01**: the passive cache hint (`maybe_emit_cache_hint_from_argv`) walks every cached venv
  to sum sizes before every non-quiet command, `--version` included.
- **X-02**: the Python runner imports `importlib.metadata`, `packaging` and `rich.prompt`
  eagerly, though most commands use none of them.
- **U-04**: `ctx.prompt` with no terminal (EOF on stdin) dies with an `EOFError` traceback.
  U-04 moved here from the product work package because it edits the same `_context.py`.

## Changes

### X-01: orphan-only hint, skipped for `--version` / `--help`

- `compute_hint` counts orphan entries (repo path gone) from the `meta.json` sidecars only. The
  size threshold and every size in the message go. `HintConfig` keeps `orphan_threshold` (10).
- `enumerate_caches_unsized` is the same scan without the per-entry directory walk.
  `enumerate_caches` keeps sizes for `self cache list` / `prune`.
- `--version`, `-V`, `--help`, `-h` (before any `--`) skip the hint. The hint existed to run
  before clap exits on those flags; an instant query is the wrong place for advice.
- Behaviour change: a cache that is only large (no orphans) no longer prints a hint.
  `toolr self cache list` still shows sizes.

Measured on a synthetic cache (5 venvs, 50,005 files), release build, mean of 30 runs
(`--version`) or 20 runs (`project venv sync` outside a project):

| Command | Before | After |
|---|---|---|
| `toolr --version` | 242.6 ms | 6-10 ms |
| `toolr project venv sync` (no project) | 209 ms | 26 ms |

With `TOOLR_NO_CACHE_HINT=1` the old binary took 6.4 ms, so the walk was the whole cost.

### X-02: lazy imports

- `rich.prompt` is imported inside `Context._prompt` (type-only names under `TYPE_CHECKING`).
- `packaging.version` is no longer imported by `toolr._runner`. The coercion hook recognises
  `packaging.version.Version` by module and name, which holds whenever the command's annotation
  imported it.
- `toolr.types.Version` resolves through a module `__getattr__` (PEP 562); type checkers
  still see the import under `TYPE_CHECKING`.
- `toolr.__version__` resolves through a module `__getattr__`, cached on first read.

`import toolr` best of 30 (`python -X importtime`): 51.2 ms to 35.9 ms.
`import toolr._runner`: 44.2 ms to 36.7 ms. `rich.console` stays eager: the runner builds
its consoles on every run.

### U-04: `ctx.prompt` without a terminal

`Context._prompt` catches `EOFError` and exits through the normal error path with a message naming
the prompt and saying input is not available, exit status 1, no traceback. Only the real-stdin path is handled: a stream passed in by
`make_context(prompt_input=...)` keeps raising `EOFError`, which its documented contract and tests rely on.

## Non-goals

- No change to prompts when input is available. No non-interactive flag or env var.
- Not lazy-loading `rich.console`, `msgspec` or the Rust extension.
