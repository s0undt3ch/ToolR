# 1.0 surface cleanup: design

Pre-1.0 is the cheapest moment to settle names, env-var rules and deprecated spellings. This package
removes or fixes six things so that 1.0 itself changes nothing. Every removal is a breaking change with
its own release note and the replacement spelled out. It builds on the startup-latency branch (lazy
`__getattr__` for `toolr.__version__`); that is left as it is.

## U-03, U-13: one boolean rule for env vars, `TOOLR_AUTO_SYNC`

- `toolr_core::env::env_truthy(name)`: `1`, `true` or `yes`, case-insensitive, surrounding whitespace
  ignored; anything else, including unset, empty and `0`, is false.
- Used by `TOOLR_TRUST_ALL`, `TOOLR_AUTO_INSTALL_UV` and `TOOLR_NO_CACHE_HINT`. The last one used to be
  "set to anything", so `TOOLR_NO_CACHE_HINT=0` disabled the hint; it now leaves it on. Noted separately.
- Every `bool` value parsed from text (positional, list or tuple element, `--opt=VALUE`, `arg(env=)`) accepts
  `1/true/yes` and `0/false/no` through `parse_bool`, instead of only `true/false`. Anything else is still an
  error, and `--help` no longer lists `[possible values: true, false]`. Noted in its own release note.
- `TOOLR_AUTO_SYNC=1` is the documented name for "answer the sync/uv-install prompts with yes".
  `TOOLR_AUTO_INSTALL_UV` stays as an alias, removed in 1.0. Once `TOOLR_AUTO_SYNC` is set at all it decides
  alone (empty or unparsable is off); the alias counts only while it is unset. Both feed the same
  `ConsentMode` field. The user-facing messages name the new variable.
- One table of the `TOOLR_*` variables lives in `docs/project-config.md`; other pages link to it.

## U-10: one `--verify=` spelling

`--verify=full|auto|checksum|none` in `install.sh`, `-Verify` in `install.ps1`, `verify:` in the action.

| value | SHA-256 | attestation | old spelling |
| --- | --- | --- | --- |
| `full` (default) | yes | required | `--verify-attestation=require` |
| `auto` | yes | if `gh` is available | `--verify-attestation=auto` |
| `checksum` | yes | skipped | `--verify-attestation=skip` |
| `none` | skipped | skipped | `--no-verify` plus `skip` |

The values mean the same everywhere. The action always checks the SHA-256, so it offers only `full`
(default) and `checksum`; a `validate-inputs` step rejects `none`, `auto` and anything else with a message.
The old `skip-attestation: true` maps to `checksum` and gains a `deprecationMessage`. The old spellings keep
working, undocumented, until 1.0; `--no-verify` stays a separate legacy switch because it skipped only the
SHA-256. `install.sh` validates `--verify=` right after parsing and resolves "check the SHA-256?" once into
`CHECK_SHA256`; `install.ps1` does the same with `$CheckSha256`.

## U-06: remove the deprecated surface

- `arg()` loses `required`, `action`, `choices`, `nargs`, `group`; `ArgumentAnnotation` stays and only those
  fields go, along with `NargsType` and `_deprecated`. The static parser stops accepting them; each gets a build-time hint naming
  the replacement (as `must_exist` already does) rather than a bare unknown-keyword error. The argparse
  scanner's own `nargs`/`choices`/`action` are unrelated and stay.
- `CommandGroup.command_group()` (method form) goes. The static parser binds `command_group` only when
  called as a name that resolves to toolr or as `<toolr module>.command_group`. Any other module-level
  `<x>.command_group(...)` is reported as an unsupported shape pointing at `command_group("parent.child",
  ...)`, so a manifest build fails loudly instead of listing a group whose import then raises
  `AttributeError`. `toolr.testing` mirrors the rule. Pattern 1 of the parent resolution is deleted.
- `ToolrDeprecationWarning` and the runner's filter stay: the class is still importable by users who
  filter on it, and costs nothing.
- `docs/migration.md` stays, reworded as "removed", and lists only the method form: `parent=` still works.
  The `project deps` shim is not part of this package.

## U-05: trim `toolr.__all__`

- Dropped from `toolr.__all__` and from the top-level imports: `report_on_import_errors` (no scaffold,
  doc or in-repo `tools/` uses it, so the module is deleted), `MANIFEST_SCHEMA_VERSION` (a constant, still in
  `toolr._decorators` for the lock-step test) and `DispatchCommand` (every doc imports it from
  `toolr.sources`). `ArgSection` stays.
- `Context.run` takes the named arguments `command.run` accepts: `cwd`, `env`, `input`, `text`,
  `encoding`, besides the existing ones, all `None` when unset and forwarded only when set. `**kwargs`
  goes, so a typo is a `TypeError`.
- `which` and `warn` get documented in `docs/` and the reference index.
- Skill references are regenerated; skill prose is searched for removed names.

## U-09: the venv cache key no longer carries the major version

`compute_repo_key` hashed `CARGO_PKG_VERSION_MAJOR`, so bumping to 1.0 would orphan every venv. The key
now hashes the literal `"0"` in that slot. Today's major is 0, so every existing key is byte-identical:
nothing is orphaned by this release, and nothing will be at 1.0. A test pins a golden key.

The old key did double duty, letting a major release change the venv layout safely. `meta.json` takes
that job: a new `layout_version` field (serde default 1, so every existing sidecar reads as the current
layout; schema version unchanged; older toolr builds ignore the unknown field). It is written on every sync and
not read yet: a freshness check for it was dropped, as it adds a file read per check and cannot fire until the
constant is bumped. A future layout change adds the check with that bump.
Bumping the constant is the lever for a future layout change; a change that needs a fresh venv rather than
an in-place `uv sync` has to remove it in the sync path. That is out of scope until one exists.

Old-key entries, if a future key change ever orphans them, are cleaned by `toolr self cache prune`
(orphan = repo path gone or key unreachable), which is the existing mechanism; no offer to prune is added.

## Non-goals

`self trusted` (U-14), `ArgSection`, the `project deps` shim, a venv wipe on layout change, any change to
the runner JSON spec (`RUNNER_SCHEMA_VERSION` is untouched; the removed kwargs never reached it).
