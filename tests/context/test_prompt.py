"""Tests for Context prompt functionality."""

from __future__ import annotations

import io
from unittest.mock import patch

import pytest

from toolr._context import Context


def test_prompt_string_default(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with default string type."""
    stream_input = "test input"
    result = ctx._prompt("Enter text", expected_type=str, stream=io.StringIO(stream_input))
    out, err = capfd.readouterr()
    assert out == "Enter text: "
    assert err == ""
    assert result == "test input"


@pytest.mark.parametrize(
    ("stream_input", "expected_value", "case_sensitive"),
    [
        ("option1", "option1", True),
        ("option2", "option2", True),
        ("OpTion1", "option1", False),
        ("OpTion2", "option2", False),
    ],
)
def test_prompt_string_with_choices(
    ctx: Context, capfd: pytest.CaptureFixture[str], stream_input: str, expected_value: str, case_sensitive: bool
):
    """Test prompt with string type and choices."""
    result = ctx._prompt(
        "Select option",
        expected_type=str,
        choices=["option1", "option2"],
        case_sensitive=case_sensitive,
        stream=io.StringIO(stream_input),
    )
    out, err = capfd.readouterr()
    assert out == "Select option [option1/option2]: "
    assert err == ""
    assert result == expected_value


def test_prompt_password(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with password type."""
    with patch("rich.console.getpass", return_value="secret123"):
        stream_input = ""
        result = ctx._prompt(
            "Enter password",
            expected_type=str,
            password=True,
            stream=io.StringIO(stream_input),
        )
        out, err = capfd.readouterr()
        assert out == "Enter password: "
        assert err == ""
        assert result == "secret123"


def test_prompt_integer(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with integer type."""
    stream_input = "42\n31\n30"
    result = ctx._prompt(
        "Enter number",
        expected_type=int,
        choices=["10", "20", "30"],
        show_choices=True,
        default=10,
        stream=io.StringIO(stream_input),
    )
    out, err = capfd.readouterr()
    assert out == "\n".join(  # noqa: FLY002
        [
            "Enter number [10/20/30] (10): Please select one of the available options",
            "Enter number [10/20/30] (10): Please select one of the available options",
            "Enter number [10/20/30] (10): ",
        ]
    )
    assert err == ""
    assert result == 30


def test_prompt_float(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with float type."""
    stream_input = "3.14\n2.0"
    result = ctx._prompt(
        "Enter decimal",
        expected_type=float,
        choices=["1.0", "2.0", "3.0"],
        show_choices=True,
        stream=io.StringIO(stream_input),
    )
    out, err = capfd.readouterr()
    assert out == "\n".join(  # noqa: FLY002
        [
            "Enter decimal [1.0/2.0/3.0]: Please select one of the available options",
            "Enter decimal [1.0/2.0/3.0]: ",
        ]
    )
    assert err == ""
    assert result == 2.0


@pytest.mark.parametrize(
    ("stream_input", "default_value", "show_default", "expected_value", "expected_output"),
    [
        ("", True, True, True, "Continue [y/n] (y): "),
        ("", False, True, False, "Continue [y/n] (n): "),
        ("", True, False, True, "Continue [y/n]: "),
        ("", False, False, False, "Continue [y/n]: "),
    ],
    ids=[
        "default-true-show-default",
        "default-false-show-default",
        "default-true-no-show-default",
        "default-false-no-show-default",
    ],
)
def test_prompt_boolean(
    ctx: Context,
    capfd: pytest.CaptureFixture[str],
    stream_input: str,
    default_value: bool,
    show_default: bool,
    expected_value: bool,
    expected_output: str,
):
    """Test prompt with boolean type."""
    result = ctx._prompt(
        "Continue",
        expected_type=bool,
        show_default=show_default,
        default=default_value,
        stream=io.StringIO(stream_input),
    )
    out, err = capfd.readouterr()
    assert out == expected_output
    assert err == ""
    assert result is expected_value


def test_prompt_unsupported_type(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with unsupported type raises ValueError."""
    with pytest.raises(ValueError, match="Unsupported expected_type: <class 'list'>"):
        ctx._prompt("Enter list:", expected_type=list, stream=io.StringIO("[]"))  # type: ignore[arg-type]
    out, err = capfd.readouterr()
    assert out == ""
    assert err == ""


def test_prompt_none_type_defaults_to_string(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test that None expected_type defaults to string."""
    stream_input = "default string"
    result = ctx._prompt("Enter text", expected_type=None, stream=io.StringIO(stream_input))
    out, err = capfd.readouterr()
    assert out == "Enter text: "
    assert err == ""
    assert result == "default string"


def test_prompt_integer_without_choices(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with integer type without choices."""
    stream_input = "100"
    result = ctx._prompt("Enter number", expected_type=int, stream=io.StringIO(stream_input))
    out, err = capfd.readouterr()
    assert out == "Enter number: "
    assert err == ""
    assert result == 100


def test_prompt_float_without_choices(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with float type without choices."""
    stream_input = "2.718"
    result = ctx._prompt("Enter decimal", expected_type=float, stream=io.StringIO(stream_input))
    out, err = capfd.readouterr()
    assert out == "Enter decimal: "
    assert err == ""
    assert result == 2.718


def test_prompt_with_empty_choices(ctx: Context, capfd: pytest.CaptureFixture[str]):
    """Test prompt with empty choices list."""
    result: str | None = None
    stream_input = "test"
    with pytest.raises(ValueError, match="choices cannot be an empty list"):
        result = ctx._prompt(  # type: ignore[assignment]
            "Enter text",
            expected_type=str,
            choices=[],
            stream=io.StringIO(stream_input),
        )
    out, err = capfd.readouterr()
    assert out == ""
    assert err == ""
    assert result is None
