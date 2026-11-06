"""Coverage for the GitHub mirror's issue and report paths and the adapter sender's failures."""

from __future__ import annotations

from types import SimpleNamespace

from dreamference.forge.forge_accounts import ForgeAccounts
from dreamference.forge.forge_container import ForgeContainer
from dreamference.forge.forge_github_mirror import ForgeGithubMirror
from dreamference.forge.forge_settings import ForgeSettings
from dreamference.hardware.adapter_verifier import AdapterVerifier
from dreamference.node.node_adapter_sync import NodeAdapterSync


def api_calls(monkeypatch, issues):
    calls = []

    def api(cls, method, path, token, body=None):
        calls.append((method, path, body))
        return 200, issues

    monkeypatch.setattr(ForgeAccounts, "admin_token", classmethod(lambda cls: "t"))
    monkeypatch.setattr(ForgeContainer, "api", classmethod(api))
    return calls


def test_an_open_issue_about_the_stopped_mirror_is_not_opened_twice(monkeypatch):
    calls = api_calls(monkeypatch, [{"title": "GitHub mirror stopped"}])
    ForgeGithubMirror.open_issue("org/repo", "why")
    assert [c[0] for c in calls] == ["GET"]


def test_a_stopped_mirror_opens_one_issue_with_the_reason(monkeypatch):
    calls = api_calls(monkeypatch, [{"title": "other"}])
    ForgeGithubMirror.open_issue("org/repo", "why")
    assert [c[0] for c in calls] == ["GET", "POST"]
    assert calls[1][2]["title"] == "GitHub mirror stopped"
    assert calls[1][2]["body"].startswith("why")


def test_the_mirror_status_is_none_until_a_run_wrote_it_and_then_has_the_time(monkeypatch):
    assert ForgeGithubMirror.status() is None
    written = {}
    monkeypatch.setattr(
        ForgeSettings, "write_secret", classmethod(lambda cls, n, t: written.update({n: t}))
    )
    ForgeGithubMirror.write_status({"ok": True})
    assert '"at"' in next(iter(written.values()))


def test_a_report_names_the_stop_the_failures_the_held_refs_and_the_backoff(capsys):
    ForgeGithubMirror.report("a/b", {"stop": True, "error": "x"})
    ForgeGithubMirror.report("a/b", {"failures": {"main": "c"}, "held": {"dev": "h"}})
    ForgeGithubMirror.report("a/b", {"ok": False, "error": "e", "backoff": "30s"})
    out = capsys.readouterr().out
    for text in ("STOPPED: x", "main: c", "dev: held: h", "a/b: e", "throttling"):
        assert text in out


def test_a_node_without_a_usable_ssh_command_stops_the_adapter_send(monkeypatch):
    monkeypatch.setattr(
        NodeAdapterSync,
        "signed_for",
        classmethod(lambda cls, key: [{"name": "n", "repo": "x/n", "commit": "c"}]),
    )

    def refuse(cls, record, request):
        raise LookupError("no address")

    monkeypatch.setattr(
        "dreamference.node.node_pairing.NodePairing.ssh_command", classmethod(refuse)
    )
    code, lines = NodeAdapterSync.send({"name": "second"}, "m")
    assert code == 1 and lines == ["❌ no address"]


def test_the_sender_closes_its_end_of_the_tar_pipe_and_fails_on_a_bad_tar(monkeypatch):
    monkeypatch.setattr(
        NodeAdapterSync,
        "signed_for",
        classmethod(lambda cls, key: [{"name": "n", "repo": "x/n", "commit": "c"}]),
    )
    monkeypatch.setattr(
        "dreamference.node.node_pairing.NodePairing.ssh_command",
        classmethod(lambda cls, record, request: ["ssh", request]),
    )
    closed = []

    class Process:
        returncode = 2

        def __init__(self, command, **kwargs):
            self.stdout = SimpleNamespace(close=lambda: closed.append(1))

        def wait(self):
            return 0

    monkeypatch.setattr("dreamference.node.node_adapter_sync.subprocess.Popen", Process)
    code, _ = NodeAdapterSync.send({"name": "second"}, "m")
    assert code == 1 and closed


def test_an_adapter_whose_files_no_longer_verify_is_not_recorded(monkeypatch, tmp_path, capsys):
    def broken(cls, snapshot, signed):
        raise ValueError("bad files")

    monkeypatch.setattr(AdapterVerifier, "check_files", classmethod(broken))
    assert NodeAdapterSync._record("n", "x/n", "c", tmp_path) == 1
    assert "bad files" in capsys.readouterr().err
