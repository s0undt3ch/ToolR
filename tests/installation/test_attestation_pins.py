"""The installers, the docs and the action pin the same release identities.

Every copy of the release pins, which all change together if releases
move off `main`: `action.yml`'s verify-attestation step (the reference,
with its literals checked in `tests/action_yml/test_verify_attestation.py`),
`installation/install.sh`, `installation/install.ps1`, the manual command
in `docs/installation/index.md` and the `workflow` pin in
`docs/installation/mise.md`. `install.ps1` has no behavioural test
without `pwsh`, so this is what keeps it in step with the others.
"""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ACTION_YML = REPO_ROOT / "action.yml"
INSTALL_SH = REPO_ROOT / "installation" / "install.sh"
INSTALL_PS1 = REPO_ROOT / "installation" / "install.ps1"
INSTALL_DOCS = REPO_ROOT / "docs" / "installation" / "index.md"
MISE_DOCS = REPO_ROOT / "docs" / "installation" / "mise.md"

DEFAULT_REPO = "s0undt3ch/ToolR"


def _collapse(text: str) -> str:
    return " ".join(text.split())


def _action_step() -> dict[str, Any]:
    data = yaml.safe_load(ACTION_YML.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == "verify-attestation":
            return step
    msg = "no `verify-attestation` step in action.yml"
    raise AssertionError(msg)


def _jq_filter(text: str) -> str:
    """The single-quoted `--jq` argument in a shell script or code block."""
    found = re.findall(r"--jq '([^']*)'", text)
    assert len(found) == 1, found
    return _collapse(found[0])


def _gh_floors(text: str, code_pattern: str) -> set[tuple[int, int]]:
    """The gh version floor in a script's comparison and in its error message."""
    floors = {
        (int(major), int(minor))
        for major, minor in re.findall(r"gh >= (\d+)\.(\d+) is required", text)
    }
    found = re.findall(code_pattern, text)
    assert found, code_pattern
    floors.update((int(major), int(minor)) for major, minor in found)
    return floors


SH_FLOOR = r'\[ "\$gh_major" -eq (\d+) \] && \[ "\$gh_minor" -lt (\d+) \]'


@pytest.fixture(scope="module")
def action_pins() -> dict[str, str]:
    step = _action_step()
    return {
        "SIGNER_WORKFLOW": step["env"]["SIGNER_WORKFLOW"],
        "RELEASE_WORKFLOW": step["env"]["RELEASE_WORKFLOW"],
        "filter": _jq_filter(step["run"]),
    }


def test_gh_version_floor_agrees() -> None:
    """Each script's comparison and message name the same minimum gh."""
    floors = {
        "action.yml": _gh_floors(_action_step()["run"], SH_FLOOR),
        "install.sh": _gh_floors(INSTALL_SH.read_text(), SH_FLOOR),
        "install.ps1": _gh_floors(INSTALL_PS1.read_text(), r"\[version\]'(\d+)\.(\d+)'"),
    }
    assert all(len(floor) == 1 for floor in floors.values()), floors
    assert len(set().union(*floors.values())) == 1, floors


def test_install_sh_matches_the_action(action_pins: dict[str, str]) -> None:
    text = INSTALL_SH.read_text()
    assert re.search(r'^REPO="\$\{TOOLR_REPO:-s0undt3ch/ToolR\}"$', text, flags=re.MULTILINE)
    for name in ("SIGNER_WORKFLOW", "RELEASE_WORKFLOW"):
        match = re.search(rf'^\s+{name}="([^"]+)"$', text, flags=re.MULTILINE)
        assert match is not None, name
        assert match.group(1).replace("${REPO}", DEFAULT_REPO) == action_pins[name]
    assert _jq_filter(text) == action_pins["filter"]
    assert '--cert-identity "$SIGNER_WORKFLOW"' in text
    assert "--deny-self-hosted-runners" in text
    assert "--signer-workflow" not in text


def test_install_ps1_matches_the_action(action_pins: dict[str, str]) -> None:
    text = INSTALL_PS1.read_text()
    assert re.search(r'^\s+\[string\]\$Repo = "s0undt3ch/ToolR",$', text, flags=re.MULTILINE)
    for variable, name in (
        ("signerWorkflow", "SIGNER_WORKFLOW"),
        ("releaseWorkflow", "RELEASE_WORKFLOW"),
    ):
        match = re.search(rf'^\s+\${variable} = "([^"]+)"$', text, flags=re.MULTILINE)
        assert match is not None, variable
        assert match.group(1).replace("$Repo", DEFAULT_REPO) == action_pins[name]
    assert "--cert-identity $signerWorkflow" in text
    assert "--deny-self-hosted-runners" in text
    assert "--signer-workflow" not in text


def test_install_ps1_filter_survives_windows_powershell(action_pins: dict[str, str]) -> None:
    """One line, no quotes, no `$`: Windows PowerShell 5.1 mangles quotes in native arguments."""
    match = re.search(r"^\s+\$jqFilter = '([^'\n]+)'$", INSTALL_PS1.read_text(), flags=re.MULTILINE)
    assert match is not None
    jq_filter = match.group(1)
    assert jq_filter == _collapse(action_pins["filter"])
    assert '"' not in jq_filter
    assert "$" not in jq_filter


def test_install_ps1_sets_and_restores_the_filter_env() -> None:
    """Every `env.X` the filter reads is set for gh, lowercased digest included, and restored."""
    text = INSTALL_PS1.read_text()
    block = re.search(r"^\s+\$filterEnv = @\{\n(.*?)^\s+\}$", text, flags=re.MULTILINE | re.DOTALL)
    assert block is not None
    values = dict(re.findall(r"^\s+([A-Z0-9_]+) = (.+)$", block.group(1), flags=re.MULTILINE))
    filter_match = re.search(r"\$jqFilter = '([^']+)'", text)
    assert filter_match is not None
    assert set(values) == set(re.findall(r"env\.([A-Z0-9_]+)", filter_match.group(1)))
    # Get-FileHash prints uppercase hex; gh's subject digests are lowercase.
    assert "Get-FileHash -Algorithm SHA256" in values["ARCHIVE_SHA256"]
    assert values["ARCHIVE_SHA256"].endswith(".ToLower()")
    restore = re.search(r"\} finally \{\n(.*?)^  \}$", text, flags=re.MULTILINE | re.DOTALL)
    assert restore is not None
    assert "foreach ($name in $savedEnv.Keys)" in restore.group(1)
    assert "foreach ($name in $filterEnv.Keys)" in text.split("$savedEnv = @{}", 1)[1]


def test_manual_command_in_docs_matches_the_action(action_pins: dict[str, str]) -> None:
    text = INSTALL_DOCS.read_text()
    assert f"export RELEASE_WORKFLOW={action_pins['RELEASE_WORKFLOW']}\n" in text
    assert f"--cert-identity {action_pins['SIGNER_WORKFLOW']} " in text
    assert "--deny-self-hosted-runners" in text
    assert _jq_filter(text) == action_pins["filter"]
    assert "--signer-workflow" not in text


def test_mise_workflow_pin_matches_the_packslip_signer(action_pins: dict[str, str]) -> None:
    """Every documented `workflow` pin names the release workflow identity."""
    blocks = re.findall(
        r"^```toml\n(.*?)^```$", MISE_DOCS.read_text(), flags=re.MULTILINE | re.DOTALL
    )
    pins = [
        spec["workflow"]
        for block in blocks
        for spec in tomllib.loads(block).get("tools", {}).values()
        if isinstance(spec, dict) and "workflow" in spec
    ]
    assert pins
    workflows = f"https://github.com/{DEFAULT_REPO}/.github/workflows"
    for pin in pins:
        # test_verify_attestation.py checks RELEASE_WORKFLOW is publish-packslip's `--identity`.
        assert f"{workflows}/{pin}" == action_pins["RELEASE_WORKFLOW"]
