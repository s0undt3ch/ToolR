"""Tests for the `codecov-publish-test-results` composite action's `publish.py`."""

from __future__ import annotations

import importlib.util
import json
import threading
from collections.abc import Callable
from collections.abc import Iterator
from dataclasses import dataclass
from dataclasses import field
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
PUBLISH_PY = REPO_ROOT / ".github" / "actions" / "codecov-publish-test-results" / "publish.py"

REPOSITORY = "s0undt3ch/ToolR"
HEAD_SHA = "a" * 40
PUSH_SHA = "b" * 40
OIDC_TOKEN = "oidc-token-value"  # noqa: S105
REQUEST_TOKEN = "request-token-value"  # noqa: S105


def _load_publish() -> ModuleType:
    spec = importlib.util.spec_from_file_location("codecov_publish_test_results", PUBLISH_PY)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


publish = _load_publish()


@dataclass
class OidcServer:
    url: str
    requests: list[dict[str, str]] = field(default_factory=list)
    body: dict[str, Any] = field(default_factory=lambda: {"value": OIDC_TOKEN})


@pytest.fixture
def oidc_server() -> Iterator[OidcServer]:
    """Serve GitHub's OIDC token endpoint locally, recording each request."""
    state = OidcServer(url="")

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            state.requests.append(
                {"path": self.path, "authorization": self.headers.get("Authorization", "")}
            )
            payload = json.dumps(state.body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    state.url = f"http://127.0.0.1:{server.server_address[1]}/token?api-version=2.0"
    try:
        yield state
    finally:
        server.shutdown()
        server.server_close()


@dataclass
class Recorder:
    commands: list[list[str]] = field(default_factory=list)
    envs: list[dict[str, str]] = field(default_factory=list)
    returncodes: dict[str, int] = field(default_factory=dict)

    def __call__(self, command: list[str], *, env: dict[str, str], check: bool) -> Any:
        assert check is False
        self.commands.append(command)
        self.envs.append(env)
        returncode = self.returncodes.get(command[1], 0)
        return type("Completed", (), {"returncode": returncode})()


@pytest.fixture
def recorder() -> Recorder:
    return Recorder()


@pytest.fixture
def stage(tmp_path: Path) -> Callable[..., Path]:
    """Create one staged directory the way `codecov-stage-test-results` lays it out."""
    root = tmp_path / "staged"

    def make(
        artifact: str, *, files: list[str], flags: list[str], name: str = "", meta: Any = None
    ) -> Path:
        directory = root / f"codecov-test-results-{artifact}"
        directory.mkdir(parents=True)
        for file in files:
            (directory / file).write_text("<testsuites/>", encoding="utf-8")
        if meta is None:
            meta = {"files": files, "flags": flags, "name": name}
        if meta is not False:
            (directory / "meta.json").write_text(json.dumps(meta), encoding="utf-8")
        return directory

    return make


@pytest.fixture
def event(tmp_path: Path) -> Callable[..., str]:
    """Write a GitHub event payload and return its path."""

    def make(*, pr: dict[str, Any] | None = None) -> str:
        path = tmp_path / "event.json"
        path.write_text(
            json.dumps({"pull_request": pr} if pr is not None else {"ref": "refs/heads/main"})
        )
        return str(path)

    return make


def _pr(head_repo: str = REPOSITORY) -> dict[str, Any]:
    return {
        "number": 42,
        "head": {"sha": HEAD_SHA, "label": "someone:topic", "repo": {"full_name": head_repo}},
    }


def _env(event_path: str, oidc: OidcServer | None = None) -> dict[str, str]:
    env = {
        "GITHUB_EVENT_PATH": event_path,
        "GITHUB_REPOSITORY": REPOSITORY,
        "GITHUB_SHA": PUSH_SHA,
        "CODECOV_TOKEN": "stale",
    }
    if oidc is not None:
        env["ACTIONS_ID_TOKEN_REQUEST_URL"] = oidc.url
        env["ACTIONS_ID_TOKEN_REQUEST_TOKEN"] = REQUEST_TOKEN
    return env


def test_uploads_each_staged_dir_with_its_own_flags_then_notifies(
    tmp_path: Path,
    stage: Callable[..., Path],
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
) -> None:
    linux = stage(
        "ubuntu-latest-3.14",
        files=["junit-3.14-ubuntu-latest.xml"],
        flags=["ubuntu-latest", "3.14"],
        name="Test / Linux (3.14)",
    )
    dist = stage(
        "dist-windows-latest", files=["junit-dist-windows-latest.xml"], flags=["windows-latest"]
    )

    rc = publish.main(
        [str(tmp_path / "staged"), "--cli", "codecovcli"],
        env=_env(event(pr=_pr()), oidc_server),
        run=recorder,
    )

    assert rc == 0
    common = ["--sha", HEAD_SHA, "--git-service", "github"]
    upload = [
        "codecovcli",
        "upload-process",
        "--report-type",
        "test_results",
        "--disable-search",
        *common,
    ]
    assert recorder.commands == [
        [
            *upload,
            "--file",
            str(dist / "junit-dist-windows-latest.xml"),
            "--flag",
            "windows-latest",
        ],
        [
            *upload,
            "--file",
            str(linux / "junit-3.14-ubuntu-latest.xml"),
            "--flag",
            "ubuntu-latest",
            "--flag",
            "3.14",
            "--name",
            "Test / Linux (3.14)",
        ],
        ["codecovcli", "send-notifications", *common],
    ]
    assert all(env["CODECOV_TOKEN"] == OIDC_TOKEN for env in recorder.envs)
    assert not any(OIDC_TOKEN in part for command in recorder.commands for part in command)
    assert oidc_server.requests == [
        {
            "path": "/token?api-version=2.0&audience=https%3A%2F%2Fcodecov.io",
            "authorization": f"bearer {REQUEST_TOKEN}",
        }
    ]


def test_fork_uploads_tokenless_with_pr_and_branch(
    tmp_path: Path, stage: Callable[..., Path], event: Callable[..., str], recorder: Recorder
) -> None:
    stage("ubuntu-latest-3.14", files=["junit.xml"], flags=["ubuntu-latest"])

    rc = publish.main(
        [str(tmp_path / "staged")], env=_env(event(pr=_pr("someone/ToolR"))), run=recorder
    )

    assert rc == 0
    notify = ["--sha", HEAD_SHA, "--git-service", "github"]
    upload = [*notify, "--pr", "42", "--branch", "someone:topic"]
    assert recorder.commands[0][5 : 5 + len(upload)] == upload
    # `send-notifications` rejects `--pr` and `--branch`.
    assert recorder.commands[-1] == ["codecovcli", "send-notifications", *notify]
    assert all("CODECOV_TOKEN" not in env for env in recorder.envs)


def test_push_reports_against_github_sha(
    tmp_path: Path,
    stage: Callable[..., Path],
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
) -> None:
    stage("ubuntu-latest-3.14", files=["junit.xml"], flags=[])

    rc = publish.main([str(tmp_path / "staged")], env=_env(event(), oidc_server), run=recorder)

    assert rc == 0
    assert all(command[command.index("--sha") + 1] == PUSH_SHA for command in recorder.commands)
    assert "--flag" not in recorder.commands[0]


def test_no_staged_results_warns_and_still_notifies(
    tmp_path: Path,
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
    capsys: pytest.CaptureFixture[str],
) -> None:
    rc = publish.main(
        [str(tmp_path / "missing")], env=_env(event(pr=_pr()), oidc_server), run=recorder
    )

    assert rc == 0
    assert [command[1] for command in recorder.commands] == ["send-notifications"]
    assert "::warning::No staged Codecov test results" in capsys.readouterr().out


def test_no_send_notifications(
    tmp_path: Path,
    stage: Callable[..., Path],
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
) -> None:
    stage("a", files=["junit.xml"], flags=[])

    rc = publish.main(
        [str(tmp_path / "staged"), "--no-send-notifications"],
        env=_env(event(pr=_pr()), oidc_server),
        run=recorder,
    )

    assert rc == 0
    assert [command[1] for command in recorder.commands] == ["upload-process"]


def test_failed_upload_still_sends_notifications_and_fails(
    tmp_path: Path,
    stage: Callable[..., Path],
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
    capsys: pytest.CaptureFixture[str],
) -> None:
    stage("a", files=["junit.xml"], flags=[])
    recorder.returncodes["upload-process"] = 3

    rc = publish.main(
        [str(tmp_path / "staged")], env=_env(event(pr=_pr()), oidc_server), run=recorder
    )

    assert rc == 1
    assert [command[1] for command in recorder.commands] == ["upload-process", "send-notifications"]
    assert "exited with 3" in capsys.readouterr().out


@pytest.mark.parametrize(
    ("meta", "message"),
    [
        (False, "unreadable"),
        ({"files": [], "flags": [], "name": ""}, "`files` must be a non-empty list"),
        ({"files": ["junit.xml"], "flags": "x", "name": ""}, "`flags` must be a list"),
        ({"files": ["junit.xml"], "flags": [], "name": 1}, "`name` must be a string"),
        ({"files": ["other.xml"], "flags": [], "name": ""}, "listed files are missing"),
        (["not", "an", "object"], "`files` must be a non-empty list"),
    ],
)
def test_bad_meta_fails_before_uploading(
    tmp_path: Path,
    stage: Callable[..., Path],
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
    capsys: pytest.CaptureFixture[str],
    meta: Any,
    message: str,
) -> None:
    stage("a", files=["junit.xml"], flags=[], meta=meta)

    rc = publish.main(
        [str(tmp_path / "staged")], env=_env(event(pr=_pr()), oidc_server), run=recorder
    )

    assert rc == 1
    assert recorder.commands == []
    assert message in capsys.readouterr().out


def test_missing_oidc_permission_fails(
    tmp_path: Path,
    event: Callable[..., str],
    recorder: Recorder,
    capsys: pytest.CaptureFixture[str],
) -> None:
    rc = publish.main([str(tmp_path / "staged")], env=_env(event(pr=_pr())), run=recorder)

    assert rc == 1
    assert recorder.commands == []
    assert "id-token: write" in capsys.readouterr().out


def test_empty_oidc_response_fails(
    tmp_path: Path,
    event: Callable[..., str],
    oidc_server: OidcServer,
    recorder: Recorder,
    capsys: pytest.CaptureFixture[str],
) -> None:
    oidc_server.body = {}

    rc = publish.main(
        [str(tmp_path / "staged")], env=_env(event(pr=_pr()), oidc_server), run=recorder
    )

    assert rc == 1
    assert "carried no token" in capsys.readouterr().out


def test_no_commit_to_report_against_fails(
    tmp_path: Path, recorder: Recorder, capsys: pytest.CaptureFixture[str]
) -> None:
    rc = publish.main(
        [str(tmp_path / "staged")], env={"GITHUB_REPOSITORY": REPOSITORY}, run=recorder
    )

    assert rc == 1
    assert "can't tell which commit" in capsys.readouterr().out


def test_oidc_url_without_query_gets_question_mark(oidc_server: OidcServer) -> None:
    url = oidc_server.url.split("?", 1)[0]

    assert publish.fetch_oidc_token(url, REQUEST_TOKEN) == OIDC_TOKEN
    assert oidc_server.requests[0]["path"] == "/token?audience=https%3A%2F%2Fcodecov.io"
