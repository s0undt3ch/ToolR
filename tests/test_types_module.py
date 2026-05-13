"""Cross-check that `toolr.types` and the rust `SupportedType` enum
agree on the public surface.

Drift between the two sides is silent — Rust would happily reject a
`toolr.types.X` annotation as "unknown" if X were added to Python but
not to `resolve_toolr_types_name` — so this test pins the set on the
Python side and a companion rust test (`parser::types::tests::
toolr_types_names_match_python_surface`) pins it on the rust side.
"""

from __future__ import annotations

import datetime
import ipaddress
import pathlib
import uuid

from packaging.version import Version as _Version

import toolr.types

# The authoritative public surface. Every entry must:
#   1. exist as an attribute on `toolr.types`
#   2. be listed in `toolr.types.__all__`
#   3. be resolved by `parser::types::resolve_toolr_types_name` in rust
EXPECTED_TOOLR_TYPES_NAMES = {
    "AbsolutePath",
    "Date",
    "DateTime",
    "Email",
    "IPv4",
    "IPv6",
    "ResolvedPath",
    "Time",
    "UUID",
    "Version",
}


def test_toolr_types_all_matches_expected_surface() -> None:
    assert set(toolr.types.__all__) == EXPECTED_TOOLR_TYPES_NAMES


def test_every_expected_name_is_importable() -> None:
    for name in EXPECTED_TOOLR_TYPES_NAMES:
        assert hasattr(toolr.types, name), f"{name} missing from toolr.types"


def test_path_aliases_resolve_to_pathlib_path() -> None:
    assert toolr.types.AbsolutePath is pathlib.Path
    assert toolr.types.ResolvedPath is pathlib.Path


def test_datetime_aliases_resolve_to_stdlib() -> None:
    assert toolr.types.DateTime is datetime.datetime
    assert toolr.types.Date is datetime.date
    assert toolr.types.Time is datetime.time


def test_uuid_alias_resolves_to_stdlib() -> None:
    assert toolr.types.UUID is uuid.UUID


def test_ip_aliases_resolve_to_stdlib() -> None:
    assert toolr.types.IPv4 is ipaddress.IPv4Address
    assert toolr.types.IPv6 is ipaddress.IPv6Address


def test_email_is_a_str_alias() -> None:
    assert toolr.types.Email is str


def test_version_alias_resolves_to_packaging_version() -> None:
    assert toolr.types.Version is _Version
