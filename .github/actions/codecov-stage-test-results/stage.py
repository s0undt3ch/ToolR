"""Stage JUnit files and a `meta.json` for the `codecov-publish-test-results` action.

Standard library only: it runs on whichever Python the job has on `PATH`.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from pathlib import Path


class StageError(Exception):
    """A problem the workflow author has to fix."""


def stage(files: str, flags: str, name: str, stage_dir: Path) -> int:
    """Copy the existing files into `stage_dir`, write `meta.json`; return how many were staged."""
    stage_dir.mkdir(parents=True, exist_ok=True)
    staged: list[str] = []
    for line in files.splitlines():
        if not line.strip():
            continue
        path = Path(line.strip())
        if not path.is_file():
            print(f"::warning::Test-result file '{path}' not found; not staging it.", flush=True)
            continue
        if path.name in staged:
            msg = f"Two test-result files share the name '{path.name}'."
            raise StageError(msg)
        shutil.copyfile(path, stage_dir / path.name)
        staged.append(path.name)
    meta = {
        "files": staged,
        "flags": [flag.strip() for flag in flags.split(",") if flag.strip()],
        "name": name,
    }
    (stage_dir / "meta.json").write_text(json.dumps(meta), encoding="utf-8")
    return len(staged)


def main(env: dict[str, str] | None = None) -> int:
    env = dict(os.environ) if env is None else env
    try:
        count = stage(
            env.get("INPUT_FILES", ""),
            env.get("INPUT_FLAGS", ""),
            env.get("INPUT_NAME", ""),
            Path(env["STAGE_DIR"]),
        )
    except StageError as exc:
        print(f"::error::{exc}", flush=True)
        return 1
    with Path(env["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        output.write(f"staged={count}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
