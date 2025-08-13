from __future__ import annotations

import pytest

from toolr._context import ConsoleVerbosity
from toolr.utils._console import setup_consoles


@pytest.fixture(
    params=[ConsoleVerbosity.QUIET, ConsoleVerbosity.NORMAL, ConsoleVerbosity.VERBOSE],
    ids=["quiet", "normal", "verbose"],
)
def verbosity(request):
    return request.param


@pytest.fixture
def _consoles(verbosity):
    return setup_consoles(verbosity)


@pytest.fixture
def stdout(_consoles):
    return _consoles[1]


@pytest.fixture
def stderr(_consoles):
    return _consoles[0]


def test_stdout(stdout, capfd):
    stdout.print("Hello, world!")
    out, err = capfd.readouterr()
    assert out == "Hello, world!\n"
    assert err == ""


def test_stderr(stderr, capfd):
    stderr.print("Hello, world!")
    out, err = capfd.readouterr()
    assert out == ""
    assert err == "Hello, world!\n"
