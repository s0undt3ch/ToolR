"""Integration tests demonstrating complete registry usage."""

from __future__ import annotations

import importlib
import importlib.resources
import importlib.util
import os
import shutil
import sys
from collections.abc import Callable
from collections.abc import Iterator
from pathlib import Path

import pytest

from toolr import Context
from toolr import _decorators
from toolr import command
from toolr import command_group
from toolr._decorators import CommandGroup
from toolr._exc import ToolrError
from toolr.testing import CommandsTester
from toolr.testing._discovery import _tester_sys_path

FILES = Path(__file__).parent / "files"
CI_MODULE = (
    "from toolr import command_group\n"
    'group = command_group("ci", "CI utils", "CI utilities")\n'
    "@group.command\n"
    "def hello(ctx):\n"
    '    """Say hi."""\n'
)
BAD_MODULE = (
    "from toolr import command_group\n"
    'g = command_group("bad", "Bad", "Bad")\n'
    "@g.command\n"
    "def nodoc(ctx):\n"
    "    pass\n"
)
BROKEN_MODULE = "raise RuntimeError('boom')\n"
TYPO_GROUP_MODULE = (
    "from toolr import command, command_group\n"
    'command_group("ci", "CI", "CI")\n'
    '@command(group="ci.typo")\n'
    "def check(ctx):\n"
    '    """Check things."""\n'
)
DEMO_MODULE = (
    "from toolr import command_group\n"
    'group = command_group("demo", "Demo", "Demo commands")\n'
    "@group.command\n"
    "def hello(ctx):\n"
    '    """Say hello."""\n'
)
LINKED_MODULE = (
    "from toolr import command_group\n"
    'group = command_group("linked", "Linked", "Linked commands")\n'
    "@group.command\n"
    "def hi(ctx):\n"
    '    """Say hi."""\n'
)
ToolsRepo = Callable[[dict[str, str | None]], Path]
STDLIB_USER_MODULE = (
    "import sqlite3, tomllib, zipfile, csv\n"
    "from toolr import command_group\n"
    'g = command_group("std", "Std", "Std")\n'
    "@g.command\n"
    "def ok(ctx):\n"
    '    """Use the stdlib."""\n'
)
HOST_ONLY_MODULE = (
    'from toolr import command_group\ngroup = command_group("host", "Host", "Host repo tools")\n'
)
STDLIB_MODULES = ("sqlite3", "tomllib", "zipfile", "csv")


@pytest.fixture
def tools_repo(tmp_path: Path) -> ToolsRepo:
    """Write a fixture repo under `tmp_path` and return its root.

    `tools/__init__.py` is written empty unless the mapping sets it, or left
    out when it maps to `None` (a namespace `tools/`); every other
    `{relative_path: source}` entry is written as given.
    """

    def factory(files: dict[str, str | None]) -> Path:
        (tmp_path / "tools").mkdir()
        files = {"tools/__init__.py": "", **files}
        for relative, source in files.items():
            if source is None:
                continue
            target = tmp_path / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source)
        return tmp_path

    return factory


def test_discover_raises_after_walking_every_module(
    tools_repo: ToolsRepo,
) -> None:
    """A module that fails to import makes `discover()` raise, but only after
    the walk has imported every other module."""
    repo = tools_repo({"tools/ci.py": CI_MODULE, "tools/broken.py": BROKEN_MODULE})
    with CommandsTester(search_path=repo) as tester:
        with pytest.raises(ExceptionGroup) as excinfo:
            tester.discover()
        walked = "tools.ci" in sys.modules
    assert walked, "discover() should have imported tools.ci before raising"
    assert excinfo.group_contains(RuntimeError, match="boom")
    (error,) = excinfo.value.exceptions
    assert "raised while importing `tools.broken`" in error.__notes__


def test_discover_raises_on_undocumented_command(
    tools_repo: ToolsRepo,
) -> None:
    """An undocumented command fails discovery the way the manifest build does."""
    repo = tools_repo({"tools/bad.py": BAD_MODULE})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert excinfo.group_contains(ValueError, match="add a docstring")


def test_discover_reports_every_failing_module(
    tools_repo: ToolsRepo,
) -> None:
    """Every broken module is reported in one group, not just the first."""
    repo = tools_repo({"tools/bad.py": BAD_MODULE, "tools/broken.py": BROKEN_MODULE})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert len(excinfo.value.exceptions) == 2
    assert "2 module(s)" in str(excinfo.value)


def test_discover_reports_a_failing_subpackage_and_keeps_walking(
    tmp_path: Path, tools_repo: ToolsRepo
) -> None:
    """A subpackage whose `__init__` raises is reported like any module: its
    body runs once and the walk goes on to the modules after it."""
    runs = tmp_path / "runs.txt"
    repo = tools_repo(
        {
            "tools/a_sub/__init__.py": (
                f"with open({str(runs)!r}, 'a') as fh:\n"
                "    fh.write('x')\n"
                "raise RuntimeError('sub boom')\n"
            ),
            "tools/a_sub/inner.py": "",
            "tools/zz.py": "raise ValueError('zz boom')\n",
        }
    )
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert excinfo.group_contains(RuntimeError, match="sub boom")
    assert excinfo.group_contains(ValueError, match="zz boom")
    assert len(excinfo.value.exceptions) == 2
    assert runs.read_text() == "x"


def test_discover_reports_an_undocumented_command_in_a_nested_group(
    tools_repo: ToolsRepo,
) -> None:
    """An undocumented command in a group package's `__init__.py` is reported
    in the group, not raised bare."""
    repo = tools_repo(
        {
            "tools/docker/__init__.py": (
                "from toolr import command_group\n"
                'docker = command_group("docker", "Docker", "Docker tools")\n'
                "@docker.command\n"
                "def nodoc(ctx):\n"
                "    pass\n"
            ),
            "tools/zz.py": CI_MODULE,
        }
    )
    with CommandsTester(search_path=repo) as tester:
        with pytest.raises(ExceptionGroup) as excinfo:
            tester.discover()
        walked = "tools.zz" in sys.modules
    assert excinfo.group_contains(ValueError, match="add a docstring")
    assert walked


def test_discover_imports_submodules_of_a_nested_package(
    tools_repo: ToolsRepo,
) -> None:
    """Modules inside a `tools` subpackage are imported too."""
    repo = tools_repo(
        {
            "tools/docker/__init__.py": (
                "from toolr import command_group\n"
                'docker = command_group("docker", "Docker", "Docker tools")\n'
            ),
            "tools/docker/image.py": (
                "from toolr import command_group\n"
                'image = command_group("docker.image", "Images", "Image tools")\n'
                "@image.command\n"
                "def build(ctx):\n"
                '    """Build an image."""\n'
            ),
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert {"tools.docker", "tools.docker.image"} <= groups.keys()


def test_discover_imports_modules_in_a_namespace_subdirectory(
    tools_repo: ToolsRepo,
) -> None:
    """A `tools/<group>/` directory without `__init__.py` is walked, as toolr does."""
    repo = tools_repo({"tools/precommit/cmd.py": CI_MODULE})
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
        imported = "tools.precommit.cmd" in sys.modules
    assert imported
    # CI_MODULE registers `command_group("ci")`, so its key is `tools.ci`.
    assert "tools.ci" in groups


def test_discover_reports_a_failing_module_in_a_namespace_subdirectory(
    tools_repo: ToolsRepo,
) -> None:
    """A broken module in a directory without `__init__.py` is reported."""
    repo = tools_repo({"tools/precommit/zz.py": BAD_MODULE})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert excinfo.group_contains(ValueError, match="add a docstring")
    (error,) = excinfo.value.exceptions
    assert "raised while importing `tools.precommit.zz`" in error.__notes__


def test_discover_rejects_a_symlinked_module(tmp_path: Path, tools_repo: ToolsRepo) -> None:
    """F-17 repro: `tools/linked.py -> ../shared/linked.py` fails discovery
    with the manifest build's message, before any module is imported.

    Runs on Windows too when the runner may create file symlinks.
    """
    (tmp_path / "shared").mkdir()
    (tmp_path / "shared" / "linked.py").write_text(LINKED_MODULE)
    repo = tools_repo({"tools/demo.py": DEMO_MODULE})
    try:
        (repo / "tools" / "linked.py").symlink_to(Path("..", "shared", "linked.py"))
    except OSError as exc:  # Windows without the symlink privilege
        pytest.skip(f"cannot create file symlinks: {exc}")
    with CommandsTester(search_path=repo) as tester:
        with pytest.raises(ToolrError) as excinfo:
            tester.discover()
        imported = {name for name in sys.modules if name.startswith("tools.")}
    assert str(excinfo.value) == (
        "symlinked modules are not supported (1):\n"
        "  - tools/linked.py\n"
        "Put the file itself under tools/, or share it as a Python package."
    )
    assert imported == set()


@pytest.mark.skip_on_windows(reason="symlinks need a privilege on Windows")
def test_discover_reports_every_symlinked_module(tmp_path: Path, tools_repo: ToolsRepo) -> None:
    """Nested, dangling and non-importable links are all reported, sorted, with
    control characters escaped, as `toolr` reports them."""
    (tmp_path / "shared").mkdir()
    (tmp_path / "shared" / "linked.py").write_text(LINKED_MODULE)
    repo = tools_repo({"tools/demo.py": DEMO_MODULE, "tools/sub/__init__.py": ""})
    tools = repo / "tools"
    (tools / "sub" / "other.py").symlink_to(tmp_path / "shared" / "linked.py")
    (tools / "my-dir").mkdir()
    (tools / "my-dir" / "l.py").symlink_to(tmp_path / "shared" / "linked.py")
    (tools / "gone.py").symlink_to(tmp_path / "shared" / "missing.py")
    (tools / "a\x1b]0;x\x07.py").symlink_to("demo.py")
    with CommandsTester(search_path=repo) as tester, pytest.raises(ToolrError) as excinfo:
        tester.discover()
    assert str(excinfo.value) == (
        "symlinked modules are not supported (4):\n"
        "  - tools/a\\u{1b}]0;x\\u{7}.py\n"
        "  - tools/gone.py\n"
        "  - tools/my-dir/l.py\n"
        "  - tools/sub/other.py\n"
        "Put the file itself under tools/, or share it as a Python package."
    )


@pytest.mark.skip_on_windows(reason="symlinks need a privilege on Windows")
def test_discover_reports_symlinked_modules_it_cannot_resolve(
    tmp_path: Path, tools_repo: ToolsRepo
) -> None:
    """A looping link and links into an unreadable directory are reported like
    any other, as `toolr` reports them, instead of failing the walk."""
    repo = tools_repo({"tools/demo.py": DEMO_MODULE, "locked/x.py": BROKEN_MODULE})
    tools = repo / "tools"
    locked = tmp_path / "locked"
    (locked / "d").mkdir()
    (tools / "loop.py").symlink_to("loop.py")
    (tools / "inlocked.py").symlink_to(Path("..", "locked", "x.py"))
    (tools / "dirlink.py").symlink_to(Path("..", "locked", "d"))
    locked.chmod(0)
    try:
        if os.access(locked, os.R_OK):
            pytest.skip("root can stat through a mode-000 directory")
        with CommandsTester(search_path=repo) as tester, pytest.raises(ToolrError) as excinfo:
            tester.discover()
    finally:
        locked.chmod(0o755)
    assert str(excinfo.value) == (
        "symlinked modules are not supported (3):\n"
        "  - tools/dirlink.py\n"
        "  - tools/inlocked.py\n"
        "  - tools/loop.py\n"
        "Put the file itself under tools/, or share it as a Python package."
    )


def test_discover_skips_what_toolr_does_not_walk_or_cannot_import(
    tmp_path: Path, tools_repo: ToolsRepo
) -> None:
    """Pruned and non-identifier directories, non-identifier and non-`.py`
    files, and symlinked directories (even one named `*.py`) are not imported,
    and `.py` links under pruned directories are not rejected."""
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "linked.py").write_text(BROKEN_MODULE)
    repo = tools_repo(
        {
            "tools/ci.py": CI_MODULE,
            "tools/__pycache__/cached.py": BROKEN_MODULE,
            "tools/.hidden/hidden.py": BROKEN_MODULE,
            "tools/not-a-package/inner.py": BROKEN_MODULE,
            "tools/not-a-module.py": BROKEN_MODULE,
            "tools/notes.txt": "not python\n",
        }
    )
    (repo / "tools" / "linked_dir").symlink_to(outside, target_is_directory=True)
    (repo / "tools" / ".venv").symlink_to(outside, target_is_directory=True)
    (repo / "tools" / "pkg.py").symlink_to(outside, target_is_directory=True)
    (repo / "tools" / "__pycache__" / "linked.py").symlink_to(outside / "linked.py")
    (repo / "tools" / ".hidden" / "linked.py").symlink_to(outside / "linked.py")
    # Every skipped `.py` raises if imported, so a clean discover() proves none was.
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert "tools.ci" in groups


def test_discover_stops_on_a_package_path_that_loops_back(
    tools_repo: ToolsRepo,
) -> None:
    """A subpackage `__path__` pointing back at a walked directory, or at a
    missing one, doesn't make the walk loop or fail."""
    repo = tools_repo(
        {
            "tools/ci.py": CI_MODULE,
            "tools/loop/__init__.py": (
                "import os\n"
                "__path__.append(os.path.dirname(__path__[0]))\n"
                "__path__.append(os.path.join(__path__[0], 'missing'))\n"
            ),
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        rewalked = "tools.loop.ci" in sys.modules
    assert not rewalked


def test_discover_raises_when_tools_package_import_fails(
    tools_repo: ToolsRepo,
) -> None:
    """A `ModuleNotFoundError` from inside `tools/__init__.py` is a failure,
    not the "no `tools` package" case."""
    repo = tools_repo({"tools/__init__.py": "import toolr_missing_dependency_xyz\n"})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert excinfo.group_contains(ModuleNotFoundError)
    (error,) = excinfo.value.exceptions
    assert "raised while importing `tools`" in error.__notes__


def test_discover_raises_when_tools_package_raises(
    tools_repo: ToolsRepo,
) -> None:
    """Any other exception from `tools/__init__.py` is reported the same way."""
    repo = tools_repo({"tools/__init__.py": BROKEN_MODULE})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert excinfo.group_contains(RuntimeError, match="boom")
    (error,) = excinfo.value.exceptions
    assert "raised while importing `tools`" in error.__notes__


def test_discover_without_tools_package_is_a_no_op(tmp_path: Path) -> None:
    """A search path without `tools/` discovers nothing and doesn't raise."""
    with CommandsTester(search_path=tmp_path) as tester:
        tester.discover()
        assert tester.collected_command_groups() == {}


def test_discover_imports_a_single_file_tools_module(tmp_path: Path) -> None:
    """A `tools.py` module (no package to walk) is imported and its group collected."""
    (tmp_path / "tools.py").write_text(
        "from toolr import command_group\n"
        'group = command_group("single", "Single", "Single file")\n'
    )
    with CommandsTester(search_path=tmp_path) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert "tools.single" in groups


@pytest.fixture
def host_repo(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A host repo with its own regular `tools` package, put on `sys.path`."""
    host = tmp_path / "host"
    (host / "tools").mkdir(parents=True)
    (host / "tools" / "__init__.py").write_text("")
    (host / "tools" / "host_only.py").write_text(HOST_ONLY_MODULE)
    monkeypatch.syspath_prepend(host)
    return host


def test_discover_imports_stdlib_modules_not_yet_loaded(
    tools_repo: ToolsRepo, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A `tools/*.py` module can import stdlib modules nobody loaded yet."""
    for name in STDLIB_MODULES:
        monkeypatch.delitem(sys.modules, name, raising=False)
    repo = tools_repo({"tools/stdlib_user.py": STDLIB_USER_MODULE})
    with CommandsTester(search_path=repo) as tester:
        # `__enter__` reinstalls the module snapshot taken at construction,
        # so check here that the imports below really have to find the files.
        assert not set(STDLIB_MODULES) & sys.modules.keys()
        tester.discover()
        groups = tester.collected_command_groups()
    group = groups["tools.std"]
    assert isinstance(group, CommandGroup)
    assert "ok" in group.get_commands()


def test_discover_ignores_tools_dirs_elsewhere_on_sys_path(
    tools_repo: ToolsRepo, host_repo: Path
) -> None:
    """The host's regular `tools` package doesn't replace a namespace `tools/` fixture."""
    fixture = tools_repo({"tools/__init__.py": None, "tools/ci.py": CI_MODULE})
    with CommandsTester(search_path=fixture) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert "tools.ci" in groups
    assert "tools.host" not in groups


def test_discover_without_tools_ignores_host_tools(tmp_path: Path, host_repo: Path) -> None:
    """A fixture with no `tools/` doesn't import the host's instead."""
    fixture = tmp_path / "fixture"
    fixture.mkdir()
    with CommandsTester(search_path=fixture) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
        imported_host = "tools.host_only" in sys.modules
    assert groups == {}
    assert not imported_host


@pytest.fixture
def restore_import_state() -> Iterator[None]:
    """Put `sys.meta_path` and `sys.path` back after the test, even if a
    tester entered by hand leaks its finder or path on failure. Listed
    first, so it tears down after `monkeypatch`."""
    meta_path = sys.meta_path[:]
    path = sys.path[:]
    yield
    sys.meta_path[:] = meta_path
    sys.path[:] = path


def test_discover_ignores_host_dirs_added_by_extend_path(
    tools_repo: ToolsRepo, host_repo: Path
) -> None:
    """A fixture `tools/__init__.py` that calls `pkgutil.extend_path` (which
    scans `sys.path` itself) doesn't get the host's `tools/` walked."""
    repo = tools_repo(
        {
            "tools/__init__.py": (
                "from pkgutil import extend_path\n__path__ = extend_path(__path__, __name__)\n"
            ),
            "tools/ci.py": CI_MODULE,
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
        imported_host = "tools.host_only" in sys.modules
    assert list(groups) == ["tools.ci"]
    assert not imported_host


def test_find_spec_for_tools_raises_without_a_fixture_tools(
    tmp_path: Path, host_repo: Path
) -> None:
    """With no fixture `tools`, looking `tools` up raises instead of returning
    `None`, so no other `tools` on `sys.path` can be found."""
    fixture = tmp_path / "fixture"
    fixture.mkdir()
    with CommandsTester(search_path=fixture), pytest.raises(ModuleNotFoundError):
        importlib.util.find_spec("tools")


@pytest.mark.usefixtures("restore_import_state")
def test_commands_tester_exit_restores_state_when_cwd_is_gone(
    tmp_path: Path, tools_repo: ToolsRepo, monkeypatch: pytest.MonkeyPatch
) -> None:
    """If the original cwd was deleted, `__exit__` still removes its finder
    and restores `sys.path` before reporting the failed `chdir`."""
    gone = tmp_path / "gone"
    gone.mkdir()
    monkeypatch.chdir(gone)
    saved_path = sys.path[:]
    saved_meta_path = sys.meta_path[:]
    repo = tools_repo({"tools/ci.py": CI_MODULE})
    tester = CommandsTester(search_path=repo)
    with pytest.raises(FileNotFoundError), tester:
        gone.rmdir()
    assert sys.meta_path == saved_meta_path
    assert sys.path == saved_path


def test_tester_sys_path_drops_only_cwd_and_search_path(tmp_path: Path) -> None:
    """Only the cwd entry and the search path itself are dropped; the search
    path goes first. `with_pkg` holds a `tools/` to guard against going back
    to pruning `tools` providers, which took everything else in those
    entries off `sys.path` with them."""
    search_path = tmp_path / "search"
    with_pkg = tmp_path / "with_pkg"
    (with_pkg / "tools").mkdir(parents=True)
    plain = tmp_path / "plain"
    plain.mkdir()
    saved = ["", str(with_pkg), str(search_path), str(plain)]
    assert _tester_sys_path(search_path, saved) == [str(search_path), str(with_pkg), str(plain)]


def test_discover_ignores_tools_under_a_dir_named_like_site_packages(
    tmp_path: Path,
    tools_repo: ToolsRepo,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A repo checked out under `.worktrees/windows-site-packages` doesn't
    merge its namespace `tools/` into the fixture's. Pins the reported
    regression: a substring match on `site-packages` once kept such a repo's
    root on `sys.path`."""
    decoy = tmp_path / "host" / ".worktrees" / "windows-site-packages"
    (decoy / "tools").mkdir(parents=True)
    (decoy / "tools" / "host_only.py").write_text(BROKEN_MODULE)
    monkeypatch.syspath_prepend(decoy)
    repo = tools_repo({"tools/__init__.py": None, "tools/ci.py": CI_MODULE})
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert list(groups) == ["tools.ci"]


@pytest.mark.usefixtures("restore_import_state")
def test_namespace_tools_path_stays_in_the_fixture_when_sys_path_changes(
    tmp_path: Path, tools_repo: ToolsRepo, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A namespace `tools.__path__` recomputed after `sys.path` changes still
    names only the fixture's `tools/`, and keeps it if that disappears."""
    other = tmp_path / "other"
    (other / "tools").mkdir(parents=True)
    repo = tools_repo({"tools/__init__.py": None, "tools/ci.py": CI_MODULE})
    with CommandsTester(search_path=repo):
        tools = importlib.import_module("tools")
        # Both prepends invalidate the import caches, forcing a recompute.
        monkeypatch.syspath_prepend(other)
        assert list(tools.__path__) == [str(repo / "tools")]
        shutil.rmtree(repo / "tools")
        monkeypatch.syspath_prepend(tmp_path)
        assert list(tools.__path__) == [str(repo / "tools")]


def test_namespace_tools_supports_importlib_resources(tools_repo: ToolsRepo) -> None:
    """`importlib.resources` still reads from a namespace `tools/` fixture."""
    repo = tools_repo({"tools/__init__.py": None, "tools/data.txt": "hello"})
    with CommandsTester(search_path=repo):
        text = importlib.resources.files("tools").joinpath("data.txt").read_text()
    assert text == "hello"


@pytest.mark.usefixtures("restore_import_state")
def test_commands_tester_removes_its_finder_on_exit(tools_repo: ToolsRepo) -> None:
    """Nested testers each remove their own finder and leave `sys.meta_path`
    as they found it."""
    before = sys.meta_path[:]
    outer_repo = tools_repo({"tools/ci.py": CI_MODULE})
    inner_repo = outer_repo / "inner"
    (inner_repo / "tools").mkdir(parents=True)
    with CommandsTester(search_path=outer_repo) as outer:
        with CommandsTester(search_path=inner_repo):
            pass
        outer.discover()
        groups = outer.collected_command_groups()
    assert sys.meta_path == before
    assert list(groups) == ["tools.ci"]


@pytest.mark.parametrize("fixture_init", ["", None], ids=["regular", "namespace"])
def test_discover_keeps_an_overlay_site_dir_holding_a_stray_tools(
    tmp_path: Path,
    tools_repo: ToolsRepo,
    monkeypatch: pytest.MonkeyPatch,
    fixture_init: str | None,
) -> None:
    """A site directory the running interpreter doesn't own (a `uv run --with`
    overlay) stays importable even when it ships a stray top-level `tools`,
    and that `tools` never stands in for the fixture's."""
    overlay = tmp_path / "uv-cache" / "archive-v0" / "abc123" / "lib" / "site-packages"
    (overlay / "tools").mkdir(parents=True)
    (overlay / "tools" / "__init__.py").write_text(BROKEN_MODULE)
    (overlay / "overlaydep").mkdir()
    (overlay / "overlaydep" / "__init__.py").write_text("VALUE = 1\n")
    monkeypatch.syspath_prepend(overlay)
    repo = tools_repo(
        {"tools/__init__.py": fixture_init, "tools/ci.py": "import overlaydep\n" + CI_MODULE}
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert list(groups) == ["tools.ci"]


def test_commands_tester_restores_sys_modules(tmp_path: Path) -> None:
    """`CommandsTester.__exit__` undoes its mutation of `sys.modules`
    (symmetric with the cwd / `sys.path` restore): a module imported inside
    the `with` block does not leak out, and the bare `clear()` on enter is
    reversed so a long-lived (e.g. pytest) process keeps its real imports
    rather than being left wiped. The volatile `tools` / example packages
    are intentionally NOT carried forward — they are reloaded per test."""
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "ci.py").write_text(
        'from toolr import command_group\ngroup = command_group("ci", "CI utils", "CI utilities")\n'
    )
    before = dict(sys.modules)
    with CommandsTester(search_path=tmp_path) as tester:
        tester.discover()
        assert "tools.ci" in sys.modules  # imported inside the block
    # The in-block import is reversed (no leak)...
    assert "tools.ci" not in sys.modules
    # ...and every real (non-volatile) module present before the block is
    # restored — the enter-time clear() does not strand the interpreter.
    nonvolatile = {
        name
        for name in before
        if name not in ("tools", "toolr_example_plugin")
        and not name.startswith(("tools.", "toolr_example_plugin."))
    }
    assert nonvolatile <= sys.modules.keys()


def test_complete_workflow_example(commands_tester):
    """Test a complete workflow demonstrating the registry usage."""
    # Simulate building a Docker tooling hierarchy
    docker_group = command_group("docker", "Docker Tools", "Docker container management")

    # Add some direct commands to docker group
    @docker_group.command("version")
    def docker_version(ctx: Context):
        """Show Docker version."""
        return "Docker version 20.10.7"

    @docker_group.command("info")
    def docker_info(ctx: Context):
        """Show Docker system info."""
        return "Docker system info"

    # Create nested command groups
    build_group = docker_group.command_group("build", "Build Tools", "Docker image building")
    compose_group = docker_group.command_group(
        "compose", "Compose Tools", "Docker Compose operations"
    )

    # Add commands to build group
    @build_group.command("image")
    def build_image(ctx: Context):
        """Build an image."""
        return "Building Docker image"

    @build_group.command("cache")
    def build_cache(ctx: Context):
        """Manage build cache."""
        return "Managing build cache"

    # Add commands to compose group
    @compose_group.command("up")
    def compose_up(ctx: Context):
        """Start services."""
        return "Starting services"

    @compose_group.command("down")
    def compose_down(ctx: Context):
        """Stop services."""
        return "Stopping services"

    # Create deeply nested group
    advanced_group = build_group.command_group(
        "advanced", "Advanced Build", "Advanced build features"
    )

    @advanced_group.command("multi-stage")
    def multi_stage_build(ctx: Context):
        """Multi-stage build."""
        return "Multi-stage build"

    # Verify the structure was created correctly
    command_groups = commands_tester.collected_command_groups()
    assert len(command_groups) == 4
    assert "tools.docker" in command_groups
    assert "tools.docker.build" in command_groups
    assert "tools.docker.compose" in command_groups
    assert "tools.docker.build.advanced" in command_groups

    # Verify command registration
    assert len(command_groups["tools.docker"].get_commands()) == 2
    assert len(command_groups["tools.docker.build"].get_commands()) == 2
    assert len(command_groups["tools.docker.compose"].get_commands()) == 2
    assert len(command_groups["tools.docker.build.advanced"].get_commands()) == 1

    # Verify group relationships
    assert docker_group.full_name == "tools.docker"
    assert build_group.full_name == "tools.docker.build"
    assert compose_group.full_name == "tools.docker.compose"
    assert advanced_group.full_name == "tools.docker.build.advanced"

    assert build_group.parent == "tools.docker"
    assert compose_group.parent == "tools.docker"
    assert advanced_group.parent == "tools.docker.build"


def test_real_world_tool_structure(commands_tester):
    """Test a realistic tool structure like you might see in a real project."""
    # Development tools
    dev_group = command_group("dev", "Development", "Development workflow tools")

    @dev_group.command("setup")
    def dev_setup(ctx: Context):
        """Set up development environment."""

    @dev_group.command("clean")
    def dev_clean(ctx: Context):
        """Clean development artifacts."""

    # Testing tools
    test_group = dev_group.command_group("test", "Testing", "Test execution and management")

    @test_group.command("unit")
    def test_unit(ctx: Context):
        """Run unit tests."""

    @test_group.command("integration")
    def test_integration(ctx: Context):
        """Run integration tests."""

    @test_group.command("e2e")
    def test_e2e(ctx: Context):
        """Run end-to-end tests."""

    # Coverage tools under testing
    coverage_group = test_group.command_group("coverage", "Coverage", "Test coverage tools")

    @coverage_group.command("report")
    def coverage_report(ctx: Context):
        """Generate coverage report."""

    @coverage_group.command("html")
    def coverage_html(ctx: Context):
        """Generate HTML coverage report."""

    # CI/CD tools
    ci_group = command_group("ci", "CI/CD", "Continuous integration and deployment")

    @ci_group.command("validate")
    def ci_validate(ctx: Context):
        "Do it."
        return "Validating CI config"

    # Deployment under CI
    deploy_group = ci_group.command_group("deploy", "Deploy", "Deployment operations")

    @deploy_group.command("staging")
    def deploy_staging(ctx: Context):
        """Deploy to staging."""

    @deploy_group.command("production")
    def deploy_production(ctx: Context):
        """Deploy to production."""

    # Verify the complex structure
    command_groups = commands_tester.collected_command_groups()
    expected_groups = [
        "tools.dev",
        "tools.dev.test",
        "tools.dev.test.coverage",
        "tools.ci",
        "tools.ci.deploy",
    ]

    for group_name in expected_groups:
        assert group_name in command_groups
        group = command_groups[group_name]
        assert group.full_name == group_name

    # Verify we have all the expected commands
    commands = command_groups["tools.dev.test.coverage"].get_commands()
    assert len(commands) == 2

    # Test specific command paths
    assert len(commands) == 2
    assert "report" in commands
    assert "html" in commands


def test_command_group_hierarchy_storage(commands_tester):
    """Test that command groups are properly stored in hierarchical structure."""
    # Create a hierarchy (avoid naming conflict with 'tools' prefix)
    app = command_group("app", "Application", "Application tools")
    docker = app.command_group("docker", "Docker", "Docker tools")
    build = docker.command_group("build", "Build", "Build tools")
    advanced = build.command_group("advanced", "Advanced", "Advanced build")

    # Test that groups are stored at correct paths
    command_groups = commands_tester.collected_command_groups()
    assert command_groups["tools.app"] == app
    assert command_groups["tools.app.docker"] == docker
    assert command_groups["tools.app.docker.build"] == build
    assert command_groups["tools.app.docker.build.advanced"] == advanced

    # Test that non-existent paths are not in the registry
    assert "nonexistent" not in command_groups
    assert "tools.nonexistent" not in command_groups
    assert "tools.app.docker.nonexistent" not in command_groups
    assert "tools.app.docker.build.advanced.nonexistent" not in command_groups


def test_standalone_command_in_group_file(tools_repo: ToolsRepo) -> None:
    """A standalone `@command(group=...)` in the group's own file lands in the group."""
    repo = tools_repo(
        {
            "tools/standalone.py": (
                "from toolr import command, command_group\n"
                "\n"
                'g = command_group("g", "G", "G")\n'
                "\n"
                '@command("x", group="g")\n'
                "def x(ctx):\n"
                '    """X."""\n'
                "\n"
                "@g.command\n"
                "def y(ctx):\n"
                '    """Y."""\n'
            ),
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    group = groups["tools.g"]
    assert isinstance(group, CommandGroup)
    assert set(group.get_commands()) == {"x", "y"}


def test_standalone_command_before_its_group_module(tools_repo: ToolsRepo) -> None:
    """A standalone command imported before its group's module still lands in the group."""
    repo = tools_repo(
        {
            "tools/a_cmds.py": (
                "from toolr import command\n"
                '@command(group="ci")\n'
                "def run_lint(ctx):\n"
                '    """Run the linters."""\n'
            ),
            "tools/b_group.py": 'from toolr import command_group\ncommand_group("ci", "CI", "CI")\n',
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    group = groups["tools.ci"]
    assert isinstance(group, CommandGroup)
    assert "run-lint" in group.get_commands()


def test_standalone_command_dotted_group(tools_repo: ToolsRepo) -> None:
    """`group=` takes the dotted path of a nested group."""
    repo = tools_repo(
        {
            "tools/helm.py": (
                "from toolr import command, command_group\n"
                'command_group("ci", "CI", "CI")\n'
                'command_group("ci.helm", "Helm", "Helm")\n'
                '@command(name="snippet-checker", group="ci.helm")\n'
                "def check_snippets(ctx):\n"
                '    """Check the snippets."""\n'
            ),
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    group = groups["tools.ci.helm"]
    assert isinstance(group, CommandGroup)
    assert "snippet-checker" in group.get_commands()


def _unknown_group_error(repo: Path) -> str:
    """Discover `repo` and return the message `collected_command_groups()` raises."""
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        with pytest.raises(ValueError, match="unknown group references") as excinfo:
            tester.collected_command_groups()
    return str(excinfo.value)


def test_collected_command_groups_rejects_unknown_group(tools_repo: ToolsRepo) -> None:
    """A `group=` naming no declared group fails the read, as the manifest build does."""
    repo = tools_repo({"tools/cmds.py": TYPO_GROUP_MODULE})
    assert _unknown_group_error(repo) == (
        "unknown group references (1):\n"
        "  - tools.cmds::check: references group `ci.typo` which has no "
        "`command_group(...)` declaration."
    )


def test_unknown_group_error_names_commands_in_source_order(tools_repo: ToolsRepo) -> None:
    """Lines name the CLI command, hyphenated or explicit, in the order they're declared."""
    repo = tools_repo(
        {
            "tools/cmds.py": (
                "from toolr import command, command_group\n"
                'command_group("ci", "CI", "CI")\n'
                '@command(group="zebra-tools")\n'
                "def run_check(ctx):\n"
                '    """Check things."""\n'
                '@command(name="alpha-lint", group="alpha-tools")\n'
                "def lint(ctx):\n"
                '    """Lint things."""\n'
            ),
        }
    )
    assert _unknown_group_error(repo) == (
        "unknown group references (2):\n"
        "  - tools.cmds::run-check: references group `zebra-tools` which has no "
        "`command_group(...)` declaration.\n"
        "  - tools.cmds::alpha-lint: references group `alpha-tools` which has no "
        "`command_group(...)` declaration."
    )


@pytest.mark.parametrize(
    ("declared", "referenced"),
    [("ci", "cj"), ("my_ci", "my-ci")],
)
def test_unknown_group_error_suggests_a_near_miss(
    tools_repo: ToolsRepo, declared: str, referenced: str
) -> None:
    """A declared group within the manifest build's edit distance is suggested."""
    repo = tools_repo(
        {
            "tools/cmds.py": (
                "from toolr import command, command_group\n"
                f'command_group("{declared}", "G", "G")\n'
                f'@command(group="{referenced}")\n'
                "def check(ctx):\n"
                '    """Check things."""\n'
            ),
        }
    )
    assert _unknown_group_error(repo) == (
        "unknown group references (1):\n"
        f"  - tools.cmds::check: references group `{referenced}` which has no "
        f"`command_group(...)` declaration. Did you mean `{declared}`?"
    )


def test_unknown_group_error_follows_file_order(tools_repo: ToolsRepo) -> None:
    """Lines follow the sorted file paths, as the manifest build's walk does, not import order."""
    repo = tools_repo(
        {
            "tools/a_mod.py": (
                "from toolr import command\n"
                "from tools import z_mod  # noqa: F401\n"
                '@command(group="aaa-nope")\n'
                "def first(ctx):\n"
                '    """First."""\n'
            ),
            "tools/z_mod.py": (
                "from toolr import command\n"
                '@command(group="zzz-nope")\n'
                "def last(ctx):\n"
                '    """Last."""\n'
            ),
        }
    )
    assert _unknown_group_error(repo) == (
        "unknown group references (2):\n"
        "  - tools.a_mod::first: references group `aaa-nope` which has no "
        "`command_group(...)` declaration.\n"
        "  - tools.z_mod::last: references group `zzz-nope` which has no "
        "`command_group(...)` declaration."
    )


def test_unknown_group_error_escapes_invisible_characters(tools_repo: ToolsRepo) -> None:
    """A tab or a zero-width space in `group=` is shown as `\\u{..}`, as the manifest build does."""
    repo = tools_repo(
        {
            "tools/cmds.py": (
                "from toolr import command, command_group\n"
                'command_group("ci", "CI", "CI")\n'
                '@command(group="ci\\tx")\n'
                "def tabbed(ctx):\n"
                '    """Tab."""\n'
                '@command(group="c\\u200bi")\n'
                "def zero_width(ctx):\n"
                '    """Zero width."""\n'
            ),
        }
    )
    assert _unknown_group_error(repo) == (
        "unknown group references (2):\n"
        "  - tools.cmds::tabbed: references group `ci\\u{9}x` which has no "
        "`command_group(...)` declaration. Did you mean `ci`?\n"
        "  - tools.cmds::zero-width: references group `c\\u{200b}i` which has no "
        "`command_group(...)` declaration. Did you mean `ci`?"
    )


def test_unknown_group_error_for_an_empty_group(tools_repo: ToolsRepo) -> None:
    """`group=""` gets the manifest build's missing-kwarg message."""
    repo = tools_repo(
        {
            "tools/cmds.py": (
                "from toolr import command, command_group\n"
                'command_group("ci", "CI", "CI")\n'
                '@command(group="")\n'
                "def check(ctx):\n"
                '    """Check things."""\n'
            ),
        }
    )
    assert _unknown_group_error(repo) == (
        "unknown group references (1):\n"
        "  - tools.cmds::check: `@command` is missing a `group=...` kwarg."
    )


def test_pending_commands_do_not_leak_between_testers(tools_repo: ToolsRepo) -> None:
    """A pending command from one `CommandsTester` doesn't reach the next."""
    repo = tools_repo({"tools/cmds.py": TYPO_GROUP_MODULE})
    with CommandsTester(search_path=repo) as first:
        first.discover()
        assert [(entry.group, entry.name) for entry in first.pending_command_collector] == [
            ("ci.typo", "check")
        ]
    assert first.pending_command_collector == []
    # Through the module, as `command()` calls it, so a patch left running shows.
    assert _decorators._get_pending_command_storage() is not first.pending_command_collector
    other = repo / "other"
    (other / "tools").mkdir(parents=True)
    (other / "tools" / "__init__.py").write_text(
        'from toolr import command_group\ncommand_group("ci", "CI", "CI")\n'
        'command_group("ci.typo", "Typo", "Typo")\n'
    )
    with CommandsTester(search_path=other) as second:
        second.discover()
        group = second.collected_command_groups()["tools.ci.typo"]
        assert isinstance(group, CommandGroup)
        assert group.get_commands() == {}


DUP_ONE_MODULE = (
    "from toolr import command_group\n"
    'g = command_group("grp", "Grp", "Grp")\n'
    "@g.command\n"
    "def dup(ctx):\n"
    '    """One."""\n'
    '@g.command(name="dup")\n'
    "def dup2(ctx):\n"
    '    """Two."""\n'
)
DUP_MODULE = (
    "from toolr import command_group\n"
    'g = command_group("grp", "Grp", "Grp")\n'
    "@g.command\n"
    "def dup(ctx):\n"
    '    """Dup."""\n'
)


def _duplicate_error(repo: Path) -> str:
    """Discover `repo` and return the duplicate-commands message `collected_command_groups()` raises."""
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        with pytest.raises(ValueError, match="duplicate commands") as excinfo:
            tester.collected_command_groups()
    return str(excinfo.value)


def test_collected_command_groups_rejects_duplicate_in_one_module(tools_repo: ToolsRepo) -> None:
    """Two defs claiming `grp dup` in one module fail as the manifest build does."""
    repo = tools_repo({"tools/m.py": DUP_ONE_MODULE})
    assert _duplicate_error(repo) == (
        "duplicate commands (1):\n"
        "  - `grp dup` is defined more than once: tools.m::dup, tools.m::dup2. "
        'Rename all but one, e.g. with `name="…"`.'
    )


def test_collected_command_groups_rejects_duplicate_across_modules(tools_repo: ToolsRepo) -> None:
    """Identical modules list their definitions in file order, whatever the import order."""
    repo = tools_repo(
        {
            "tools/m1.py": "from tools import m2  # noqa: F401\n" + DUP_MODULE,
            "tools/m2.py": DUP_MODULE,
        }
    )
    assert _duplicate_error(repo) == (
        "duplicate commands (1):\n"
        "  - `grp dup` is defined more than once: tools.m1::dup, tools.m2::dup. "
        'Rename all but one, e.g. with `name="…"`.'
    )


def test_duplicate_error_lists_every_definition_and_escapes(tools_repo: ToolsRepo) -> None:
    """Three definitions of one command, standalone and bound, and an escaped name."""
    repo = tools_repo(
        {
            "tools/a.py": DUP_ONE_MODULE,
            "tools/b.py": (
                "from toolr import command\n"
                '@command("dup", group="grp")\n'
                "def other(ctx):\n"
                '    """Three."""\n'
                '@command(group="grp")\n'
                "def solo(ctx):\n"
                '    """Solo."""\n'
            ),
            "tools/c.py": (
                "from toolr import command_group\n"
                'g = command_group("other", "O", "O")\n'
                "@g.command\n"
                "def dup(ctx):\n"
                '    """Elsewhere."""\n'
                '@g.command("x\\x1b")\n'
                "def esc(ctx):\n"
                '    """Esc."""\n'
                '@g.command("x\\x1b")\n'
                "def esc2(ctx):\n"
                '    """Esc."""\n'
            ),
        }
    )
    assert _duplicate_error(repo) == (
        "duplicate commands (2):\n"
        "  - `grp dup` is defined more than once: tools.a::dup, tools.a::dup2, tools.b::other. "
        'Rename all but one, e.g. with `name="…"`.\n'
        "  - `other x\\u{1b}` is defined more than once: tools.c::esc, tools.c::esc2. "
        'Rename all but one, e.g. with `name="…"`.'
    )


def test_unknown_group_reported_before_duplicate(tools_repo: ToolsRepo) -> None:
    """A typo'd group is reported first, as the manifest build does."""
    repo = tools_repo({"tools/m.py": DUP_ONE_MODULE, "tools/t.py": TYPO_GROUP_MODULE})
    _unknown_group_error(repo)


def test_duplicate_commands_do_not_leak_between_testers(tools_repo: ToolsRepo) -> None:
    """A duplicate from one `CommandsTester` doesn't reach the next."""
    repo = tools_repo({"tools/m.py": DUP_ONE_MODULE})
    with CommandsTester(search_path=repo) as first:
        first.discover()
        assert len(first.duplicate_command_collector) == 1
    assert first.duplicate_command_collector == []
    assert _decorators._get_duplicate_command_storage() is not first.duplicate_command_collector
    other = repo / "other"
    (other / "tools").mkdir(parents=True)
    (other / "tools" / "__init__.py").write_text(CI_MODULE)
    with CommandsTester(search_path=other) as second:
        second.discover()
        assert set(second.collected_command_groups()) == {"tools.ci"}


UNSUPPORTED_MODULE = '''\
from toolr import command, command_group

GROUP = "grp"
NAME = "renamed"
g = command_group("grp", "Grp", "Grp")


def register():
    @g.command
    def nested(ctx):
        """Nested."""


register()


if True:

    @command(group="grp")
    def guarded(ctx):
        """Guarded."""


class Holder:
    @g.command
    def method(self, ctx):
        """Method."""


@command(group=GROUP)
def nonlit(ctx):
    """Non-literal group."""


@command(NAME, group="grp")
def renamed(ctx):
    """Non-literal name."""


@g.command(name=NAME)
def renamed_kw(ctx):
    """Non-literal name keyword."""


def other():
    @cli.command  # noqa: F821
    def ignored(ctx):
        pass
'''
NESTED_LINE = "`@command` must decorate a top-level `def`, which toolr reads without running it."


def _unsupported_error(repo: Path) -> str:
    """Discover `repo` and return the unsupported-declarations message."""
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        with pytest.raises(ValueError, match="unsupported command declarations") as excinfo:
            tester.collected_command_groups()
    return str(excinfo.value)


def test_collected_command_groups_rejects_unsupported_declarations(tools_repo: ToolsRepo) -> None:
    """Nested defs and non-literal `group=`/names fail with the manifest build's text."""
    repo = tools_repo({"tools/m.py": UNSUPPORTED_MODULE})
    assert _unsupported_error(repo) == (
        "unsupported command declarations (6):\n"
        f"  - tools.m::nested: {NESTED_LINE}\n"
        f"  - tools.m::guarded: {NESTED_LINE}\n"
        f"  - tools.m::method: {NESTED_LINE}\n"
        "  - tools.m::nonlit: `group=` must be a string literal.\n"
        "  - tools.m::renamed: the command name must be a string literal.\n"
        "  - tools.m::renamed_kw: the command name must be a string literal."
    )


def test_unsupported_declarations_follow_file_order_and_come_first(tools_repo: ToolsRepo) -> None:
    """Pending and duplicate commands are checked too, in file order, before unknown groups."""
    repo = tools_repo(
        {
            "tools/a.py": (
                "import toolr\n"
                "from tools import b  # noqa: F401\n"
                "def register():\n"
                '    @toolr.command(group="nope")\n'
                "    async def other(ctx):\n"
                '        """Other."""\n'
                "register()\n"
            ),
            "tools/b.py": (
                "from toolr import command_group\n"
                'g = command_group("grp", "Grp", "Grp")\n'
                "NAME = 'dup'\n"
                "@g.command\n"
                "def dup(ctx):\n"
                '    """Dup."""\n'
                "@g.command(NAME)\n"
                "def dup2(ctx):\n"
                '    """Dup."""\n'
            ),
        }
    )
    assert _unsupported_error(repo) == (
        "unsupported command declarations (2):\n"
        f"  - tools.a::other: {NESTED_LINE}\n"
        "  - tools.b::dup2: the command name must be a string literal."
    )


def test_shape_check_reads_sources_as_python_does(tmp_path: Path) -> None:
    """A UTF-8 byte order mark and a `# coding:` cookie don't break the shape check."""
    shutil.copytree(FILES / "source_encodings", tmp_path, dirs_exist_ok=True)
    (tmp_path / "tools" / "__init__.py").write_text("")
    with CommandsTester(search_path=tmp_path) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    group = groups["tools.grp"]
    assert isinstance(group, CommandGroup)
    assert sorted(group.get_commands()) == ["bom", "latin1"]


def test_commands_outside_tools_are_not_shape_checked(tools_repo: ToolsRepo) -> None:
    """A command a test body defines, nested and with a non-literal group, still registers."""
    repo = tools_repo({"tools/ci.py": CI_MODULE})
    group_name = "ci"
    with CommandsTester(search_path=repo) as tester:
        tester.discover()

        @command(group=group_name)
        def extra(ctx: Context) -> None:
            """Extra."""

        group = tester.collected_command_groups()["tools.ci"]
    assert isinstance(group, CommandGroup)
    assert set(group.get_commands()) == {"hello", "extra"}


def test_imperative_registration_is_not_shape_checked(tools_repo: ToolsRepo) -> None:
    """`g.command("x")(func)` has no command decorator to read; the tester registers it unreported.

    Registering one function twice under its name isn't a duplicate.
    """
    repo = tools_repo(
        {
            "tools/m.py": (
                "import types\n"
                "from toolr import command_group\n"
                'g = command_group("grp", "Grp", "Grp")\n'
                "helper = types.SimpleNamespace(command=lambda f: f)\n"
                "def passthrough(f):\n"
                "    return f\n"
                "@passthrough\n"
                "@helper.command\n"
                "def build(ctx):\n"
                '    """Build."""\n'
                'g.command("make")(build)\n'
                'g.command("make")(build)\n'
            ),
        }
    )
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        group = tester.collected_command_groups()["tools.grp"]
    assert isinstance(group, CommandGroup)
    assert set(group.get_commands()) == {"make"}


def test_wrapped_command_is_shape_checked(tools_repo: ToolsRepo) -> None:
    """A `functools.wraps` decorator under `@command` doesn't hide its `group=` from the check."""
    repo = tools_repo(
        {
            "tools/m.py": (
                "import functools\n"
                "from toolr import command, command_group\n"
                'GROUP = "grp"\n'
                "command_group('grp', 'Grp', 'Grp')\n"
                "def logged(f):\n"
                "    @functools.wraps(f)\n"
                "    def wrapper(*args, **kwargs):\n"
                "        return f(*args, **kwargs)\n"
                "    return wrapper\n"
                "@command(group=GROUP)\n"
                "@logged\n"
                "def deploy(ctx):\n"
                '    """Deploy."""\n'
            ),
        }
    )
    assert _unsupported_error(repo) == (
        "unsupported command declarations (1):\n"
        "  - tools.m::deploy: `group=` must be a string literal."
    )
