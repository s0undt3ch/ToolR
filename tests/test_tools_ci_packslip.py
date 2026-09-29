"""Tests for ``toolr ci packslip-check`` in ``tools/ci.py``.

The statement assertions are exercised through ``_packslip_statement_failures``
against factory-built statements and skills trees, so they run without the
packslip CLI. The orchestration is exercised with a stubbed ``ctx.run``, and
once end-to-end against the real CLI when ``packslip`` is on ``PATH``.
"""

from __future__ import annotations

import copy
import json
import shutil
import subprocess
import tarfile
import zipfile
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
from tools.ci import REPO_ROOT
from tools.ci import PackslipFailure
from tools.ci import _packslip_statement_failures
from tools.ci import _skill_frontmatter_name
from tools.ci import packslip_check

from toolr.testing import make_command_result
from toolr.testing import make_context
from toolr.types import ResolvedPath
from toolr.utils.command import CommandResult

SKILL_NAMES = ("alpha", "beta")

StatementFactory = Callable[..., dict[str, Any]]
SkillsTreeFactory = Callable[..., Path]


def _checks(failures: list[PackslipFailure]) -> list[str]:
    return [f.check for f in failures]


@pytest.fixture
def statement() -> StatementFactory:
    """Factory: a passing packslip statement, with artifacts/resources overridable."""

    default_artifacts = [
        {
            "name": "toolr-0.0.0-x86_64-unknown-linux-musl.tar.gz",
            "os": "linux",
            "libc": "musl",
            "bin": ["toolr-0.0.0-x86_64-unknown-linux-musl/toolr"],
        },
        {
            "name": "toolr-0.0.0-aarch64-apple-darwin.tar.gz",
            "os": "macos",
            "bin": [{"path": "toolr-0.0.0-aarch64-apple-darwin/toolr", "name": "toolr"}],
        },
        {
            "name": "toolr-0.0.0-x86_64-pc-windows-msvc.zip",
            "os": "windows",
            "bin": ["toolr-0.0.0-x86_64-pc-windows-msvc/toolr.exe"],
        },
    ]
    default_resources = [
        {"kind": "completion", "shells": ["bash", "zsh", "fish"]},
        *({"kind": "skill", "name": n, "repo": f"skills/{n}"} for n in SKILL_NAMES),
    ]

    def _make(
        *,
        artifacts: list[dict[str, Any]] | None = None,
        resources: list[dict[str, Any]] | None = None,
    ) -> dict[str, Any]:
        return {
            "predicate": {
                "artifacts": copy.deepcopy(default_artifacts) if artifacts is None else artifacts,
                "resources": copy.deepcopy(default_resources) if resources is None else resources,
            }
        }

    return _make


@pytest.fixture
def skills_tree(tmp_path: Path) -> SkillsTreeFactory:
    """Factory: a ``skills/`` dir under tmp_path; ``skills`` maps dir name → SKILL.md text."""

    def _make(skills: dict[str, str] | None = None, *, extra_dirs: tuple[str, ...] = ()) -> Path:
        root = tmp_path / "skills"
        root.mkdir()
        if skills is None:
            skills = {n: f"---\nname: {n}\ndescription: d\n---\n\n# {n}\n" for n in SKILL_NAMES}
        for name, text in skills.items():
            (root / name).mkdir()
            (root / name / "SKILL.md").write_bytes(text.encode())
        for name in extra_dirs:
            (root / name).mkdir()
        return root

    return _make


def test_passing_statement_has_no_failures(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    assert _packslip_statement_failures(statement(), skills_tree()) == []


def test_no_artifacts_fails(statement: StatementFactory, skills_tree: SkillsTreeFactory) -> None:
    assert _checks(_packslip_statement_failures(statement(artifacts=[]), skills_tree())) == [
        "artifacts"
    ]


@pytest.mark.parametrize(
    "bin_entries",
    [
        pytest.param(None, id="missing"),
        pytest.param([], id="empty"),
        pytest.param(["a/toolr", "b/toolr"], id="two"),
    ],
)
def test_bin_entry_count_must_be_one(
    statement: StatementFactory, skills_tree: SkillsTreeFactory, bin_entries: list[str] | None
) -> None:
    artifact: dict[str, Any] = {"name": "x.tar.gz", "os": "macos"}
    if bin_entries is not None:
        artifact["bin"] = bin_entries
    failures = _packslip_statement_failures(statement(artifacts=[artifact]), skills_tree())
    assert _checks(failures) == ["artifact bin entries"]
    assert "expected exactly one bin entry" in failures[0].detail


@pytest.mark.parametrize(
    "entry",
    [
        pytest.param("dir/toolr", id="string-toolr"),
        pytest.param("dir/toolr.exe", id="string-exe"),
        pytest.param({"path": "dir/toolr", "name": "toolr"}, id="object"),
    ],
)
def test_bin_entry_accepted_shapes(
    statement: StatementFactory, skills_tree: SkillsTreeFactory, entry: object
) -> None:
    artifacts = [{"name": "x.tar.gz", "os": "macos", "bin": [entry]}]
    assert _packslip_statement_failures(statement(artifacts=artifacts), skills_tree()) == []


@pytest.mark.parametrize(
    "entry",
    [
        pytest.param("dir/tool", id="wrong-name"),
        pytest.param("dir/toolr.sh", id="wrong-suffix"),
        pytest.param("toolr", id="no-directory"),
        pytest.param({"path": "dir/other", "name": "toolr"}, id="object-wrong-path"),
        pytest.param({"name": "toolr"}, id="object-no-path"),
        pytest.param({"path": None}, id="object-null-path"),
    ],
)
def test_bin_entry_must_end_in_toolr(
    statement: StatementFactory, skills_tree: SkillsTreeFactory, entry: object
) -> None:
    artifacts = [{"name": "x.tar.gz", "os": "macos", "bin": [entry]}]
    failures = _packslip_statement_failures(statement(artifacts=artifacts), skills_tree())
    assert _checks(failures) == ["artifact bin entries"]
    assert "does not end in /toolr or /toolr.exe" in failures[0].detail


@pytest.mark.parametrize("libc", [None, "gnu"])
def test_linux_artifact_must_be_musl(
    statement: StatementFactory, skills_tree: SkillsTreeFactory, libc: str | None
) -> None:
    artifact: dict[str, Any] = {"name": "x.tar.gz", "os": "linux", "bin": ["d/toolr"]}
    if libc is not None:
        artifact["libc"] = libc
    failures = _packslip_statement_failures(statement(artifacts=[artifact]), skills_tree())
    assert failures == [
        PackslipFailure(
            "linux libc", f"x.tar.gz: os=linux but libc={libc or 'null'} (expected musl)"
        )
    ]


def test_missing_skills_dir_fails(statement: StatementFactory, tmp_path: Path) -> None:
    failures = _packslip_statement_failures(statement(), tmp_path / "skills")
    assert _checks(failures) == ["skill resources"]
    assert "directory not found" in failures[0].detail


def test_no_skill_resources_fails(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    resources = [{"kind": "completion", "shells": ["bash", "zsh", "fish"]}]
    assert _checks(_packslip_statement_failures(statement(resources=resources), skills_tree())) == [
        "skill resources"
    ]


def test_no_skill_resources_and_no_skills_fails(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    resources = [{"kind": "completion", "shells": ["bash", "zsh", "fish"]}]
    assert _checks(
        _packslip_statement_failures(statement(resources=resources), skills_tree({}))
    ) == ["skill resources"]


def test_skill_dir_without_resource_fails(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    skills = {n: f"---\nname: {n}\n---\n" for n in (*SKILL_NAMES, "gamma")}
    failures = _packslip_statement_failures(statement(), skills_tree(skills))
    assert _checks(failures) == ["skill resources"]
    assert "gamma" in failures[0].detail


def test_skill_resource_without_dir_fails(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    skills = {"alpha": "---\nname: alpha\n---\n"}
    failures = _packslip_statement_failures(statement(), skills_tree(skills))
    assert _checks(failures) == ["skill resources"]


def test_duplicate_skill_resource_fails(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    resources = statement()["predicate"]["resources"]
    resources.append({"kind": "skill", "name": "alpha", "repo": "skills/alpha"})
    assert _checks(_packslip_statement_failures(statement(resources=resources), skills_tree())) == [
        "skill resources"
    ]


def test_non_skill_dirs_are_ignored(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    root = skills_tree(extra_dirs=("_shared",))
    (root / "README.md").write_text("not a skill\n")
    assert _packslip_statement_failures(statement(), root) == []


def test_skill_repo_path_must_match_name(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    resources = statement()["predicate"]["resources"]
    resources[1]["repo"] = "skills/beta"
    del resources[2]["repo"]
    failures = _packslip_statement_failures(statement(resources=resources), skills_tree())
    assert failures == [
        PackslipFailure("skill repo paths", "alpha: repo is skills/beta, expected skills/alpha"),
        PackslipFailure("skill repo paths", "beta: repo is null, expected skills/beta"),
    ]


@pytest.mark.parametrize(
    "text",
    [
        pytest.param("---\nname: beta\n---\n", id="plain"),
        pytest.param('---\nname: "beta"\n---\n', id="double-quoted"),
        pytest.param("---\nname: 'beta'\n---\n", id="single-quoted"),
        pytest.param("---\nname:   beta   \n---\n", id="whitespace"),
        pytest.param("---\r\nname: beta\r\n---\r\n", id="crlf"),
        pytest.param("---  \r\nname: 'beta' \r\n---\r\n", id="crlf-quoted-trailing-ws"),
        pytest.param("---\ndescription: d\nname: beta\nname: other\n---\n", id="first-name-wins"),
    ],
)
def test_frontmatter_name_parsing(tmp_path: Path, text: str) -> None:
    path = tmp_path / "SKILL.md"
    path.write_bytes(text.encode())
    assert _skill_frontmatter_name(path) == "beta"


@pytest.mark.parametrize(
    "text",
    [
        pytest.param("# beta\n\n---\nname: beta\n---\n", id="frontmatter-not-on-line-1"),
        pytest.param("---\ndescription: d\n---\nname: beta\n", id="name-after-frontmatter"),
        pytest.param("---\ndescription: d\n---\n", id="no-name"),
        pytest.param("", id="empty-file"),
        pytest.param("---\nname: \"beta'\n---\n", id="mismatched-quotes"),
    ],
)
def test_frontmatter_name_outside_frontmatter_is_not_read(tmp_path: Path, text: str) -> None:
    path = tmp_path / "SKILL.md"
    path.write_bytes(text.encode())
    assert _skill_frontmatter_name(path) != "beta"


def test_frontmatter_name_mismatch_fails(
    statement: StatementFactory, skills_tree: SkillsTreeFactory
) -> None:
    skills = {"alpha": "---\nname: alpha\n---\n", "beta": "---\nname: 'gamma'\n---\n"}
    failures = _packslip_statement_failures(statement(), skills_tree(skills))
    assert _checks(failures) == ["skill frontmatter"]
    assert "has name: 'gamma', expected 'beta'" in failures[0].detail


@pytest.mark.parametrize(
    "shells",
    [
        pytest.param(None, id="no-completion"),
        pytest.param(["bash", "zsh"], id="missing-shell"),
        pytest.param(["zsh", "bash", "fish"], id="wrong-order"),
        pytest.param(["bash", "zsh", "fish", "nu"], id="extra-shell"),
    ],
)
def test_completion_resource_shells_must_match(
    statement: StatementFactory, skills_tree: SkillsTreeFactory, shells: list[str] | None
) -> None:
    resources = [r for r in statement()["predicate"]["resources"] if r["kind"] == "skill"]
    if shells is not None:
        resources.append({"kind": "completion", "shells": shells})
    failures = _packslip_statement_failures(statement(resources=resources), skills_tree())
    assert _checks(failures) == ["completion resource"]


def test_every_failure_is_reported(statement: StatementFactory, tmp_path: Path) -> None:
    artifacts = [{"name": "x.tar.gz", "os": "linux", "bin": ["d/other"]}]
    failures = _packslip_statement_failures(
        statement(artifacts=artifacts, resources=[]), tmp_path / "skills"
    )
    assert _checks(failures) == [
        "artifact bin entries",
        "linux libc",
        "skill resources",
        "completion resource",
    ]


@pytest.fixture
def archive_dir(tmp_path: Path) -> Callable[..., ResolvedPath]:
    """Factory: a dir of fake toolr release archives, one ``toolr`` binary each."""

    def _make(
        names: tuple[str, ...] = (
            "toolr-0.0.0-x86_64-unknown-linux-musl.tar.gz",
            "toolr-0.0.0-aarch64-apple-darwin.tar.gz",
            "toolr-0.0.0-x86_64-pc-windows-msvc.zip",
        ),
    ) -> ResolvedPath:
        out = tmp_path / "archives"
        out.mkdir()
        for name in names:
            stem = name.removesuffix(".tar.gz").removesuffix(".zip")
            if name.endswith(".zip"):
                with zipfile.ZipFile(out / name, "w") as zf:
                    zf.writestr(f"{stem}/toolr.exe", b"MZ")
            else:
                binary = tmp_path / "toolr"
                binary.write_bytes(b"\x7fELF")
                with tarfile.open(out / name, "w:gz") as tf:
                    tf.add(binary, arcname=f"{stem}/toolr")
            (out / f"{name}.sha256").write_text("0" * 64)
        return ResolvedPath(out)

    return _make


@pytest.fixture
def fake_packslip(tmp_path: Path) -> ResolvedPath:
    path = tmp_path / "bin" / "packslip"
    path.parent.mkdir()
    path.write_text("#!/bin/sh\n")
    path.chmod(0o755)
    return ResolvedPath(path)


@pytest.fixture
def repo_root(tmp_path: Path, skills_tree: SkillsTreeFactory) -> Path:
    skills_tree()
    manifest = tmp_path / ".github" / "packslip.toml"
    manifest.parent.mkdir()
    manifest.write_text('bin = ["toolr"]\n')
    return tmp_path


def _stub_packslip(
    statement: dict[str, Any], *, fail: str | None = None
) -> Callable[..., CommandResult[str] | CommandResult[bytes]]:
    """A ``ctx.run`` stand-in: git and each packslip subcommand, with one optionally failing."""

    def _run(cmdline: tuple[str, ...], **_: Any) -> CommandResult[str] | CommandResult[bytes]:
        if cmdline[0] == "git":
            return make_command_result(args=list(cmdline), stdout="a" * 40 + "\n")
        sub = cmdline[1]
        if sub == fail:
            return make_command_result(args=list(cmdline), stderr=f"{sub} broke", returncode=1)
        if sub == "create":
            out = Path(cmdline[cmdline.index("--out") + 1])
            out.mkdir()
            (out / "packslip.sigstore.json").write_text("{}")
        if sub == "show":
            return make_command_result(args=list(cmdline), stdout=json.dumps(statement))
        return make_command_result(args=list(cmdline))

    return _run


def test_command_passes(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: Callable[..., ResolvedPath],
    fake_packslip: ResolvedPath,
) -> None:
    calls: list[tuple[str, ...]] = []
    stub = _stub_packslip(statement())

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return stub(cmdline, **kwargs)

    archives = archive_dir()
    ctx = make_context(repo_root, run=_run)
    packslip_check(ctx, archives, packslip=fake_packslip)
    assert "packslip-check: ok" in ctx.stdout

    create = next(c for c in calls if c[1:2] == ("create",))
    expected_archives = sorted(str(p) for p in archives.iterdir() if not p.name.endswith(".sha256"))
    assert list(create[-3:]) == expected_archives
    assert create[create.index("--manifest") + 1] == str(repo_root / ".github" / "packslip.toml")
    assert create[create.index("--commit") + 1] == "a" * 40
    verify = next(c for c in calls if c[1:2] == ("verify",))
    assert verify[2].endswith("packslip.sigstore.json")
    assert [verify[i + 1] for i, a in enumerate(verify) if a == "--artifact"] == expected_archives


@pytest.mark.parametrize("sub", ["keygen", "create", "verify", "show"])
def test_command_names_the_failing_packslip_step(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: Callable[..., ResolvedPath],
    fake_packslip: ResolvedPath,
    sub: str,
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip(statement(), fail=sub))
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir(), packslip=fake_packslip)
    assert exc.value.code == 1
    assert f"packslip-check: packslip {sub} failed: {sub} broke" in ctx.stderr


def test_command_reports_statement_failures(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: Callable[..., ResolvedPath],
    fake_packslip: ResolvedPath,
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip(statement(resources=[])))
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir(), packslip=fake_packslip)
    assert exc.value.code == 1
    assert "packslip-check: skill resources failed:" in ctx.stderr
    assert (
        "packslip-check: completion resource failed: "
        'no completion resource with shells == ["bash", "zsh", "fish"] found'
    ) in ctx.stderr


def test_command_fails_without_archives(
    repo_root: Path, archive_dir: Callable[..., ResolvedPath], fake_packslip: ResolvedPath
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip({}))
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir(names=()), packslip=fake_packslip)
    assert "packslip-check: archives failed: no *.tar.gz or *.zip files found in" in ctx.stderr


def test_command_fails_when_archive_dir_is_a_file(
    repo_root: Path, fake_packslip: ResolvedPath
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip({}))
    with pytest.raises(SystemExit):
        packslip_check(ctx, fake_packslip, packslip=fake_packslip)
    assert "packslip-check: archives failed:" in ctx.stderr
    assert "is not a directory" in ctx.stderr


def test_command_fails_when_packslip_is_not_a_file(
    repo_root: Path, archive_dir: Callable[..., ResolvedPath]
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip({}))
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir(), packslip=ResolvedPath(repo_root))
    assert "packslip-check: packslip failed:" in ctx.stderr
    assert "is not a file" in ctx.stderr


def test_command_fails_without_packslip_on_path(
    repo_root: Path, archive_dir: Callable[..., ResolvedPath], monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("PATH", str(repo_root / "empty-bin"))
    ctx = make_context(repo_root, run=_stub_packslip({}))
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir())
    assert "packslip-check: packslip failed: packslip not found on PATH" in ctx.stderr


def test_command_fails_without_manifest(
    repo_root: Path, archive_dir: Callable[..., ResolvedPath], fake_packslip: ResolvedPath
) -> None:
    (repo_root / ".github" / "packslip.toml").unlink()
    ctx = make_context(repo_root, run=_stub_packslip({}))
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir(), packslip=fake_packslip)
    assert "packslip-check: manifest failed:" in ctx.stderr


def test_command_uses_explicit_manifest(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: Callable[..., ResolvedPath],
    fake_packslip: ResolvedPath,
    tmp_path: Path,
) -> None:
    calls: list[tuple[str, ...]] = []
    stub = _stub_packslip(statement())

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return stub(cmdline, **kwargs)

    other = tmp_path / "other.toml"
    other.write_text("")
    packslip_check(
        make_context(repo_root, run=_run),
        archive_dir(),
        packslip=fake_packslip,
        manifest=ResolvedPath(other),
    )
    create = next(c for c in calls if c[1:2] == ("create",))
    assert create[create.index("--manifest") + 1] == str(other)


def test_command_fails_on_non_json_show_output(
    repo_root: Path, archive_dir: Callable[..., ResolvedPath], fake_packslip: ResolvedPath
) -> None:
    stub = _stub_packslip({})

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        if cmdline[1:2] == ("show",):
            return make_command_result(args=list(cmdline), stdout="not json")
        return stub(cmdline, **kwargs)

    ctx = make_context(repo_root, run=_run)
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir(), packslip=fake_packslip)
    assert "packslip-check: packslip show failed:" in ctx.stderr


def _packslip_runs() -> bool:
    # A mise shim is on PATH even when no packslip version is configured, and then errors.
    path = shutil.which("packslip")
    if path is None:
        return False
    try:
        result = subprocess.run([path, "--version"], capture_output=True, check=False)  # noqa: S603
    except OSError:
        return False
    return result.returncode == 0


@pytest.mark.skipif(
    not _packslip_runs(),
    reason="packslip CLI not runnable from PATH (e.g. `mise x github:jdx/packslip@<version> -- pytest`)",
)
def test_command_end_to_end_with_real_packslip(archive_dir: Callable[..., ResolvedPath]) -> None:
    ctx = make_context(REPO_ROOT)
    packslip_check(ctx, archive_dir())
    assert "packslip-check: ok" in ctx.stdout
