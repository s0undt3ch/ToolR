"""Basic tests for the command registry functionality."""

from __future__ import annotations

from toolr import Context
from toolr import command_group
from toolr._registry import CommandGroup


def test_create_simple_command_group(commands_tester):
    """Test creating a simple command group."""
    group = command_group("test", "Test Commands", "Test command description")

    assert isinstance(group, CommandGroup)
    assert group.name == "test"
    assert group.title == "Test Commands"
    assert group.description == "Test command description"
    assert group.parent == "tools"  # Default parent is now "tools"
    assert group.full_name == "tools.test"
    command_groups = commands_tester.collected_command_groups()
    assert command_groups["tools.test"] == group


def test_create_nested_command_group(commands_tester):
    """Test creating nested command groups."""
    # Create parent group (gets tools. prefix automatically)
    parent_group = command_group("parent", "Parent Commands", "Parent description")

    # Create child group
    child_group = parent_group.command_group("child", "Child Commands", "Child description")

    assert child_group.name == "child"
    assert child_group.parent == "tools.parent"
    assert child_group.full_name == "tools.parent.child"
    command_groups = commands_tester.collected_command_groups()
    assert command_groups["tools.parent.child"] == child_group


def test_deeply_nested_command_groups(commands_tester):
    """Test creating deeply nested command groups."""
    # Create hierarchy: tools.parent -> tools.parent.child -> tools.parent.child.grandchild
    parent = command_group("parent", "Parent", "Parent desc")
    child = parent.command_group("child", "Child", "Child desc")
    grandchild = child.command_group("grandchild", "Grandchild", "Grandchild desc")

    assert grandchild.full_name == "tools.parent.child.grandchild"
    command_groups = commands_tester.collected_command_groups()
    assert command_groups["tools.parent.child.grandchild"] == grandchild


def test_command_registration(commands_tester):
    """Test registering commands on a command group."""
    group = command_group("test", "Test Commands", "Test description")

    @group.command("hello")
    def hello_cmd(ctx: Context):
        """Say hello."""

    # Check that the command was registered
    command_groups = commands_tester.collected_command_groups()
    tools_test_group = command_groups["tools.test"]
    commands = tools_test_group.get_commands()
    assert len(commands) == 1
    assert "hello" in commands
    assert commands["hello"] == hello_cmd


def test_multiple_commands_same_group(commands_tester):
    """Test registering multiple commands on the same group."""
    group = command_group("test", "Test Commands", "Test description")

    @group.command("cmd1")
    def cmd1(ctx: Context):
        """Command 1."""

    @group.command("cmd2")
    def cmd2(ctx: Context):
        """Command 2."""

    command_groups = commands_tester.collected_command_groups()
    commands = command_groups["tools.test"].get_commands()
    assert len(commands) == 2
    assert "cmd1" in commands
    assert commands["cmd1"] == cmd1
    assert "cmd2" in commands
    assert commands["cmd2"] == cmd2


def test_commands_on_nested_groups(commands_tester):
    """Test registering commands on nested groups."""
    parent = command_group("parent", "Parent", "Parent desc")
    child = parent.command_group("child", "Child", "Child desc")

    @parent.command("parent_cmd")
    def parent_cmd(ctx: Context):
        """Parent command."""

    @child.command("child_cmd")
    def child_cmd(ctx: Context):
        """Child command."""

    command_groups = commands_tester.collected_command_groups()
    commands = command_groups["tools.parent"].get_commands()
    assert len(commands) == 1
    assert "parent_cmd" in commands
    assert commands["parent_cmd"] == parent_cmd

    commands = command_groups["tools.parent.child"].get_commands()
    assert len(commands) == 1
    assert "child_cmd" in commands
    assert commands["child_cmd"] == child_cmd


def test_command_group_storage(commands_tester):
    """Test that command groups are properly stored in the registry."""
    group1 = command_group("test1", "Test 1", "Test 1 desc")
    group2 = command_group("test2", "Test 2", "Test 2 desc")
    nested = group1.command_group("nested", "Nested", "Nested desc")

    # Verify groups are stored with correct paths
    command_groups = commands_tester.collected_command_groups()
    assert command_groups["tools.test1"] == group1
    assert command_groups["tools.test2"] == group2
    assert command_groups["tools.test1.nested"] == nested
    assert "nonexistent" not in command_groups


def test_command_group_hierarchy(commands_tester):
    """Test that command groups properly maintain hierarchy."""
    # All top-level groups get "tools" as parent automatically
    parent_group = command_group("parent", "Parent", "Parent desc")
    assert parent_group.parent == "tools"

    # Nested groups get their parent's full name as parent
    child_group = parent_group.command_group("child", "Child", "Child desc")
    assert child_group.parent == "tools.parent"

    # Deeply nested groups continue the pattern
    grandchild_group = child_group.command_group("grandchild", "Grandchild", "Grandchild desc")
    assert grandchild_group.parent == "tools.parent.child"


def test_command_group_full_name(commands_tester):
    """Test that full_name property works correctly."""
    # Top-level group gets tools. prefix
    group1 = command_group("top", "Top", "Top desc")
    assert group1.full_name == "tools.top"

    # Nested group
    group2 = group1.command_group("sub", "Sub", "Sub desc")
    assert group2.full_name == "tools.top.sub"

    # Deep nesting
    group3 = group2.command_group("deep", "Deep", "Deep desc")
    assert group3.full_name == "tools.top.sub.deep"


def test_command_decorator_returns_function(commands_tester):
    """Test that the command decorator returns the original function."""
    group = command_group("test", "Test", "Test desc")

    @group.command("test_cmd")
    def original_function(ctx: Context):
        """Test command."""
        return "test"

    # The decorator should return the original function unchanged
    assert original_function(None) == "test"


def test_function_name_to_command_name_conversion(commands_tester):
    """Test that function names are converted to command names using hyphens."""
    group = command_group("test", "Test", "Test desc")

    @group.command
    def simple_function(ctx: Context):
        """Simple function."""

    @group.command
    def function_with_underscores(ctx: Context):
        """Function with underscores."""

    @group.command
    def multiple_underscores_in_name(ctx: Context):
        """Function with multiple underscores."""

    @group.command
    def _leading_underscore(ctx: Context):
        """Function with leading underscore."""

    @group.command
    def trailing_underscore_(ctx: Context):
        """Function with trailing underscore."""

    @group.command
    def _both_underscores_(ctx: Context):
        """Function with both leading and trailing underscores."""

    @group.command("both-underscores")
    def _both_underscores_with_name_(ctx: Context):
        """Function with both leading and trailing underscores."""

    # Check that the commands were registered with the correct names
    command_groups = commands_tester.collected_command_groups()
    assert len(command_groups["tools.test"].get_commands()) == 7

    # Find each command and verify the name conversion
    command_map = command_groups["tools.test"].get_commands()

    assert "simple-function" in command_map
    assert command_map["simple-function"] == simple_function

    assert "function-with-underscores" in command_map
    assert command_map["function-with-underscores"] == function_with_underscores

    assert "multiple-underscores-in-name" in command_map
    assert command_map["multiple-underscores-in-name"] == multiple_underscores_in_name

    assert "-leading-underscore" in command_map
    assert command_map["-leading-underscore"] == _leading_underscore

    assert "trailing-underscore-" in command_map
    assert command_map["trailing-underscore-"] == trailing_underscore_

    assert "-both-underscores-" in command_map
    assert command_map["-both-underscores-"] == _both_underscores_

    assert "both-underscores" in command_map
    assert command_map["both-underscores"] == _both_underscores_with_name_
