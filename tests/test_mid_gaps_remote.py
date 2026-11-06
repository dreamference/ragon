"""Coverage for the remote setup stages' failure paths."""

from __future__ import annotations

from dreamference.remote.remote_access import RemoteAccess
from dreamference.remote.remote_control_plane import RemoteControlPlane
from dreamference.remote.remote_settings import RemoteSettings
from dreamference.remote.remote_tunnel import RemoteTunnel

RUN = {
    "dns_name": "box.example",
    "facts": {"arch": "arm64", "packages": [], "host_key": "k"},
    "relay": None,
    "secret": "s",
    "ssh_to": None,
}


def test_a_box_stage_stops_when_the_tunnel_key_cannot_be_made(monkeypatch, capsys):
    monkeypatch.setattr(RemoteTunnel, "public_key", classmethod(lambda cls: None))
    assert RemoteAccess._stage_box(RUN) is False
    assert "ssh-keygen" in capsys.readouterr().out


def test_a_tunnel_stage_reports_a_unit_systemd_did_not_start(monkeypatch, capsys):
    monkeypatch.setattr(RemoteTunnel, "install", classmethod(lambda cls, name: False))
    assert RemoteAccess._stage_tunnel(RUN) is False
    assert "journalctl" in capsys.readouterr().out


def test_a_node_that_did_not_join_leaves_its_record_and_says_to_run_setup_again(
    monkeypatch, capsys
):
    written = []
    monkeypatch.setattr(RemoteSettings, "write", classmethod(lambda cls, r: written.append(r)))
    monkeypatch.setattr(RemoteAccess, "_join_node", classmethod(lambda cls, run: False))
    run = {
        **RUN,
        "overlay_name": "n",
        "token": "t",
        "scope": "s",
        "existing": {"public_certificate": "pc"},
        "node_id": "id",
    }
    assert RemoteAccess._stage_node_peer(run) is False
    assert written and written[0]["public_certificate"] == "pc"
    assert "did not join the overlay" in capsys.readouterr().out


def test_no_control_plane_token_when_it_will_not_start_or_does_not_answer(monkeypatch, capsys):
    monkeypatch.setattr(RemoteControlPlane, "start", classmethod(lambda cls, setup_pat: False))
    assert RemoteAccess._control_plane_token("box", "id") is None
    monkeypatch.setattr(RemoteControlPlane, "start", classmethod(lambda cls, setup_pat: True))
    monkeypatch.setattr(RemoteControlPlane, "wait_ready", classmethod(lambda cls, name: None))
    assert RemoteAccess._control_plane_token("box", "id") is None
    assert "did not answer on loopback" in capsys.readouterr().out


def test_a_box_that_cannot_be_deployed_stops_the_box_stage(monkeypatch):
    from dreamference.remote.remote_box import RemoteBox
    from dreamference.remote.remote_relay_certificate import RemoteRelayCertificate

    monkeypatch.setattr(RemoteTunnel, "public_key", classmethod(lambda cls: "key"))
    monkeypatch.setattr(
        RemoteRelayCertificate, "box_files", classmethod(lambda cls, name, relay: ("c", "k"))
    )
    monkeypatch.setattr(RemoteBox, "deploy", classmethod(lambda cls, *args, **kwargs: False))
    assert RemoteAccess._stage_box(RUN) is False


def test_the_overlay_name_is_advertised_through_the_node_service_file(monkeypatch):
    from dreamference.node.node_service_file import NodeServiceFile

    seen = {}
    monkeypatch.setattr(NodeServiceFile, "update", classmethod(lambda cls, **kw: seen.update(kw)))
    RemoteAccess._advertise("n.overlay")
    assert seen == {"remote": "n.overlay"}
