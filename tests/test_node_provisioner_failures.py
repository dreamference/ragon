"""Tests for provisioning's install step, its dry run's changes and its summary table.

Every ssh command, rsync and sleep is a stand-in.
"""

from pathlib import Path
from types import SimpleNamespace

import pytest

from dreamference.node import node_provisioner
from dreamference.node.fleet_bundle import FleetBundle
from dreamference.node.node_provisioner import NodeProvisioner


@pytest.fixture
def provisioner(tmp_path, monkeypatch):
    """A provisioner for `node-b` that never sleeps, with a bundle folder."""
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    monkeypatch.setattr(node_provisioner.time, "sleep", lambda seconds: None)
    monkeypatch.setattr(FleetBundle, "version", classmethod(lambda cls, bundle: "v1"))
    made = NodeProvisioner(["node-b"], {"user": "owner", "model": "org/model"})
    made.bundle = Path(tmp_path / "bundle")
    made.results["node-b"] = {"host": "node-b", "done": [], "probe": {"venv": "1"}}
    return made


def _answer(code, err="", out=""):
    return SimpleNamespace(returncode=code, stderr=err, stdout=out)


class _Session:
    """A session whose rsync and run answers are set per test."""

    host = "node-b"

    def __init__(self, copy, install):
        self.copy = copy
        self.install = install
        self.ran = []

    def rsync(self, source, remote):
        return self.copy

    def run(self, command, timeout=None):
        self.ran.append(command)
        return self.install


def test_an_install_of_the_current_version_runs_nothing(provisioner):
    provisioner.results["node-b"]["probe"].update(bundle="v1", admin="1")
    session = _Session(_answer(1), _answer(1))
    assert provisioner._step_install("node-b", session) is True
    assert session.ran == []


def test_a_failed_copy_of_the_bundle_stops_before_the_install(provisioner):
    session = _Session(_answer(23, err="rsync: no space"), _answer(0))
    assert provisioner._step_install("node-b", session) is False
    result = provisioner.results["node-b"]
    assert result["failed"] == "copy the bundle"
    assert "no space" in result["reason"]
    assert result["complete"] is False
    assert session.ran == []


def test_a_failed_install_names_the_step_and_the_command_to_rerun(provisioner):
    session = _Session(_answer(0), _answer(1, err="", out="install.sh: boom"))
    assert provisioner._step_install("node-b", session) is False
    result = provisioner.results["node-b"]
    assert result["failed"] == "install Ragon"
    assert "boom" in result["reason"]
    assert result["left"] == "ragon-admin node provision node-b"


def test_a_failed_venv_install_stops_the_install_step(provisioner):
    provisioner.results["node-b"]["probe"]["venv"] = "0"
    provisioner.passwords = {}
    session = SimpleNamespace(
        host="node-b",
        sudo=lambda command, password: _answer(100, err="apt: locked"),
        rsync=lambda *a: pytest.fail("must not copy"),
    )
    assert provisioner._step_install("node-b", session) is False
    assert provisioner.results["node-b"]["failed"] == "install python3-venv"


def test_the_summary_prints_a_row_for_a_machine_never_reached(provisioner, capsys):
    provisioner.hosts = ["node-b", "node-c"]
    provisioner.results["node-b"].update(
        node="0123456789", copied_bytes=2 * 1024**3, throughput="100 MB/s", complete=True
    )
    provisioner.print_summary()
    out = capsys.readouterr().out
    assert "node-c" in out
    assert "re-run" in out
    assert "2.0 GiB" in out
    assert "never a password" in out


def test_a_machine_that_does_not_return_from_the_reboot_fails_the_wait(provisioner):
    session = SimpleNamespace(open=lambda: False)
    assert provisioner._wait_for_ssh(session, timeout=0) is False
    assert provisioner._wait_for_ssh(SimpleNamespace(open=lambda: True)) is True
