# Replace chrono with jiff

## Why

Scorecard alert [#11](https://github.com/s0undt3ch/ToolR/security/code-scanning/11) flags
[RUSTSEC-2026-0543](https://osv.dev/RUSTSEC-2026-0543), published 2026-09-14: "The chrono crate is
unmaintained; all versions are affected." No chrono release clears it, so Renovate's OSV alerts
can never open a fix PR. chrono is a direct dependency (`Cargo.toml:20`). Nothing else in the graph
pulls it in.

## Goals

- Remove chrono from `Cargo.lock` so the Scorecard `Vulnerabilities` check clears.
- Keep every on-disk and user-visible format compatible in both directions: a toolr built with
  jiff reads files written by older releases, and older releases read files it writes.

## Non-goals

- No new date/time features (time zones, calendar arithmetic, new `toolr.types`).
- No changes to the runner JSON schema (`RUNNER_SCHEMA_VERSION` / `SCHEMA_VERSION` stay as they
  are).

## Anti-goals

- Any change to how `trust.json` or the venv-cache `meta` sidecar serialise timestamps that an
  older toolr can't parse.
- A user-visible change in which `--flag` values the `DateTime`, `Date` and `Time` types accept,
  unless it is written down in a release note.

## Current use of chrono

| Where | What it does |
|-------|--------------|
| `toolr-core/src/trust.rs:30` | `trusted_at: DateTime<Utc>` is serialised with serde into the trust store |
| `toolr-core/src/cache/meta.rs:42,44` | `created_at` and `last_used_at: DateTime<Utc>` are serialised with serde into the cache sidecar |
| `toolr-core/src/cache/{touch,hint,classify}.rs` | `Utc::now()`, age calculation (`signed_duration_since`), and a stale threshold (`Duration::days`) |
| `toolr/src/self_cache.rs:96` | `human_ago` puts ages into coarse buckets (`num_seconds`, `num_minutes`, `num_hours`, `num_days`) |
| `toolr/src/self_cache_prune.rs:75`, `toolr/src/main.rs:269` | `Utc::now()` |
| `toolr/src/value_parsers.rs:319-342` | clap parsers for `DateTime` (RFC 3339 → UTC string), `Date` (`%Y-%m-%d`) and `Time` (`%H:%M:%S[%.f]`). `_runner.py:386-390` turns those strings back into objects with `fromisoformat`. |
| `toolr-core/src/parser/types/supported.rs:256,263,270` | `validated_by: "clap (chrono …)"` doc strings |
| tests | `Utc.with_ymd_and_hms(...)` fixtures in `cache/tests.rs`, `self_cache.rs` and `self_cache_prune.rs` |

## Approaches

1. **jiff (chosen).** `jiff::Timestamp` does the same job as `DateTime<Utc>`. Its serde
   implementation reads and writes RFC 3339 strings with no per-field attributes.
   `SignedDuration` replaces `chrono::Duration`, and `jiff::fmt::strtime` covers the `%Y-%m-%d`
   and `%H:%M:%S%.f` formats. jiff 1.0.1 is stable 1.x and needs Rust 1.85, well under the
   workspace's 1.99.1 (`Cargo.toml:8`).
2. **time 0.3.** This works too, but its serde output is not RFC 3339 by default. Every timestamp
   field would need `#[serde(with = "time::serde::rfc3339")]`, and one missing attribute would
   silently change the on-disk format, which is the anti-goal above. Its format descriptions are
   also clumsier than strftime strings for the three parsers. More friction, no extra benefit.
3. **Ignore it with `osv-scanner.toml`.** No code changes, but it only hides the warning. An
   unmaintained crate would stay in the trust and cache code paths, and any future chrono
   advisory would have no fix.
4. **std only (`SystemTime` plus hand-written RFC 3339).** Not worth it: the `DateTime` parser has
   to accept arbitrary offsets from user input, and writing that ourselves is worse than taking a
   small, maintained dependency.

## Design

- Workspace dependency: `jiff = { version = "1", default-features = false, features = ["std", "serde"] }`.
  No time-zone database features: everything is UTC or fixed-offset. In `toolr-core` it stays
  `optional = true` and is listed as `dep:jiff` in `full`, the same way chrono is today
  (`crates/toolr-core/Cargo.toml:20,47`).
- `DateTime<Utc>` → `jiff::Timestamp`. `Utc::now()` → `Timestamp::now()`.
- Ages: `now.duration_since(then)` (`SignedDuration`). The stale threshold is
  `SignedDuration::from_hours(days * 24)`. `human_ago` gets its buckets from whole seconds (it
  already reads `num_seconds()` first), so it no longer depends on chrono's `num_days` helpers.
- `DateTime` parser: parse with `s.parse::<Timestamp>()` (RFC 3339, any offset). Write the output
  as RFC 3339 with a `+00:00` offset, so the string passed to Python keeps chrono's
  `to_rfc3339()` shape. `fromisoformat` would accept either form; matching the old shape just
  takes away the question.
- `Date` / `Time` parsers: use `jiff::fmt::strtime` with the current format strings. The existing
  `value_parsers.rs` tests must pass unchanged. Inputs that chrono accepted but jiff rejects, or
  the other way round, each get a test and a release note.
- Doc strings: `"clap (chrono …)"` → `"clap (jiff …)"`.
- Test fixtures: `Utc.with_ymd_and_hms(...)` → a `date(y, m, d).at(h, mi, s, 0)` UTC timestamp,
  or `"…Z".parse::<Timestamp>()`.

## Compatibility tests (must exist before merge)

- An old-format `trust.json` and an old-format cache `meta` file, both written by chrono, parse.
  Use one file with `Z`, one with `+00:00` and one with fractional seconds. `trust.rs:909` already
  has a `"2026-01-01T00:00:00Z"` case to build on.
- Older releases can read what jiff writes: golden-value tests pin the exact strings jiff
  writes for those fields, and the PR shows them parsing with the last chrono-based release.
  chrono is not kept as a dev-dependency for this, because that would leave it in `Cargo.lock`
  and the alert would stay open.
- The `DateTime` value parser's output round-trips through `_runner.py`'s `fromisoformat` on
  Python 3.11 and the newest supported Python.

## Release

Behaviour should not change, so the default is `Release-Notes: skip` with a
`fix(deps): replace unmaintained chrono with jiff` commit. If the `Date`/`Time` parsers end up
accepting different inputs, add a release note instead.

## Left to the builder

- Exact jiff APIs for building `SignedDuration` and formatting the `+00:00` offset.
- Whether `human_ago` keeps its 30-day/365-day approximations exactly. It must: existing tests pin
  them.
- Patch coverage at 99% per `.codecov.yml`.
