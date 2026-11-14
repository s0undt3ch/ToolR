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


STRUCT_RE = re.compile(
    r"^(?P<attrs>(?:[ ]*(?:\#\[.*\]|///.*)\n)*)"
    r"(?:pub(?:\([^)]+\))? )?struct (?P<name>\w+)(?:<[^>]*>)?(?P<open>\s*[{(;]|\s*where)",
    re.MULTILINE,
)
# The 4-space indent is deliberate: rustfmt puts struct fields there, and it skips nested items.
FIELD_RE = re.compile(r"^ {4}(?:pub(?:\([^)]+\))? )?(?:r#)?(?P<name>\w+)\s*:(?!:)")
RENAME_RE = re.compile(r'\brename\s*=\s*"(?P<wire>[^"]+)"')


def _wire_name(name: str, attrs: list[str], struct: str) -> str | None:
    """The key serde writes for a field, or ``None`` when serde never serialises it."""
    text = " ".join(attrs)
    skipped_both_ways = re.search(r"\bskip_serializing\b", text) and re.search(
        r"\bskip_deserializing\b", text
    )
    if re.search(r"\bskip\b", text) or skipped_both_ways:
        return None
    assert not re.search(r"\b(rename\(|flatten)", text), (
        f"`{struct}.{name}`: unsupported serde attribute `{text}`"
    )
    renamed = RENAME_RE.search(text)
    return renamed["wire"] if renamed else name


def rust_structs(source: str) -> dict[str, list[str]]:
    """Wire field names of every struct in ``source``, in declaration order.

    Not a Rust parser: it reads the one-attribute-per-line style rustfmt produces and
    fails loudly on a shape it would misread (tuple structs, ``rename_all``, ``flatten``).
    """
    structs: dict[str, list[str]] = {}
    for match in STRUCT_RE.finditer(source):
        name = match["name"]
        assert match["open"].strip() == "{", (
            f"`struct {name}` is not a braced struct; the lock-step test cannot read it"
        )
        assert "rename_all" not in match["attrs"], (
            f"`struct {name}` uses serde `rename_all`; the lock-step test does not apply it"
        )
        body = source[match.end() :].split("\n}", 1)[0]
        fields: list[str] = []
        attrs: list[str] = []
        for line in body.splitlines():
            if line.lstrip().startswith("#["):
                attrs.append(line.strip())
                continue
            field = FIELD_RE.match(line)
            if field:
                wire = _wire_name(field["name"], attrs, name)
                if wire is not None:
                    fields.append(wire)
                attrs = []
        structs[name] = fields
    return structs


def rust_fields(source: str, struct: str) -> list[str]:
    structs = rust_structs(source)
    assert struct in structs, f"`struct {struct}` not found in {SPEC_RS}"
    return structs[struct]


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
    declared = set(rust_structs(SPEC_RS.read_text()))
    assert declared == set(TWINS), (
        f"spec.rs structs without a Python twin check: {declared ^ set(TWINS)}"
    )


@pytest.mark.parametrize(
    ("source", "expected"),
    [
        pytest.param(
            "pub struct S {\n    pub a: u8,\n    pub b: String,\n}\n", ["a", "b"], id="pub"
        ),
        pytest.param(
            "pub struct S {\n    a: u8,\n    pub(crate) b: u8,\n}\n",
            ["a", "b"],
            id="private-and-pub-crate",
        ),
        pytest.param(
            "pub struct S {\n    pub(super) a: u8,\n    pub(in crate::x) b: u8,\n}\n",
            ["a", "b"],
            id="pub-in-path",
        ),
        pytest.param(
            "pub struct S {\n    pub r#type: u8,\n    pub r#match: u8,\n}\n",
            ["type", "match"],
            id="raw-identifiers",
        ),
        pytest.param(
            'pub struct S {\n    #[serde(rename = "kind")]\n    pub ty: u8,\n    pub b: u8,\n}\n',
            ["kind", "b"],
            id="rename",
        ),
        pytest.param(
            'pub struct S {\n    #[serde(default, rename = "k")]\n    /// doc\n    pub ty: u8,\n}\n',
            ["k"],
            id="rename-with-doc-between",
        ),
        pytest.param(
            "pub struct S {\n    #[serde(skip)]\n    pub a: u8,\n    pub b: u8,\n}\n",
            ["b"],
            id="skip",
        ),
        pytest.param(
            "pub struct S {\n    #[serde(skip_serializing, skip_deserializing)]\n    pub a: u8,\n    pub b: u8,\n}\n",
            ["b"],
            id="skip-both-directions",
        ),
        pytest.param(
            "pub struct S {\n    #[serde(skip_serializing)]\n    pub a: u8,\n}\n",
            ["a"],
            id="skip-one-direction-stays",
        ),
        pytest.param(
            'pub struct S {\n    #[serde(skip_serializing_if = "Option::is_none")]\n    pub a: Option<u8>,\n}\n',
            ["a"],
            id="skip-serializing-if-stays",
        ),
        pytest.param("pub struct S<'a> {\n    pub a: &'a str,\n}\n", ["a"], id="generic"),
        pytest.param(
            "struct S {\n    a: std::collections::BTreeMap<String, u8>,\n    b: u8,\n}\n",
            ["a", "b"],
            id="private-struct-and-path-type",
        ),
    ],
)
def test_rust_structs_follows_serde_wire_names(source: str, expected: list[str]) -> None:
    assert rust_structs(source) == {"S": expected}


@pytest.mark.parametrize(
    ("source", "message"),
    [
        pytest.param("pub struct S(pub u8);\n", "not a braced struct", id="tuple-struct"),
        pytest.param("pub struct S;\n", "not a braced struct", id="unit-struct"),
        pytest.param(
            '#[serde(rename_all = "camelCase")]\npub struct S {\n    pub a_b: u8,\n}\n',
            "rename_all",
            id="rename-all",
        ),
        pytest.param(
            "pub struct S {\n    #[serde(flatten)]\n    pub a: u8,\n}\n",
            "unsupported serde attribute",
            id="flatten",
        ),
        pytest.param(
            'pub struct S {\n    #[serde(rename(serialize = "x"))]\n    pub a: u8,\n}\n',
            "unsupported serde attribute",
            id="split-rename",
        ),
    ],
)
def test_rust_structs_rejects_shapes_it_cannot_read(source: str, message: str) -> None:
    with pytest.raises(AssertionError, match=message):
        rust_structs(source)
