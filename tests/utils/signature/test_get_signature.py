"""Tests for the get_signature function."""

from __future__ import annotations

import enum
from typing import Annotated

import pytest

from toolr import Context
from toolr._exc import SignatureError
from toolr.utils._signature import Arg
from toolr.utils._signature import KwArg
from toolr.utils._signature import VarArg
from toolr.utils._signature import arg
from toolr.utils._signature import get_signature


class OptionEnum(enum.Enum):
    """Test enum for choices testing."""

    OPTION1 = "option1"
    OPTION2 = "option2"
    OPTION3 = "option3"


def test_get_signature_basic():
    """Test get_signature with basic function."""

    def test_func(ctx: Context, name: str) -> None:
        """Test function.

        Args:
            name: The name parameter.
        """

    signature = get_signature(test_func)
    assert signature.func == test_func
    assert signature.short_description == "Test function."
    assert len(signature.arguments) == 1
    assert signature.arguments[0].name == "name"
    assert signature.arguments[0].type is str


def test_get_signature_with_default():
    """Test get_signature with function that has default values."""

    def test_func(ctx: Context, name: str, count: int = 10) -> None:
        """Test function.

        Args:
            name: The name parameter.
            count: The count parameter.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 2

    # First argument should be positional
    assert signature.arguments[0].name == "name"
    assert isinstance(signature.arguments[0], Arg)

    # Second argument should be keyword
    assert signature.arguments[1].name == "count"
    assert isinstance(signature.arguments[1], KwArg)
    assert signature.arguments[1].default == 10


def test_get_signature_with_annotated():
    """Test get_signature with Annotated types."""

    def test_func(ctx: Context, name: Annotated[str, arg(metavar="NAME")]) -> None:
        """Test function.

        Args:
            name: The name parameter.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 1
    arg_obj = signature.arguments[0]
    assert arg_obj.name == "name"
    assert arg_obj.metavar == "NAME"


def test_get_signature_with_union():
    """Test get_signature with Union types."""

    def test_func(ctx: Context, name: str | None = None) -> None:
        """Test function.

        Args:
            name: The name parameter.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 1
    arg = signature.arguments[0]
    assert arg.name == "name"
    assert arg.type is str  # Should extract the non-None type


def test_get_signature_with_enum():
    """Test get_signature with Enum types."""

    def test_func(ctx: Context, option: OptionEnum) -> None:
        """Test function.

        Args:
            option: The option parameter.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 1
    arg = signature.arguments[0]
    assert arg.name == "option"
    assert "Choices: 'option1', 'option2', 'option3'." in arg.description


def test_get_signature_with_boolean_defaults():
    """Test get_signature with boolean defaults."""

    def test_func(ctx: Context, verbose: bool = False, quiet: bool = True) -> None:
        """Test function.

        Args:
            verbose: Verbose mode.
            quiet: Quiet mode.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 2

    verbose_arg = signature.arguments[0]
    assert verbose_arg.name == "verbose"
    assert verbose_arg.action == "store_true"

    quiet_arg = signature.arguments[1]
    assert quiet_arg.name == "quiet"
    assert quiet_arg.action == "store_false"


def test_get_signature_with_list_type():
    """Test get_signature with list types."""

    def test_func(ctx: Context, files: list[str]) -> None:
        """Test function.

        Args:
            files: List of files.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 1
    arg = signature.arguments[0]
    assert arg.name == "files"
    assert arg.action == "append"


def test_get_signature_no_docstring():
    """Test get_signature with function that has no docstring."""

    def test_func(ctx: Context, name: str) -> None: ...

    with pytest.raises(SignatureError, match=r"Function test_func has no docstring"):
        get_signature(test_func)


def test_get_signature_no_parameters():
    """Test get_signature with function that has no parameters."""

    def test_func() -> None:
        """Test function."""

    with pytest.raises(
        SignatureError, match=r"Function test_func must have at least one parameter"
    ):
        get_signature(test_func)


def test_get_signature_wrong_first_parameter_name():
    """Test get_signature with wrong first parameter name."""

    def test_func(context: Context, name: str) -> None:
        """Test function.

        Args:
            name: The name parameter.
        """

    with pytest.raises(
        SignatureError, match=r"Function test_func must have 'ctx: Context' as the first parameter"
    ):
        get_signature(test_func)


def test_get_signature_missing_param_description():
    """Test get_signature with missing parameter description."""

    def test_func(ctx: Context, name: str) -> None:
        """Test function.

        Args:
            ctx: The context.
        """

    with pytest.raises(SignatureError, match=r"Arg 'name' has no description in the docstring"):
        get_signature(test_func)


def test_get_signature_positional_with_aliases():
    """Test get_signature with positional argument that has aliases."""

    def test_func(ctx: Context, name: Annotated[str, arg(aliases=["--name"])]) -> None:
        """Test function.

        Args:
            name: The name parameter.
        """

    with pytest.raises(SignatureError, match=r"Positional parameter 'name' cannot have aliases."):
        get_signature(test_func)


def test_get_signature_union_with_more_than_two_types():
    """Test get_signature with Union that has more than two types."""

    def test_func(ctx: Context, value: str | float | bool) -> None:
        """Test function.

        Args:
            value: The value parameter.
        """

    with pytest.raises(SignatureError, match=r"Arg 'value' has more than two types"):
        get_signature(test_func)


def test_get_signature_union_second_type_not_none():
    """Test get_signature with Union where second type is not None."""

    def test_func(ctx: Context, value: str | int) -> None:
        """Test function.

        Args:
            value: The value parameter.
        """

    with pytest.raises(SignatureError, match=r"The second type of Arg 'value' must be None"):
        get_signature(test_func)


def test_get_signature_var_positional_returns_vararg():
    """get_signature must return a VarArg for *args-style (VAR_POSITIONAL) parameters.

    Regression test: previously _parse_parameter set the local ``klass`` to ``VarArg``
    for VAR_POSITIONAL parameters but the final return statement unconditionally built
    an ``Arg``. That meant ``Signature.__call__`` never took the VarArg ``.extend``
    branch, and values arrived in the user function nested inside a single-element
    list instead of being spread.
    """

    def test_func(ctx: Context, *items: str) -> None:
        """Test function with variable positional arguments.

        Args:
            items: The items.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 1
    assert signature.arguments[0].name == "items"
    assert signature.arguments[0].nargs == "*"
    assert isinstance(signature.arguments[0], VarArg)


def test_get_signature_var_positional_with_other_args():
    """VAR_POSITIONAL must remain a VarArg even when preceded by regular positionals."""

    def test_func(ctx: Context, first: str, *rest: str) -> None:
        """Test function with a positional and trailing varargs.

        Args:
            first: The first arg.
            rest: The remaining args.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 2
    assert isinstance(signature.arguments[0], Arg)
    assert not isinstance(signature.arguments[0], VarArg)
    assert isinstance(signature.arguments[1], VarArg)
    assert signature.arguments[1].nargs == "*"


def test_get_signature_optional_without_default_is_zero_or_one_positional():
    """`T | None` without a default → positional with `nargs="?"`.

    This is the type-driven replacement for `arg(nargs="?")`; argparse
    stores `None` when the user omits the slot, matching the function's
    `Optional[T]` hint.
    """

    def test_func(ctx: Context, new_version: str | None) -> None:
        """Bump.

        Args:
            new_version: Explicit version to bump to.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 1
    arg_obj = signature.arguments[0]
    assert isinstance(arg_obj, Arg)
    assert not isinstance(arg_obj, KwArg)
    assert arg_obj.nargs == "?"
    assert arg_obj.type is str


def test_get_signature_optional_with_none_default_stays_a_flag():
    """`T | None = None` is a keyword flag (existing behavior); zero-or-one only triggers when there's no default."""

    def test_func(ctx: Context, name: str | None = None) -> None:
        """Greet.

        Args:
            name: Who to greet.
        """

    signature = get_signature(test_func)
    arg_obj = signature.arguments[0]
    assert isinstance(arg_obj, KwArg)
    # KwArg means it's a flag, not a positional.
    assert arg_obj.nargs is None


def test_get_signature_rejects_multiple_zero_or_one_positionals():
    """At most one zero-or-one positional per command — clap/argparse can't disambiguate."""

    def test_func(ctx: Context, a: str | None, b: str | None) -> None:
        """Bad.

        Args:
            a: first.
            b: second.
        """

    with pytest.raises(SignatureError, match=r"Only one zero-or-one positional"):
        get_signature(test_func)


def test_get_signature_rejects_required_positional_after_zero_or_one():
    """Required positionals must come before any `T | None` slot."""

    def test_func(ctx: Context, maybe: str | None, name: str) -> None:
        """Bad ordering.

        Args:
            maybe: optional.
            name: required.
        """

    with pytest.raises(SignatureError, match=r"Required positional 'name' follows"):
        get_signature(test_func)


def test_get_signature_rejects_zero_or_one_with_var_positional():
    """`T | None` and `*args: T` both compete for the trailing slot."""

    def test_func(ctx: Context, maybe: str | None, *files: str) -> None:
        """Bad combo.

        Args:
            maybe: optional.
            files: trailing.
        """

    with pytest.raises(SignatureError, match=r"Cannot combine the zero-or-one positional"):
        get_signature(test_func)


def test_get_signature_allows_required_positional_before_zero_or_one():
    """Required positionals (no default) may precede the zero-or-one slot."""

    def test_func(ctx: Context, name: str, maybe: str | None) -> None:
        """OK combo.

        Args:
            name: required.
            maybe: optional trailing.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 2
    assert isinstance(signature.arguments[0], Arg)
    assert isinstance(signature.arguments[1], Arg)
    assert signature.arguments[0].nargs is None
    assert signature.arguments[1].nargs == "?"
