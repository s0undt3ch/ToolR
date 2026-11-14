"""Tests for the `codecov-stage-test-results` composite action's `stage.py`."""

from __future__ import annotations

import importlib.util
import json
from collections.abc import Callable
from pathlib import Path
from types import ModuleType

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
STAGE_PY = REPO_ROOT / ".github" / "actions" / "codecov-stage-test-results" / "stage.py"


def _load_stage() -> ModuleType:
    spec = importlib.util.spec_from_file_location("codecov_stage_test_results", STAGE_PY)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


stage_module = _load_stage()


@pytest.fixture
def run_stage(tmp_path: Path) -> Callable[..., tuple[int, Path, str]]:
    """Run `stage.py`'s `main`; return its exit code, the stage directory and `GITHUB_OUTPUT`."""

    def run(*, files: str, flags: str = "", name: str = "") -> tuple[int, Path, str]:
        stage_dir = tmp_path / "stage" / "artifact"
        output = tmp_path / "github_output"
        output.write_text("", encoding="utf-8")
        returncode = stage_module.main(
            {
                "INPUT_FILES": files,
                "INPUT_FLAGS": flags,
                "INPUT_NAME": name,
                "STAGE_DIR": str(stage_dir),
                "GITHUB_OUTPUT": str(output),
            }
        )
        return returncode, stage_dir, output.read_text(encoding="utf-8")

    return run


def test_stages_files_and_meta(
    tmp_path: Path, run_stage: Callable[..., tuple[int, Path, str]]
) -> None:
    (tmp_path / "junit-3.14-ubuntu-latest.xml").write_text("<testsuites/>", encoding="utf-8")

    returncode, stage_dir, output = run_stage(
        files=f"{tmp_path / 'junit-3.14-ubuntu-latest.xml'}\n\n",
        flags="ubuntu-latest, 3.14,",
        name='Test / "Linux" (3.14)',
    )

    assert returncode == 0
    assert (stage_dir / "junit-3.14-ubuntu-latest.xml").read_text(
        encoding="utf-8"
    ) == "<testsuites/>"
    assert json.loads((stage_dir / "meta.json").read_text(encoding="utf-8")) == {
        "files": ["junit-3.14-ubuntu-latest.xml"],
        "flags": ["ubuntu-latest", "3.14"],
        "name": 'Test / "Linux" (3.14)',
    }
    assert output == "staged=1\n"


def test_missing_file_warns_and_stages_nothing(
    tmp_path: Path,
    run_stage: Callable[..., tuple[int, Path, str]],
    capsys: pytest.CaptureFixture[str],
) -> None:
    missing = tmp_path / "junit-missing.xml"

    returncode, stage_dir, output = run_stage(files=str(missing))

    assert returncode == 0
    assert f"::warning::Test-result file '{missing}' not found" in capsys.readouterr().out
    assert json.loads((stage_dir / "meta.json").read_text(encoding="utf-8")) == {
        "files": [],
        "flags": [],
        "name": "",
    }
    assert output == "staged=0\n"


def test_duplicate_file_names_fail(
    tmp_path: Path,
    run_stage: Callable[..., tuple[int, Path, str]],
    capsys: pytest.CaptureFixture[str],
) -> None:
    for sub in ("a", "b"):
        (tmp_path / sub).mkdir()
        (tmp_path / sub / "junit.xml").write_text("<testsuites/>", encoding="utf-8")

    returncode, _, output = run_stage(
        files=f"{tmp_path / 'a' / 'junit.xml'}\n{tmp_path / 'b' / 'junit.xml'}"
    )

    assert returncode == 1
    assert "::error::Two test-result files share the name 'junit.xml'." in capsys.readouterr().out
    assert output == ""
