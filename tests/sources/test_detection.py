"""Tests for the DispatchCommand-based dispatcher detection rule."""

from __future__ import annotations

import pytest

from toolr.sources import DispatchCommand
from toolr.utils._signature import DispatcherDetectionError
from toolr.utils._signature import detect_dispatch_parameter


def test_normal_command_returns_none():
    def cmd(ctx, *, name: str = "x") -> None: ...

    assert detect_dispatch_parameter(cmd) is None


def test_dispatcher_returns_parameter_name():
    def cmd(ctx, *, cpu: str = "1", dispatched: DispatchCommand) -> None: ...

    assert detect_dispatch_parameter(cmd) == "dispatched"


def test_dispatcher_param_name_is_free():
    def cmd(ctx, *, target: DispatchCommand) -> None: ...

    assert detect_dispatch_parameter(cmd) == "target"


def test_multiple_dispatchcommand_params_raises():
    def cmd(ctx, *, a: DispatchCommand, b: DispatchCommand) -> None: ...

    with pytest.raises(DispatcherDetectionError, match="more than one"):
        detect_dispatch_parameter(cmd)


def test_dispatchcommand_in_positional_raises():
    def cmd(ctx, dispatched: DispatchCommand) -> None: ...

    with pytest.raises(DispatcherDetectionError, match="keyword-only"):
        detect_dispatch_parameter(cmd)
