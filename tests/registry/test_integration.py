"""Integration tests demonstrating complete registry usage."""

from __future__ import annotations

import sys
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr import Context
from toolr import _decorators
from toolr import command_group
from toolr._decorators import CommandGroup
from toolr._exc import ToolrError
from toolr.testing import CommandsTester
from toolr.testing._discovery import _isolated_sys_path

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


@pytest.mark.skipif(sys.platform == "win32", reason="symlinks need a privilege on Windows")
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


def test_discover_keeps_sys_path_entries_without_tools(
    tmp_path: Path,
    tools_repo: ToolsRepo,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """`sys.path` entries that can't shadow `tools` (src-layout editable
    installs, `PYTHONPATH`) stay importable during discovery."""
    libdir = tmp_path / "libdir"
    libdir.mkdir()
    (libdir / "fixture_helper.py").write_text("VALUE = 1\n")
    monkeypatch.syspath_prepend(libdir)
    repo = tools_repo({"tools/ci.py": "import fixture_helper\n" + CI_MODULE})
    with CommandsTester(search_path=repo) as tester:
        tester.discover()
        groups = tester.collected_command_groups()
    assert "tools.ci" in groups


def test_isolated_sys_path_drops_tools_providers_and_cwd(tmp_path: Path) -> None:
    """Entries that provide a `tools` module, the cwd entry and the search
    path itself are dropped; the search path goes first."""
    search_path = tmp_path / "search"
    with_pkg = tmp_path / "with_pkg"
    (with_pkg / "tools").mkdir(parents=True)
    with_module = tmp_path / "with_module"
    with_module.mkdir()
    (with_module / "tools.py").write_text("")
    plain = tmp_path / "plain"
    plain.mkdir()
    saved = ["", str(with_pkg), str(search_path), str(with_module), str(plain)]
    assert _isolated_sys_path(search_path, saved) == [str(search_path), str(plain)]


def test_isolated_sys_path_matches_the_tools_name_exactly(tmp_path: Path) -> None:
    """A `Tools/` directory doesn't count as a `tools` provider, even on a
    case-insensitive filesystem, and a missing entry is kept as is."""
    capitalised = tmp_path / "capitalised"
    (capitalised / "Tools").mkdir(parents=True)
    missing = tmp_path / "missing"
    search_path = tmp_path / "search"
    assert _isolated_sys_path(search_path, [str(capitalised), str(missing)]) == [
        str(search_path),
        str(capitalised),
        str(missing),
    ]


def test_isolated_sys_path_keeps_site_packages_with_tools(tmp_path: Path) -> None:
    """A site-packages directory is kept even when it holds a `tools` package."""
    site_packages = tmp_path / "lib" / "site-packages"
    (site_packages / "tools").mkdir(parents=True)
    search_path = tmp_path / "search"
    assert _isolated_sys_path(search_path, [str(site_packages)]) == [
        str(search_path),
        str(site_packages),
    ]


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
