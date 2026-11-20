# Replace chrono with jiff — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove chrono from `Cargo.lock` (RUSTSEC-2026-0543, Scorecard alert #11) by switching to jiff, with no change
to on-disk or user-visible formats.

**Architecture:** Pin today's formats with tests while chrono is still in place. Then move the three clap value parsers
to jiff, and in one atomic step change every `DateTime<Utc>` to `jiff::Timestamp`. The atomic step is needed because
`toolr_core::cache::Meta` is shared by `toolr` and its integration tests, so neither crate compiles on its own
half-migrated.

**Tech Stack:** Rust 1.99.1, jiff 1.0.1 (`std` and `serde` features only), serde_json, clap value parsers, Python
`_runner.py` `fromisoformat`.

**Spec:** `specs/2026-11-20-chrono-to-jiff-design.md`

## Global Constraints

- Workspace dependency, exactly: `jiff = { version = "1", default-features = false, features = ["std", "serde"] }`.
- In `toolr-core` it stays `optional = true` and is listed as `dep:jiff` in the `full` feature, where `dep:chrono` is
  today (`crates/toolr-core/Cargo.toml:20,47`).
- No time-zone database features. The only UTC conversion API is `jiff::tz::Offset::UTC` (`to_timestamp`,
  `display_with_offset`).
- `RUNNER_SCHEMA_VERSION` and `SCHEMA_VERSION` don't change.
- After the last task, `chrono` appears nowhere in `Cargo.lock`, and `git grep -n chrono -- crates Cargo.toml` finds
  nothing except the unrelated `run-synchronously` hits.
- Commits use Conventional Commits. No `Co-Authored-By` trailer.
- Run lint and format through `prek`, never `cargo fmt` or `cargo clippy` directly.

## Verified API facts (probed against jiff 1.0.1 and chrono 0.4.45 on 2026-11-20)

| Need | jiff call | Output |
|------|-----------|--------|
| now | `Timestamp::now()` | — |
| serde write | derive on a `Timestamp` field | `"2026-05-11T12:00:00Z"`, `"2026-05-11T12:00:00.123456789Z"`, the same as chrono |
| serde read | derive | accepts `Z`, `+00:00`, `+02:00`, and fractional seconds |
| DateTime parser output | `s.parse::<Timestamp>()?.display_with_offset(Offset::UTC)` | `2026-05-11T12:00:00+00:00`, the same as chrono's `to_rfc3339()` |
| age | `now.duration_since(then)` → `SignedDuration`; `.as_secs()`, `.as_mins()`, `.as_hours()` | — |
| days | `SignedDuration::from_hours(days * 24)` | — |
| arithmetic | `ts - SignedDuration`, `ts -= SignedDuration` | — |
| fixture from fields | `Offset::UTC.to_timestamp(jiff::civil::date(y, m, d).at(h, mi, s, 0)).unwrap()` | (`DateTime::to_zoned` needs tz features: don't use it) |
| whole-second RFC 3339 (integration fixtures) | `ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()` | the same as chrono's `to_rfc3339_opts(Secs, true)` |
| Date | `strtime::parse("%Y-%m-%d", s)?.to_date()?`, `.strftime("%Y-%m-%d")` | — |
| Time | `strtime::parse("%H:%M:%S", s)`, falling back to `"%H:%M:%S%.f"`, then `.to_time()?` and `.strftime("%H:%M:%S%.f")` | — |

The probe found these behaviour differences between the two libraries. Each one gets a test in Task 2:

| Input | chrono | jiff | Python effect |
|-------|--------|------|---------------|
| DateTime `2026-05-11T12:00:00.5Z` | `…00.500+00:00` | `…00.5+00:00` | identical `datetime` (checked with `fromisoformat` on 3.11.15 and 3.14.10) |
| Time `12:34:56.5` | `12:34:56.500` | `12:34:56.5` | identical `time` |
| Time `12:34:60` (leap second) | accepted as `12:34:60` | `12:34:59` | chrono's string makes `fromisoformat` raise `second must be in 0..59`. jiff fixes a latent crash. |
| Date `2026-1-5`, `26-01-05`, `2026-02-30`; Time `1:2:3`, `24:00:00` | same result in both | | none |

## Review Focus

1. **A trust record or cache sidecar written by the current release (chrono)** has to load after the upgrade. Otherwise
   users are asked to re-trust every repo, or every venv is treated as corrupt. Covered in Task 1, before the swap.
2. **A file written after the upgrade, read by an older toolr** (a downgrade, or two versions sharing `~/.cache`). The
   golden serialised strings are pinned in Task 1 and have to survive Task 3 unchanged.
3. **A `DateTime` argument with a non-UTC offset** (`+02:00`) has to reach Python as the same UTC instant. Task 2.
4. **The stale/orphan boundary in `self cache prune`.** An entry exactly `stale_after_days` old counts as stale
   (`age >= threshold`, `classify.rs:52`). The existing classify tests must pass unchanged. Task 3.
5. **`human_ago` buckets** at the 30-day and 365-day edges. The existing `human_ago_picks_the_coarsest_bucket` must pass
   with only its fixture types changed. Task 3.

---

### Task 1: Pin today's timestamp formats (still on chrono)

**Files:**

- Modify: `crates/toolr-core/src/trust.rs` (tests module)
- Modify: `crates/toolr-core/src/cache/tests.rs`

**Interfaces:**

- Consumes: `TrustRecord` (`trust.rs:23`) and `Meta` (`cache/meta.rs`), both serde-derived.
- Produces: golden strings that Task 3 must not change.
- [ ] **Step 1: Add `trust_record_reads_chrono_written_timestamps`.** Build a v2 `TrustRecord` JSON (copy the shape from
      `serde_json::to_value` of a real record, as `list_skips_old_schema_records` at `trust.rs:903` does) once for each
      of `"2026-01-01T00:00:00Z"`, `"2026-01-01T00:00:00+00:00"` and `"2026-01-01T00:00:00.123456789Z"`.
      `serde_json::from_str::<TrustRecord>` must succeed every time. Assert that the first two compare equal on
      `trusted_at`.
- [ ] **Step 2: Add `trust_record_serialises_trusted_at_as_rfc3339_z`.** Take a record, set `trusted_at` to 2026-05-11
      12:34:56 UTC, serialise it, and assert `json["trusted_at"] == "2026-05-11T12:34:56Z"`.
- [ ] **Step 3: Add `meta_reads_chrono_written_timestamps` in `cache/tests.rs`**, the same three inputs as Step 1, built
      from the `loads_v1_sidecar_without_interpreter_fields` JSON shape. Write each one to `meta.json` in a `TempDir`.
      `Meta::load(dir)` must succeed for each.
- [ ] **Step 4: Add `meta_serialises_golden_timestamps`.** Serialise `sample_meta()` and assert
      `created_at == "2026-05-11T12:00:00Z"` and `last_used_at == "2026-05-11T12:34:56Z"`. Then set `last_used_at` to
      `…12:34:56.123456789Z` and assert that exact string comes back.
- [ ] **Step 5: Run** `cargo test -p toolr-core trust:: cache::`. Expected: PASS. These are characterisation tests and
      pass on chrono by design.
- [ ] **Step 6: Commit** `test(core): pin trust and cache timestamp formats before the chrono swap`.

### Task 2: Move the DateTime, Date and Time value parsers to jiff

**Files:**

- Modify: `Cargo.toml` (workspace `[dependencies]`: add `jiff`, keep `chrono` for now)
- Modify: `crates/toolr/Cargo.toml` (add `jiff.workspace = true`)
- Modify: `crates/toolr/src/value_parsers.rs:12,319-342` and its tests module
- Modify: `crates/toolr-core/src/parser/types/supported.rs:256,263,270` (`"clap (chrono …)"` → `"clap (jiff …)"`)
- Modify: `tests/runner/test_runner_coverage.py:396-418` (parametrize table)

**Interfaces:**

- Consumes: nothing from Task 1.
- Produces: `datetime_parser()`, `date_parser()` and `time_parser()` keep their `fn() -> ValueParser` signatures, and
  the values they accept are still `String`.
- [ ] **Step 1: Write the parser tests** in the `value_parsers.rs` tests module, using `build_command_with` and
  `get_one::<String>("v")`:
    - `datetime_parser_normalises_to_utc_offset`: `"2026-05-11T14:00:00+02:00"` → `"2026-05-11T12:00:00+00:00"`, and
      `"2026-05-11T12:00:00Z"` → `"2026-05-11T12:00:00+00:00"`.
    - `datetime_parser_keeps_fractional_seconds`: `"2026-05-11T12:00:00.123456789Z"` →
      `"2026-05-11T12:00:00.123456789+00:00"`.
    - `date_parser_normalises_and_rejects_invalid`: `"2026-1-5"` → `"2026-01-05"`. `"2026-02-30"` and `"not-a-date"` are
      errors.
    - `time_parser_accepts_whole_and_fractional_seconds`: `"12:34:56"` → `"12:34:56"`, `"1:2:3"` → `"01:02:03"`,
      `"12:34:56.5"` → `"12:34:56.5"`. `"24:00:00"` is an error.
    - `time_parser_clamps_leap_second`: `"12:34:60"` → `"12:34:59"`.
- [ ] **Step 2: Run** `cargo test -p toolr value_parsers`. Expected: FAIL on `"12:34:56.5"` (chrono gives `.500`) and on
      the leap second (chrono gives `:60`). Everything else passes.
- [ ] **Step 3: Rewrite the three parser bodies** with the jiff calls in the API table. Keep the existing error message
      prefixes (`expected RFC 3339 datetime, got`, `expected YYYY-MM-DD, got`, `expected HH:MM:SS[.fff], got`). Remove
      the `chrono` import from `value_parsers.rs`.
- [ ] **Step 4: Add Python cases** to the `test_dec_hook_coerces_known_types` parametrize table, with jiff's output shapes:
    - `(dt.datetime, "2026-05-11T12:00:00.5+00:00", dt.datetime(2026, 5, 11, 12, 0, 0, 500000, tzinfo=dt.UTC))`
    - `(dt.time, "12:34:56.5", dt.time(12, 34, 56, 500000))`
    - `(dt.time, "12:34:59", dt.time(12, 34, 59))`
- [ ] **Step 5: Run** `cargo test -p toolr value_parsers`, then
      `uv run --python 3.11 pytest tests/runner/test_runner_coverage.py -k dec_hook` and the same on the newest
      supported Python. Expected: PASS.
- [ ] **Step 6: Queue the release note** with `toolr release-notes add --text "…"`. The note says that
      `toolr.types.Time` no longer passes a leap second (`:60`) through to Python, which used to crash the command, and
      now turns it into `:59`.
- [ ] **Step 7: Commit** `fix(cli): parse DateTime, Date and Time arguments with jiff`.

### Task 3: Move every `DateTime<Utc>` to `jiff::Timestamp` and drop chrono

**Files:**

- Modify: `Cargo.toml` (remove the `chrono` line)
- Modify: `crates/toolr-core/Cargo.toml:20,47` (`dep:chrono` → `dep:jiff`)
- Modify: `crates/toolr/Cargo.toml:19` (remove `chrono`)
- Modify: `crates/toolr-core/src/trust.rs:9,30,264`
- Modify: `crates/toolr-core/src/cache/{meta,touch,hint,classify}.rs`, `cache/tests.rs`
- Modify: `crates/toolr/src/{self_cache,self_cache_prune,main}.rs`
- Modify: `crates/toolr/tests/{cache_fixtures,self_cache_prune,cache_hint_quiet,self_cache_list,cli_smoke}.rs`

**Interfaces:**

- Consumes: Task 1's golden tests, which must pass unchanged.
- Produces, with the types changed:
    - `TrustRecord::trusted_at: jiff::Timestamp`
    - `Meta::{created_at, last_used_at}: jiff::Timestamp`
    - `compute_hint(cache_root: &Path, orphan_threshold: usize, now: jiff::Timestamp) -> Result<Option<String>>`
    - `classify_entries(entries: Vec<CachedVenv>, now: jiff::Timestamp, stale_after_days: u32) -> Classification`
    - `human_ago(now: jiff::Timestamp, then: jiff::Timestamp) -> String`
- [ ] **Step 1: Change the types and call sites** listed above. In `classify_entries`, the threshold is
      `SignedDuration::from_hours(i64::from(stale_after_days) * 24)` and the comparison stays `age >= threshold`.
      Rewrite `human_ago` on `delta.as_secs()`, with days = `secs / 86_400`, keeping the existing bucket edges (60 s, 3
      600 s, 86 400 s, 30 d, 365 d) and divisors (`/30`, `/365`).
- [ ] **Step 2: Port the test fixtures.**
    - `Utc.with_ymd_and_hms(...)` → the `Offset::UTC.to_timestamp(date(..).at(..))` form from the API table.
    - `chrono::Duration::{seconds,minutes,hours,days}(n)` → `SignedDuration::{from_secs,from_mins,from_hours}(n)`, with
      `from_hours(n * 24)` for days.
    - `to_rfc3339_opts(SecondsFormat::Secs, true)` → `strftime("%Y-%m-%dT%H:%M:%SZ")`.
    - Don't edit any assertion values.
- [ ] **Step 3: Run** `cargo test -p toolr-core` and `cargo test -p toolr`. Watch for stalls and poll every 30–60 s.
      Expected: PASS, including Task 1's four golden tests, `human_ago_picks_the_coarsest_bucket`, and every `classify`
      test, with their assertions unchanged.
- [ ] **Step 4: Confirm chrono is gone.** `cargo tree -i chrono` prints "did not match any packages".
      `grep -c '^name = "chrono"' Cargo.lock` prints `0`.
- [ ] **Step 5: Run** `prek run --all-files`. This includes the lean `toolr-core`/`toolr-py` clippy pass, which checks
      that `dep:jiff` sits behind `full`. Expected: PASS.
- [ ] **Step 6: Commit** `fix(deps): replace unmaintained chrono with jiff` with a `Release-Notes: skip` trailer,
      because Task 2 already queued the only user-visible note.

### Task 4: Verify, check coverage, and archive

**Files:**

- Move: `specs/2026-11-20-chrono-to-jiff-{design,plan}.md` → `specs/archive/2026/`
- [ ] **Step 1: Run** `mise run test`. Expected: PASS. This covers the skill-refs drift gate, `cargo test --workspace`
      and pytest.
- [ ] **Step 2: Check patch coverage.** Run the `cargo llvm-cov` command from `CLAUDE.md` § Coverage, then
      `cargo xtask patch-coverage --base main`. Expected: ≥ 99%. Close any gap with a real test, not a mock.
- [ ] **Step 3: Run** `toolr release-notes check --base main`. Expected: PASS.
- [ ] **Step 4: Archive the specs** with `git mv` into `specs/archive/2026/`, as the final commit:
      `docs(specs): archive the chrono-to-jiff design and plan`.
