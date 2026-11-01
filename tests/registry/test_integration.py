"""Integration tests demonstrating complete registry usage."""

from __future__ import annotations

import sys
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr import Context
from toolr import command_group
from toolr._decorators import CommandGroup
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
def tools_repo(tmp_path: Path) -> Callable[[dict[str, str]], Path]:
    """Write a fixture repo under `tmp_path` and return its root.

    `tools/__init__.py` is written empty unless the mapping sets it; every
    other `{relative_path: source}` entry is written as given.
    """

    def factory(files: dict[str, str]) -> Path:
        (tmp_path / "tools").mkdir()
        files = {"tools/__init__.py": "", **files}
        for relative, source in files.items():
            target = tmp_path / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source)
        return tmp_path

    return factory


def test_discover_raises_after_walking_every_module(
    tools_repo: Callable[[dict[str, str]], Path],
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
    tools_repo: Callable[[dict[str, str]], Path],
) -> None:
    """An undocumented command fails discovery the way the manifest build does."""
    repo = tools_repo({"tools/bad.py": BAD_MODULE})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert excinfo.group_contains(ValueError, match="add a docstring")


def test_discover_reports_every_failing_module(
    tools_repo: Callable[[dict[str, str]], Path],
) -> None:
    """Every broken module is reported in one group, not just the first."""
    repo = tools_repo({"tools/bad.py": BAD_MODULE, "tools/broken.py": BROKEN_MODULE})
    with CommandsTester(search_path=repo) as tester, pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()
    assert len(excinfo.value.exceptions) == 2
    assert "2 module(s)" in str(excinfo.value)


def test_discover_raises_when_tools_package_import_fails(
    tools_repo: Callable[[dict[str, str]], Path],
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
    tools_repo: Callable[[dict[str, str]], Path],
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
    tools_repo: Callable[[dict[str, str]], Path], monkeypatch: pytest.MonkeyPatch
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


def test_discover_ignores_tools_dirs_elsewhere_on_sys_path(tmp_path: Path, host_repo: Path) -> None:
    """A namespace `tools/` fixture doesn't merge with the host's `tools` package."""
    fixture = tmp_path / "fixture"
    (fixture / "tools").mkdir(parents=True)
    (fixture / "tools" / "ci.py").write_text(CI_MODULE)
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
    tools_repo: Callable[[dict[str, str]], Path],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """`sys.path` entries that can't shadow `tools` (editable installs,
    `PYTHONPATH`) stay importable during discovery."""
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
