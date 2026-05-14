# Follow-up: revisit `rich-argparse` dependency after Python frontend retirement

- **Created:** 2026-05-14
- **Related plan:** [`../15-plan-12-workspace-split.md`](../15-plan-12-workspace-split.md)
- **Status:** Closed

## Context

The cargo workspace split plan retires the Python CLI frontend
(`__main__.py`, `_parser.py`, `_registry.py`) in favour of the Rust
binary. Today the only consumer of `rich-argparse` in this repo is
`python/toolr/_parser.py:15`:

```python
from rich_argparse import ArgumentDefaultsRichHelpFormatter
```

used at `_parser.py:70` to set `formatter_class` on the argparse parser.

Once `_parser.py` is deleted (Stage 8 of the split plan), no remaining
Python code uses `rich-argparse` — verified at plan-writing time via:

```bash
grep -rn "rich.argparse\|rich_argparse\|RichHelpFormatter" python/ tests/
```

which returns only the two `_parser.py` lines above.

The `[project] dependencies` block in `crates/toolr-py/pyproject.toml`
(per the design spec, Section 3) currently lists `rich-argparse>=1.7.0`
— carried over from today's root `pyproject.toml`. After Stage 8 it
becomes a dead dep.

## What to do

After the workspace split lands and the Python frontend is gone:

1. Re-run the grep against `crates/toolr-py/python/` to confirm
   `rich-argparse` has zero importers in the surviving Python tree.
2. Check whether anything in the design spec or product direction
   wants to keep `rich-argparse` available for *user tool scripts* to
   import — e.g., if the framework's `Context` API documents
   `rich-argparse` formatters as a supported integration point.
3. If neither (1) nor (2) finds a reason to keep it, remove
   `rich-argparse>=1.7.0` from `crates/toolr-py/pyproject.toml`
   `[project] dependencies`. Bump the wheel's metadata to reflect a
   leaner dep tree.
4. If a justification exists (e.g., "user tools rely on it being
   pre-installed"), document that rationale in a code comment near
   the dependency declaration so the next person doesn't have to
   rediscover it.

## Ticketing

This is a local follow-up note. If a tracking issue is desired, file
under the repository's issue tracker referencing this file by path.

---

## Resolution (2026-05-14)

Verified zero importers in the post-Stage-8 codebase (`grep -rn
"rich.argparse\|rich_argparse\|RichHelpFormatter"` over `crates/toolr-py/python/`,
`tools/`, `tests/`, `docs/`, `scripts/` returns no production-code hits — only
comment/docstring references in `crates/toolr/src/cli.rs`,
`crates/toolr/src/markdown.rs`, `CHANGELOG.md`, and the spec/followup docs).
Removed `rich-argparse>=1.7.0` from `crates/toolr-py/pyproject.toml [project]
dependencies`. `uv sync --dev` re-resolves cleanly; `rich-argparse` is gone
from `uv.lock` (28-line net deletion from the lockfile). Wheel METADATA no
longer lists it: `unzip -p toolr_py-*.whl '*METADATA' | grep -i rich`
returns only `Requires-Dist: rich>=13.0.0,<14.3` plus an unrelated README
line.

A side-effect surfaced while validating: `rich.console.getpass` (patched in
`tests/context/test_prompt.py::test_prompt_password`) is gone in `rich`
14.3+. Previously rich-argparse 1.8.0 transitively pinned `rich==14.2.0`, so
the lockfile masked the issue. To keep tests green without scope-creeping
into a test refactor, `rich` is now declared as a direct dep with the
constraint `rich>=13.0.0,<14.3` and a code comment marking it as a
follow-up. Tests: 281 Python pass, full cargo workspace passes (`cargo test
--workspace --release`).

Closing.
