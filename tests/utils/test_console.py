from __future__ import annotations

import pytest

from toolr.utils._console import Consoles
from toolr.utils._console import ConsoleVerbosity


@pytest.fixture(
    params=[ConsoleVerbosity.QUIET, ConsoleVerbosity.NORMAL, ConsoleVerbosity.VERBOSE],
    ids=["quiet", "normal", "verbose"],
)
def verbosity(request):
    return request.param


@pytest.fixture
def _consoles(verbosity):
    return Consoles.setup(verbosity)


@pytest.fixture
def stdout(_consoles):
    return _consoles.stdout


@pytest.fixture
def stderr(_consoles):
    return _consoles.stderr


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
