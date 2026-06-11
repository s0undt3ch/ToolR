# Static-only manifest: never execute repository code to discover commands

**Date:** 2026-06-10
**Status:** Proposed (brainstorming approved)
**Supersedes runtime behaviour from:** `specs/archive/2026/rust-front-end/07-plan-6-dynamic-manifest.md`
**Closes:** audit finding SEC-01 (and subsumes SEC-03); large simplification of the dynamic layer.

## Problem

`toolr --help`, bare `toolr`, and first-run-in-a-repo can execute attacker-controlled code
*before the user has chosen to run any of the repo's tools*. Two mechanisms, one root cause —
the pre-clap bootstrap builds the manifest by **executing Python**:

1. **Repo-relative interpreter.** `should_skip_auto_rebuild` deliberately returns `false` for
   `--help`/`-h`/bare-`toolr` (`crates/toolr/src/bootstrap.rs:71-75`). Bootstrap then resolves
   the venv interpreter and spawns it if `is_file()` (`bootstrap.rs:44-53`). For
   `venv-location = "in-tree"` that interpreter is the repo-relative `tools/.venv/bin/python`
   (`crates/toolr-core/src/venv/resolve.rs:52-66`). A malicious repo commits a fake
   `tools/.venv/bin/python` (+ `venv-location = "in-tree"`, no manifest) and gets code
   execution on `toolr --help`.
2. **Importing every tool module.** Even with a legitimate interpreter,
   `rebuild_manifest_full` runs `python -m toolr._introspect`, which imports every `tools.*`
   module (`crates/toolr-py/python/toolr/_introspect.py:54`), executing import-time code. The
   generated `.gitignore` excludes `.toolr-manifest.json`, so a fresh clone always hits this.

Users reasonably treat `--help` as side-effect-free. It is not.

## Key facts established during design

- The dynamic layer is **add-only**: `merge_dynamic` keeps the static definition on any
  name conflict and only appends commands/groups the static parser missed
  (`crates/toolr-core/src/dynamic/merge.rs`). It also captures those with `"arguments": []`
  (`_introspect.py:115-118`) — i.e. in a degraded, arg-less form.
- **Third-party command sources are static, not dynamic.** They come from globbing
  `<venv>/.../site-packages/*/toolr-manifest.json` via `build_static_manifest_with_venv`
  (`crates/toolr-core/src/dynamic/rebuild.rs:38`), which reads JSON and executes nothing.
  Dropping the dynamic layer does **not** touch the plugin story.
- **toolr dogfoods nothing dynamic.** `tools/ci.py` and `tools/version.py` use idiomatic
  `command_group(...)` + `@group.command`, fully statically parseable. The example plugin
  uses the static third-party-manifest glob.
- A per-repo, **out-of-repo** sidecar already exists: `cache/meta.rs` writes
  `$XDG_CACHE_HOME/toolr/<repo-key>/meta.json` atomically with `repo_path`, `toolr_version`,
  `python_version`. It is the natural home for interpreter provenance.

## Principle

> **toolr discovers and describes commands only by static analysis. Repository Python
> executes only when the user explicitly dispatches a command.**

Manifest building is execution-free, so every read-only surface (`--help`, completion, bare
`toolr`, listing, first-run) is safe by construction.

## Design

### §1 — Static-only manifest construction

The manifest is built from exactly two execution-free sources:

1. **First-party** — Rust AST parse of `tools/*.py` (`build_static_manifest`). Needs no venv.
2. **Third-party** — glob of `<venv>/.../site-packages/*/toolr-manifest.json`
   (`build_static_manifest_with_venv`). Reads JSON only.

**Removed:**

- `crates/toolr-core/src/dynamic/runner.rs`, `payload.rs`, `merge.rs`, and the
  Python-spawning orchestration in `rebuild.rs`.
- `crates/toolr-py/python/toolr/_introspect.py` and `tests/introspect/*`.
- `Origin::Dynamic` (`crates/toolr-core/src/manifest/model.rs:245`) and
  `PAYLOAD_SCHEMA_VERSION`.
- The Python-spawning bootstrap path (`bootstrap.rs:44-53`). With no expensive/unsafe path
  left, `should_skip_auto_rebuild` collapses: `--help`, bare `toolr`, completion, and
  first-run all take the same cheap static path. (`__version`/built-in fast-paths may remain
  for latency, but no longer for safety.)

**Kept:** `compute_third_party_hash`, the venv glob, and all freshness machinery
(`crates/toolr-core/src/freshness/`, `complete/freshness.rs`) — now purely static.

**Property unlocked:** first-party commands render in `toolr --help` on a fresh clone with
**no venv at all**. Third-party commands appear once a venv exists — still no execution.

`rebuild_manifest_full` is renamed/reduced to a static-only builder (first-party AST +
third-party glob + hashes + write). No `python` argument.

### §2 — Interpreter provenance (closes the committed-`.venv` vector)

Validation happens at **dispatch time**, before spawning the interpreter (not at manifest
time — manifest building no longer spawns anything):

- **Cache venv (default).** Canonicalize the resolved interpreter; require it to live under
  `$XDG_CACHE_HOME/toolr/`. A repo cannot write there → trusted, no marker needed.
- **In-tree venv (`tools/.venv`).** When toolr provisions it (`venv sync`), record the
  canonical interpreter path + a content hash in the out-of-repo `meta.json` (extend
  `cache/meta.rs`; bump its `SCHEMA_VERSION`). Note: `resolve_venv_path` currently leaves
  `repo_key` empty for in-tree mode (`resolve.rs:53`) and in-tree venvs don't touch the cache
  dir today. This design requires computing a repo-key for in-tree repos too — call
  `compute_repo_key(repo_root, python_version)` (a pure function of the canonical repo path,
  independent of venv location) and store provenance at
  `$XDG_CACHE_HOME/toolr/<repo-key>/meta.json`. The venv stays in `tools/.venv`; only the
  provenance record lives in the (un-committable) cache. At dispatch, an interpreter inside
  the repo tree is executed when it matches that record.

  **`validate_venv` fallback (added after a Windows distribution-test regression).** Requiring
  a toolr-written record outright broke the legitimate "build the in-tree venv with `uv`
  directly, then run toolr" workflow (what IDEs, `activate`, and CI rely on, and what the
  plugin-contract distribution test does). So when an in-repo interpreter has no/mismatched
  record, fall back to `validate_venv`: if `tools/.venv` is a *structurally real* toolr venv
  (a `pyvenv.cfg`-style layout containing the installed `toolr` package), accept it; otherwise
  refuse. The cheap committed-fake attack — a `#!/bin/sh` script at `tools/.venv/bin/python`
  with no `toolr` package — fails `validate_venv` and stays refused. Residual (accepted,
  documented): committing a *full real venv* with a trojaned `python` binary would pass — far
  costlier and more conspicuous than the script fake this gate blocks; `toolr project venv
  sync` records provenance and skips the fallback. Refusal message:

  ```text
  toolr: refusing to run <path>: it lives inside the repository and was not provisioned by toolr — run 'toolr project venv sync'
  ```

This builds on `validate_venv` (which checks "has python + has toolr package" —
`crates/toolr-core/src/venv/validate.rs:94-99`), now also used as the in-tree trust fallback.

**Rationale for keeping in-tree:** a predictable `tools/.venv` is what IDEs point their
interpreter at, what `source tools/.venv/bin/activate` needs, and what CI caches by a known
path. Provenance is cheap because the out-of-repo sidecar already exists.

### §3 — The manifest file is a pure, verifiable cache

`.toolr-manifest.json` is treated as a rebuildable cache, never a trusted input:

- Every invocation verifies the manifest's `static_hash` against a freshly computed hash of
  `tools/`. On mismatch → rebuild from AST and overwrite. A committed/doctored manifest whose
  entries don't match the real `tools/` is detected as drift and replaced with the honest
  static parse.
- **Venv appeared ⇒ rebuild.** The venv's state feeds the third-party freshness axis: with no
  venv the manifest carries an empty third-party hash; when the venv is created the hash
  changes → `ThirdPartyDrift` → rebuild picks up plugin commands. Additionally,
  `toolr project venv sync` rebuilds the manifest as its **final step**, so plugin commands
  appear immediately after sync without waiting for the next freshness check.
- This **subsumes SEC-03** (carry-forward trust): with no `Dynamic` origin there is nothing
  untrusted to carry forward — third-party is re-globbed, first-party re-parsed.

### §4 — Static-only contract is documented, not warned

Dropping the dynamic layer means a `tools/*.py` that registers commands non-statically
(`for n in names: group.command(...)`, factory-on-import) produces nothing. **Decision: say
nothing at runtime; document the static-only contract.** No heuristic warning, no hard error
(heuristics risk false positives; the contract is simple to state). Update the
command-authoring docs/skill to state plainly: *toolr registers only statically-declared
commands — `command_group(...)` at module top level and `@group.command` on module-level
functions; commands registered dynamically are not discovered.*

## Error handling

- Missing/stale manifest → rebuild from source (never fail to an empty manifest silently;
  preserve the existing "warn and keep cache on rebuild error" behaviour for the rare
  AST-parse failure, `bootstrap.rs:193-208`).
- In-repo interpreter without a provenance record → hard refuse with the `venv sync`
  remedy message above (do not fall back to executing it).
- No venv yet → manifest = first-party only; third-party commands simply absent until sync.

## Testing

- **Security regression:** a repo with `venv-location = "in-tree"` + committed
  `tools/.venv/bin/python` + no manifest → `toolr --help` spawns **no** Python (assert via no
  `python` exec) and never runs the committed interpreter; `toolr <cmd>` refuses it with the
  sync remedy.
- **Fresh-clone help:** `toolr --help` with `tools/pyproject.toml` and no venv renders
  first-party commands using a static-only build; no Python process.
- **Venv-appeared rebuild:** after `venv sync`, third-party plugin commands appear in
  `--help`/dispatch without a manual rebuild.
- **Doctored manifest:** a committed `.toolr-manifest.json` with entries not matching
  `tools/` is detected as drift and overwritten.
- **Provenance:** cache-venv interpreter (under `$XDG_CACHE_HOME/toolr/`) runs; in-tree
  interpreter runs iff it matches the recorded provenance.
- **Parity:** the static parser already covers idiomatic registration; confirm
  `tools/ci.py`/`version.py` commands are unchanged after the dynamic layer is removed.

## Migration / compatibility

- Existing `.toolr-manifest.json` files: loader must tolerate a legacy `"dynamic"` origin
  (map to dropped/ignored) and `"imports"` keys; first freshness check rebuilds them cleanly.
- `cache/meta.rs` `SCHEMA_VERSION` bump with lenient load of older sidecars (the module
  already silently upgrades older versions).
- Release note: dynamically-registered commands are no longer discovered (was already a
  degraded, arg-less capture). Document the static-only contract.

## Out of scope (tracked separately)

- The other audit items (SEC-02 sys.path, SEC-04 editable-install, SEC-06 terminal escapes,
  supply-chain SEC-08..13) are independent and remain in `audit/2026-06-10/`.
- This design naturally overlaps the simplification of the dynamic layer; the deletions here
  are the security-motivated subset.
