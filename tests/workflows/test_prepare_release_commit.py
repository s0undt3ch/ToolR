"""Tests for the `Commit changes` step of `_prepare-release.yml`.

The step's `run:` body is extracted from the workflow and run under `bash`
against a scratch repository, with a stub `prek` on `PATH`.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest
import yaml

from tests.workflows.conftest import git
from tests.workflows.conftest import git_env

REPO_ROOT = Path(__file__).resolve().parents[2]
PREPARE_RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "_prepare-release.yml"

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the step runs under bash on ubuntu-latest only"
)

PREK_OK = "exit 0"
PREK_FAILS = "exit 1"


@pytest.fixture
def stub_prek(tmp_path: Path) -> Callable[[list[str]], Path]:
    """Return a factory that writes a stub `prek` into a `bin/` directory.

    Call `N` (zero-based) of the stub runs `snippets[N]` under bash, so a test can
    script each prek pass. The number of calls is kept in `bin/prek-calls`. Returns
    the `bin/` directory to prepend to `PATH`.
    """

    def make(snippets: list[str]) -> Path:
        bin_dir = tmp_path / "bin"
        bin_dir.mkdir()
        for index, snippet in enumerate(snippets):
            (bin_dir / f"prek-call-{index}.sh").write_text(snippet + "\n")
        prek = bin_dir / "prek"
        prek.write_text(
            "#!/usr/bin/env bash\n"
            "set -eu\n"
            f'counter="{bin_dir}/prek-calls"\n'
            'calls=$(cat "$counter" 2>/dev/null || echo 0)\n'
            'echo $((calls + 1)) > "$counter"\n'
            f'snippet="{bin_dir}/prek-call-$calls.sh"\n'
            'if [ ! -f "$snippet" ]; then echo "stub prek: unexpected call $calls" >&2; exit 99; fi\n'
            'exec bash "$snippet"\n'
        )
        prek.chmod(0o755)
        return bin_dir

    return make


@pytest.fixture
def release_repo(git_repo: Callable[..., Path]) -> Path:
    """A repository in the state the release job leaves it in before the commit step."""
    repo = git_repo(
        {
            "Cargo.toml": 'version = "9.9.8"\n',
            "CHANGELOG.md": "# Changelog\n",
            "docs/other.md": "# Other\n",
        }
    )
    (repo / "Cargo.toml").write_text('version = "9.9.9"\n')
    (repo / "CHANGELOG.md").write_text("# Changelog\n\n## 9.9.9\n\n- A change\n")
    (repo / "release-notes.md").write_text("- A change\n")
    return repo


@pytest.fixture
def run_commit_step(
    workflow_step_script: Callable[[Path, str, str], str], tmp_path: Path
) -> Callable[[Path, Path], subprocess.CompletedProcess[str]]:
    """Return a callable that runs the commit step in `repo` with the stub `prek` in `bin_dir`."""
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available")
    script = tmp_path / "commit-step.sh"
    script.write_text(
        workflow_step_script(PREPARE_RELEASE_YML, "prepare-release", "commit-release")
    )

    def run(repo: Path, bin_dir: Path) -> subprocess.CompletedProcess[str]:
        env = git_env()
        env["PATH"] = f"{bin_dir}{os.pathsep}{env['PATH']}"
        env["RELEASE_VERSION"] = "9.9.9"
        # Mirrors the `shell: bash` invocation GitHub Actions uses.
        return subprocess.run(  # noqa: S603  # bash + script path are test-controlled
            [bash, "--noprofile", "--norc", "-eo", "pipefail", str(script)],
            cwd=repo,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    return run


def commit_count(repo: Path) -> int:
    return int(git(repo, "rev-list", "--count", "HEAD"))


def prek_calls(bin_dir: Path) -> int:
    return int((bin_dir / "prek-calls").read_text())


def test_commit_step_fails_when_prek_keeps_failing(
    release_repo: Path,
    stub_prek: Callable[[list[str]], Path],
    run_commit_step: Callable[[Path, Path], subprocess.CompletedProcess[str]],
) -> None:
    bin_dir = stub_prek([PREK_FAILS, f"echo 'strict pass failed' >&2; {PREK_FAILS}"])

    result = run_commit_step(release_repo, bin_dir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "strict pass failed" in result.stderr
    assert prek_calls(bin_dir) == 2
    assert commit_count(release_repo) == 1


def test_commit_step_commits_fixes_to_release_files(
    release_repo: Path,
    stub_prek: Callable[[list[str]], Path],
    run_commit_step: Callable[[Path, Path], subprocess.CompletedProcess[str]],
) -> None:
    bin_dir = stub_prek([f"printf '\\n' >> CHANGELOG.md; {PREK_FAILS}", PREK_OK])

    result = run_commit_step(release_repo, bin_dir)

    assert result.returncode == 0, result.stdout + result.stderr
    assert commit_count(release_repo) == 2
    assert (
        git(release_repo, "log", "-1", "--format=%s")
        == "chore(release): Prepare for 9.9.9 release\n"
    )
    committed = git(release_repo, "show", "HEAD:CHANGELOG.md")
    assert committed == "# Changelog\n\n## 9.9.9\n\n- A change\n\n"
    assert git(release_repo, "status", "--porcelain") == "?? release-notes.md\n"


def test_commit_step_rejects_fixes_outside_release_files(
    release_repo: Path,
    stub_prek: Callable[[list[str]], Path],
    run_commit_step: Callable[[Path, Path], subprocess.CompletedProcess[str]],
) -> None:
    bin_dir = stub_prek([f"printf 'Fixed by a hook\\n' >> docs/other.md; {PREK_FAILS}", PREK_OK])

    result = run_commit_step(release_repo, bin_dir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "docs/other.md" in result.stderr
    assert "+Fixed by a hook" in result.stderr
    assert commit_count(release_repo) == 1
    assert prek_calls(bin_dir) == 1


def test_commit_step_rejects_new_untracked_files(
    release_repo: Path,
    stub_prek: Callable[[list[str]], Path],
    run_commit_step: Callable[[Path, Path], subprocess.CompletedProcess[str]],
) -> None:
    bin_dir = stub_prek(
        [f"mkdir -p skills && printf 'new\\n' > skills/new.md; {PREK_FAILS}", PREK_OK]
    )

    result = run_commit_step(release_repo, bin_dir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "Untracked files prek created:\n  skills/new.md" in result.stderr
    assert commit_count(release_repo) == 1
    assert prek_calls(bin_dir) == 1


def test_commit_step_ignores_preexisting_untracked(
    release_repo: Path,
    stub_prek: Callable[[list[str]], Path],
    run_commit_step: Callable[[Path, Path], subprocess.CompletedProcess[str]],
) -> None:
    bin_dir = stub_prek([PREK_OK, PREK_OK])

    result = run_commit_step(release_repo, bin_dir)

    assert result.returncode == 0, result.stdout + result.stderr
    assert commit_count(release_repo) == 2
    committed = git(release_repo, "show", "--name-only", "--format=", "HEAD").split()
    assert sorted(committed) == ["CHANGELOG.md", "Cargo.toml"]
    assert git(release_repo, "status", "--porcelain") == "?? release-notes.md\n"
    assert prek_calls(bin_dir) == 2


def test_skill_refs_regenerated_before_commit_step() -> None:
    """The bump rewrites `action.yml`, which a skill reference is generated from.

    The commit step fails on prek fixes to files the release didn't change, so the
    reference has to be regenerated before it, as part of the release's own changes.
    """
    steps = yaml.safe_load(PREPARE_RELEASE_YML.read_text())["jobs"]["prepare-release"]["steps"]
    ids = [step.get("id") for step in steps]
    regen = [
        index
        for index, step in enumerate(steps)
        if "cargo xtask build-skill-refs" in step.get("run", "")
    ]
    assert regen, "no step runs `cargo xtask build-skill-refs`"
    assert ids.index("bump-version") < regen[0] < ids.index("commit-release")
