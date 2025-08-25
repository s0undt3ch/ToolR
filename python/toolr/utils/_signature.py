"""
Utilities to parse function signatures.
"""

from __future__ import annotations

import contextlib
import inspect
import logging
from argparse import Action
from collections.abc import Callable
from enum import Enum
from functools import partial
from inspect import Parameter
from types import GenericAlias
from types import UnionType
from typing import TYPE_CHECKING
from typing import Annotated
from typing import Any
from typing import Generic
from typing import Literal
from typing import TypeAlias
from typing import TypeVar
from typing import get_args
from typing import get_origin
from typing import get_type_hints

from msgspec import Struct

from toolr._exc import SignatureError
from toolr._exc import SignatureParameterError
from toolr.utils._docstrings import parse_docstring

if TYPE_CHECKING:
    from argparse import ArgumentParser
    from argparse import Namespace
    from argparse import _MutuallyExclusiveGroup

    from toolr._context import Context
    from toolr.utils._docstrings import Docstring


F = TypeVar("F", bound=Callable[..., Any])
NargsType: TypeAlias = Literal["*", "+", "?"] | int

log = logging.getLogger(__name__)


class Arg(Struct, frozen=True):
    name: str
    type: Any
    action: partial[EnumAction] | type[AppendBoolAction] | str | None
    description: str
    aliases: list[str]
    default: Any | None
    metavar: str | None
    choices: list[Any] | None
    nargs: NargsType | None

    def __repr__(self) -> str:
        repr_str = f"{self.__class__.__name__}(name={self.name!r}, type={self.type!r}, "
        for field in self.__struct_fields__:
            if field in ("name", "type", "description"):
                continue
            if isinstance(self, Arg) and field == "aliases":
                # This will be just one alias, the argument name
                continue
            repr_str += f"{field}={getattr(self, field)!r}, "
        repr_str += f"description={self.description!r})"
        return repr_str

    def _build_parser_kwargs(self) -> dict[str, Any]:
        kwargs: dict[str, Any] = {
            "help": self.description,
            "action": self.action,
        }
        if self.action not in ("store_true", "store_false"):
            kwargs["type"] = self.type
            kwargs["metavar"] = self.metavar
        if self.default is not None:
            kwargs["default"] = self.default
        if self.choices is not None:
            kwargs["choices"] = self.choices
        if self.nargs is not None:
            kwargs["nargs"] = self.nargs
        return kwargs

    def setup_parser(self, parser: ArgumentParser) -> None:
        args = self.aliases
        parser.add_argument(*args, **self._build_parser_kwargs())


class VarArg(Arg, Struct, frozen=True):
    """VarArg is a special case of Arg that is used to represent a variable number of arguments."""


class KwArg(Arg, Struct, frozen=True):
    required: bool
    group: str | None

    def _build_parser_kwargs(self) -> dict[str, Any]:
        kwargs = super()._build_parser_kwargs()
        kwargs["dest"] = self.name
        kwargs["required"] = self.required
        return kwargs

    def setup_parser(self, parser: ArgumentParser | _MutuallyExclusiveGroup) -> None:
        args = self.aliases
        parser.add_argument(*args, **self._build_parser_kwargs())


class Signature(Struct, Generic[F], frozen=True):
    func: F
    short_description: str
    long_description: str
    arguments: list[Arg | KwArg]
    signature: inspect.Signature

    def setup_parser(self, parser: ArgumentParser) -> None:
        mutually_exclusive_groups: dict[str, list[KwArg]] = {}
        for argument in self.arguments:
            if isinstance(argument, KwArg) and argument.group is not None:
                mutually_exclusive_groups.setdefault(argument.group, []).append(argument)
                continue
            argument.setup_parser(parser)

        for group_name, group_arguments in mutually_exclusive_groups.items():
            if not group_arguments:  # pragma: no cover
                # How did this ever happen?!
                err_msg = f"Group {group_name} has no arguments"
                raise SignatureError(err_msg, self.func)

            group = parser.add_mutually_exclusive_group()
            for argument in group_arguments:
                argument.setup_parser(group)

        parser.set_defaults(func=self)

    def __repr__(self) -> str:
        return (
            f"{self.__class__.__name__}(func={self.func.__name__!r}, "
            f"short_description={self.short_description!r}, "
            f"arguments={self.arguments!r})"
        )

    def __call__(self, ctx: Context, options: Namespace) -> None:
        args: list[Any] = []
        kwargs: dict[str, Any] = {}
        for argument in self.arguments:
            argument_value = getattr(options, argument.name)
            if isinstance(argument, VarArg):
                args.extend(argument_value)
            elif isinstance(argument, Arg):
                args.append(argument_value)
            elif isinstance(argument, KwArg):
                kwargs[argument.name] = argument_value
            else:  # pragma: no cover
                err_msg = f"Unknown argument type: {argument}"
                raise TypeError(err_msg)
        bound = self.signature.bind_partial(*args, **kwargs)
        self.func(ctx, *bound.args, **bound.kwargs)


class ArgumentAnnotation(Struct, frozen=True):
    aliases: list[str] | None = None
    required: bool | None = None
    metavar: str | None = None
    action: str | None = None
    choices: list[Any] | None = None
    nargs: NargsType | None = None
    group: str | None = None


def arg(
    *,
    aliases: list[str] | None = None,
    required: bool | None = None,
    metavar: str | None = None,
    action: str | None = None,
    choices: list[Any] | None = None,
    nargs: NargsType | None = None,
    group: str | None = None,
) -> ArgumentAnnotation:
    """
    Create an ArgumentAnnotation.

    This function is meant to be used with :class:`typing.Annotated` to create an ArgumentAnnotation.

    Args:
        aliases: Aliases for the argument.
        required: Whether the argument is required.
        metavar: The metavar for the argument.
        action: The action for the argument.
        choices: The choices for the argument.
        nargs: The number of arguments to accept.
        group: The name of the mutually exclusive group for the argument.
    """
    return ArgumentAnnotation(
        aliases=aliases,
        required=required,
        metavar=metavar,
        action=action,
        choices=choices,
        nargs=nargs,
        group=group,
    )


def get_signature(func: F) -> Signature:
    if func.__doc__ is None:
        err_msg = f"Function {func.__name__} has no docstring"
        raise SignatureError(err_msg, func)

    parsed_docstring = parse_docstring(func.__doc__)
    short_description = parsed_docstring.short_description
    long_description = parsed_docstring.long_description or short_description

    signature = inspect.signature(func)
    params = list(signature.parameters.items())

    if not params:
        err_msg = f"Function {func.__name__} must have at least one parameter (ctx: Context)"
        raise SignatureError(err_msg, func)

    first_param_name, first_param = params.pop(0)

    # Define the error message for the context parameter
    context_err_msg = (
        f"Function {func.__name__} must have 'ctx: Context' as the first parameter, "
        f"got '{first_param_name}: {first_param.annotation}' (type: {type(first_param.annotation)})"
    )

    # For consistency sake, check if the first parameter is named "ctx"
    if first_param_name != "ctx":
        raise SignatureError(context_err_msg, func)

    # Get resolved type hints (handles string annotations from __future__ import annotations)
    try:
        type_hints = get_type_hints(func, include_extras=True)
    except (NameError, AttributeError, TypeError) as exc:
        err_msg = f"Failed to get type hints for {func.__name__}"
        raise SignatureError(err_msg, func) from exc

    arguments = []

    # Parse remaining parameters (skip the first Context parameter)
    for param_name, param in params:
        # Use resolved type hint if available, otherwise fall back to raw annotation
        resolved_annotation = type_hints.get(param_name, param.annotation)
        try:
            parameter = _parse_parameter(param_name, param, resolved_annotation, parsed_docstring)
        except SignatureParameterError as exc:
            raise SignatureError(exc.message, func) from None
        arguments.append(parameter)

    return Signature(
        func=func,
        short_description=short_description,
        long_description=long_description,
        arguments=arguments,
        signature=signature,
    )


class EnumAction(Action):
    def __init__(self, choices_mapping: dict[str, Enum], **kwargs: Any):
        super().__init__(**kwargs)
        self.choices_mapping = choices_mapping

    def __call__(
        self,
        parser: ArgumentParser,
        namespace: Namespace,
        values: Any,
        option_string: str | None = None,  # noqa: ARG002
    ) -> None:
        err_msg = (
            f"Invalid choice: '{values}'. Available choices are {', '.join(repr(c) for c in self.choices_mapping)}."
        )
        if isinstance(values, Enum):
            if values in self.choices_mapping.values():
                setattr(namespace, self.dest, values)
                return
            parser.error(err_msg)
        with contextlib.suppress(KeyError):
            setattr(namespace, self.dest, self.choices_mapping[values.lower()])
            return
        parser.error(err_msg)


class AppendBoolAction(Action):
    def __call__(
        self,
        parser: ArgumentParser,
        namespace: Namespace,
        values: Any,
        option_string: str | None = None,  # noqa: ARG002
    ) -> None:
        if isinstance(values, str):
            values = values.lower()
            if values not in ("true", "false"):
                parser.error(f"Invalid value for {self.dest}: {values}")
            values = values == "true"
        if values not in (True, False):
            parser.error(f"Invalid value for {self.dest}: {values}")
        try:
            getattr(namespace, self.dest).append(values)
        except AttributeError:
            setattr(namespace, self.dest, [values])


def _parse_parameter(  # noqa: PLR0915
    param_name: str,
    param: Parameter,
    annotation: Any,
    docstring: Docstring,
) -> Arg | KwArg:
    """Parse a single parameter into argparse configuration."""
    default = param.default
    required: bool | None = default is param.empty
    positional: bool = default is param.empty
    metavar: str | None = param_name.upper()
    aliases: list[str] | None = None
    action: partial[EnumAction] | type[AppendBoolAction] | str | None = None
    choices: list[Any] | None = None
    nargs: NargsType | None = None
    group: str | None = None
    klass: type[Arg | VarArg | KwArg]
    if param.kind == Parameter.VAR_POSITIONAL:
        klass = VarArg
    elif positional:
        klass = Arg
    else:
        klass = KwArg

    log.debug(
        "Parsing parameter %r, positional=%s, annotation=%s, default=%s", param_name, positional, annotation, default
    )

    # Extract Argument config from Annotated if present
    arg_config = None
    actual_type: Any = annotation
    original_type = actual_type

    # Note: String annotations should already be resolved by get_type_hints()
    # in the calling function, so annotation should be the actual type object

    if get_origin(annotation) is Annotated:
        args = get_args(annotation)
        actual_type = args[0]  # First arg is the actual type
        log.debug("Found Annotated type, actual_type=%s, metadata=%s", actual_type, args[1:])

        # Look for ArgumentSpec in metadata
        for metadata in args[1:]:
            log.debug("Checking metadata: %s, type=%s", metadata, type(metadata))
            log.debug("isinstance(metadata, ArgumentAnnotation): %s", isinstance(metadata, ArgumentAnnotation))
            if isinstance(metadata, ArgumentAnnotation):
                arg_config = metadata
                log.debug("Found ArgumentAnnotation config: %s", arg_config)
                break

    if isinstance(actual_type, UnionType):
        if len(actual_type.__args__) > 2:
            err_msg = f"{klass.__name__} {param_name!r} has more than two types: , ".join(
                arg.__name__ for arg in actual_type.__args__
            )
            raise SignatureParameterError(err_msg)

        # If it doesn't have more than two types, the second type must be None
        if actual_type.__args__[1] is not type(None):
            err_msg = f"The second type of {klass.__name__} {param_name!r} must be None"
            raise SignatureParameterError(err_msg)

        # Now, the type that we're really interested in is the first one
        actual_type = actual_type.__args__[0]

    description: str | None = docstring.params.get(param_name)
    if description is None:
        err_msg = (
            f"{klass.__name__} {param_name!r} has no description in the docstring which is required "
            "to generate the help message."
        )
        raise SignatureParameterError(err_msg)

    # If we have an ArgumentSpec config, use it as base
    if arg_config:
        aliases = arg_config.aliases
        if arg_config.required is not None:
            required = arg_config.required
        if arg_config.metavar is not None:
            metavar = arg_config.metavar
        if arg_config.action is not None:
            action = arg_config.action
        if arg_config.choices is not None:
            choices = arg_config.choices
        if arg_config.nargs is not None:
            nargs = arg_config.nargs
        if arg_config.group is not None:
            if positional:
                err_msg = f"Positional parameter {param_name!r} cannot be in a mutually exclusive group."
                raise SignatureParameterError(err_msg)
            group = arg_config.group

    if nargs is None and param.kind == Parameter.VAR_POSITIONAL:
        nargs = "*"

    if default is param.empty:
        # Reset default to None if it's empty
        default = None

    if inspect.isclass(actual_type) and issubclass(actual_type, Enum):
        if choices is None:
            choices = list(actual_type)
        else:
            for choice in choices:
                if not isinstance(choice, Enum):
                    err_msg = f"{klass.__name__} {param_name!r} has choices and they are not of an Enum type."
                    raise SignatureParameterError(err_msg)

                if not isinstance(choice, actual_type):
                    err_msg = (
                        f"{klass.__name__} {param_name!r} has choices and they are not of the same type as the enum."
                    )
                    raise SignatureParameterError(err_msg)
        choices_mapping = {choice.name.lower(): choice for choice in choices}
        # Now reset choices to None so that argparse does not handle them, our action does.
        choices = None
        action = partial(EnumAction, choices_mapping=choices_mapping)
        if not description.endswith("."):
            description += "."
        description += f" Choices: {', '.join(repr(c) for c in choices_mapping)}."
        actual_type = str

    if action is None:
        if default is True:
            # We should not pass type to boolean actions
            action = "store_false"
        elif default is False:
            # We should not pass type to boolean actions
            action = "store_true"
        elif isinstance(actual_type, GenericAlias):
            if len(actual_type.__args__) > 1:
                err_msg = f"{klass.__name__} {param_name!r} has more than one type: {original_type}"
                raise SignatureParameterError(err_msg)

            # Now, the type that we're really interested in is the first one
            actual_type = actual_type.__args__[0]
            if isinstance(actual_type, UnionType):
                err_msg = f"{klass.__name__} {param_name!r} has more than one type: {original_type}"
                raise SignatureParameterError(err_msg)

            if actual_type is bool:
                action = AppendBoolAction
                # We need to make argparse handle the boolean values as strings since 'bool("False")' is True
                actual_type = str
            elif param.kind != Parameter.VAR_POSITIONAL and nargs is None:
                action = "append"

    aliases = _build_aliases(param_name, positional, aliases)

    if TYPE_CHECKING:
        assert aliases is not None
        assert isinstance(aliases, list)

    if positional:
        return Arg(
            name=param_name,
            type=actual_type,
            description=description,
            aliases=aliases,
            default=default,
            metavar=metavar,
            action=action,
            choices=choices,
            nargs=nargs,
        )

    return KwArg(
        name=param_name,
        type=actual_type,
        description=description,
        aliases=aliases,
        required=required or False,
        default=default,
        metavar=metavar,
        action=action,
        choices=choices,
        nargs=nargs,
        group=group,
    )


def _build_aliases(param_name: str, positional: bool, aliases: list[str] | None) -> list[str]:
    if positional is True:
        if aliases:
            err_msg = f"Positional parameter {param_name!r} cannot have aliases."
            raise SignatureParameterError(err_msg)
        return [param_name]
    default_alias = f"--{param_name.replace('_', '-')}"
    if aliases is None:
        return [default_alias]
    if default_alias in aliases and aliases[0] != default_alias:
        aliases.remove(default_alias)
    if default_alias not in aliases:
        aliases.insert(0, default_alias)
    return aliases
