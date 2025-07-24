"""Tests for enum argument parsing and type casting."""

from __future__ import annotations

from enum import Enum
from typing import Annotated

import pytest

from toolr import Context
from toolr._registry import CommandGroup
from toolr.utils._signature import arg


class LogLevel(Enum):
    """Log level enumeration."""

    DEBUG = "debug"
    INFO = "info"
    WARNING = "warning"
    ERROR = "error"


class Environment(Enum):
    """Environment enumeration."""

    DEVELOPMENT = "dev"
    STAGING = "staging"
    PRODUCTION = "prod"


class Status(Enum):
    """Status enumeration."""

    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"


class CustomEnum(Enum):
    """Custom enum with different values."""

    ONE = "1"
    TWO = "2"
    THREE = "3"


class IntEnum(Enum):
    """Integer enum."""

    ZERO = 0
    ONE = 1
    TWO = 2


@pytest.fixture
def command_group(command_group: CommandGroup) -> None:
    @command_group.command("log")
    def log_test(ctx: Context, level: LogLevel) -> None:
        """Test logging.

        Args:
            ctx: The context object.
            level: The log level to use.
        """

    @command_group.command("log-default")
    def log_test_default(ctx: Context, level: LogLevel = LogLevel.INFO) -> None:
        """Test logging with default.

        Args:
            ctx: The context object.
            level: The log level to use.
        """

    @command_group.command("log-choices")
    def log_test_choices(
        ctx: Context, level: Annotated[LogLevel, arg(choices=[LogLevel.INFO, LogLevel.ERROR])] = LogLevel.INFO
    ) -> None:
        """Test logging with choices.

        Args:
            ctx: The context object.
            level: The log level to use.
        """

    @command_group.command("log-metavar")
    def log_test_metavar(ctx: Context, level: Annotated[LogLevel, arg(metavar="LEVEL")] = LogLevel.DEBUG) -> None:
        """Test logging with metavar.

        Args:
            ctx: The context object.
            level: The log level to use.
        """

    @command_group.command("log-aliases")
    def log_test_aliases(
        ctx: Context, level: Annotated[LogLevel, arg(aliases=["-l", "--log-level"])] = LogLevel.INFO
    ) -> None:
        """Test logging with aliases.

        Args:
            ctx: The context object.
            level: The log level to use.
        """

    @command_group.command("log-required")
    def log_test_required(ctx: Context, level: Annotated[LogLevel, arg(required=True)]) -> None:
        """Test required logging.

        Args:
            ctx: The context object.
            level: The log level to use.
        """

    @command_group.command("deploy")
    def deploy_test(ctx: Context, environment: Environment, status: Status = Status.PENDING) -> None:
        """Test deployment.

        Args:
            ctx: The context object.
            environment: The environment to deploy to.
            status: The deployment status.
        """

    @command_group.command("custom")
    def custom_test(ctx: Context, value: CustomEnum) -> None:
        """Test custom enum.

        Args:
            ctx: The context object.
            value: The custom enum value.
        """

    @command_group.command("int")
    def int_test(ctx: Context, value: IntEnum) -> None:
        """Test integer enum.

        Args:
            ctx: The context object.
            value: The integer enum value.
        """


def test_basic_enum_argument(cli_parser):
    """Test basic enum argument parsing."""
    args = cli_parser.parse_args(["test", "log", "info"])
    assert args.level == LogLevel.INFO
    assert isinstance(args.level, LogLevel)


def test_enum_with_default(cli_parser):
    """Test enum argument with default value."""
    # Test with default
    args = cli_parser.parse_args(["test", "log-default"])
    assert args.level == LogLevel.INFO
    assert isinstance(args.level, LogLevel)

    # Test with custom value
    args = cli_parser.parse_args(["test", "log-default", "--level", "error"])
    assert args.level == LogLevel.ERROR
    assert isinstance(args.level, LogLevel)


@pytest.mark.parametrize("level", [LogLevel.INFO, LogLevel.ERROR])
def test_enum_with_choices_override(cli_parser, level, subtests):
    """Test enum argument with custom choices override."""
    with subtests.test("by_value"):
        args = cli_parser.parse_args(["test", "log-choices", "--level", level.value])
        assert args.level == level
    with subtests.test("by_name"):
        args = cli_parser.parse_args(["test", "log-choices", "--level", level.name])
        assert args.level == level


def test_enum_with_metavar(cli_parser):
    """Test enum argument with custom metavar."""
    args = cli_parser.parse_args(["test", "log-metavar", "--level", "debug"])
    assert args.level == LogLevel.DEBUG
    assert isinstance(args.level, LogLevel)


@pytest.mark.parametrize("alias", ["--level", "--log-level", "-l"])
def test_enum_with_aliases(cli_parser, alias):
    """Test enum argument with custom aliases."""
    args = cli_parser.parse_args(["test", "log-aliases", alias, "warning"])
    assert args.level == LogLevel.WARNING


def test_enum_required(cli_parser):
    """Test required enum argument."""
    args = cli_parser.parse_args(["test", "log-required", "info"])
    assert args.level == LogLevel.INFO
    assert isinstance(args.level, LogLevel)


def test_multiple_enum_arguments(cli_parser):
    """Test multiple enum arguments."""
    # Test both arguments
    args = cli_parser.parse_args(["test", "deploy", "staging", "--status", "running"])
    assert args.environment == Environment.STAGING
    assert args.status == Status.RUNNING

    # Test with default status
    args = cli_parser.parse_args(["test", "deploy", "production"])
    assert args.environment == Environment.PRODUCTION
    assert args.status == Status.PENDING


def test_enum_with_custom_values(cli_parser):
    """Test enum with custom string values."""
    args = cli_parser.parse_args(["test", "custom", "two"])
    assert args.value == CustomEnum.TWO
    assert args.value.value == "2"


def test_enum_with_integer_values(cli_parser):
    """Test enum with integer values."""
    args = cli_parser.parse_args(["test", "int", "one"])
    assert args.value == IntEnum.ONE
    assert args.value.value == 1


def test_enum_case_insensitive(cli_parser):
    """Test enum parsing is case sensitive (should match exact values)."""
    # Test exact case match
    args = cli_parser.parse_args(["test", "log", "info"])
    assert args.level == LogLevel.INFO

    # Test that case doesn't matter for the enum value
    # (this depends on how argparse handles the choices)
    args = cli_parser.parse_args(["test", "log", "INFO"])
    assert args.level == LogLevel.INFO
