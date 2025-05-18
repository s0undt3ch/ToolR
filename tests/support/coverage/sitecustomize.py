from __future__ import annotations

try:
    import coverage

    coverage.process_startup()
except ImportError:
    pass
