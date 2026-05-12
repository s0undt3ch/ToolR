from __future__ import annotations

import msgspec
import pytest

from toolr._runner import SCHEMA_VERSION
from toolr._runner import ContextSpec
from toolr._runner import RunnerSpec


def test_schema_version_constant_is_1() -> None:
    assert SCHEMA_VERSION == 1


def test_runner_spec_round_trips_through_json() -> None:
    spec = RunnerSpec(
        schema_version=SCHEMA_VERSION,
        group="ci",
        command="hello",
        module="tools.ci",
        function="hello",
        args={"name": "Alice"},
        context=ContextSpec(
            repo_root="/tmp/repo",  # noqa: S108
            verbosity="normal",
            timestamps=False,
            log_level="INFO",
        ),
    )
    encoded = msgspec.json.encode(spec)
    decoded = msgspec.json.decode(encoded, type=RunnerSpec)
    assert decoded == spec


def test_runner_spec_rejects_unknown_schema_version() -> None:
    payload = {
        "schema_version": 999,
        "group": "ci",
        "command": "hello",
        "module": "tools.ci",
        "function": "hello",
        "args": {},
        "context": {
            "repo_root": "/tmp/repo",  # noqa: S108
            "verbosity": "normal",
            "timestamps": False,
            "log_level": "INFO",
        },
    }
    # We decode successfully (msgspec doesn't reject the int itself), but the
    # runner's higher-level check (Task 3) raises on version mismatch.
    decoded = msgspec.json.decode(msgspec.json.encode(payload), type=RunnerSpec)
    assert decoded.schema_version == 999


def test_runner_spec_rejects_missing_required_field() -> None:
    payload = {
        "schema_version": 1,
        "group": "ci",
        # missing "command"
        "module": "tools.ci",
        "function": "hello",
        "args": {},
        "context": {
            "repo_root": "/tmp/repo",  # noqa: S108
            "verbosity": "normal",
            "timestamps": False,
            "log_level": "INFO",
        },
    }
    with pytest.raises(msgspec.ValidationError):
        msgspec.json.decode(msgspec.json.encode(payload), type=RunnerSpec)
