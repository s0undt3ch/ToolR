"""Per-parameter annotation resolution, with `TYPE_CHECKING` import rescue."""

from __future__ import annotations

import ast
import sys
from collections.abc import Callable
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest

from toolr._exc import SpecError
from toolr._runner import _coerce_args
from toolr.utils._annotations import resolve_hints
from toolr.utils._signature import detect_dispatch_parameter

ModuleFactory = Callable[..., ModuleType]


def test_type_checking_path_import_is_rescued(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from pathlib import Path

        def f(ctx, path: Path, count: int = 1): ...
        """
    )
    _, keyword = _coerce_args(mod.f, {"path": "some/dir", "count": "3"})
    assert keyword == {"path": Path("some/dir"), "count": 3}
    assert isinstance(keyword["path"], Path)


def test_aliased_type_checking_import_is_rescued(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from pathlib import Path as P

        def f(ctx, path: P): ...
        """
    )
    assert resolve_hints(mod.f).hints == {"path": Path}


def test_plain_module_type_checking_import_is_rescued(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            import pathlib

        def f(ctx, path: pathlib.Path): ...
        """
    )
    assert resolve_hints(mod.f).hints == {"path": Path}


def test_relative_type_checking_import_is_rescued(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from .types_ import Thing

        def f(ctx, thing: Thing): ...
        """,
        name="cmd",
        package="pkg",
        files={"types_.py": "class Thing:\n    pass\n"},
    )
    resolved = resolve_hints(mod.f)
    assert resolved.unresolved == {}
    assert resolved.hints["thing"] is sys.modules["pkg.types_"].Thing


def test_typing_dot_type_checking_block_is_scanned(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        import typing
        if typing.TYPE_CHECKING:
            from pathlib import Path

        def f(ctx, path: Path): ...
        """
    )
    assert resolve_hints(mod.f).hints == {"path": Path}


def test_unresolvable_name_resolves_the_rest(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations

        def f(ctx, x: DoesNotExist, count: int): ...
        """
    )
    resolved = resolve_hints(mod.f)
    assert list(resolved.unresolved) == ["x"]
    assert isinstance(resolved.unresolved["x"], NameError)
    assert resolved.hints == {"count": int}


def test_module_global_wins_over_type_checking_import(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from pathlib import Path

        class MyPath(str):
            pass

        Path = MyPath

        def f(ctx, path: Path): ...
        """
    )
    assert resolve_hints(mod.f).hints == {"path": mod.MyPath}


def test_source_parsed_once(
    make_tools_module: ModuleFactory, monkeypatch: pytest.MonkeyPatch
) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from pathlib import Path, PurePath

        def f(ctx, a: Path, b: PurePath, c: Missing): ...
        def g(ctx, d: Missing): ...
        """
    )
    calls: list[object] = []
    real_parse = ast.parse

    def _counting_parse(source: str, *args: Any, **kwargs: Any) -> ast.AST:
        calls.append(source)
        return real_parse(source, *args, **kwargs)

    monkeypatch.setattr(ast, "parse", _counting_parse)
    resolve_hints(mod.f)
    resolve_hints(mod.g)
    assert len(calls) == 1


def test_source_not_parsed_without_name_error(
    make_tools_module: ModuleFactory, monkeypatch: pytest.MonkeyPatch
) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations

        def f(ctx, count: int): ...
        """
    )
    calls: list[object] = []
    monkeypatch.setattr(ast, "parse", lambda *args, **_: calls.append(args))
    assert resolve_hints(mod.f).hints == {"count": int}
    assert calls == []


def test_non_name_error_becomes_spec_error(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import Annotated

        def f(ctx, broken: Annotated[int, 1 / 0], count: int): ...
        """
    )
    with pytest.raises(SpecError, match=r"`broken` on tc_mod\.f.*division by zero"):
        resolve_hints(mod.f)


@pytest.mark.skipif(sys.version_info < (3, 14), reason="lazy annotations need Python 3.14+")
def test_lazy_annotations_resolve_per_parameter(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from pathlib import Path
            from toolr.sources import DispatchCommand

        def f(ctx, path: Path, x: DoesNotExist, count: int = 1): ...
        def opt(ctx, p: Path | None): ...
        def disp(ctx, other: DoesNotExist, *, cmd: DispatchCommand): ...
        """
    )
    resolved = resolve_hints(mod.f)
    assert resolved.hints == {"path": Path, "count": int}
    assert list(resolved.unresolved) == ["x"]
    _, keyword = _coerce_args(mod.f, {"path": "some/dir", "x": "raw", "count": "3"}, quiet=True)
    assert keyword == {"path": Path("some/dir"), "x": "raw", "count": 3}
    assert _coerce_args(mod.opt, {})[1] == {"p": None}
    assert detect_dispatch_parameter(mod.disp) == "cmd"


@pytest.mark.skipif(sys.version_info < (3, 14), reason="lazy annotations need Python 3.14+")
def test_lazy_annotations_keep_closure_names(make_tools_module: ModuleFactory) -> None:
    mod = make_tools_module(
        """
        from decimal import Decimal
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from pathlib import Path

        def make():
            D = Decimal

            def f(ctx, x: D, path: Path): ...

            return f

        f = make()
        """
    )
    resolved = resolve_hints(mod.f)
    assert resolved.unresolved == {}
    assert resolved.hints == {"x": mod.Decimal, "path": Path}


@pytest.mark.parametrize(
    "guard",
    [
        "from typing import TYPE_CHECKING as TC\nif TC:",
        "import typing as t\nif t.TYPE_CHECKING:",
        "import typing_extensions\nif typing_extensions.TYPE_CHECKING:",
    ],
)
def test_aliased_type_checking_guard_is_scanned(
    make_tools_module: ModuleFactory, guard: str
) -> None:
    mod = make_tools_module(
        f"from __future__ import annotations\n{guard}\n    from pathlib import Path\n\ndef f(ctx, path: Path): ...\n"
    )
    assert resolve_hints(mod.f).hints == {"path": Path}


def test_failed_rescue_reports_the_import_error(
    make_tools_module: ModuleFactory, capsys: pytest.CaptureFixture[str]
) -> None:
    mod = make_tools_module(
        """
        from __future__ import annotations
        from typing import TYPE_CHECKING
        if TYPE_CHECKING:
            from not_installed_anywhere import Thing

        def f(ctx, thing: Thing): ...
        """
    )
    resolved = resolve_hints(mod.f)
    assert isinstance(resolved.unresolved["thing"], ImportError)
    assert isinstance(resolved.unresolved["thing"].__context__, NameError)
    _coerce_args(mod.f, {"thing": "raw"})
    err = capsys.readouterr().err
    assert "No module named 'not_installed_anywhere'" in err
    assert "not only under `if TYPE_CHECKING:`" not in err


@pytest.mark.parametrize("future", ["from __future__ import annotations\n", ""])
def test_literal_annotation_error_becomes_spec_error(
    make_tools_module: ModuleFactory, future: str
) -> None:
    if not future and sys.version_info < (3, 14):
        pytest.skip("without __future__ the module fails at import before 3.14")
    mod = make_tools_module(
        f"{future}from typing import Annotated\n\ndef f(ctx, broken: Annotated[int, 1 / 0], count: int): ...\n"
    )
    with pytest.raises(SpecError, match=r"tc_mod\.f.*division by zero"):
        _coerce_args(mod.f, {"count": "1"})
    with pytest.raises(SpecError, match=r"tc_mod\.f.*division by zero"):
        detect_dispatch_parameter(mod.f)
