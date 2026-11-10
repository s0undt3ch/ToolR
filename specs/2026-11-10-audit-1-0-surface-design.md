# 1.0 surface cleanup: design

Pre-1.0 is the cheapest moment to settle names, env-var rules and deprecated spellings. This package
removes or fixes six things so that 1.0 itself changes nothing. Every removal is a breaking change with
its own release note and the replacement spelled out. It builds on the startup-latency branch (lazy
`__getattr__` for `toolr.__version__`); that is left as it is.

## U-03, U-13: one boolean rule for env vars, `TOOLR_AUTO_SYNC`

- `toolr_core::env::env_truthy(name)`: `1`, `true` or `yes`, case-insensitive, surrounding whitespace
  ignored; anything else, including unset, empty and `0`, is false.
- Used by `TOOLR_TRUST_ALL`, `TOOLR_AUTO_INSTALL_UV` and `TOOLR_NO_CACHE_HINT`. The last one used to be
  "set to anything", so `TOOLR_NO_CACHE_HINT=0` disabled the hint; it now enables it. Noted separately.
- `arg(env=)` on a `bool` flag accepts `1/true/yes` and `0/false/no` instead of only `true/false`.
  Anything else is still an error.
- `TOOLR_AUTO_SYNC=1` is the documented name for "answer the sync/uv-install prompts with yes".
  `TOOLR_AUTO_INSTALL_UV` stays as an alias until 1.0 and is documented as deprecated. Both feed the same
  `ConsentMode` field. The user-facing messages name the new variable.

## U-10: one `--verify=` spelling

`--verify=full|auto|checksum|none` in `install.sh`, `-Verify` in `install.ps1`, `verify:` in the action.

| value | SHA-256 | attestation | old spelling |
| --- | --- | --- | --- |
| `full` (default) | yes | required | `--verify-attestation=require` |
| `auto` | yes | if `gh` is available | `--verify-attestation=auto` |
| `checksum` | yes | skipped | `--verify-attestation=skip` |
| `none` | skipped | skipped | `--no-verify` plus `skip` |

The action has no checksum-only mode of its own (it always checks the SHA-256), so it accepts `full`
(default) and `none` (the old `skip-attestation: true`). The old spellings keep working, hidden from help
and docs, for one release. In the action, `skip-attestation` gains a `deprecationMessage`.

## U-06: remove the deprecated surface

- `arg()` loses `required`, `action`, `choices`, `nargs`, `group`; `ArgumentAnnotation`, `NargsType` and
  `_deprecated` go with them. The static parser stops accepting them; each gets a build-time hint naming
  the replacement (as `must_exist` already does) rather than a bare unknown-keyword error. The argparse
  scanner's own `nargs`/`choices`/`action` are unrelated and stay.
- `CommandGroup.command_group()` (method form) goes. The static parser binds `command_group` only when
  called as a name that resolves to toolr or as `<toolr module>.command_group`. Any other module-level
  `<x>.command_group(...)` is reported as an unsupported shape pointing at `command_group("parent.child",
  ...)`, so a manifest build fails loudly instead of listing a group whose import then raises
  `AttributeError`. `toolr.testing` mirrors the rule. Pattern 1 of the parent resolution is deleted.
- `ToolrDeprecationWarning` and the runner's filter stay: the class is still importable by users who
  filter on it, and costs nothing.
- `docs/migration.md` stays one more release, reworded as "removed in 0.36". The `project deps` shim is
  not part of this package.

## U-05: trim `toolr.__all__`

- Dropped from `toolr.__all__` and from the top-level imports: `report_on_import_errors` (no scaffold,
  doc or in-repo `tools/` uses it), `MANIFEST_SCHEMA_VERSION` (a constant, still in
  `toolr._decorators` for the lock-step test) and `DispatchCommand` (every doc imports it from
  `toolr.sources`). `ArgSection` stays.
- `Context.run` takes the named arguments `command.run` accepts: `cwd`, `env`, `input`, `text`,
  `encoding`, besides the existing ones. `**kwargs` goes, so a typo is a `TypeError`.
- `which` and `warn` get documented in `docs/` and the reference index.
- Skill references are regenerated; skill prose is searched for removed names.

## U-09: the venv cache key no longer carries the major version

`compute_repo_key` hashed `CARGO_PKG_VERSION_MAJOR`, so bumping to 1.0 would orphan every venv. The key
now hashes the literal `"0"` in that slot. Today's major is 0, so every existing key is byte-identical:
nothing is orphaned by this release, and nothing will be at 1.0. A test pins the digest.

The old key did double duty, letting a major release change the venv layout safely. `meta.json` takes
that job: a new `layout_version` field (serde default 1, so every existing sidecar reads as the current
layout; schema version unchanged; older toolr builds ignore the unknown field). `check_freshness` reports
`Stale` when the sidecar's layout is older than `VENV_LAYOUT_VERSION`, and a successful sync stamps the
current value, so the venv is re-synced once. A missing sidecar counts as current (nothing to compare).
Bumping the constant is the lever for a future layout change; a change that needs a fresh venv rather than
an in-place `uv sync` has to remove it in the sync path. That is out of scope until one exists.

Old-key entries, if a future key change ever orphans them, are cleaned by `toolr self cache prune`
(orphan = repo path gone or key unreachable), which is the existing mechanism; no offer to prune is added.

## Non-goals

`self trusted` (U-14), `ArgSection`, the `project deps` shim, a venv wipe on layout change, any change to
the runner JSON spec (`RUNNER_SCHEMA_VERSION` is untouched; the removed kwargs never reached it).
