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
import typing
import uuid

import pytest
from packaging.version import Version as _Version

import toolr.types

# The authoritative public surface. Every entry must:
#   1. exist as an attribute on `toolr.types`
#   2. be listed in `toolr.types.__all__`
#   3. be resolved by `parser::types::resolve_toolr_types_name` in rust
EXPECTED_TOOLR_TYPES_NAMES = {
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
    "UUID",
    "Version",
    "WritableDirectoryPath",
}


def test_toolr_types_all_matches_expected_surface() -> None:
    assert set(toolr.types.__all__) == EXPECTED_TOOLR_TYPES_NAMES


def test_every_expected_name_is_importable() -> None:
    for name in EXPECTED_TOOLR_TYPES_NAMES:
        assert hasattr(toolr.types, name), f"{name} missing from toolr.types"


PATH_TYPE_PARENTS = {
    "AbsolutePath": pathlib.Path,
    "NewPath": toolr.types.AbsolutePath,
    "ResolvedPath": pathlib.Path,
    "FilePath": toolr.types.ResolvedPath,
    "DirectoryPath": toolr.types.ResolvedPath,
    "ExecutablePath": toolr.types.FilePath,
    "WritableDirectoryPath": toolr.types.DirectoryPath,
}


@pytest.mark.parametrize(("name", "parent"), PATH_TYPE_PARENTS.items(), ids=PATH_TYPE_PARENTS)
def test_path_types_are_newtypes_over_their_parent(name, parent) -> None:
    path_type = getattr(toolr.types, name)
    assert isinstance(path_type, typing.NewType)
    assert path_type.__supertype__ is parent


def test_path_types_hand_back_the_path_they_are_given() -> None:
    value = pathlib.Path("x")
    for name in PATH_TYPE_PARENTS:
        assert getattr(toolr.types, name)(value) is value


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
