"""Tests for command discovery and parser building."""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from .conftest import RegistryTestCase

if TYPE_CHECKING:
    from toolr import Context

CASES_PATH = Path(__file__).parent / "cases"


def test_discover_commands_no_tools_dir(tmp_path):
    """Test discovery when tools directory doesn't exist."""
    with RegistryTestCase(tmp_path) as test_case:
        registry = test_case.registry

        # Should not raise an error
        registry._discover_commands()

        # Should have no command groups or pending commands
        assert len(registry._command_groups) == 0
        assert len(registry._pending_commands) == 0


def test_discover_simple_case():
    """Test discovery with the simple test case."""
    # Point to our test case directory
    test_case_dir = CASES_PATH / "simple"
    with RegistryTestCase(test_case_dir) as test_case:
        registry = test_case.registry

        # Run discovery
        registry._discover_commands()

        # Should have discovered command groups
        assert len(registry._command_groups) >= 2  # docker and git groups
        assert "tools.docker" in registry._command_groups
        assert "tools.git" in registry._command_groups

        # Should have pending commands
        assert len(registry._pending_commands) >= 4  # 2 docker + 2 git commands

        # Check specific command groups
        docker_group = registry._command_groups["tools.docker"]
        git_group = registry._command_groups["tools.git"]

        assert docker_group.title == "Docker Commands"
        assert git_group.title == "Git Commands"


def test_discover_nested_case():
    """Test discovery with nested command groups."""
    test_case_dir = CASES_PATH / "nested"
    with RegistryTestCase(test_case_dir) as test_case:
        registry = test_case.registry

        # Run discovery
        registry._discover_commands()

        # Should have discovered nested command groups
        assert "tools.docker" in registry._command_groups
        assert "tools.docker.build" in registry._command_groups
        assert "tools.docker.compose" in registry._command_groups

        # Check nested group relationships
        docker_group = registry._command_groups["tools.docker"]
        assert docker_group
        build_group = registry._command_groups["tools.docker.build"]
        compose_group = registry._command_groups["tools.docker.compose"]

        assert build_group.parent == "tools.docker"
        assert compose_group.parent == "tools.docker"
        assert build_group.full_name == "tools.docker.build"
        assert compose_group.full_name == "tools.docker.compose"


def test_discover_mixed_case():
    """Test discovery with mixed command structures."""
    test_case_dir = CASES_PATH / "mixed"
    with RegistryTestCase(test_case_dir) as test_case:
        registry = test_case.registry

        # Run discovery
        registry._discover_commands()

        # Should have top-level groups
        assert "tools.deployment" in registry._command_groups
        assert "tools.utils" in registry._command_groups

        # Should have nested groups
        assert "tools.deployment.k8s" in registry._command_groups
        assert "tools.deployment.aws" in registry._command_groups

        # Check that we have both top-level and nested commands
        deployment_commands = [
            full_name for (full_name, _, _) in registry._pending_commands if full_name == "tools.deployment"
        ]
        k8s_commands = [
            full_name for (full_name, _, _) in registry._pending_commands if full_name == "tools.deployment.k8s"
        ]

        assert len(deployment_commands) >= 2  # status, rollback
        assert len(k8s_commands) >= 2  # deploy, scale


def test_build_parsers_empty_registry(registry):
    """Test building parsers with an empty registry."""
    # Verify initial state
    assert not registry._built
    assert len(registry._command_groups) == 0
    assert len(registry._pending_commands) == 0

    # Building parsers should complete without errors
    registry._build_parsers()

    # Should be marked as built
    assert registry._built
    # Should still have no command groups or pending commands
    assert len(registry._command_groups) == 0
    assert len(registry._pending_commands) == 0


def test_build_simple_command_group(registry):
    """Test building parsers for a simple command group."""

    # Create a simple command group with a command
    group = registry.command_group("test", "Test", "Test description")

    @group.command("hello")
    def hello_cmd(ctx: Context):
        """Say hello."""
        return "hello"

    # Verify initial state
    assert not registry._built
    assert len(registry._command_groups) == 1
    assert len(registry._pending_commands) == 1
    assert "tools.test" in registry._command_groups

    # Building parsers should complete without errors
    registry._build_parsers()

    # Should be marked as built
    assert registry._built
    # Command groups and commands should still be registered
    assert len(registry._command_groups) == 1
    assert len(registry._pending_commands) == 1
    # Verify the command group details
    test_group = registry._command_groups["tools.test"]
    assert test_group.name == "test"
    assert test_group.title == "Test"
    assert test_group.description == "Test description"


def test_build_nested_command_groups(registry):
    """Test building parsers for nested command groups."""

    # Create nested command groups
    parent = registry.command_group("parent", "Parent", "Parent desc")
    child = parent.command_group("child", "Child", "Child desc")

    @parent.command("parent_cmd")
    def parent_cmd(ctx: Context):
        """Parent command."""
        return "parent"

    @child.command("child_cmd")
    def child_cmd(ctx: Context):
        """Child command."""
        return "child"

    # Verify initial state
    assert not registry._built
    assert len(registry._command_groups) == 2  # parent and child
    assert len(registry._pending_commands) == 2  # parent_cmd and child_cmd
    assert "tools.parent" in registry._command_groups
    assert "tools.parent.child" in registry._command_groups

    # Building parsers should complete without errors
    registry._build_parsers()

    # Should be marked as built
    assert registry._built
    # Verify nested structure is maintained
    parent_group = registry._command_groups["tools.parent"]
    child_group = registry._command_groups["tools.parent.child"]
    assert parent_group.name == "parent"
    assert child_group.name == "child"
    assert child_group.parent == "tools.parent"
    assert child_group.full_name == "tools.parent.child"


def test_build_parsers_called_once(registry):
    """Test that _build_parsers can be called multiple times safely."""

    group = registry.command_group("test", "Test", "Test desc")

    @group.command("cmd")
    def test_cmd(ctx: Context):
        """Test command."""
        return "test"

    # Verify initial state
    assert not registry._built
    assert len(registry._command_groups) == 1
    assert len(registry._pending_commands) == 1

    # Call build_parsers first time
    registry._build_parsers()
    assert registry._built

    # State should remain the same after first build
    first_build_groups = len(registry._command_groups)
    first_build_commands = len(registry._pending_commands)

    # Call build_parsers second time
    registry._build_parsers()

    # Should still be built and state should be unchanged
    assert registry._built
    assert len(registry._command_groups) == first_build_groups
    assert len(registry._pending_commands) == first_build_commands


def test_discover_and_build_integration():
    """Test the full discover_and_build integration."""
    # Use the simple test case for real integration testing
    test_case_dir = CASES_PATH / "simple"
    with RegistryTestCase(test_case_dir) as test_case:
        registry = test_case.registry

        # Verify initial state
        assert not registry._built
        assert len(registry._command_groups) == 0
        assert len(registry._pending_commands) == 0

        # Run full discover and build
        registry.discover_and_build()

        # Should be marked as built
        assert registry._built

        # Should have discovered command groups from the test case
        assert len(registry._command_groups) >= 2  # docker and git groups
        assert "tools.docker" in registry._command_groups
        assert "tools.git" in registry._command_groups

        # Should have discovered commands
        assert len(registry._pending_commands) >= 4  # 2 docker + 2 git commands


def test_discover_and_build_with_manual_groups(registry):
    """Test discover_and_build works with manually created groups."""

    # Create some command groups manually before discovery
    group1 = registry.command_group("group1", "Group 1", "Description 1")
    group2 = registry.command_group("group2", "Group 2", "Description 2")

    @group1.command("cmd1")
    def cmd1(ctx: Context):
        """Command 1."""
        return "cmd1"

    @group2.command("cmd2")
    def cmd2(ctx: Context):
        """Command 2."""
        return "cmd2"

    # Verify initial state
    assert not registry._built
    assert len(registry._command_groups) == 2
    assert len(registry._pending_commands) == 2

    # Run discover_and_build (will discover nothing from empty tmp_path)
    registry.discover_and_build()

    # Should be marked as built
    assert registry._built

    # Should still have our manually created command groups
    assert "tools.group1" in registry._command_groups
    assert "tools.group2" in registry._command_groups
    assert len(registry._command_groups) == 2
    assert len(registry._pending_commands) == 2
