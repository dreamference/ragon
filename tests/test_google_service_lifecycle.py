"""The Google service container's lifecycle and shared secret, with Docker and the disk mocked."""

import os
import pathlib
import subprocess

import pytest

from dreamference.chat import google_service as module
from dreamference.chat.gmail_credentials import GmailCredentials
from dreamference.chat.google_service import GoogleService
from dreamference.chat.sidecar_network import SidecarNetwork


class Docker:
    """Records `subprocess.run` calls and answers from a per-verb table."""

    def __init__(self):
        self.calls = []
        self.state = ""
        self.env = ""
        self.fail = set()
        self.envs = []

    def __call__(self, argv, **kwargs):
        self.calls.append(argv)
        self.envs.append(kwargs.get("env"))
        verb = argv[1]
        if verb == "inspect" and "{{.State.Status}}" in argv:
            code, out = (0, self.state) if self.state else (1, "")
        elif verb == "inspect":
            code, out = 0, self.env
        else:
            code, out = (1, "") if verb in self.fail else (0, "")
            if verb == "rm" and code == 0:
                self.state = ""
        return subprocess.CompletedProcess(argv, code, stdout=out + "\n", stderr="bad " * 100)

    def verbs(self):
        return [call[1] for call in self.calls]


@pytest.fixture(autouse=True)
def reset_class_state(monkeypatch):
    from conftest import REAL_GOOGLE_SERVICE_START

    monkeypatch.setattr(GoogleService, "start", REAL_GOOGLE_SERVICE_START)
    GoogleService.problem = ""
    GoogleService.restaged = False
    yield
    GoogleService.problem = ""
    GoogleService.restaged = False


@pytest.fixture
def creds(tmp_path, monkeypatch):
    folder = str(tmp_path / "gmail")
    monkeypatch.setattr("dreamference.chat.gmail_credentials.CREDENTIALS_DIR", folder)
    return pathlib.Path(folder)


@pytest.fixture
def docker(monkeypatch):
    fake = Docker()
    monkeypatch.setattr(module.subprocess, "run", fake)
    monkeypatch.setattr(SidecarNetwork, "ensure", classmethod(lambda cls: True))
    return fake


def test_state_is_empty_without_a_container(docker):
    assert GoogleService.state() == ""
    docker.state = "running"
    assert GoogleService.state() == "running"


def test_secret_is_created_once_private_and_reused(creds):
    first = GoogleService.secret()
    assert first and GoogleService.secret() == first
    path = creds / module.SECRET_FILE_NAME
    assert path.read_text() == first
    assert path.stat().st_mode & 0o777 == 0o600
    assert not list(creds.glob("*.tmp"))


def test_secret_reports_a_folder_it_cannot_create(creds):
    creds.parent.chmod(0o500)
    try:
        if os.access(creds.parent, os.W_OK):
            pytest.skip("directory permissions are not enforced")
        assert GoogleService.secret() is None
    finally:
        creds.parent.chmod(0o700)
    assert "shared secret" in GoogleService.problem


def test_store_once_keeps_the_earlier_secret_when_another_start_won(tmp_path):
    path = tmp_path / "s"
    path.write_text("earlier\n")
    assert GoogleService._store_once(path, "later") == "earlier"
    assert path.read_text() == "earlier\n"
    assert not list(tmp_path.glob("*.tmp"))


def test_store_once_replaces_an_empty_file(tmp_path):
    path = tmp_path / "s"
    path.write_text("")
    assert GoogleService._store_once(path, "mine") == "mine"
    assert path.read_text() == "mine"


def test_stage_copies_both_files_then_leaves_them_alone(tmp_path):
    folder = tmp_path / "cfg"
    assert GoogleService.stage(str(folder)) is True
    assert GoogleService.restaged is True
    assert (folder / module.STAGED_SERVICE).is_file() and (folder / module.STAGED_READER).is_file()
    assert GoogleService.stage(str(folder)) is True
    assert GoogleService.restaged is False
    (folder / module.STAGED_READER).write_text("stale")
    GoogleService.stage(str(folder))
    assert GoogleService.restaged is True


def test_stage_reports_an_unwritable_folder(tmp_path):
    blocker = tmp_path / "file"
    blocker.write_text("x")
    assert GoogleService.stage(str(blocker / "sub")) is False
    assert "Could not stage" in GoogleService.problem


def test_carries_secret_matches_only_the_current_variable(docker):
    docker.env = f"A=1\n{module.SECRET_ENV}=s3\nOTHER_GMAIL_SECRET=old"
    assert GoogleService.carries_secret("s3") is True
    assert GoogleService.carries_secret("old") is False


def test_run_command_names_the_secret_without_giving_it():
    argv = GoogleService.run_command("/d")
    assert argv[argv.index("-e") + 1] == module.SECRET_ENV
    assert "127.0.0.1:8767:8000" in argv
    assert "/d:/config" in argv


def test_create_needs_a_secret_and_a_network(creds, docker, monkeypatch):
    monkeypatch.setattr(GoogleService, "secret", classmethod(lambda cls: None))
    assert GoogleService._create(str(creds)) is False
    assert "shared secret" in GoogleService.problem
    monkeypatch.undo()
    monkeypatch.setattr(module.subprocess, "run", docker)
    monkeypatch.setattr(SidecarNetwork, "ensure", classmethod(lambda cls: False))
    assert GoogleService._create(str(creds)) is False
    assert "Docker network" in GoogleService.problem


def test_create_passes_the_secret_by_environment_and_trims_errors(creds, docker, monkeypatch):
    assert GoogleService._create(str(creds)) is True
    secret = (creds / module.SECRET_FILE_NAME).read_text()
    assert docker.envs[-1][module.SECRET_ENV] == secret
    assert secret not in docker.calls[-1]
    docker.fail.add("run")
    assert GoogleService._create(str(creds)) is False
    assert GoogleService.problem.startswith("Could not start the Google service: bad")
    assert len(GoogleService.problem) < 260


def test_start_creates_when_absent_and_stops_when_staging_fails(creds, docker, monkeypatch):
    assert GoogleService.start() is True
    assert docker.verbs()[-1] == "run"
    monkeypatch.setattr(GoogleService, "stage", classmethod(lambda cls, d: False))
    docker.calls.clear()
    assert GoogleService.start() is False
    assert docker.calls == []


def test_start_leaves_a_current_running_container_alone(creds, docker):
    GoogleService.start()
    docker.state = "running"
    docker.env = f"{module.SECRET_ENV}={GoogleService.secret()}"
    docker.calls.clear()
    assert GoogleService.start() is True
    assert "restart" not in docker.verbs() and "run" not in docker.verbs()


def test_start_restarts_a_running_container_when_a_file_changed(creds, docker):
    GoogleService.start()
    docker.state = "running"
    docker.env = f"{module.SECRET_ENV}={GoogleService.secret()}"
    (creds / module.STAGED_SERVICE).write_text("stale")
    docker.calls.clear()
    assert GoogleService.start() is True
    assert docker.calls[-1][1:] == ["restart", module.GOOGLE_CONTAINER_NAME]


def test_start_starts_an_exited_container_and_reports_a_failure(creds, docker):
    GoogleService.start()
    docker.state = "exited"
    docker.env = f"{module.SECRET_ENV}={GoogleService.secret()}"
    assert GoogleService.start() is True
    assert docker.calls[-1][1] == "start"
    docker.fail.add("start")
    assert GoogleService.start() is False
    assert "Could not start dreamference-gmail" in GoogleService.problem


def test_start_replaces_a_container_without_the_secret(creds, docker):
    docker.state = "exited"
    docker.env = "OTHER_GMAIL_SECRET=old"
    assert GoogleService.start() is True
    assert "rm" in docker.verbs() and docker.verbs()[-1] == "run"


def test_start_gives_up_when_the_old_container_cannot_be_removed(creds, docker, monkeypatch):
    docker.state = "exited"
    docker.env = "OTHER_GMAIL_SECRET=old"
    monkeypatch.setattr(GoogleService, "stop", classmethod(lambda cls: False))
    assert GoogleService.start() is False
    assert "made with another secret" in GoogleService.problem


def test_start_fails_when_an_existing_container_has_no_secret(creds, docker, monkeypatch):
    docker.state = "running"
    monkeypatch.setattr(GoogleService, "secret", classmethod(lambda cls: None))
    assert GoogleService.start() is False


def test_stop_reports_whether_a_container_is_left(docker):
    docker.state = "running"
    assert GoogleService.stop() is True
    docker.state = "running"
    docker.fail.add("rm")
    assert GoogleService.stop() is False


def test_ensure_on_node_is_none_off_a_node_or_with_a_container(docker, monkeypatch):
    from dreamference.node.node_identity import NodeIdentity

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: None))
    assert GoogleService.ensure_on_node() is None
    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: "node"))
    docker.state = "exited"
    assert GoogleService.ensure_on_node() is None


def test_ensure_on_node_starts_an_absent_service_and_never_raises(docker, monkeypatch):
    from dreamference.node.node_identity import NodeIdentity

    monkeypatch.setattr(NodeIdentity, "read", classmethod(lambda cls: "node"))
    monkeypatch.setattr(GoogleService, "start", classmethod(lambda cls: True))
    assert GoogleService.ensure_on_node() is True

    def boom(cls):
        raise subprocess.TimeoutExpired("docker", 30)

    monkeypatch.setattr(GoogleService, "start", classmethod(boom))
    assert GoogleService.ensure_on_node() is False
    assert "was not started" in GoogleService.problem


def test_gmail_credentials_directory_and_front(tmp_path, monkeypatch, capsys):
    from dreamference.chat import gmail_credentials as gc

    target = tmp_path / "a" / "b"
    monkeypatch.setattr(gc, "CREDENTIALS_DIR", str(target))
    assert GmailCredentials.ensure_directory() is True
    assert target.is_dir()
    assert GmailCredentials.load() == []
    assert GmailCredentials.forget() is True
    blocker = tmp_path / "f"
    blocker.write_text("x")
    monkeypatch.setattr(gc, "CREDENTIALS_DIR", str(blocker / "sub"))
    assert GmailCredentials.ensure_directory() is False
    assert "Could not create" in capsys.readouterr().out
