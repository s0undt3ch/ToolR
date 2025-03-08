from __future__ import annotations

import logging
import os
import pathlib
import sys

CWD: pathlib.Path = pathlib.Path.cwd()
BASE_PATH = pathlib.Path(os.environ.get("TOOLR_SCRIPTS_PATH") or CWD).expanduser()

TOOLR_VENVS_PATH = BASE_PATH / ".toolr-venvs" / "py{}.{}".format(*sys.version_info)

DEFAULT_TOOLR_VENV_PATH = TOOLR_VENVS_PATH / "default"
if str(DEFAULT_TOOLR_VENV_PATH) in sys.path:
    sys.path.remove(str(DEFAULT_TOOLR_VENV_PATH))

log = logging.getLogger(__name__)


def main() -> None:
    """
    Main CLI entry-point.
    """


if __name__ == "__main__":
    main()
