"""Guard the admin-permission gate in `release.yml`."""

from __future__ import annotations

import yaml

from tests._paths import REPO_ROOT

RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"


def test_admin_gate_fails_for_non_admins() -> None:
    steps = yaml.safe_load(RELEASE_YML.read_text(encoding="utf-8"))["jobs"]["prepare-ci"]["steps"]
    [gate] = [
        s for s in steps if s.get("uses", "").startswith("actions-cool/check-user-permission@")
    ]

    # Without `error-if-missing` the action only sets an output and the step passes for anyone.
    assert gate["with"]["require"] == "admin"
    assert gate["with"]["error-if-missing"] is True
