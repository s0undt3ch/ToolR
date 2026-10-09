"""The runner hands every path type to the command as a `pathlib.Path`."""

from __future__ import annotations

import inspect
import pathlib

from toolr import types as tt
from toolr._runner import _coerce_args


def _all_path_types(
    ctx: object,
    absolute: tt.AbsolutePath,
    new: tt.NewPath,
    resolved: tt.ResolvedPath,
    file: tt.FilePath,
    directory: tt.DirectoryPath,
    executable: tt.ExecutablePath,
    writable: tt.WritableDirectoryPath,
    files: list[tt.FilePath],
    maybe: tt.DirectoryPath | None,
    *rest: tt.ExecutablePath,
) -> None: ...


SCALARS = ("absolute", "new", "resolved", "file", "directory", "executable", "writable")


def test_every_path_type_reaches_the_command_as_a_pathlib_path() -> None:
    raw: dict[str, object] = dict.fromkeys(SCALARS, "/srv/x")
    raw |= {"files": ["/srv/a", "/srv/b"], "maybe": "/srv/d", "rest": ["/srv/e"]}
    positional, keyword = _coerce_args(_all_path_types, raw)
    bound = inspect.signature(_all_path_types).bind(None, *positional, **keyword).arguments
    path_cls = type(pathlib.Path())
    for name in SCALARS:
        assert type(bound[name]) is path_cls, name
    assert [type(p) for p in bound["files"]] == [path_cls, path_cls]
    assert type(bound["maybe"]) is path_cls
    assert [type(p) for p in bound["rest"]] == [path_cls]
