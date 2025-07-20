"""Integration tests demonstrating complete registry usage."""

from __future__ import annotations

import pytest

from toolr._parser import Parser
from toolr._registry import CommandRegistry

from .conftest import RegistryTestCase


@pytest.fixture
def registry(tmp_path):
    """Create a fresh registry with a real parser for each test."""
    with RegistryTestCase(tmp_path) as _registry_test_case:
        yield _registry_test_case.registry


def test_complete_workflow_example(registry):
    """Test a complete workflow demonstrating the registry usage."""
    # Simulate building a Docker tooling hierarchy
    docker_group = registry.command_group("docker", "Docker Tools", "Docker container management")

    # Add some direct commands to docker group
    @docker_group.command("version")
    def docker_version(args):
        """Show Docker version."""
        return "Docker version 20.10.7"

    @docker_group.command("info")
    def docker_info(args):
        """Show Docker system info."""
        return "Docker system info"

    # Create nested command groups
    build_group = docker_group.command_group("build", "Build Tools", "Docker image building")
    compose_group = docker_group.command_group("compose", "Compose Tools", "Docker Compose operations")

    # Add commands to build group
    @build_group.command("image")
    def build_image(args):
        """Build an image."""
        return "Building Docker image"

    @build_group.command("cache")
    def build_cache(args):
        """Manage build cache."""
        return "Managing build cache"

    # Add commands to compose group
    @compose_group.command("up")
    def compose_up(args):
        """Start services."""
        return "Starting services"

    @compose_group.command("down")
    def compose_down(args):
        """Stop services."""
        return "Stopping services"

    # Create deeply nested group
    advanced_group = build_group.command_group("advanced", "Advanced Build", "Advanced build features")

    @advanced_group.command("multi-stage")
    def multi_stage_build(args):
        """Multi-stage build."""
        return "Multi-stage build"

    # Verify the structure was created correctly
    assert len(registry._command_groups) == 4
    assert "tools.docker" in registry._command_groups
    assert "tools.docker.build" in registry._command_groups
    assert "tools.docker.compose" in registry._command_groups
    assert "tools.docker.build.advanced" in registry._command_groups

    # Verify command registration
    assert len(registry._pending_commands) == 7

    # Verify group relationships
    assert docker_group.full_name == "tools.docker"
    assert build_group.full_name == "tools.docker.build"
    assert compose_group.full_name == "tools.docker.compose"
    assert advanced_group.full_name == "tools.docker.build.advanced"

    assert build_group.parent == "tools.docker"
    assert compose_group.parent == "tools.docker"
    assert advanced_group.parent == "tools.docker.build"


def test_real_world_tool_structure(registry):
    """Test a realistic tool structure like you might see in a real project."""
    # Development tools
    dev_group = registry.command_group("dev", "Development", "Development workflow tools")

    @dev_group.command("setup")
    def dev_setup(args):
        """Set up development environment."""
        return "Setting up dev environment"

    @dev_group.command("clean")
    def dev_clean(args):
        """Clean development artifacts."""
        return "Cleaning dev artifacts"

    # Testing tools
    test_group = dev_group.command_group("test", "Testing", "Test execution and management")

    @test_group.command("unit")
    def test_unit(args):
        """Run unit tests."""
        return "Running unit tests"

    @test_group.command("integration")
    def test_integration(args):
        """Run integration tests."""
        return "Running integration tests"

    @test_group.command("e2e")
    def test_e2e(args):
        """Run end-to-end tests."""
        return "Running e2e tests"

    # Coverage tools under testing
    coverage_group = test_group.command_group("coverage", "Coverage", "Test coverage tools")

    @coverage_group.command("report")
    def coverage_report(args):
        """Generate coverage report."""
        return "Generating coverage report"

    @coverage_group.command("html")
    def coverage_html(args):
        """Generate HTML coverage report."""
        return "Generating HTML coverage"

    # CI/CD tools
    ci_group = registry.command_group("ci", "CI/CD", "Continuous integration and deployment")

    @ci_group.command("validate")
    def ci_validate(args):
        """Validate CI configuration."""
        return "Validating CI config"

    # Deployment under CI
    deploy_group = ci_group.command_group("deploy", "Deploy", "Deployment operations")

    @deploy_group.command("staging")
    def deploy_staging(args):
        """Deploy to staging."""
        return "Deploying to staging"

    @deploy_group.command("production")
    def deploy_production(args):
        """Deploy to production."""
        return "Deploying to production"

    # Verify the complex structure
    expected_groups = ["tools.dev", "tools.dev.test", "tools.dev.test.coverage", "tools.ci", "tools.ci.deploy"]

    for group_name in expected_groups:
        assert group_name in registry._command_groups
        group = registry._command_groups[group_name]
        assert group.full_name == group_name

    # Verify we have all the expected commands
    assert len(registry._pending_commands) == 10

    # Test specific command paths
    coverage_commands = [
        (full_name, cmd) for (full_name, cmd, _) in registry._pending_commands if full_name == "tools.dev.test.coverage"
    ]
    assert len(coverage_commands) == 2
    assert any(cmd == "report" for (_, cmd) in coverage_commands)
    assert any(cmd == "html" for (_, cmd) in coverage_commands)


def test_command_group_hierarchy_storage(registry):
    """Test that command groups are properly stored in hierarchical structure."""
    # Create a hierarchy (avoid naming conflict with 'tools' prefix)
    app = registry.command_group("app", "Application", "Application tools")
    docker = app.command_group("docker", "Docker", "Docker tools")
    build = docker.command_group("build", "Build", "Build tools")
    advanced = build.command_group("advanced", "Advanced", "Advanced build")

    # Test that groups are stored at correct paths
    assert registry._command_groups["tools.app"] == app
    assert registry._command_groups["tools.app.docker"] == docker
    assert registry._command_groups["tools.app.docker.build"] == build
    assert registry._command_groups["tools.app.docker.build.advanced"] == advanced

    # Test that non-existent paths are not in the registry
    assert "nonexistent" not in registry._command_groups
    assert "tools.nonexistent" not in registry._command_groups
    assert "tools.app.docker.nonexistent" not in registry._command_groups
    assert "tools.app.docker.build.advanced.nonexistent" not in registry._command_groups


def test_multiple_registries_isolation(tmp_path):
    """Test that multiple registry instances are properly isolated."""

    parser1 = Parser(repo_root=tmp_path / "registry1")
    parser2 = Parser(repo_root=tmp_path / "registry2")
    registry1 = CommandRegistry(_parser=parser1)
    registry2 = CommandRegistry(_parser=parser2)

    # Create different structures in each registry
    group1 = registry1.command_group("test1", "Test 1", "Test registry 1")
    group2 = registry2.command_group("test2", "Test 2", "Test registry 2")

    @group1.command("cmd1")
    def cmd1(args):
        """Command 1."""
        return "cmd1"

    @group2.command("cmd2")
    def cmd2(args):
        """Command 2."""
        return "cmd2"

    # Verify isolation
    assert len(registry1._command_groups) == 1
    assert len(registry2._command_groups) == 1
    assert "tools.test1" in registry1._command_groups
    assert "tools.test2" in registry2._command_groups
    assert "tools.test1" not in registry2._command_groups
    assert "tools.test2" not in registry1._command_groups

    assert len(registry1._pending_commands) == 1
    assert len(registry2._pending_commands) == 1
    assert registry1._pending_commands[0][1] == "cmd1"
    assert registry2._pending_commands[0][1] == "cmd2"
