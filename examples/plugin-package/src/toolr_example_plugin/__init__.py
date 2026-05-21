"""Canonical example of a third-party toolr plugin.

Commands live in :mod:`toolr_example_plugin.commands`. There is no need
to re-export them from here — toolr's discovery path imports each
command's module by its fully-qualified name as recorded in
``toolr-manifest.json``, and ``toolr self build-manifest`` walks the
package's submodules on its own.
"""

from __future__ import annotations

__version__ = "1.0.0"
