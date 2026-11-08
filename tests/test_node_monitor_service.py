"""The node monitor's unit text and its set-up, with systemd mocked."""

import subprocess

from dreamference.node import node_monitor_service as module
from dreamference.node.node_monitor_service import UNIT, NodeMonitorService


def test_the_unit_is_a_bounded_notify_service_with_a_watchdog_logging_to_the_journal():
    text = NodeMonitorService.unit_text()
    for line in (
        "Type=notify",
        "NotifyAccess=main",
        "WatchdogSec=60",
        "MemoryMax=64M",
        "MemoryHigh=48M",
        "CPUQuota=5%",
        "TasksMax=32",
        "StandardOutput=journal",
        "Restart=on-failure",
        "RestartMaxDelaySec=10min",
        "WantedBy=default.target",
    ):
        assert line in text.splitlines()
    assert "ragon-node-monitor run" in text


def test_setup_writes_the_unit_and_enables_it_only_when_the_binary_exists(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(
        NodeMonitorService,
        "systemctl",
        classmethod(lambda cls, *a: calls.append(a) or subprocess.CompletedProcess(a, 0, "", "")),
    )
    monkeypatch.setattr(NodeMonitorService, "unit_dir", classmethod(lambda cls: tmp_path))
    monkeypatch.setattr(NodeMonitorService, "executable", classmethod(lambda cls: tmp_path / "bin"))
    assert NodeMonitorService.setup() is False
    assert (tmp_path / UNIT).read_text() == NodeMonitorService.unit_text()
    assert ("enable", "--now", UNIT) not in calls
    (tmp_path / "bin").write_text("")
    assert NodeMonitorService.setup() is True
    assert ("enable", "--now", UNIT) in calls
    assert NodeMonitorService.off() is True
    assert ("disable", "--now", UNIT) in calls
    assert module.UNIT == UNIT
