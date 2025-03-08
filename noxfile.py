from __future__ import annotations

import os
import pathlib

import nox

# Paths
REPO_ROOT = pathlib.Path(__file__).resolve().parent
# Change current directory to REPO_ROOT
os.chdir(str(REPO_ROOT))


@nox.session()
def test(session: nox.Session):
    session.install("wheel")
    session.install("-e", str(REPO_ROOT), "wheel")
    # Ensure build uses version of setuptools-rust under development
    session.install(
        "--no-build-isolation",
        "-r",
        str(REPO_ROOT / "requirements.txt"),
        "-r",
        str(REPO_ROOT / "requirements-dev.txt"),
    )
    # Test Python package
    session.run("pytest", *session.posargs)
