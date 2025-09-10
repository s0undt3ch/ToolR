"""Tests for parser error cases."""

from __future__ import annotations

import pytest
from msgspec import structs

from toolr._parser import Parser


def test_parser_run_without_parse_args():
    """Test that run() raises error when parse_args() wasn't called."""
    parser = Parser()

    with pytest.raises(RuntimeError, match=r"parser.parse_args\(\) was not called"):
        parser.run()


def test_parser_parse_args_no_command(capfd):
    """Test parse_args when no command is provided."""
    parser = Parser()

    # Parse args with no command
    with pytest.raises(SystemExit):
        parser.parse_args([])

    out, err = capfd.readouterr()
    assert "error: the following arguments are required: command" in err
    assert not out


def test_parser_getattr_proxy():
    """Test that __getattr__ proxies unknown attributes to parser."""
    parser = Parser()

    # Test that unknown attributes are proxied to the underlying parser
    # The parser should have a 'prog' attribute
    assert hasattr(parser, "prog")
    assert parser.prog == "toolr"


def test_parser_getattr_options():
    """Test that __getattr__ doesn't proxy 'options' attribute."""
    parser = Parser()

    # Set options directly using force_setattr
    structs.force_setattr(parser, "options", "test_options")

    # Should return the direct attribute, not proxy
    assert parser.options == "test_options"
