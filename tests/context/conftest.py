"""Common fixtures for Context tests."""

from __future__ import annotations

import os
import pathlib
from argparse import ArgumentParser

import pytest

from toolr._context import ConsoleVerbosity
from toolr._context import Context
from toolr.utils._console import setup_consoles


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
    verbosity = ConsoleVerbosity.NORMAL
    console_stderr, console_stdout = setup_consoles(verbosity)
    return Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )


@pytest.fixture
def verbose_ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.VERBOSE
    console_stderr, console_stdout = setup_consoles(verbosity)
    return Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )


@pytest.fixture
def quiet_ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.QUIET
    console_stderr, console_stdout = setup_consoles(verbosity)
    return Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )
