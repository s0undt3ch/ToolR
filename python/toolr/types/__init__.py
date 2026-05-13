"""Annotation-only types recognised by the toolr binary.

Each name in this module is a deliberate alias for a stdlib type, used
**as an annotation** to opt into specialised parsing on the rust side.
The runtime value handed to your command function is always the stdlib
type — `toolr.types.DateTime` is `datetime.datetime`, etc.

Importing from this module is the documented opt-in for any non-
primitive parameter type. If you annotate a command parameter with a
type that toolr doesn't recognise (e.g. `datetime.datetime` directly,
or a custom dataclass), manifest-build will reject the file with a
clear message pointing here.

Supported entries:

- :data:`DateTime`, :data:`Date`, :data:`Time` — alias for the
  matching ``datetime.*`` types. The rust side parses RFC 3339 /
  ``YYYY-MM-DD`` / ``HH:MM:SS`` and validates at CLI parse time.
- :data:`UUID` — alias for :class:`uuid.UUID`. Hyphenated form
  validated by clap.
- :data:`IPv4`, :data:`IPv6` — aliases for
  :class:`ipaddress.IPv4Address` and :class:`ipaddress.IPv6Address`.
- :data:`Email` — single ``local@domain`` address validated at CLI
  parse time. Runtime value is :class:`str`. Display names and
  comments are not accepted; one address per parameter.
- :data:`Version` — alias for :class:`packaging.version.Version`. The
  rust side validates PEP 440 grammar (epoch, pre / post / dev
  releases, local segment) via the ``pep440_rs`` crate; the
  runtime value is the matching :class:`packaging.version.Version`.
- :data:`AbsolutePath` — alias for :class:`pathlib.Path`. The rust
  side resolves the value against the working directory; no
  filesystem check, so the path may not yet exist (useful for
  output directories, new files).
- :data:`ResolvedPath` — alias for :class:`pathlib.Path`. The rust
  side calls ``canonicalize()`` — the path **must exist** and
  symlinks / ``..`` segments are resolved. Useful for input files
  and configs where you want to fail early on a missing path.

For bare :class:`pathlib.Path`, no rust-side processing is applied —
the value reaches your command function as whatever the user typed
(wrapped in ``Path``).
"""

from __future__ import annotations

import datetime as _dt
import ipaddress as _ip
import pathlib as _pathlib
import uuid as _uuid

from packaging.version import Version as _Version

DateTime = _dt.datetime
Date = _dt.date
Time = _dt.time
UUID = _uuid.UUID
IPv4 = _ip.IPv4Address
IPv6 = _ip.IPv6Address
AbsolutePath = _pathlib.Path
ResolvedPath = _pathlib.Path
# Email is a pre-validated string at runtime — the rust CLI rejects
# malformed input before the runner subprocess ever starts, so the
# value reaching the command function is guaranteed to be a syntactically
# valid `local@domain` address.
Email = str
Version = _Version

__all__ = [
    "UUID",
    "AbsolutePath",
    "Date",
    "DateTime",
    "Email",
    "IPv4",
    "IPv6",
    "ResolvedPath",
    "Time",
    "Version",
]
