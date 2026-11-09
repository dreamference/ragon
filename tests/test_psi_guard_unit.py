"""The `ragon-psi-guard` user unit: started with the server, stopped with it; systemd mocked."""

import subprocess

from dreamference.vllm_server import psi_guard_unit, psi_watchdog
from dreamference.vllm_server.psi_guard_unit import PSI_GUARD_UNIT, PsiGuardUnit
from dreamference.vllm_server.vllm_server_manager import VLLMServerManager


def _record(monkeypatch, returncode=0):
    calls = []

    def run(cmd, **kwargs):
        calls.append(list(cmd))
        return subprocess.CompletedProcess(cmd, returncode)

    monkeypatch.setattr(psi_guard_unit.subprocess, "run", run)
    return calls


def test_start_replaces_a_stale_unit_then_runs_the_watchdog_module(monkeypatch):
    calls = _record(monkeypatch)
    assert PsiGuardUnit.start("c1") is True
    assert calls[0] == ["systemctl", "--user", "stop", PSI_GUARD_UNIT]
    assert calls[1][:2] == ["systemd-run", "--user"]
    assert f"--unit={PSI_GUARD_UNIT}" in calls[1]
    assert calls[1][-1] == "c1"


def test_start_reports_a_failed_systemd_run(monkeypatch):
    _record(monkeypatch, returncode=1)
    assert PsiGuardUnit.start("c1") is False


def test_start_survives_a_missing_systemd(monkeypatch):
    def boom(*a, **k):
        raise FileNotFoundError

    monkeypatch.setattr(psi_guard_unit.subprocess, "run", boom)
    assert PsiGuardUnit.start("c1") is False
    PsiGuardUnit.stop()


def test_server_stop_stops_the_unit(monkeypatch):
    calls = _record(monkeypatch)
    monkeypatch.setattr(
        "dreamference.vllm_server.model_gate.ModelGate.stop", classmethod(lambda c, p: None)
    )
    VLLMServerManager().stop_server(18001)
    assert ["systemctl", "--user", "stop", PSI_GUARD_UNIT] in calls


def test_main_needs_exactly_one_container():
    assert psi_watchdog.main([]) == 2
