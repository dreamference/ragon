"""The `ragon-admin` handlers for the sidecars and `remote certificate`: every branch, mocked."""

import argparse

import pytest

from dreamference.chat.gmail_client import GmailClient
from dreamference.chat.google_service import GoogleService
from dreamference.chat.image_search_sidecar import ImageSearchSidecar
from dreamference.chat.speech_sidecar import SpeechSidecar
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as Cli
from dreamference.remote.remote_access import RemoteAccess


class _Config:
    vllm_host = "http://127.0.0.1:1"
    model = "m"


def _images(command, no_siglip=False):
    return argparse.Namespace(images_command=command, no_siglip=no_siglip)


def test_images_start_reports_success_and_passes_the_siglip_choice(monkeypatch, capsys):
    seen = {}

    def start(host, model, siglip):
        seen.update(host=host, model=model, siglip=siglip)
        return True

    monkeypatch.setattr(ImageSearchSidecar, "start", staticmethod(start))
    assert Cli._run_images(_images("start", no_siglip=True), _Config()) == 0
    assert seen == {"host": _Config.vllm_host, "model": "m", "siglip": False}
    assert "Image search is running" in capsys.readouterr().out


def test_images_start_failure_prints_the_problem(monkeypatch, capsys):
    monkeypatch.setattr(ImageSearchSidecar, "start", staticmethod(lambda *a, **k: False))
    monkeypatch.setattr(ImageSearchSidecar, "problem", "no docker", raising=False)
    assert Cli._run_images(_images("start"), _Config()) == 1
    assert "no docker" in capsys.readouterr().out


@pytest.mark.parametrize(("stopped", "code"), [(True, 0), (False, 1)])
def test_images_stop_exit_code_follows_the_sidecar(monkeypatch, stopped, code):
    monkeypatch.setattr(ImageSearchSidecar, "stop", staticmethod(lambda: stopped))
    assert Cli._run_images(_images("stop"), _Config()) == code


def test_images_status_prints_each_line_and_unknown_command_is_usage(monkeypatch, capsys):
    monkeypatch.setattr(ImageSearchSidecar, "status_lines", staticmethod(lambda: ["a", "b"]))
    assert Cli._run_images(_images("status"), _Config()) == 0
    assert capsys.readouterr().out.split() == ["a", "b"]
    assert Cli._run_images(_images("nope"), _Config()) == 2
    assert "usage: ragon-admin images" in capsys.readouterr().out


def test_voice_start_success_and_failure(monkeypatch, capsys):
    monkeypatch.setattr(SpeechSidecar, "start", staticmethod(lambda: True))
    assert Cli._run_voice("start") == 0
    assert "Speech-to-text is running" in capsys.readouterr().out
    monkeypatch.setattr(SpeechSidecar, "start", staticmethod(lambda: False))
    monkeypatch.setattr(SpeechSidecar, "problem", "port busy", raising=False)
    assert Cli._run_voice("start") == 1
    assert "port busy" in capsys.readouterr().out


@pytest.mark.parametrize(("stopped", "code"), [(True, 0), (False, 1)])
def test_voice_stop_exit_code_follows_the_sidecar(monkeypatch, stopped, code):
    monkeypatch.setattr(SpeechSidecar, "stop", staticmethod(lambda: stopped))
    assert Cli._run_voice("stop") == code


def test_voice_status_and_usage(monkeypatch, capsys):
    monkeypatch.setattr(SpeechSidecar, "status_lines", staticmethod(lambda: ["ok"]))
    assert Cli._run_voice("status") == 0
    assert "ok" in capsys.readouterr().out
    assert Cli._run_voice(None) == 2
    assert "usage: ragon-admin voice" in capsys.readouterr().out


def test_google_start_success_and_failure(monkeypatch, capsys):
    monkeypatch.setattr(GoogleService, "start", staticmethod(lambda: True))
    assert Cli._google_start() == 0
    assert "Google service is running" in capsys.readouterr().out
    monkeypatch.setattr(GoogleService, "start", staticmethod(lambda: False))
    monkeypatch.setattr(GoogleService, "problem", "boom", raising=False)
    assert Cli._google_start() == 1
    assert "boom" in capsys.readouterr().out


@pytest.mark.parametrize(("stopped", "code"), [(True, 0), (False, 1)])
def test_google_stop_exit_code(monkeypatch, stopped, code):
    monkeypatch.setattr(GoogleService, "stop", staticmethod(lambda: stopped))
    assert Cli._google_stop() == code


def test_google_status_lists_accounts_with_short_scope_names(monkeypatch, capsys):
    monkeypatch.setattr(GoogleService, "state", staticmethod(lambda: "running"))
    answer = {
        "accounts": [
            {"email": "a@x", "scopes": ["https://www.googleapis.com/auth/drive/", "mail"]}
        ],
        "error": "late",
    }
    monkeypatch.setattr(GmailClient, "status", staticmethod(lambda: answer))
    assert Cli._google_status() == 0
    out = capsys.readouterr().out
    assert "Container: running" in out
    assert "a@x: drive, mail" in out
    assert "late" in out


def test_google_status_old_service_names_accounts_in_email_only(monkeypatch, capsys):
    monkeypatch.setattr(GoogleService, "state", staticmethod(lambda: None))
    monkeypatch.setattr(GmailClient, "status", staticmethod(lambda: {"email": "a@x, b@x"}))
    assert Cli._google_status() == 0
    out = capsys.readouterr().out
    assert "Container: absent" in out
    assert "a@x: mail.google.com" in out
    assert "b@x: mail.google.com" in out
    assert "predates Drive and Calendar" in out


def test_run_google_unknown_command_exits_2_and_known_exits_with_its_code(monkeypatch, capsys):
    with pytest.raises(SystemExit) as info:
        Cli._run_google(None)
    assert info.value.code == 2
    assert "usage: ragon-admin google" in capsys.readouterr().out
    monkeypatch.setattr(GoogleService, "stop", staticmethod(lambda: True))
    with pytest.raises(SystemExit) as info:
        Cli._run_google("stop")
    assert info.value.code == 0


def test_remote_certificate_dispatch(monkeypatch, capsys):
    monkeypatch.setattr(RemoteAccess, "certificate_renew", staticmethod(lambda: 7))
    monkeypatch.setattr(
        RemoteAccess, "certificate_issue", staticmethod(lambda staging: 10 if staging else 11)
    )
    monkeypatch.setattr(
        RemoteAccess, "certificate_relay", staticmethod(lambda staging, name: (staging, name))
    )
    ns = argparse.Namespace(certificate_command="renew", staging=False, name=None)
    assert Cli._remote_certificate(ns) == 7
    ns.certificate_command, ns.staging = "issue", True
    assert Cli._remote_certificate(ns) == 10
    ns.certificate_command, ns.name = "relay", "n"
    assert Cli._remote_certificate(ns) == (True, "n")
    ns.certificate_command = None
    assert Cli._remote_certificate(ns) == 2
    assert "usage: ragon-admin remote certificate" in capsys.readouterr().out
