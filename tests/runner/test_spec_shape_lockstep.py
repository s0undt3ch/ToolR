"""The runner JSON spec has the same fields on both sides of the Rust/Python boundary.

``crates/toolr-core/src/execute/spec.rs`` serialises the spec and
``toolr._runner`` decodes it with ``msgspec``. ``msgspec`` ignores unknown keys, so a field
added on the Rust side without a Python twin is dropped silently. This reads the Rust struct
definitions and fails when the field names differ.
"""

from __future__ import annotations

import re

import msgspec
import pytest

from toolr._runner import ContextSpec
from toolr._runner import DispatchPayloadSpec
from toolr._runner import RunnerSpec
from toolr.sources import ArgSchema
from toolr.sources import CommandSchema

from tools.ci import REPO_ROOT

SPEC_RS = REPO_ROOT / "crates" / "toolr-core" / "src" / "execute" / "spec.rs"

#: Rust struct name -> the msgspec struct that decodes it.
TWINS: dict[str, type[msgspec.Struct]] = {
    "ExecutionSpec": RunnerSpec,
    "ContextSpec": ContextSpec,
    "DispatchSpec": DispatchPayloadSpec,
    "CommandSchemaSpec": CommandSchema,
    "ArgSchemaSpec": ArgSchema,
}


def rust_fields(source: str, struct: str) -> list[str]:
    """Field names of ``pub struct <struct> { ... }``, in declaration order."""
    match = re.search(
        rf"^pub struct {struct} \{{\n(?P<body>.*?)^\}}", source, re.MULTILINE | re.DOTALL
    )
    assert match, f"`pub struct {struct}` not found in {SPEC_RS}"
    return re.findall(r"^    pub (\w+):", match["body"], re.MULTILINE)


@pytest.mark.parametrize("rust_struct", TWINS)
def test_rust_spec_fields_match_python_twin(rust_struct: str) -> None:
    python_struct = TWINS[rust_struct]
    rust = rust_fields(SPEC_RS.read_text(), rust_struct)
    python = [field.name for field in msgspec.structs.fields(python_struct)]
    assert rust, f"no fields parsed for {rust_struct}"
    assert sorted(rust) == sorted(python), (
        f"`{rust_struct}` (spec.rs) and `{python_struct.__name__}` disagree on fields. "
        "Add the twin, and bump RUNNER_SCHEMA_VERSION and SCHEMA_VERSION together if the "
        "peers can no longer understand each other."
    )


def test_every_rust_spec_struct_is_covered() -> None:
    declared = set(re.findall(r"^pub struct (\w+) \{", SPEC_RS.read_text(), re.MULTILINE))
    assert declared == set(TWINS), (
        f"spec.rs structs without a Python twin check: {declared ^ set(TWINS)}"
    )
