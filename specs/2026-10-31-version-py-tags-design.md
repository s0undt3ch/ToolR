# Parse pre-release tags and validate `--new-version` in `tools/version.py`

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-26 (low).
- **Plan:** [`2026-10-31-version-py-tags-plan.md`](./2026-10-31-version-py-tags-plan.md)

## Problem

`toolr version bump` (`tools/version.py`) runs in every `Prepare Release` job: on each PR and push through `ci.yml`
(dev version) and on each release through `release.yml` (`--new-version`). It has two input-handling bugs.

**A pre-release tag breaks every dev build.** `_compute_dev_version` runs
`git describe --tags --long --match 'v[0-9]*.[0-9]*.[0-9]*'`. The glob's last `[0-9]*` matches any suffix, so a tag
like `v1.0.0-rc1` matches, and describe returns `v1.0.0-rc1-3-gabc1234`. Then:

```python
base, count, sha = describe.split("-")  # 4 parts: ValueError
```

Splitting from the right doesn't save it: `_bump_patch("1.0.0-rc1")` then fails on `int("0-rc1")`. Once one such tag
exists, every PR's `Prepare Release` job fails until it's deleted.

**`--new-version` is used before it's checked.** `bump` writes `release-version`, `release-patch-name` and
`RELEASE_VERSION` to `GITHUB_OUTPUT`/`GITHUB_ENV` before `cargo set-version` sees the value. With
`--new-version v0.36.0` (an easy slip, since the tags carry the `v`):

- `--check-existing-tag` looks for `vv0.36.0`, finds nothing, and passes.
- `release-version=v0.36.0` is written out.
- `cargo set-version` rejects it and the job fails, after the outputs were written. A malformed value like `0.36`
  takes the same path.

## Decision

**Parse describe output with one regex, bump from the numeric core.**

```python
_DESCRIBE_RE = re.compile(
    r"^v(?P<core>\d+\.\d+\.\d+)(?:-[0-9A-Za-z.-]+)?-(?P<count>\d+)-(?P<sha>g[0-9a-f]+)$"
)
```

The optional group swallows a semver pre-release (`-rc1`, `-rc.1`, even `-rc-1`; the regex backtracks to the last two
`-`-separated fields). `v1.0.0-rc1-3-gabc1234` gives `1.0.1-dev3`. That is greater than `1.0.0-rc1`, which matters:
after an rc release `Cargo.toml` holds `1.0.0-rc1`, and `cargo set-version` refuses to go lower. Output that doesn't
match is reported with `ctx.error` naming the describe string, then `ctx.exit(1)`, instead of a traceback.

**Normalise and validate `--new-version` first.** A new helper `_normalise_release_version(raw: str) -> str` strips
one leading `v` (`removeprefix`, not `lstrip`) and requires a SemVer 2.0 match:

```python
_SEMVER_RE = re.compile(
    r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
```

It raises `ValueError` on no match. `bump` calls it at the top, before the tag check and any file write. On error it
calls `ctx.error` and `ctx.exit(1)`. Validation applies to the explicit input only. The derived dev version
(`X.Y.Z-devN[+gSHA]`) is built by the code and stays as it is.

Rejected alternatives:

- **`--exclude 'v*-*'` on `git describe`.** It skips rc tags, so the dev version derives from the last final
  release (`0.36.1-devN`), which is lower than a `1.0.0-rc1` already in `Cargo.toml`: `cargo set-version` would
  refuse it.
- **`rsplit("-", 2)` alone.** Fixes the unpack, then fails one line later in `_bump_patch`.
- **Reject a leading `v`.** The action's `version` input accepts both forms; accepting both here is consistent and
  removes the slip rather than reporting it.

## Open question

Should `--new-version` accept pre-release versions such as `1.0.0-rc1`? Recommended: yes. SemVer and `cargo
set-version` allow them, and the point of the check is typos, not release policy. If they are accepted, note that
`release.yml` passes `--latest` to `gh release create` for every release, so an rc would become the "latest" GitHub
release and the Setup ToolR action's `version: latest` would pick it up. Handling that belongs to whoever cuts the
first rc.

## Scope and compatibility

- No runner or manifest schema change.
- Release tooling only; nothing ships to users, so no `UNRELEASED.md` entry.
- `--new-version v0.36.0` now behaves exactly like `--new-version 0.36.0`.
