"""Smoke tests for `mise use packslip:github.com/s0undt3ch/ToolR@<version>`."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any
from typing import NoReturn

import pytest

from tests.distribution.mise.conftest import MiseProject

SHELLS = ("bash", "zsh", "fish")
# The library itself, not Homebrew's `profile.d` wrapper, which only loads it when `PS1` is set.
BASH_COMPLETION_SCRIPTS = (
    Path("/usr/share/bash-completion/bash_completion"),
    Path("/opt/homebrew/share/bash-completion/bash_completion"),
    Path("/usr/local/share/bash-completion/bash_completion"),
)
# Built-in `toolr self` subcommands, all of which a completion must offer.
SELF_SUBCOMMANDS = ("build-manifest", "cache", "completion")


def _declared(manifest: dict[str, Any], kind: str) -> list[dict[str, Any]]:
    return [resource for resource in manifest.get("resources", []) if resource.get("kind") == kind]


def _declared_skill_names(manifest: dict[str, Any]) -> set[str]:
    return {resource["name"] for resource in _declared(manifest, "skill")}


def _shell_missing(reason: str) -> NoReturn:
    # CI installs the shells, so a missing one there is a broken setup, not a skip.
    if os.environ.get("TOOLR_SMOKE_REQUIRE_SHELLS"):
        pytest.fail(f"{reason}, but TOOLR_SMOKE_REQUIRE_SHELLS is set")
    pytest.skip(reason)


def _toolr_path_env(project: MiseProject) -> dict[str, str]:
    return {
        **project.env,
        "PATH": f"{project.installed_toolr().parent}{os.pathsep}{project.env['PATH']}",
    }


def test_manifest_matches_release(packslip_manifest: dict[str, Any], smoke_version: str) -> None:
    assert packslip_manifest["version"] == smoke_version
    assert _declared_skill_names(packslip_manifest), "the packslip declares no skills"
    completions = _declared(packslip_manifest, "completion")
    assert completions, "the packslip declares no shell completion"
    assert set(SHELLS) <= {shell for resource in completions for shell in resource["shells"]}


def test_toolr_runs_at_release_version(packslip_project: MiseProject, smoke_version: str) -> None:
    result = packslip_project.run_toolr("--version")
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == f"toolr {smoke_version}"


def test_every_declared_skill_is_installed(
    packslip_project: MiseProject, packslip_manifest: dict[str, Any]
) -> None:
    # `mise use` exits 0 even when a skill fetch fails, so this is the only check that catches it.
    result = packslip_project.run("skills", "ls", "--json")
    assert result.returncode == 0, result.stderr
    installed = {skill["name"] for skill in json.loads(result.stdout)}
    assert installed == _declared_skill_names(packslip_manifest), result.stderr


def test_skills_sync_links_every_declared_skill(
    packslip_project: MiseProject,
    packslip_manifest: dict[str, Any],
    tmp_path: Path,
) -> None:
    target = tmp_path / "skills"
    result = packslip_project.run("skills", "sync", "--dir", str(target))
    assert result.returncode == 0, result.stderr
    for name in _declared_skill_names(packslip_manifest):
        assert (target / name / "SKILL.md").is_file(), (
            f"{name} not linked:\n{result.stdout}\n{result.stderr}"
        )


@pytest.mark.parametrize("shell", SHELLS)
def test_completion_script_generates(packslip_project: MiseProject, shell: str) -> None:
    # The short `--tool toolr` form is what docs/skills.md documents.
    result = packslip_project.run("completion", shell, "--tool", "toolr")
    assert result.returncode == 0, result.stderr
    assert "toolr __complete" in result.stdout


@pytest.mark.skipif(sys.platform == "win32", reason="bash completion is not exercised on Windows")
def test_bash_completion_completes(packslip_project: MiseProject, tmp_path: Path) -> None:
    bash = shutil.which("bash")
    bash_completion = next((path for path in BASH_COMPLETION_SCRIPTS if path.is_file()), None)
    if bash is None or bash_completion is None:
        _shell_missing("bash with bash-completion not available")
    script = tmp_path / "toolr.bash"
    script.write_text(packslip_project.run("completion", "bash", "--tool", "toolr").stdout)
    # Emulates readline on `toolr self <TAB>`: the completion function gets (cmd, cur, prev).
    driver = (
        f'source "{bash_completion}"; source "{script}"\n'
        'COMP_WORDS=(toolr self ""); COMP_CWORD=2; COMP_LINE="toolr self "; COMP_POINT=${#COMP_LINE}\n'
        "func=$(complete -p toolr | sed -E 's/.*-F ([^ ]+).*/\\1/')\n"
        '"$func" toolr "" self\n'
        'printf "%s\\n" "${COMPREPLY[@]}"\n'
    )
    result = subprocess.run(  # noqa: S603
        [bash, "--norc", "--noprofile", "-c", driver],
        cwd=packslip_project.root,
        env=_toolr_path_env(packslip_project),
        capture_output=True,
        text=True,
        check=False,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
    assert set(SELF_SUBCOMMANDS) <= set(result.stdout.split()), result.stderr


@pytest.mark.skipif(sys.platform == "win32", reason="fish completion is not exercised on Windows")
def test_fish_completion_completes(packslip_project: MiseProject, tmp_path: Path) -> None:
    fish = shutil.which("fish")
    if fish is None:
        _shell_missing("fish not available")
    script = tmp_path / "toolr.fish"
    script.write_text(packslip_project.run("completion", "fish", "--tool", "toolr").stdout)
    result = subprocess.run(  # noqa: S603
        [fish, "--no-config", "-c", f"source {script}; complete -C 'toolr self '"],
        cwd=packslip_project.root,
        env=_toolr_path_env(packslip_project),
        capture_output=True,
        text=True,
        check=False,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
    offered = {line.split("\t", 1)[0] for line in result.stdout.splitlines()}
    assert set(SELF_SUBCOMMANDS) <= offered, result.stderr


@pytest.mark.skipif(sys.platform == "win32", reason="zsh completion is not exercised on Windows")
def test_zsh_completion_script_parses(packslip_project: MiseProject, tmp_path: Path) -> None:
    # Syntax only: driving zsh's completion system needs a pty. The engine behind it,
    # `toolr __complete`, is shared with bash and fish, which the tests above exercise.
    zsh = shutil.which("zsh")
    if zsh is None:
        _shell_missing("zsh not available")
    script = tmp_path / "_toolr"
    script.write_text(packslip_project.run("completion", "zsh", "--tool", "toolr").stdout)
    result = subprocess.run(  # noqa: S603
        [zsh, "-n", str(script)],
        capture_output=True,
        text=True,
        check=False,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
