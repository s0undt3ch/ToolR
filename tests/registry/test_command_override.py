"""Tests for command overriding and augmentation between local and 3rd-party commands."""

from __future__ import annotations

import inspect

import pytest

from toolr import Context
from toolr import command_group
from toolr.testing import CommandsTester


@pytest.fixture
def skip_loading_entry_points():
    """Skip loading entry points."""
    return False


def test_local_commands_override_3rd_party_commands(commands_tester: CommandsTester):
    """Test that local commands override 3rd-party commands with the same name."""
    # First, verify that 3rd-party commands exist
    command_groups = commands_tester.collected_command_groups()
    assert "tools.third-party" in command_groups
    assert "tools.utils" in command_groups

    # Get the original 3rd-party commands
    third_party_group = command_groups["tools.third-party"]
    utils_group = command_groups["tools.utils"]

    # Find the original hello command from 3rd-party
    original_commands = third_party_group.get_commands()
    assert "hello" in original_commands
    original_hello = original_commands["hello"]

    # Find the original echo command from 3rd-party
    original_utils_commands = utils_group.get_commands()
    assert "echo" in original_utils_commands
    original_echo = original_utils_commands["echo"]

    # Now create local commands with the same names to override them
    local_third_party = command_group("third-party", "Local Third Party", "Local third party tools")

    @local_third_party.command("hello")
    def local_hello(ctx: Context, name: str = "Local World") -> None:
        """Local hello command that overrides 3rd-party."""

    local_utils = command_group("utils", "Local Utils", "Local utility commands")

    @local_utils.command("echo")
    def local_echo(ctx: Context, message: str, repeat: int = 1) -> None:
        """Local echo command that overrides 3rd-party."""

    # Verify that the local commands have overridden the 3rd-party ones
    updated_groups = commands_tester.collected_command_groups()

    # The groups should be the same (extended), but commands should be overridden
    assert "tools.third-party" in updated_groups
    assert "tools.utils" in updated_groups

    # Check that the hello command has been overridden
    updated_third_party = updated_groups["tools.third-party"]
    updated_commands = updated_third_party.get_commands()
    assert "hello" in updated_commands
    updated_hello = updated_commands["hello"]
    assert updated_hello == local_hello  # Should be our local function
    assert updated_hello != original_hello  # Should not be the original

    # Check that the echo command has been overridden
    updated_utils = updated_groups["tools.utils"]
    updated_utils_commands = updated_utils.get_commands()
    assert "echo" in updated_utils_commands
    updated_echo = updated_utils_commands["echo"]
    assert updated_echo == local_echo  # Should be our local function
    assert updated_echo != original_echo  # Should not be the original


def test_local_commands_extend_3rd_party_groups(commands_tester: CommandsTester):
    """Test that local commands can extend existing 3rd-party command groups."""
    # First, verify the original 3rd-party structure
    command_groups = commands_tester.collected_command_groups()
    assert "tools.third-party" in command_groups
    assert "tools.utils" in command_groups

    original_third_party_commands = list(command_groups["tools.third-party"].get_commands())
    original_utils_commands = list(command_groups["tools.utils"].get_commands())

    # Verify original commands exist
    assert "hello" in original_third_party_commands
    assert "version" in original_third_party_commands
    assert "echo" in original_utils_commands
    assert "info" in original_utils_commands

    # Create command groups with the same names - this should EXTEND the existing groups
    third_party_group = command_group("third-party", "Third Party Tools", "Third party tools")

    @third_party_group.command("local-only")
    def local_only_command(ctx: Context) -> None:
        """A local-only command in the third-party group."""

    utils_group = command_group("utils", "Utility Commands", "General utility commands")

    @utils_group.command("local-utility")
    def local_utility_command(ctx: Context, value: str) -> None:
        """A local utility command."""

    # Verify that the groups now contain both original and new commands (extension, not replacement)
    updated_groups = commands_tester.collected_command_groups()

    updated_third_party_commands = list(updated_groups["tools.third-party"].get_commands())
    updated_utils_commands = list(updated_groups["tools.utils"].get_commands())

    # Original commands should still be there (extension, not replacement)
    for cmd in original_third_party_commands:
        assert cmd in updated_third_party_commands
    for cmd in original_utils_commands:
        assert cmd in updated_utils_commands

    # New commands should be added
    assert "local-only" in updated_third_party_commands
    assert "local-utility" in updated_utils_commands

    # Verify the new commands are the ones we defined
    updated_third_party_commands_dict = updated_groups["tools.third-party"].get_commands()
    updated_utils_commands_dict = updated_groups["tools.utils"].get_commands()

    assert updated_third_party_commands_dict["local-only"] == local_only_command
    assert updated_utils_commands_dict["local-utility"] == local_utility_command


def test_local_commands_create_new_groups(commands_tester: CommandsTester):
    """Test that local commands can create entirely new command groups."""
    # Create a completely new local command group
    local_group = command_group("local-tools", "Local Tools", "Local development tools")

    @local_group.command("build")
    def build_command(ctx: Context, target: str = "all") -> None:
        """Build the project."""

    @local_group.command("test")
    def test_command(ctx: Context, suite: str = "unit") -> None:
        """Run tests."""

    # Verify the new group exists alongside 3rd-party groups
    command_groups = commands_tester.collected_command_groups()

    # 3rd-party groups should still exist
    assert "tools.third-party" in command_groups
    assert "tools.utils" in command_groups

    # New local group should exist
    assert "tools.local-tools" in command_groups

    # Verify the new group has our commands
    local_tools_group = command_groups["tools.local-tools"]
    local_commands = local_tools_group.get_commands()
    assert "build" in local_commands
    assert "test" in local_commands

    # Verify the functions are correct
    assert local_commands["build"] == build_command
    assert local_commands["test"] == test_command


def test_command_discovery_order_preserves_local_overrides(commands_tester: CommandsTester):
    """Test that the discovery order ensures local commands override 3rd-party ones."""
    # This test verifies that local commands take precedence over 3rd-party ones
    # due to the discovery order (3rd-party first, then local)

    # First, create local commands that would conflict with 3rd-party ones
    local_third_party = command_group("third-party", "Local Third Party", "Local third party tools")

    @local_third_party.command("hello")
    def local_hello_override(ctx: Context, name: str = "Override") -> None:
        """Local hello that should override 3rd-party."""

    @local_third_party.command("version")
    def local_version_override(ctx: Context) -> None:
        """Local version that should override 3rd-party."""

    # Now verify that our local commands are the ones that exist in the registry
    command_groups = commands_tester.collected_command_groups()
    third_party_group = command_groups["tools.third-party"]
    third_party_commands = third_party_group.get_commands()

    # Check that our local commands are the ones registered
    assert "hello" in third_party_commands
    assert "version" in third_party_commands
    assert third_party_commands["hello"] == local_hello_override
    assert third_party_commands["version"] == local_version_override


def test_nested_command_groups_override_and_extend(commands_tester: CommandsTester):
    """Test that nested command groups can be overridden and extended."""
    # Create a nested structure that mirrors potential 3rd-party structure
    parent_group = command_group("parent", "Parent Group", "Parent command group")

    # Create a child group that might conflict with 3rd-party
    child_group = parent_group.command_group("child", "Child Group", "Child command group")

    @child_group.command("action")
    def local_action(ctx: Context, param: str) -> None:
        """Local action command."""

    @child_group.command("new-action")
    def new_action(ctx: Context) -> None:
        """New action command."""

    # Verify the nested structure exists
    command_groups = commands_tester.collected_command_groups()
    assert "tools.parent" in command_groups
    assert "tools.parent.child" in command_groups

    # Verify the commands are registered
    child_group_commands = command_groups["tools.parent.child"].get_commands()

    assert "action" in child_group_commands
    assert "new-action" in child_group_commands

    # Verify the functions are correct
    assert child_group_commands["action"] == local_action
    assert child_group_commands["new-action"] == new_action


def test_command_group_extension_behavior(commands_tester: CommandsTester):
    """Test that creating a command group with the same name extends the existing group."""
    # This test demonstrates the new behavior where command groups are extended, not replaced

    # First, verify we have 3rd-party commands
    command_groups = commands_tester.collected_command_groups()
    assert "tools.third-party" in command_groups

    original_group = command_groups["tools.third-party"]
    original_commands = list(original_group.get_commands())
    assert len(original_commands) >= 2  # Should have hello and version at least

    # Create a new group with the same name - this should EXTEND the existing group
    extension_group = command_group("third-party", "Extension Group", "This extends the original")

    @extension_group.command("extension-command")
    def extension_command(ctx: Context) -> None:
        """An extension command."""

    # Verify the group has been extended, not replaced
    updated_groups = commands_tester.collected_command_groups()
    updated_group = updated_groups["tools.third-party"]

    # The group should still have the original title and description (not replaced)
    # Note: The first group created sets the title/description, subsequent calls return the existing group
    assert updated_group.title == "Third Party Tools"  # Original title
    assert updated_group.description == "Tools from third-party packages"  # Original description

    # All original commands should still be there
    updated_commands = updated_group.get_commands()
    for original_cmd in original_commands:
        assert original_cmd in updated_commands

    # The new command should be added
    assert "extension-command" in updated_commands

    # Verify the command function is correct
    assert updated_commands["extension-command"] == extension_command


def test_real_world_override_scenario(commands_tester: CommandsTester):
    """Test a realistic scenario where local commands override and extend 3rd-party ones."""
    # This test simulates a real-world scenario where you have 3rd-party tools
    # and want to customize some commands while adding new ones

    # First, verify we have 3rd-party commands
    command_groups = commands_tester.collected_command_groups()
    assert "tools.third-party" in command_groups
    assert "tools.utils" in command_groups

    # Scenario 1: Override a 3rd-party command with local implementation
    local_third_party = command_group("third-party", "Third Party Tools", "Third party tools")

    @local_third_party.command("hello")
    def custom_hello(ctx: Context, name: str = "Custom") -> None:
        """Custom hello command that overrides 3rd-party."""

    # Scenario 2: Create a new command group for local tools
    dev_tools = command_group("dev", "Development Tools", "Local development tools")

    @dev_tools.command("lint")
    def lint_command(ctx: Context, files: str = ".") -> None:
        """Run linting on the codebase."""

    @dev_tools.command("format")
    def format_command(ctx: Context) -> None:
        """Format the codebase."""

    # Scenario 3: Extend an existing group with new commands
    utils_group = command_group("utils", "Utility Commands", "General utility commands")

    @utils_group.command("validate")
    def validate_command(ctx: Context, config: str) -> None:
        """Validate configuration."""

    # Verify the final state
    final_groups = commands_tester.collected_command_groups()

    # The third-party group should have our custom hello command
    third_party_commands = final_groups["tools.third-party"].get_commands()
    assert "hello" in third_party_commands

    # Verify it's our custom implementation
    assert third_party_commands["hello"] == custom_hello

    # The new dev group should exist with our commands
    assert "tools.dev" in final_groups
    dev_commands = final_groups["tools.dev"].get_commands()
    assert "lint" in dev_commands
    assert "format" in dev_commands

    # The utils group should have the original commands plus our new one
    utils_commands = final_groups["tools.utils"].get_commands()
    assert "echo" in utils_commands  # Original command
    assert "info" in utils_commands  # Original command
    assert "validate" in utils_commands  # Our new command

    # Verify our new validate command
    assert utils_commands["validate"] == validate_command


def test_multiple_overrides_same_command(commands_tester: CommandsTester):
    """Test that multiple overrides of the same command work correctly."""
    # This test verifies that the last override wins

    # First, create an initial override
    group1 = command_group("third-party", "First Override", "First override group")

    @group1.command("hello")
    def first_hello(ctx: Context) -> None:
        """First hello override."""

    # Then create a second override
    group2 = command_group("third-party", "Second Override", "Second override group")

    @group2.command("hello")
    def second_hello(ctx: Context) -> None:
        """Second hello override."""

    # Verify that the last override wins
    command_groups = commands_tester.collected_command_groups()
    third_party_commands = command_groups["tools.third-party"].get_commands()

    assert "hello" in third_party_commands
    assert third_party_commands["hello"] == second_hello  # Last override wins
    assert third_party_commands["hello"] != first_hello  # Not the first override


def test_command_group_metadata_preservation(commands_tester: CommandsTester):
    """Test that command group metadata is preserved when extending groups."""
    # This test verifies that the first group created sets the metadata

    # Create a group with specific metadata
    original_group = command_group("metadata-test", "Original Title", "Original description")

    # Try to create another group with the same name but different metadata
    duplicate_group = command_group("metadata-test", "Different Title", "Different description")

    # Verify that the original metadata is preserved
    command_groups = commands_tester.collected_command_groups()
    final_group = command_groups["tools.metadata-test"]

    assert final_group.title == "Original Title"
    assert final_group.description == "Original description"

    # Verify that both groups reference the same object
    assert original_group is final_group
    assert duplicate_group is final_group


def test_command_override_with_different_signatures(commands_tester: CommandsTester):
    """Test that command overrides can have different signatures than the original."""
    # Override a 3rd-party command with a different signature
    local_third_party = command_group("third-party", "Third Party Tools", "Third party tools")

    @local_third_party.command("hello")
    def custom_hello(ctx: Context, name: str = "Custom", greeting: str = "Hello") -> None:
        """Custom hello with different signature."""

    # Verify the override works
    command_groups = commands_tester.collected_command_groups()
    third_party_commands = command_groups["tools.third-party"].get_commands()

    assert "hello" in third_party_commands
    assert third_party_commands["hello"] == custom_hello

    # The function should have the new signature
    sig = inspect.signature(custom_hello)
    assert "greeting" in sig.parameters
    assert "name" in sig.parameters
