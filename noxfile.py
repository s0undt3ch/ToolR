import os
import pathlib
import nox

# Paths
REPO_ROOT = pathlib.Path(__file__).resolve().parent
# Change current directory to REPO_ROOT
os.chdir(str(REPO_ROOT))


@nox.session()
def test(session: nox.Session):
    session.install(str(REPO_ROOT), "wheel")
    # Ensure build uses version of setuptools-rust under development
    session.install("--no-build-isolation", ".[dev]")
    # Test Python package
    session.run("pytest", *session.posargs)
