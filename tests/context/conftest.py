"""Common fixtures for Context tests."""

from __future__ import annotations

import os
import pathlib
from argparse import ArgumentParser

import pytest

from toolr.testing import ContextForTesting
from toolr.utils._console import Consoles
from toolr.utils._console import ConsoleVerbosity


@pytest.fixture
def temp_cwd(tmp_path):
    original_cwd = pathlib.Path.cwd()
    cwd = tmp_path / "cwd"
    cwd.mkdir()
    try:
        os.chdir(cwd)
        yield cwd
    finally:
        os.chdir(original_cwd)


@pytest.fixture
def repo_root(tmp_path):
    repo_root = tmp_path / "repo"
    repo_root.mkdir()
    return repo_root


@pytest.fixture
def parser():
    return ArgumentParser()


@pytest.fixture
def ctx(parser, repo_root):
    """A real `ContextForTesting` (see `toolr.testing`), so tests get `.replace(...)` for
    free. Built with real, non-StringIO consoles (this file's tests use `capfd`, not
    `.stdout`/`.stderr`) — those two properties aren't usable on a fixture built this way.
    """
    verbosity = ConsoleVerbosity.NORMAL
    consoles = Consoles.setup_no_colors(verbosity)
    return ContextForTesting(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )


@pytest.fixture
def verbose_ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.VERBOSE
    consoles = Consoles.setup_no_colors(verbosity)
    return ContextForTesting(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )


@pytest.fixture
def quiet_ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.QUIET
    consoles = Consoles.setup_no_colors(verbosity)
    return ContextForTesting(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )
