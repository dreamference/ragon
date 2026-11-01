"""The egress audit's forge line and its default model server."""

import pytest

from dreamference.audit import EgressAudit
from dreamference.audit.egress_trace import EgressTrace
from dreamference.config import DreamferenceConfig
from dreamference.forge.forge_github_mirror import ForgeGithubMirror
from dreamference.forge.forge_settings import ROLE_RECORD, ForgeSettings


def forge(monkeypatch, enabled=True, role=ROLE_RECORD, mirror=True, airgapped=False, hosts=("gh",)):
    monkeypatch.setattr(EgressAudit, "_unit_enabled", classmethod(lambda cls, unit, user: enabled))
    monkeypatch.setattr(ForgeSettings, "role", classmethod(lambda cls: role))
    monkeypatch.setattr(ForgeGithubMirror, "settings", classmethod(lambda cls: {"enabled": mirror}))
    monkeypatch.setattr(ForgeGithubMirror, "destinations", classmethod(lambda cls: list(hosts)))
    monkeypatch.setattr(ForgeGithubMirror, "airgapped", classmethod(lambda cls: airgapped))


def test_no_forge_line_when_its_unit_is_off(monkeypatch):
    forge(monkeypatch, enabled=False)
    assert EgressAudit.forge_lines() == []


def test_the_forge_line_names_every_mirror_destination_and_the_air_gap(monkeypatch):
    forge(monkeypatch, airgapped=True, hosts=("github.com/a", "github.com/b"))
    [line] = EgressAudit.forge_lines()
    assert "pushes to github.com/a, github.com/b (stopped: /airgapped is on)" in line


@pytest.mark.parametrize(
    "arguments",
    [{"mirror": False}, {"role": "replica"}, {"role": None}, {"hosts": ()}],
)
def test_a_forge_without_an_active_mirror_reaches_nothing_outside_the_lan(monkeypatch, arguments):
    forge(monkeypatch, **arguments)
    [line] = EgressAudit.forge_lines()
    assert "no outbound mirror is configured" in line


def test_the_audit_asks_the_configured_model_server_when_none_is_given(monkeypatch, capsys):
    monkeypatch.setattr("shutil.which", lambda name: "/usr/bin/strace")
    monkeypatch.setattr(
        EgressAudit,
        "trace_session",
        classmethod(lambda cls, *args, **kwargs: (EgressTrace(), False, None)),
    )
    EgressAudit.run(ragon_bin="/opt/ragon")
    assert f"against {DreamferenceConfig().vllm_host} " in capsys.readouterr().out
