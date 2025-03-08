from __future__ import annotations

import pathlib

import ptscripts
from ptscripts.models import DefaultPipConfig

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
REQUIREMENTS_FILES_PATH = REPO_ROOT / "requirements"
DEFAULT_REQS_CONFIG = DefaultPipConfig(
    requirements_files=[
        REQUIREMENTS_FILES_PATH / "tools.txt",
    ],
)
ptscripts.set_default_config(DEFAULT_REQS_CONFIG)
ptscripts.register_tools_module("tools.precommit")
ptscripts.register_tools_module("tools.precommit.changelog")
