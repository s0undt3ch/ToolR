# Follow-up: revisit `rich-argparse` dependency after Python frontend retirement

- **Created:** 2026-05-14
- **Related plan:** [`../15-plan-12-workspace-split.md`](../15-plan-12-workspace-split.md)
- **Status:** Open

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
