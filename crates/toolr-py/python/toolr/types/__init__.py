"""Annotation-only types recognised by the toolr binary.

Each name in this module stands for a stdlib type, used
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
- Path types. Each is a :class:`typing.NewType` over :class:`pathlib.Path`
  (or over another path type), so type checkers tell them apart while the
  runtime value is always a plain ``pathlib.Path``. The toolr binary
  checks the path before the command runs:

  - :data:`AbsolutePath` — joined to the working directory; no check.
  - :data:`NewPath` — absolute; must not exist, parent directory must.
  - :data:`ResolvedPath` — canonicalised; must exist.
  - :data:`FilePath` — canonicalised; must be a regular file.
  - :data:`DirectoryPath` — canonicalised; must be a directory.
  - :data:`ExecutablePath` — canonicalised; must be an executable file.
  - :data:`WritableDirectoryPath` — canonicalised; must be a writable
    directory.

  A bare :class:`pathlib.Path` gets no processing: the value is what the
  user typed.

- :data:`Count` — alias for :class:`int`. The rust front-end gives
  this special clap treatment: the matching CLI flag uses
  ``ArgAction::Count``, so repeating the short form (``-vvv``) yields
  the number of repetitions on the python side. Default of ``0`` is
  expected.
"""

from __future__ import annotations

import datetime as _dt
import ipaddress as _ip
import pathlib as _pathlib
import uuid as _uuid
from typing import NewType

from packaging.version import Version as _Version

DateTime = _dt.datetime
Date = _dt.date
Time = _dt.time
UUID = _uuid.UUID
IPv4 = _ip.IPv4Address
IPv6 = _ip.IPv6Address
# Each path type is a `NewType`: a distinct type to type checkers, the
# plain `pathlib.Path` it wraps at runtime. The toolr binary does every
# check before the command runs; nothing here validates anything.
AbsolutePath = NewType("AbsolutePath", _pathlib.Path)
NewPath = NewType("NewPath", AbsolutePath)
ResolvedPath = NewType("ResolvedPath", _pathlib.Path)
FilePath = NewType("FilePath", ResolvedPath)
DirectoryPath = NewType("DirectoryPath", ResolvedPath)
ExecutablePath = NewType("ExecutablePath", FilePath)
WritableDirectoryPath = NewType("WritableDirectoryPath", DirectoryPath)
# Email is a pre-validated string at runtime — the rust CLI rejects
# malformed input before the runner subprocess ever starts, so the
# value reaching the command function is guaranteed to be a syntactically
# valid `local@domain` address.
Email = str
Version = _Version
# Count is just an int at runtime; the rust parser detects the
# `toolr.types.Count` annotation and configures clap with
# ArgAction::Count so `-v -v -v` (or `-vvv`) increments the slot.
Count = int

__all__ = [
    "UUID",
    "AbsolutePath",
    "Count",
    "Date",
    "DateTime",
    "DirectoryPath",
    "Email",
    "ExecutablePath",
    "FilePath",
    "IPv4",
    "IPv6",
    "NewPath",
    "ResolvedPath",
    "Time",
    "Version",
    "WritableDirectoryPath",
]
