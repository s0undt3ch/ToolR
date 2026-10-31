"""Tests for the DispatchCommand-based dispatcher detection rule."""

from __future__ import annotations

from collections.abc import Callable
from types import ModuleType
from typing import Annotated

import pytest

from toolr import arg
from toolr._exc import SpecError
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


def test_malformed_arg_metadata_propagates_typeerror():
    # `arg(conflicts_with=...)` requires a list / tuple / set; a bare
    # string is rejected with TypeError. Under `from __future__ import
    # annotations`, evaluating the `Annotated[...]` metadata happens
    # inside `get_type_hints`. If the detector silently swallowed that
    # TypeError, dispatch detection would fail and callers would emit
    # a misleading "manifest out of sync" error instead of the real,
    # actionable message. Make sure the TypeError surfaces.
    def cmd(
        ctx,
        *,
        dispatched: DispatchCommand,
        follow: Annotated[bool, arg(conflicts_with="no_follow")] = False,
    ) -> None: ...

    with pytest.raises(SpecError, match=r"conflicts_with=.*bare `str`"):
        detect_dispatch_parameter(cmd)


def test_dispatch_detected_despite_unresolvable_sibling() -> None:
    def cmd(ctx, other: DoesNotExist, *, cmd: DispatchCommand) -> None: ...  # type: ignore[name-defined]  # noqa: F821

    assert detect_dispatch_parameter(cmd) == "cmd"


def test_dispatch_detected_with_type_checking_dispatchcommand_import(
    make_tools_module: Callable[..., ModuleType],
) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from toolr.sources import DispatchCommand

        def cmd(ctx, *, target: DispatchCommand) -> None: ...
        """
    )
    assert detect_dispatch_parameter(mod.cmd) == "target"
