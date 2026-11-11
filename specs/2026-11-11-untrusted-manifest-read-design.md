# Don't read the manifest cache in an untrusted repository

Audit finding U-16 (2026-11-09). Decided 2026-11-11: keep the cache in `tools/`, but stop reading it
until the repository is trusted.

## Why

`tools/.toolr-manifest.json` is a cache. toolr already refuses to *write* it into an untrusted
repository, but it still *reads* it there. Whoever controls the repository controls `tools/` too, so
they can commit a manifest whose freshness hash matches the tree. toolr then serves it as-is. The
result: `--help`, bare `toolr` and tab completion show whatever command list, help text and argument
choices the file claims, not what `tools/*.py` declares.

Nothing runs this way, because dispatch still asks for trust first. The harm is a misleading command
list before trust. The value of the change is a simpler rule: **in an untrusted repository, toolr
believes nothing it cached in the tree, and parses the source instead.** That rule is easier to explain
and audit than "never written, but read if present".

Moving the cache out of the tree (the finding's original fix) was rejected: effort L, and the path is
already public through `.gitignore` templates and CI recipes.

## Behaviour

| Repository | Read the cache | Write the cache |
|---|---|---|
| Trusted, normal run | yes | yes, when stale (unchanged) |
| Trusted, tab completion | yes | no (unchanged) |
| Untrusted, any run | **no** (new) | no (unchanged) |
| Any, `toolr project manifest rebuild` | n/a, always rebuilds | yes (unchanged) |

"Trusted" means what `toolr_core::trust::is_trusted_now` says: `TOOLR_TRUST_ALL` is on, or the trust
store holds a record for the repository. It never prompts, writes or prints.

In an untrusted repository every run builds the manifest in memory from `tools/*.py` (a pure-Rust
parse that runs no repository code), exactly as a missing cache does today.

### Consequences to accept

- **Speed.** Untrusted runs pay for the parse on every run. Measured on the main release binary
  (2026-11-11): about +3 ms per Tab and per `--help` for this repository's `tools/` (8 modules, 2.4k
  lines), about +11 ms for a 14k-line `tools/`. Roughly 0.8 ms per 1,000 lines. Trusted runs are
  unchanged apart from the trust check Tab now makes (see below).
- **Tab loses plugin commands before trust.** Tab doesn't glob the venv; today it carries plugin
  entries forward from the cache. With no cache read, an untrusted Tab shows only the commands in
  `tools/*.py`. Normal runs still glob the venv, so `--help` keeps them.
- **No fallback on a broken module before trust.** Today a failed rebuild serves the stale cache. In an
  untrusted repository there is no cache to fall back to, so a module that fails to parse leaves its
  commands out, with the existing warning.
- **`project manifest rebuild` stays trust-free** (`docs/concepts.md` lists it as a command that needs
  no trust because nothing executes). In an untrusted repository it writes a file that toolr ignores
  until the repository is trusted. That's harmless; the command reports what it wrote as today.

## Where the cache is read today

Every read goes through `toolr_core::manifest::load_manifest`, from two call sites:

1. `manifest_build::rebuild::refresh` (`check` closure). Called by
   - `toolr::bootstrap::ensure_manifest`, which already computes `is_trusted_now` to pick
     `Persist::Write` or `Persist::InMemory`;
   - `complete::freshness::resolve_manifest_at_tab`, with `Persist::InMemory` and no trust check.
2. `toolr::main::load_or_empty`, reached when `ensure_manifest` returns `None`. That includes an
   untrusted run whose build failed with no cache (`Refresh::Failed { cached: None }`). **This path must
   be closed too**, or gating `refresh` alone just moves the read here.

`toolr project manifest rebuild` (`rebuild_manifest_full`) never reads the cache.

## Decisions for the builder

- Make the mode explicit in `refresh`'s signature instead of adding a boolean. Replace `Persist` with a
  three-way mode, for example:
    - `ReadWrite`: trusted normal run (today's `Persist::Write`);
    - `ReadOnly`: trusted Tab (today's `Persist::InMemory`);
    - `Untrusted`: no read, no lock, no write.

  Names are the builder's call; the three behaviours are not.
- `resolve_manifest_at_tab` calls `is_trusted_now` on the discovered project root. It creates nothing
  and reads one small record file. Measure Tab in a trusted repository before and after, and state
  the number in the PR. If it adds more than 1 ms, stop and report rather than optimise.
- Update the `bootstrap::ensure_manifest` doc comment, which says Tab never reads the trust store.
- `load_or_empty` must not read the file in an untrusted repository; it returns an empty manifest
  there. Choose the cleanest way (pass the trust decision down from `run`, or have `ensure_manifest`
  return an empty manifest itself for the untrusted no-cache failure).
- `drop_unsafe_names` still runs on every manifest, cached or built.

## Tests

Integration tests in `crates/toolr/tests/`, through the real binary, with an isolated trust store:

1. **Planted manifest is ignored before trust.** Build the manifest in a trusted copy
   (`TOOLR_TRUST_ALL=1`), then add a command to the JSON that no module declares. Untrusted:
   `toolr --help` and `toolr __complete <root> ""` do not list it. Trusted: both do. This is the
   regression test for the finding.
2. **Broken module, planted manifest, untrusted.** A module that fails to parse, plus a planted
   manifest. The run warns and does not show the planted commands (covers `load_or_empty`).
3. **Untrusted runs still write nothing** (existing behaviour; keep or extend the current test).
4. **Trusted Tab still serves a fresh cache without rebuilding** (existing behaviour).

Unit tests in `rebuild.rs` for the new mode: `Untrusted` never calls `load_manifest` (an unreadable
manifest file produces no `unreadable` error) and never writes.

## Docs and release note

- `docs/concepts.md`, the paragraph starting "toolr doesn't write `tools/.toolr-manifest.json`": say
  toolr neither reads nor writes it in an untrusted repository, and that Tab shows plugin commands only
  after trust.
- Release note: in an untrusted repository toolr now ignores a committed or leftover
  `tools/.toolr-manifest.json` and parses `tools/` instead; Tab there no longer offers plugin commands
  until the repository is trusted. Not breaking.

## Out of scope

- Moving the cache out of `tools/`.
- Reading plugin fragments from an untrusted repository's in-tree venv (`tools/.venv`). Normal runs
  glob it today whether or not the repository is trusted. Same class of issue; raise it separately if
  wanted.
- `toolr self build-manifest`, which writes a plugin's own `toolr-manifest.json`, not this cache.
