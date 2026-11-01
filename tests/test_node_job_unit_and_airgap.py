"""A job's unit state as systemd reports it, and the airgap level when the config cannot be read."""

import subprocess

import pytest

from dreamference.config import DreamferenceConfig
from dreamference.node import node_job
from dreamference.node.node_job import NodeJob


@pytest.mark.parametrize(
    "reported, running",
    [
        ("active\n", True),
        ("activating\n", True),
        ("inactive\n", False),
        ("failed\n", False),
        ("", False),
    ],
)
def test_a_job_runs_while_its_unit_is_active_or_starting(monkeypatch, reported, running):
    asked: list[list[str]] = []

    def fake_run(command, **kwargs):
        asked.append(command)
        return subprocess.CompletedProcess(command, 0, reported, "")

    monkeypatch.setattr(node_job.subprocess, "run", fake_run)
    monkeypatch.setattr(NodeJob, "user_bus_environment", classmethod(lambda cls: {}))
    assert NodeJob.unit_active("j1") is running
    assert asked[0][:3] == ["systemctl", "--user", "is-active"]


def test_an_unreadable_airgap_setting_counts_as_on(monkeypatch):
    def broken(self):
        raise RuntimeError("bad config")

    monkeypatch.setattr(DreamferenceConfig, "resolve_airgapped_level", staticmethod(broken))
    assert NodeJob.node_airgap_level() == "on"
