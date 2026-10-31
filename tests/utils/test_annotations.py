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

        def f(ctx, path: Path, x: DoesNotExist, count: int = 1): ...
        """
    )
    resolved = resolve_hints(mod.f)
    assert resolved.hints == {"path": Path, "count": int}
    assert list(resolved.unresolved) == ["x"]
