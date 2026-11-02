"""The installers, the docs and the action pin the same release identities.

`action.yml`'s verify-attestation step is the reference (tested in
`tests/action_yml/test_verify_attestation.py`). `install.ps1` has no
behavioural test without `pwsh`, so this is what keeps its identities
and `--jq` filter in step with the others.
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
RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"

DEFAULT_REPO = "s0undt3ch/ToolR"
WORKFLOWS = f"https://github.com/{DEFAULT_REPO}/.github/workflows"


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


@pytest.fixture(scope="module")
def action_pins() -> dict[str, str]:
    step = _action_step()
    return {
        "SIGNER_WORKFLOW": step["env"]["SIGNER_WORKFLOW"],
        "RELEASE_WORKFLOW": step["env"]["RELEASE_WORKFLOW"],
        "filter": _jq_filter(step["run"]),
    }


def test_action_pins_are_the_expected_identities(action_pins: dict[str, str]) -> None:
    assert (
        action_pins["SIGNER_WORKFLOW"] == f"{WORKFLOWS}/_build-binary-archive.yml@refs/heads/main"
    )
    assert action_pins["RELEASE_WORKFLOW"] == f"{WORKFLOWS}/release.yml@refs/heads/main"


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
    match = re.search(r"^\s+\$jqFilter = '([^']+)'$", text, flags=re.MULTILINE)
    assert match is not None
    jq_filter = match.group(1)
    assert _collapse(jq_filter) == action_pins["filter"]
    # Windows PowerShell 5.1 mangles embedded double quotes in native arguments.
    assert '"' not in jq_filter
    assert "$" not in jq_filter
    assert "$env:RELEASE_WORKFLOW = $releaseWorkflow" in text
    assert "$env:ARCHIVE_NAME = $archiveName" in text
    assert "$env:ARCHIVE_SHA256 = $archiveSha256" in text
    # Get-FileHash prints uppercase hex; gh's subject digests are lowercase.
    assert "$archiveSha256 = (Get-FileHash -Algorithm SHA256 $Path).Hash.ToLower()" in text
    assert "--cert-identity $signerWorkflow" in text
    assert "--deny-self-hosted-runners" in text
    assert "--signer-workflow" not in text


def test_manual_command_in_docs_matches_the_action(action_pins: dict[str, str]) -> None:
    text = INSTALL_DOCS.read_text()
    assert f"export RELEASE_WORKFLOW={action_pins['RELEASE_WORKFLOW']}\n" in text
    assert f"--cert-identity {action_pins['SIGNER_WORKFLOW']} " in text
    assert "--deny-self-hosted-runners" in text
    assert _jq_filter(text) == action_pins["filter"]
    assert "--signer-workflow" not in text


def test_mise_workflow_pin_matches_the_packslip_signer(action_pins: dict[str, str]) -> None:
    """The documented `workflow` pin names the identity `publish-packslip` signs as."""
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
    for pin in pins:
        identity = f"{WORKFLOWS}/{pin}"
        assert f"--identity {identity} " in RELEASE_YML.read_text()
        assert identity == action_pins["RELEASE_WORKFLOW"]
